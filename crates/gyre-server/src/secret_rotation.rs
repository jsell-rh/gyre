//! Secret expiry check background job (platform-model.md §7, task-098/100).
//!
//! Runs every 5 minutes (`secret_rotation_check` job). Scans tenant-,
//! workspace-, and repo-scoped secrets for expiry within the next hour.
//! For each expiring secret with a rotation handler (§7 Rotated type, e.g.
//! OAuth refresh tokens — see [`SecretRotationHandler`]) the job attempts
//! automatic rotation; secrets without a handler (or whose rotation
//! failed) get a priority-7 `SecretExpiring` notification for each
//! Admin/Owner workspace member responsible for the secret's scope:
//!
//! - repo-scoped secret  → Admin/Owner members of the repo's workspace
//! - workspace-scoped    → Admin/Owner members of that workspace
//! - tenant-scoped       → Admin/Owner members of every workspace in the tenant
//!
//! Re-creation is suppressed while an active `SecretExpiring` notification
//! for the same (user, workspace, secret) exists, and for 24 h after a user
//! dismisses one (same suppression shape as the trust-suggestion job).

use std::sync::Arc;

use anyhow::Context as _;
use gyre_common::{Id, Notification, NotificationType, Secret};
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

// ── Rotation handlers (platform-model.md §7 Rotated type) ───────────────────

/// Result of a successful handler rotation: the new plaintext value and the
/// new expiry (Unix epoch seconds).
pub struct RotatedSecret {
    pub value: Vec<u8>,
    pub expires_at: Option<u64>,
}

/// A rotation handler performs a real credential refresh for an expiring
/// secret (platform-model.md §7: Rotated secrets are refreshed by a
/// "Background job refreshes before expiry"). Handlers match on the secret
/// name; a secret with no matching handler — or one whose handler fails —
/// falls back to an admin notification.
///
/// Handlers receive the decrypted plaintext only as opaque bytes; they MUST
/// never log or serialize it.
#[async_trait::async_trait]
#[allow(clippy::double_must_use)] // async_trait generates a must_use future for each method.
pub trait SecretRotationHandler: Send + Sync {
    /// Whether this handler can rotate `secret`.
    fn handles(&self, secret: &Secret) -> bool;

    /// Refresh the credential and return the new value + expiry.
    async fn rotate(
        &self,
        http: &reqwest::Client,
        secret: &Secret,
        current_value: &[u8],
    ) -> anyhow::Result<RotatedSecret>;
}

/// OAuth 2.0 refresh-token grant handler for secrets whose name ends with
/// `_refresh_token` (e.g. `oauth_refresh_token`, `claude_max_refresh_token`).
///
/// The secret's plaintext value is a JSON credential document set by the
/// admin via `POST /api/v1/secrets`:
///
/// ```json
/// { "token_endpoint": "https://issuer.example/token",
///   "client_id": "...", "client_secret": "...",
///   "refresh_token": "..." }
/// ```
///
/// The handler performs a standard `refresh_token` grant (RFC 6749 §6)
/// against `token_endpoint` and stores the updated document — new access
/// token, and the rotated refresh token when the provider issues one. The
/// secret's `expires_at` is extended by the provider's `expires_in`.
pub struct OAuthRefreshHandler;

/// The credential document carried as an `_refresh_token` secret's value.
#[derive(serde::Serialize, serde::Deserialize)]
struct OAuthCredentialDoc {
    token_endpoint: String,
    client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    client_secret: Option<String>,
    refresh_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    access_token: Option<String>,
}

#[async_trait::async_trait]
impl SecretRotationHandler for OAuthRefreshHandler {
    fn handles(&self, secret: &Secret) -> bool {
        secret.name.to_ascii_lowercase().ends_with("_refresh_token")
    }

