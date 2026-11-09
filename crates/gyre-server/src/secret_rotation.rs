//! Secret expiry check background job (platform-model.md §7, task-098).
//!
//! Runs every 5 minutes (`secret_rotation_check` job). Scans tenant-,
//! workspace-, and repo-scoped secrets for expiry within the next hour and
//! creates a priority-7 `SecretExpiring` notification for each Admin/Owner
//! workspace member responsible for the secret's scope:
//!
//! - repo-scoped secret  → Admin/Owner members of the repo's workspace
//! - workspace-scoped    → Admin/Owner members of that workspace
//! - tenant-scoped       → Admin/Owner members of every workspace in the tenant
//!
//! Re-creation is suppressed while an active `SecretExpiring` notification
//! for the same (user, workspace, secret) exists, and for 24 h after a user
//! dismisses one (same suppression shape as the trust-suggestion job).

use std::sync::Arc;

use gyre_common::{Id, Notification, NotificationType};
use tracing::{debug, info, warn};

use crate::AppState;

/// Window before expiry that triggers a notification (seconds).
const EXPIRY_WINDOW_SECS: u64 = 3600;

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Run one pass of the secret expiry check across all tenants.
pub async fn run_once(state: &Arc<AppState>) -> anyhow::Result<()> {
    let now = now_secs();
    let tenants = state.tenants.list().await?;
    debug!(tenants = tenants.len(), "secret_rotation: scanning tenants");

    for tenant in &tenants {
        let tenant_id = tenant.id.to_string();

        // Tenant-scoped secrets notify the admins of every workspace in the
        // tenant (the secret applies to all of them).
        let workspaces = state.workspaces.list_by_tenant(&tenant.id).await?;
        let tenant_secrets = state
            .secrets
            .list_by_scope(gyre_common::SecretScope::Tenant, &tenant_id, &tenant_id)
            .await
            .unwrap_or_else(|e| {
                warn!("secret_rotation: tenant-scope list failed for {tenant_id}: {e:#}");
                Vec::new()
            });
        for secret in expiring(&tenant_secrets, now) {
            for ws in &workspaces {
                notify_workspace_admins(state, ws.id.clone(), secret, now).await;
            }
        }

        for ws in &workspaces {
            let ws_id = ws.id.to_string();
            // Workspace-scoped secrets.
            let ws_secrets = state
                .secrets
                .list_by_scope(gyre_common::SecretScope::Workspace, &ws_id, &tenant_id)
                .await
                .unwrap_or_else(|e| {
                    warn!("secret_rotation: workspace-scope list failed for {ws_id}: {e:#}");
                    Vec::new()
                });
            for secret in expiring(&ws_secrets, now) {
                notify_workspace_admins(state, ws.id.clone(), secret, now).await;
            }

            // Repo-scoped secrets.
            let repos = state.repos.list_by_workspace(&ws.id).await?;
            for repo in &repos {
                let repo_id = repo.id.to_string();
                let repo_secrets = state
                    .secrets
                    .list_by_scope(gyre_common::SecretScope::Repo, &repo_id, &tenant_id)
                    .await
                    .unwrap_or_else(|e| {
                        warn!("secret_rotation: repo-scope list failed for {repo_id}: {e:#}");
                        Vec::new()
                    });
                for secret in expiring(&repo_secrets, now) {
                    notify_workspace_admins(state, ws.id.clone(), secret, now).await;
                }
            }
        }
    }
    Ok(())
}

/// Filter to secrets whose expiry is within [`EXPIRY_WINDOW_SECS`] of `now`
/// (already-expired secrets included — an admin must know either way).
fn expiring(secrets: &[gyre_common::Secret], now: u64) -> Vec<&gyre_common::Secret> {
    secrets
        .iter()
        .filter(|s| {
            s.expires_at
                .map(|e| e <= now + EXPIRY_WINDOW_SECS)
                .unwrap_or(false)
        })
        .collect()
}