    async fn rotate(
        &self,
        http: &reqwest::Client,
        _secret: &Secret,
        current_value: &[u8],
    ) -> anyhow::Result<RotatedSecret> {
        let doc: OAuthCredentialDoc = serde_json::from_slice(current_value).context(
            "value is not an OAuth credential document (JSON with token_endpoint, client_id, refresh_token)",
        )?;

        let mut form: Vec<(&str, &str)> = vec![
            ("grant_type", "refresh_token"),
            ("refresh_token", doc.refresh_token.as_str()),
            ("client_id", doc.client_id.as_str()),
        ];
        if let Some(cs) = doc.client_secret.as_deref() {
            form.push(("client_secret", cs));
        }

        // `http` is the AppState client, built with a 30 s total timeout, so
        // a hung provider cannot stall the job loop.
        let resp = http
            .post(&doc.token_endpoint)
            .form(&form)
            .send()
            .await
            .context("token endpoint request failed")?;
        let status = resp.status();
        let body = resp.bytes().await.context("reading token response")?;
        anyhow::ensure!(status.is_success(), "token endpoint returned HTTP {status}");

        let tokens: serde_json::Value =
            serde_json::from_slice(&body).context("token response is not JSON")?;
        let access_token = tokens
            .get("access_token")
            .and_then(|v| v.as_str())
            .context("token response missing access_token")?
            .to_string();
        // RFC 6749 §6: the provider MAY issue a new refresh token; when
        // absent the old one remains valid.
        let refresh_token = tokens
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .unwrap_or(&doc.refresh_token)
            .to_string();
        let expires_in = tokens
            .get("expires_in")
            .and_then(|v| v.as_u64())
            .unwrap_or(3600);

        let updated = OAuthCredentialDoc {
            token_endpoint: doc.token_endpoint,
            client_id: doc.client_id,
            client_secret: doc.client_secret,
            refresh_token,
            access_token: Some(access_token),
        };
        let value = serde_json::to_vec(&updated).context("serializing rotated credential")?;
        Ok(RotatedSecret {
            value,
            expires_at: Some(now_secs().saturating_add(expires_in)),
        })
    }
}

static OAUTH_REFRESH: OAuthRefreshHandler = OAuthRefreshHandler;

/// Handlers consulted (in order) by the rotation job.
static ROTATION_HANDLERS: &[&dyn SecretRotationHandler] = &[&OAUTH_REFRESH];

/// The handler that can rotate `secret`, if any.
fn handler_for(secret: &Secret) -> Option<&'static dyn SecretRotationHandler> {
    ROTATION_HANDLERS
        .iter()
        .copied()
        .find(|h| h.handles(secret))
}

/// Attempt automatic rotation of `secret` via its handler. On success the
/// stored value is replaced, `last_rotated_at` is set, `expires_at` is
/// extended past the expiry window, and a `secret.rotated` audit event is
/// logged (name, never value). Errors propagate to the caller, which falls
/// back to admin notification.
async fn attempt_rotation(
    state: &Arc<AppState>,
    handler: &dyn SecretRotationHandler,
    secret: &Secret,
) -> anyhow::Result<()> {
    let current = state
        .secrets
        .get_value(&secret.id, &secret.tenant_id)
        .await?
        .with_context(|| format!("secret {} value vanished before rotation", secret.id))?;

    let rotated = handler.rotate(&state.http_client, secret, &current).await?;

    // Replace the value + `last_rotated_at` via the port, then rewrite the
    // row's metadata (extended `expires_at`) with the same delete + create
    // pattern the Admin API uses for a force-rotate with expiry change. The
    // new value is re-supplied from the handler, never read back out of
    // storage.
    state
        .secrets
        .rotate(&secret.id, &rotated.value, &secret.tenant_id)
        .await?;
    let mut updated = state
        .secrets
        .find_by_id(&secret.id, &secret.tenant_id)
        .await?
        .context("secret vanished after rotate")?;
    updated.expires_at = rotated.expires_at;
    state.secrets.delete(&secret.id, &secret.tenant_id).await?;
    state.secrets.create(&updated, &rotated.value).await?;

    // §7 audit event: secret name (never value), scope, rotating actor.
    info!(
        event = "secret.rotated",
        secret_name = %secret.name,
        scope = secret.scope.as_str(),
        scope_id = %secret.scope_id,
        tenant_id = %secret.tenant_id,
        rotated_by = "secret_rotation_check",
        expires_at = rotated.expires_at,
        "secret.rotated: rotation handler refreshed expiring secret"
    );
    Ok(())
}

/// Attempt handler rotation for one expiring secret. Returns `true` when a
/// handler rotated it (suppressing the admin notification); `false` when
/// there is no handler or rotation failed — a failed rotation is exactly
/// what an admin must be warned about, so the caller notifies.
async fn try_rotate(state: &Arc<AppState>, secret: &Secret) -> bool {
    let Some(handler) = handler_for(secret) else {
        return false;
    };
    match attempt_rotation(state, handler, secret).await {
        Ok(()) => true,
        Err(e) => {
            warn!(
                secret_name = %secret.name,
                scope = secret.scope.as_str(),
                tenant_id = %secret.tenant_id,
                "secret_rotation: automatic rotation failed, falling back to admin notification: {e:#}"
            );
            false
        }
    }
}

// ── Agent teardown revocation (platform-model.md §7) ────────────────────────

/// Revoke an agent's task-scoped secrets on teardown (agent completion or
/// admin kill): delete every Task-scoped secret for `task_id` from the
/// secrets table and log a `secret.revoked` audit event per secret (name,
/// never value).
///
/// The tenant is resolved from the agent's workspace record; an
/// unresolvable workspace skips revocation with a warn — a scope is never
/// fabricated.
pub async fn revoke_task_secrets(
    state: &AppState,
    workspace_id: &Id,
    task_id: &str,
    agent_id: &str,
) {
    let tenant_id = match state.workspaces.find_by_id(workspace_id).await {
        Ok(Some(ws)) => ws.tenant_id.to_string(),
        Ok(None) => {
            warn!(
                agent_id,
                task_id, "secret teardown: workspace unresolvable; skipping task-secret revocation"
            );
            return;
        }
        Err(e) => {
            warn!(
                agent_id,
                task_id, "secret teardown: workspace lookup failed: {e:#}"
            );
            return;
        }
    };

    let secrets = match state
        .secrets
        .list_by_scope(gyre_common::SecretScope::Task, task_id, &tenant_id)
        .await
    {
        Ok(s) => s,
        Err(e) => {
            warn!(
                agent_id,
                task_id, "secret teardown: task-scope list failed: {e:#}"
            );
            return;
        }
    };

    for secret in secrets {
        match state.secrets.delete(&secret.id, &tenant_id).await {
            Ok(()) => {
                // §7 audit event: secret name (never value), scope, agent_id.
                info!(
                    event = "secret.revoked",
                    agent_id,
                    secret_name = %secret.name,
                    scope = "task",
                    task_id,
                    tenant_id = %tenant_id,
                    "secret.revoked: task-scoped secret deleted on agent teardown"
                );
            }
            Err(e) => {
                warn!(
                    agent_id,
                    secret_name = %secret.name,
                    "secret teardown: delete failed: {e:#}"
                );
            }
        }
    }
}