/// Create a SecretExpiring notification for each Admin/Owner member of the
/// workspace, suppressing duplicates (active notification exists or a recent
/// dismissal).
///
/// `workspace_id` is the workspace that OWNS the secret (resolved by the
/// caller from the secret's own scope), and every notification is created
/// for a member of that same workspace — not a caller-propagated scope.
async fn notify_workspace_admins(
    state: &Arc<AppState>,
    workspace_id: Id,
    secret: &gyre_common::Secret,
    now: u64,
) {
    // caller-scope:ok — workspace_id is the secret's own governing workspace;
    // members and notifications are resolved from it directly by design.
    let members = match state
        .workspace_memberships
        .list_by_workspace(&workspace_id)
        .await
    {
        Ok(m) => m,
        Err(e) => {
            warn!(
                workspace_id = %workspace_id,
                "secret_rotation: member list failed: {e:#}"
            );
            return;
        }
    };

    for member in &members {
        if !matches!(
            member.role,
            gyre_domain::WorkspaceRole::Admin | gyre_domain::WorkspaceRole::Owner
        ) {
            continue;
        }

        // Suppress while an active SecretExpiring for this secret exists, or
        // for 24 h after the user dismissed one for THIS secret (a dismissal
        // for a different secret must not suppress this one).
        let existing = state
            .notifications
            .list_for_user(
                &member.user_id,
                Some(&workspace_id),
                None,
                None,
                Some(NotificationType::SecretExpiring.as_str()),
                50,
                0,
            )
            .await
            .unwrap_or_default();
        let dismissal_cutoff = now as i64 - 86_400;
        let suppressed = existing.iter().any(|n| {
            n.source.entity_id == secret.id.as_str()
                && (n.is_active()
                    || n.dismissed_at
                        .map(|d| d >= dismissal_cutoff)
                        .unwrap_or(false))
        });
        if suppressed {
            continue;
        }

        let status = if secret.is_expired(now) {
            "expired"
        } else {
            "expiring"
        };
        let notif = Notification::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            workspace_id.clone(),
            member.user_id.clone(),
            NotificationType::SecretExpiring,
            format!(
                "Secret '{}' ({} scope) is {}",
                secret.name,
                secret.scope.as_str(),
                status
            ),
            secret.tenant_id.clone(),
            now as i64,
        );
        let mut notif = notif;
        notif.body = Some(
            serde_json::json!({
                "secret_id": secret.id.to_string(),
                "name": secret.name,
                "scope": secret.scope.as_str(),
                "scope_id": secret.scope_id,
                "expires_at": secret.expires_at,
                "status": status,
            })
            .to_string(),
        );
        // Source the notification to the secret itself; a repo-scoped
        // secret carries its repo so the source is scoped like the entity.
        notif.source.entity_type = "secret".to_string();
        notif.source.entity_id = secret.id.to_string();
        if secret.scope == gyre_common::SecretScope::Repo {
            notif.source.repo_id = Some(Id::new(secret.scope_id.clone()));
        }

        if let Err(e) = state.notifications.create(&notif).await {
            warn!(
                workspace_id = %workspace_id,
                user_id = %member.user_id,
                "secret_rotation: notification create failed: {e:#}"
            );
        } else {
            info!(
                workspace_id = %workspace_id,
                user_id = %member.user_id,
                secret = %secret.name,
                status,
                "secret_rotation: expiry notification created"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_domain::{Workspace, WorkspaceMembership, WorkspaceRole};

    /// Seed: tenant "default", workspace with an Admin member, a Developer
    /// member, and secrets with controlled expiry. The tenant must be
    /// created explicitly — the mem tenant store starts empty, and
    /// `run_once` walks `tenants.list()`, so without it the scan finds
    /// nothing (the seeded workspace would be orphaned from the scan).
    async fn seed(now: u64) -> Arc<AppState> {
        let state = crate::mem::test_state();

        state
            .tenants
            .create(&gyre_domain::Tenant::new(
                Id::new("default"),
                "Default",
                "default",
                now,
            ))
            .await
            .unwrap();

        let ws = Workspace::new(
            Id::new("ws-rot"),
            Id::new("default"),
            "Rotation WS",
            "rotation-ws",
            now,
        );
        state.workspaces.create(&ws).await.unwrap();

        // Admin member + developer member.
        for (uid, role) in [
            ("user-admin", WorkspaceRole::Admin),
            ("user-dev", WorkspaceRole::Developer),
        ] {
            let m = WorkspaceMembership::new(
                Id::new(format!("mem-{uid}")),
                Id::new(uid),
                ws.id.clone(),
                role,
                Id::new("inviter"),
                now,
            );
            state.workspace_memberships.create(&m).await.unwrap();
        }

        let mk = |id: &str,
                  name: &str,
                  scope: gyre_common::SecretScope,
                  scope_id: &str,
                  expires_at: Option<u64>| gyre_common::Secret {
            id: Id::new(id),
            name: name.to_string(),
            scope,
            scope_id: scope_id.to_string(),
            secret_type: gyre_common::SecretType::Rotated,
            created_by: "system".to_string(),
            created_at: now,
            expires_at,
            last_rotated_at: None,
            tenant_id: "default".to_string(),
        };
        // Expiring in 30 minutes → notified.
        state
            .secrets
            .create(
                &mk(
                    "sec-exp",
                    "OAUTH_TOKEN",
                    gyre_common::SecretScope::Workspace,
                    "ws-rot",
                    Some(now + 1800),
                ),
                b"v",
            )
            .await
            .unwrap();
        // No expiry → never notified.
        state
            .secrets
            .create(
                &mk(
                    "sec-static",
                    "DATABASE_URL",
                    gyre_common::SecretScope::Workspace,
                    "ws-rot",
                    None,
                ),
                b"v",
            )
            .await
            .unwrap();
        // Expires in 2 hours → outside the window, not notified.
        state
            .secrets
            .create(
                &mk(
                    "sec-later",
                    "LATER_KEY",
                    gyre_common::SecretScope::Workspace,
                    "ws-rot",
                    Some(now + 7200),
                ),
                b"v",
            )
            .await
            .unwrap();
        // Already expired → notified (status "expired").
        state
            .secrets
            .create(
                &mk(
                    "sec-past",
                    "PAST_KEY",
                    gyre_common::SecretScope::Workspace,
                    "ws-rot",
                    Some(now - 60),
                ),
                b"v",
            )
            .await
            .unwrap();
        state
    }

    #[tokio::test]
    async fn detects_expiring_secrets_and_notifies_admins_only() {
        // Fixed "now" so the seeded expiries are deterministic. run_once uses
        // real wall-clock now, so seed relative to real now instead.
        let now = now_secs();
        let state = seed(now).await;
        run_once(&state).await.unwrap();

        let admin_notifs = state
            .notifications
            .list_for_user(&Id::new("user-admin"), None, None, None, None, 50, 0)
            .await
            .unwrap();
        let secret_notifs: Vec<&Notification> = admin_notifs
            .iter()
            .filter(|n| n.notification_type == NotificationType::SecretExpiring)
            .collect();
        assert_eq!(
            secret_notifs.len(),
            2,
            "expiring + expired secrets must each notify, got: {admin_notifs:#?}"
        );
        let names: Vec<String> = secret_notifs
            .iter()
            .map(|n| {
                let body: serde_json::Value =
                    serde_json::from_str(n.body.as_deref().unwrap()).unwrap();
                body["name"].as_str().unwrap().to_string()
            })
            .collect();
        assert!(names.contains(&"OAUTH_TOKEN".to_string()));
        assert!(names.contains(&"PAST_KEY".to_string()));

        // Developer member gets nothing.
        let dev_notifs = state
            .notifications
            .list_for_user(&Id::new("user-dev"), None, None, None, None, 50, 0)
            .await
            .unwrap();
        assert!(dev_notifs
            .iter()
            .all(|n| n.notification_type != NotificationType::SecretExpiring));
    }

    #[tokio::test]
    async fn does_not_duplicate_notifications_on_rerun() {
        let now = now_secs();
        let state = seed(now).await;
        run_once(&state).await.unwrap();
        run_once(&state).await.unwrap();

        let admin_notifs = state
            .notifications
            .list_for_user(
                &Id::new("user-admin"),
                None,
                None,
                None,
                Some("SecretExpiring"),
                50,
                0,
            )
            .await
            .unwrap();
        assert_eq!(admin_notifs.len(), 2, "second pass must not re-notify");
    }

    #[tokio::test]
    async fn healthy_secrets_never_notify() {
        let now = now_secs();
        let state = crate::mem::test_state();
        let ws = Workspace::new(
            Id::new("ws-healthy"),
            Id::new("default"),
            "Healthy WS",
            "healthy-ws",
            now,
        );
        state.workspaces.create(&ws).await.unwrap();
        let m = WorkspaceMembership::new(
            Id::new("mem-h"),
            Id::new("user-h-admin"),
            ws.id,
            WorkspaceRole::Admin,
            Id::new("inviter"),
            now,
        );
        state.workspace_memberships.create(&m).await.unwrap();
        state
            .secrets
            .create(
                &gyre_common::Secret {
                    id: Id::new("sec-healthy"),
                    name: "FINE_KEY".to_string(),
                    scope: gyre_common::SecretScope::Workspace,
                    scope_id: "ws-healthy".to_string(),
                    secret_type: gyre_common::SecretType::Static,
                    created_by: "system".to_string(),
                    created_at: now,
                    expires_at: Some(now + 30 * 86400),
                    last_rotated_at: None,
                    tenant_id: "default".to_string(),
                },
                b"v",
            )
            .await
            .unwrap();

        run_once(&state).await.unwrap();
        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user-h-admin"),
                None,
                None,
                None,
                Some("SecretExpiring"),
                50,
                0,
            )
            .await
            .unwrap();
        assert!(notifs.is_empty(), "healthy secret must not notify");
    }
}