/// Revoke the task-scoped ephemeral secrets of `agent`'s current task, if
/// any — the shared teardown step for every path that ends an agent run
/// (REST and MCP completion, admin kill, heartbeat abort, compose teardown,
/// repo-archive stop, process/container/SSH exit monitors). Agents without a
/// current task (e.g. orchestrators) have nothing to revoke.
pub async fn revoke_agent_task_secrets(state: &AppState, agent: &gyre_domain::Agent) {
    if let Some(task_id) = agent.current_task_id.as_ref() {
        revoke_task_secrets(
            state,
            &agent.workspace_id,
            &task_id.to_string(),
            &agent.id.to_string(),
        )
        .await;
    }
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
            // Rotated secrets with a handler are refreshed automatically;
            // only no-handler / failed-rotation secrets notify admins.
            if try_rotate(state, secret).await {
                continue;
            }
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
                // Rotated secrets with a handler are refreshed automatically;
                // only no-handler / failed-rotation secrets notify admins.
                if try_rotate(state, secret).await {
                    continue;
                }
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
                    // Rotated secrets with a handler are refreshed
                    // automatically; only no-handler / failed-rotation
                    // secrets notify admins.
                    if try_rotate(state, secret).await {
                        continue;
                    }
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
                None,
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

        if let Err(e) = state.create_notification(&notif).await {
            warn!(
                workspace_id = %workspace_id,
                user_id = %member.user_id,
                "secret_rotation: notification create failed: {e:#}"
            );
        } else {
            // Channel fan-out per user-management.md §Delivery Channels.
            crate::notification_dispatcher::dispatch_to_channels(state, &notif).await;
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
    async fn expiry_notification_fans_out_to_configured_channels() {
        // Emitter-path guard: the secret-expiry scan's notification creation
        // must go through the channel dispatcher — a configured email
        // channel queues an outbox entry for the notified Admin.
        let now = now_secs();
        let state = seed(now).await;
        state
            .user_channel_prefs
            .upsert(
                &Id::new("user-admin"),
                &gyre_domain::NotificationChannels {
                    in_app: true,
                    email: gyre_domain::EmailConfig {
                        enabled: true,
                        digest: gyre_domain::DigestFrequency::Immediate,
                        min_priority: gyre_domain::NotificationPriority::Low,
                    },
                    webhook: None,
                    slack: None,
                },
            )
            .await
            .unwrap();

        run_once(&state).await.unwrap();

        let admin_notifs = state
            .notifications
            .list_for_user(&Id::new("user-admin"), None, None, None, None, None, 50, 0)
            .await
            .unwrap();
        assert!(
            admin_notifs
                .iter()
                .any(|n| n.notification_type == NotificationType::SecretExpiring),
            "expected in-app SecretExpiring notification"
        );
        let outbox = state
            .kv_store
            .kv_list(crate::notification_dispatcher::EMAIL_OUTBOX_NS)
            .await
            .unwrap();
        assert!(
            outbox.iter().any(|(k, _)| k.starts_with("user-admin:")),
            "secret-expiry notification must reach the configured email channel"
        );
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
            .list_for_user(&Id::new("user-admin"), None, None, None, None, None, 50, 0)
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
            .list_for_user(&Id::new("user-dev"), None, None, None, None, None, 50, 0)
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
                None,
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
                None,
                50,
                0,
            )
            .await
            .unwrap();
        assert!(notifs.is_empty(), "healthy secret must not notify");
    }

    // ── Rotation handler (task-100) ───────────────────────────────────────────

    /// Minimal seed for handler tests: tenant + workspace + Admin member + a
    /// single expiring `oauth_refresh_token` secret with the given value.
    async fn seed_oauth(now: u64, value: &[u8]) -> Arc<AppState> {
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
        let m = WorkspaceMembership::new(
            Id::new("mem-oauth-admin"),
            Id::new("user-admin"),
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
                    id: Id::new("sec-oauth"),
                    name: "oauth_refresh_token".to_string(),
                    scope: gyre_common::SecretScope::Workspace,
                    scope_id: "ws-rot".to_string(),
                    secret_type: gyre_common::SecretType::Rotated,
                    created_by: "system".to_string(),
                    created_at: now,
                    // Expiring in 30 minutes → inside the check window.
                    expires_at: Some(now + 1800),
                    last_rotated_at: None,
                    tenant_id: "default".to_string(),
                },
                value,
            )
            .await
            .unwrap();
        state
    }

    /// Spawn a local OAuth token endpoint returning `response` with
    /// `status`, capturing the received form body. Returns the endpoint URL.
    async fn spawn_token_endpoint(
        status: axum::http::StatusCode,
        response: serde_json::Value,
    ) -> (String, Arc<tokio::sync::Mutex<String>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(tokio::sync::Mutex::new(String::new()));
        let seen_clone = Arc::clone(&seen);
        let app = axum::Router::new().route(
            "/token",
            axum::routing::post(move |body: String| {
                let seen = Arc::clone(&seen_clone);
                async move {
                    *seen.lock().await = body;
                    (status, axum::Json(response))
                }
            }),
        );
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://127.0.0.1:{port}/token"), seen)
    }

    #[tokio::test]
    async fn rotation_handler_refreshes_expiring_oauth_secret() {
        let now = now_secs();
        let (endpoint, seen) = spawn_token_endpoint(
            axum::http::StatusCode::OK,
            serde_json::json!({
                "access_token": "new-access-token",
                "expires_in": 7200,
                "refresh_token": "new-refresh-token"
            }),
        )
        .await;
        let cred = serde_json::json!({
            "token_endpoint": endpoint,
            "client_id": "gyre-test",
            "client_secret": "test-client-secret",
            "refresh_token": "old-refresh-token"
        });
        let state = seed_oauth(now, cred.to_string().as_bytes()).await;

        run_once(&state).await.unwrap();

        // The handler performed a real refresh_token grant with the stored
        // credential (old refresh token + client id posted to the endpoint).
        let posted = seen.lock().await.clone();
        assert!(
            posted.contains("grant_type=refresh_token"),
            "grant_type must be refresh_token, posted: {posted}"
        );
        assert!(
            posted.contains("refresh_token=old-refresh-token"),
            "the stored refresh token must be used, posted: {posted}"
        );
        assert!(
            posted.contains("client_id=gyre-test"),
            "client_id must be posted, posted: {posted}"
        );

        // The stored value is the rotated credential document: new tokens,
        // untouched config preserved.
        let value = state
            .secrets
            .get_value(&Id::new("sec-oauth"), "default")
            .await
            .unwrap()
            .expect("rotated secret must still exist");
        let doc: serde_json::Value = serde_json::from_slice(&value).unwrap();
        assert_eq!(doc["refresh_token"], "new-refresh-token");
        assert_eq!(doc["access_token"], "new-access-token");
        assert_eq!(doc["client_id"], "gyre-test");

        // Expiry extended beyond the 1-hour window and last_rotated_at set.
        let stored = state
            .secrets
            .find_by_id(&Id::new("sec-oauth"), "default")
            .await
            .unwrap()
            .unwrap();
        assert!(
            stored.expires_at.unwrap() > now + 3600,
            "rotation must extend expiry past the check window, got {:?}",
            stored.expires_at
        );
        assert!(stored.last_rotated_at.is_some());

        // Successful rotation suppresses the admin notification.
        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user-admin"),
                None,
                None,
                None,
                Some("SecretExpiring"),
                None,
                50,
                0,
            )
            .await
            .unwrap();
        assert!(
            notifs.is_empty(),
            "a handler-rotated secret must not notify admins"
        );
    }

    #[tokio::test]
    async fn rotation_handler_failure_falls_back_to_admin_notification() {
        let now = now_secs();
        let (endpoint, _seen) = spawn_token_endpoint(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({"error": "invalid_grant"}),
        )
        .await;
        let cred = serde_json::json!({
            "token_endpoint": endpoint,
            "client_id": "gyre-test",
            "refresh_token": "old-refresh-token"
        });
        let original = cred.to_string();
        let state = seed_oauth(now, original.as_bytes()).await;

        run_once(&state).await.unwrap();

        // Value untouched: the failed grant must not corrupt the credential.
        let value = state
            .secrets
            .get_value(&Id::new("sec-oauth"), "default")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(value, original.as_bytes());

        // The admin is notified — a failed rotation is exactly what an
        // admin must be warned about.
        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user-admin"),
                None,
                None,
                None,
                Some("SecretExpiring"),
                None,
                50,
                0,
            )
            .await
            .unwrap();
        assert_eq!(notifs.len(), 1, "failed rotation must notify admins");
    }

    #[tokio::test]
    async fn malformed_handler_value_falls_back_to_admin_notification() {
        let now = now_secs();
        let state = seed_oauth(now, b"not-a-json-credential").await;

        run_once(&state).await.unwrap();

        let value = state
            .secrets
            .get_value(&Id::new("sec-oauth"), "default")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(value, b"not-a-json-credential");
        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user-admin"),
                None,
                None,
                None,
                Some("SecretExpiring"),
                None,
                50,
                0,
            )
            .await
            .unwrap();
        assert_eq!(
            notifs.len(),
            1,
            "unparseable credential must fall back to admin notification"
        );
    }

    // ── Task-scoped teardown revocation (task-100) ───────────────────────────

    #[tokio::test]
    async fn revoke_task_secrets_deletes_only_that_task_scope() {
        let now = now_secs();
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
            Id::new("ws-teardown"),
            Id::new("default"),
            "Teardown WS",
            "teardown-ws",
            now,
        );
        state.workspaces.create(&ws).await.unwrap();

        let mk = |id: &str, name: &str, scope: gyre_common::SecretScope, scope_id: &str| {
            gyre_common::Secret {
                id: Id::new(id),
                name: name.to_string(),
                scope,
                scope_id: scope_id.to_string(),
                secret_type: gyre_common::SecretType::Ephemeral,
                created_by: "system".to_string(),
                created_at: now,
                expires_at: None,
                last_rotated_at: None,
                tenant_id: "default".to_string(),
            }
        };
        // Task-scoped secret for the completing agent's task (revoked), a
        // task-scoped secret for a different task (kept), and a
        // workspace-scoped secret (kept).
        state
            .secrets
            .create(
                &mk(
                    "sec-t1",
                    "EPHEMERAL_DB",
                    gyre_common::SecretScope::Task,
                    "task-1",
                ),
                b"v1",
            )
            .await
            .unwrap();
        state
            .secrets
            .create(
                &mk(
                    "sec-t2",
                    "EPHEMERAL_DB",
                    gyre_common::SecretScope::Task,
                    "task-2",
                ),
                b"v2",
            )
            .await
            .unwrap();
        state
            .secrets
            .create(
                &mk(
                    "sec-ws",
                    "SHARED_KEY",
                    gyre_common::SecretScope::Workspace,
                    "ws-teardown",
                ),
                b"v3",
            )
            .await
            .unwrap();

        revoke_task_secrets(&state, &Id::new("ws-teardown"), "task-1", "agent-1").await;

        assert!(
            state
                .secrets
                .find_by_id(&Id::new("sec-t1"), "default")
                .await
                .unwrap()
                .is_none(),
            "the completed task's secret must be revoked"
        );
        assert!(
            state
                .secrets
                .find_by_id(&Id::new("sec-t2"), "default")
                .await
                .unwrap()
                .is_some(),
            "another task's secret must survive"
        );
        assert!(
            state
                .secrets
                .find_by_id(&Id::new("sec-ws"), "default")
                .await
                .unwrap()
                .is_some(),
            "workspace-scoped secrets must survive task teardown"
        );
    }

    #[tokio::test]
    async fn revoke_task_secrets_skips_unresolvable_workspace() {
        // No workspace record exists for this id: revocation must skip
        // (warn) rather than fabricate a tenant scope or panic.
        let state = crate::mem::test_state();
        revoke_task_secrets(&state, &Id::new("ws-ghost"), "task-1", "agent-1").await;
    }
}
