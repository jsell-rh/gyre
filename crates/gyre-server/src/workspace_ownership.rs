//! Workspace ownership tracking (user-management.md §Ownership Transfer &
//! Reclamation).
//!
//! When a workspace Owner leaves (account deactivated, removed from the
//! tenant):
//!
//! 1. Tenant Admins of the workspace's tenant receive a priority-2
//!    `WorkspaceOwnerless` notification ("Workspace X has no active Owner").
//! 2. The ownerless transition is recorded (`ownerless_since` timestamp in
//!    the `workspace_ownership` kv namespace) by the
//!    `orphaned_workspace_check` background job and, for immediacy, inline
//!    when a user is deleted (SCIM deactivate).
//! 3. If no Tenant Admin reassigns ownership within a configurable period
//!    (default: 30 days, `GYRE_ORPHANED_WORKSPACE_DAYS`), the workspace is
//!    flagged as "orphaned" in the admin dashboard
//!    (`GET /api/v1/admin/orphaned-workspaces`).
//!
//! The spec trigger is an owner *departure* ("when a workspace Owner
//! leaves"): a workspace that never had an Owner — created by the system
//! principal or an agent, which have no user identity to hold a membership
//! — is healthy by design and is neither recorded nor notified. Evidence
//! that an owner once existed is an Owner membership row (the departed
//! owner's row survives account deletion/deactivation; user deletion does
//! not cascade memberships).
//!
//! Reassigning ownership (`PUT /api/v1/workspaces/{id}/members/{user_id}/role`
//! with `{"role": "Owner"}`) clears the ownerless record and the flag.

use std::sync::Arc;

use gyre_common::{Id, Notification, NotificationType};
use gyre_domain::{Workspace, WorkspaceRole};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::AppState;

/// kv namespace holding one record per ownerless workspace.
const OWNERLESS_KV_NAMESPACE: &str = "workspace_ownership";

/// Default orphan flag period (user-management.md §Ownership Transfer &
/// Reclamation: "default: 30 days").
const DEFAULT_ORPHANED_THRESHOLD_DAYS: u64 = 30;

/// Persisted ownerless marker for one workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnerlessRecord {
    /// Unix seconds at which the workspace was first observed with no
    /// active Owner.
    pub ownerless_since: u64,
    /// Unix seconds at which the TenantAdmin notification was successfully
    /// created (None until then — retried on every job cycle).
    pub notified_at: Option<u64>,
}

/// The orphan flag period in days, from `GYRE_ORPHANED_WORKSPACE_DAYS`
/// (default 30).
pub fn orphaned_threshold_days() -> u64 {
    std::env::var("GYRE_ORPHANED_WORKSPACE_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|d| *d > 0)
        .unwrap_or(DEFAULT_ORPHANED_THRESHOLD_DAYS)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// An orphaned workspace as surfaced in the admin dashboard.
#[derive(Debug, Clone, Serialize)]
pub struct OrphanedWorkspace {
    pub workspace_id: String,
    pub name: String,
    pub tenant_id: String,
    /// Unix seconds at which the workspace was first observed with no
    /// active Owner.
    pub ownerless_since: u64,
    /// Whole days without an active Owner.
    pub days_ownerless: u64,
}

/// Whether the workspace currently has an active Owner: an Owner membership
/// whose member's user record still exists (a deactivated/removed user's
/// lingering membership is not an active Owner — that is exactly the
/// ownerless transition this module detects).
pub async fn has_active_owner(state: &Arc<AppState>, workspace_id: &Id) -> anyhow::Result<bool> {
    let members = state
        .workspace_memberships
        .list_by_workspace(workspace_id)
        .await?;
    for m in &members {
        if m.role != WorkspaceRole::Owner {
            continue;
        }
        // The user must still exist (not deactivated/removed).
        if let Ok(Some(_)) = state.users.find_by_id(&m.user_id).await {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether the workspace has ever had an Owner: any Owner membership row,
/// even one whose user record is gone. User deletion does not cascade
/// memberships, so a departed Owner's row survives as evidence that an
/// owner existed — the spec's trigger for the ownerless flow ("when a
/// workspace Owner leaves"). A workspace with no Owner membership at all
/// was never owned (system/agent-operated by design) and must not be
/// flagged or notified.
async fn has_ever_had_owner(state: &Arc<AppState>, workspace_id: &Id) -> anyhow::Result<bool> {
    Ok(state
        .workspace_memberships
        .list_by_workspace(workspace_id)
        .await?
        .iter()
        .any(|m| m.role == WorkspaceRole::Owner))
}

/// Read the persisted ownerless record for a workspace, if any.
async fn ownerless_record(
    state: &Arc<AppState>,
    workspace_id: &Id,
) -> anyhow::Result<Option<OwnerlessRecord>> {
    let raw = state
        .kv_store
        .kv_get(OWNERLESS_KV_NAMESPACE, workspace_id.as_str())
        .await?;
    match raw {
        Some(s) => Ok(serde_json::from_str(&s).map_err(|e| {
            anyhow::anyhow!("corrupt ownerless record for workspace {workspace_id}: {e}")
        })?),
        None => Ok(None),
    }
}

/// Create priority-2 `WorkspaceOwnerless` notifications for every TenantAdmin
/// of the workspace's tenant (user-management.md §Ownership Transfer &
/// Reclamation step 1). Deduplicated: an admin with an already-active
/// WorkspaceOwnerless notification for this workspace is skipped, so job
/// re-runs never stack duplicates.
async fn notify_tenant_admins(state: &Arc<AppState>, ws: &Workspace, since: u64) -> u64 {
    let users = match state.users.list().await {
        Ok(u) => u,
        Err(e) => {
            warn!(
                workspace_id = %ws.id,
                "workspace_ownership: failed to list users for TenantAdmin notification: {e}"
            );
            return 0;
        }
    };

    let tenant_id_str = ws.tenant_id.to_string();
    let now = now_secs() as i64;
    let mut created = 0u64;

    for user in users {
        if user.tenant_id.as_ref() != Some(&ws.tenant_id) {
            continue;
        }
        if user.global_role != gyre_domain::GlobalRole::TenantAdmin {
            continue;
        }

        // Dedup: skip when an active WorkspaceOwnerless notification already
        // exists for this (workspace, user) pair.
        let existing = state
            .notifications
            .list_for_user(
                &user.id,
                Some(&ws.id),
                None,
                None,
                Some(NotificationType::WorkspaceOwnerless.as_str()),
                None,
                20,
                0,
            )
            .await
            .unwrap_or_default();
        if existing
            .iter()
            .any(|n| n.notification_type == NotificationType::WorkspaceOwnerless && n.is_active())
        {
            continue;
        }

        let mut notif = Notification::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            ws.id.clone(),
            user.id.clone(),
            NotificationType::WorkspaceOwnerless,
            format!("Workspace '{}' has no active Owner", ws.name),
            tenant_id_str.clone(),
            now,
        );
        notif.body = Some(
            serde_json::json!({
                "workspace_id": ws.id.to_string(),
                "workspace_name": ws.name,
                "ownerless_since": since,
                "action": "Reassign ownership via PUT /api/v1/workspaces/{id}/members/{user_id}/role with {\"role\": \"Owner\"}",
            })
            .to_string(),
        );
        // `Notification::new` already set `source` to the workspace entity
        // (entity_type "workspace", entity_id = ws.id) — no extra ref needed.

        match state.create_notification(&notif).await {
            Ok(()) => {
                created += 1;
                // Channel fan-out per user-management.md §Delivery Channels.
                // AppState::create_notification only persists + SSE-broadcasts;
                // email/webhook/Slack fan-out is call-site responsibility
                // (review F1: this was the last production creation path
                // without it — a tenant admin with channels configured
                // received no channel delivery).
                crate::notification_dispatcher::dispatch_to_channels(state, &notif).await;
                info!(
                    workspace_id = %ws.id,
                    tenant_admin = %user.id,
                    "workspace_ownership: TenantAdmin notified of ownerless workspace"
                );
            }
            Err(e) => {
                warn!(
                    workspace_id = %ws.id,
                    tenant_admin = %user.id,
                    "workspace_ownership: failed to create WorkspaceOwnerless notification: {e}"
                );
            }
        }
    }
    created
}

/// Track the ownerless state of one workspace:
///
/// - Has an active Owner → clear any ownerless record (the workspace was
///   reclaimed; ownership was reassigned).
/// - No active Owner and never had one → not tracked (no owner departed;
///   system/agent-operated workspaces are healthy by design).
/// - No active Owner but an Owner membership exists (the owner left) and
///   no record → record `ownerless_since = now` and notify the
///   TenantAdmins.
/// - No active Owner and a record → retry the notification when it has not
///   been created yet; otherwise nothing (the orphan flag is derived from
///   `ownerless_since` + the threshold, not re-recorded).
async fn track_workspace(state: &Arc<AppState>, ws: &Workspace) -> anyhow::Result<()> {
    if has_active_owner(state, &ws.id).await? {
        if ownerless_record(state, &ws.id).await?.is_some() {
            state
                .kv_store
                .kv_remove(OWNERLESS_KV_NAMESPACE, ws.id.as_str())
                .await?;
            info!(
                workspace_id = %ws.id,
                "workspace_ownership: owner present, ownerless record cleared"
            );
        }
        return Ok(());
    }

    // Never owned → no departure ever happened → not ownerless (spec:
    // "when a workspace Owner leaves").
    if !has_ever_had_owner(state, &ws.id).await? {
        return Ok(());
    }

    let now = now_secs();
    match ownerless_record(state, &ws.id).await? {
        Some(mut record) => {
            if record.notified_at.is_none() {
                let created = notify_tenant_admins(state, ws, record.ownerless_since).await;
                if created > 0 {
                    record.notified_at = Some(now);
                    state
                        .kv_store
                        .kv_set(
                            OWNERLESS_KV_NAMESPACE,
                            ws.id.as_str(),
                            serde_json::to_string(&record)?,
                        )
                        .await?;
                }
            }
        }
        None => {
            let record = OwnerlessRecord {
                ownerless_since: now,
                notified_at: None,
            };
            state
                .kv_store
                .kv_set(
                    OWNERLESS_KV_NAMESPACE,
                    ws.id.as_str(),
                    serde_json::to_string(&record)?,
                )
                .await?;
            let created = notify_tenant_admins(state, ws, now).await;
            if created > 0 {
                let record = OwnerlessRecord {
                    ownerless_since: now,
                    notified_at: Some(now),
                };
                state
                    .kv_store
                    .kv_set(
                        OWNERLESS_KV_NAMESPACE,
                        ws.id.as_str(),
                        serde_json::to_string(&record)?,
                    )
                    .await?;
            }
        }
    }
    Ok(())
}

/// One cycle of the `orphaned_workspace_check` background job: track every
/// workspace's ownerless state. Workspaces ownerless longer than the
/// configured threshold are surfaced by [`orphaned_workspaces`].
pub async fn run_once(state: &Arc<AppState>) -> anyhow::Result<()> {
    let workspaces = state.workspaces.list().await?;
    for ws in &workspaces {
        if let Err(e) = track_workspace(state, ws).await {
            warn!(
                workspace_id = %ws.id,
                "workspace_ownership: tracking failed: {e}"
            );
        }
    }
    Ok(())
}

/// Inline check for the user-deletion path (SCIM deactivate = delete): the
/// job runs daily, but the TenantAdmin notification should not wait for the
/// next cycle. Tracks every workspace the removed user was a member of.
pub async fn on_user_removed(state: &Arc<AppState>, user_id: &Id) {
    let memberships = match state.workspace_memberships.list_by_user(user_id).await {
        Ok(m) => m,
        Err(e) => {
            warn!(
                user_id = %user_id,
                "workspace_ownership: failed to list removed user's memberships: {e}"
            );
            return;
        }
    };
    for m in &memberships {
        if let Ok(Some(ws)) = state.workspaces.find_by_id(&m.workspace_id).await {
            if let Err(e) = track_workspace(state, &ws).await {
                warn!(
                    workspace_id = %ws.id,
                    "workspace_ownership: post-removal tracking failed: {e}"
                );
            }
        }
    }
}

/// Clear the ownerless record for a workspace — called when ownership is
/// (re)assigned via the role-change endpoint, so a reclaimed workspace stops
/// being flagged immediately rather than at the next job cycle.
pub async fn clear_ownerless(state: &Arc<AppState>, workspace_id: &Id) {
    if let Err(e) = state
        .kv_store
        .kv_remove(OWNERLESS_KV_NAMESPACE, workspace_id.as_str())
        .await
    {
        warn!(
            workspace_id = %workspace_id,
            "workspace_ownership: failed to clear ownerless record: {e}"
        );
    }
}

/// Workspaces ownerless for at least `threshold_days`, with their
/// `ownerless_since` timestamps (admin dashboard "orphaned" flag).
pub async fn orphaned_workspaces_with(
    state: &Arc<AppState>,
    threshold_days: u64,
) -> anyhow::Result<Vec<OrphanedWorkspace>> {
    let mut out = Vec::new();
    let workspaces = state.workspaces.list().await?;
    let now = now_secs();
    let threshold_secs = threshold_days.saturating_mul(24 * 3600);

    for ws in workspaces {
        let Some(record) = ownerless_record(state, &ws.id).await? else {
            continue;
        };
        if now.saturating_sub(record.ownerless_since) >= threshold_secs {
            out.push(OrphanedWorkspace {
                workspace_id: ws.id.to_string(),
                name: ws.name.clone(),
                tenant_id: ws.tenant_id.to_string(),
                ownerless_since: record.ownerless_since,
                days_ownerless: (now.saturating_sub(record.ownerless_since)) / (24 * 3600),
            });
        }
    }
    Ok(out)
}

/// Workspaces ownerless for at least the configured threshold
/// (`GYRE_ORPHANED_WORKSPACE_DAYS`, default 30).
pub async fn orphaned_workspaces(state: &Arc<AppState>) -> anyhow::Result<Vec<OrphanedWorkspace>> {
    orphaned_workspaces_with(state, orphaned_threshold_days()).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;
    use gyre_common::NotificationPriority;
    use gyre_domain::{GlobalRole, User, Workspace, WorkspaceMembership, WorkspaceRole as Role};

    fn make_workspace(id: &str) -> Workspace {
        Workspace::new(
            Id::new(id),
            Id::new("t1"),
            format!("ws-{id}"),
            format!("ws-{id}"),
            1_000_000,
        )
    }

    fn make_user(id: &str, tenant: &str, global: GlobalRole) -> User {
        let mut u = User::new(
            Id::new(id),
            format!("ext-{id}"),
            format!("user-{id}"),
            1_000_000,
        );
        u.tenant_id = Some(Id::new(tenant));
        u.global_role = global;
        u
    }

    fn make_membership(user_id: &str, ws_id: &str, role: Role) -> WorkspaceMembership {
        WorkspaceMembership::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            Id::new(user_id),
            Id::new(ws_id),
            role,
            Id::new("admin"),
            1_000_000,
        )
    }

    async fn ownerless_notifications(
        state: &Arc<AppState>,
        user_id: &str,
        ws_id: &str,
    ) -> Vec<Notification> {
        state
            .notifications
            .list_for_user(
                &Id::new(user_id),
                Some(&Id::new(ws_id)),
                None,
                None,
                Some(NotificationType::WorkspaceOwnerless.as_str()),
                None,
                50,
                0,
            )
            .await
            .unwrap()
            .into_iter()
            .filter(|n| n.notification_type == NotificationType::WorkspaceOwnerless)
            .collect()
    }

    #[tokio::test]
    async fn ownerless_workspace_notifies_tenant_admins_only() {
        let state = test_state();
        let ws = make_workspace("ws1");
        state.workspaces.create(&ws).await.unwrap();
        // A tenant admin and a plain member of the same tenant, plus a
        // TenantAdmin of a DIFFERENT tenant (must not be notified — tenants
        // are hard isolation boundaries).
        state
            .users
            .create(&make_user("admin1", "t1", GlobalRole::TenantAdmin))
            .await
            .unwrap();
        state
            .users
            .create(&make_user("member1", "t1", GlobalRole::Member))
            .await
            .unwrap();
        state
            .users
            .create(&make_user("admin2", "t2", GlobalRole::TenantAdmin))
            .await
            .unwrap();
        // The workspace's Owner left (user record gone, membership row
        // remains) and a Developer member is still present — the workspace
        // is ownerless by departure, not never-owned.
        state
            .workspace_memberships
            .create(&make_membership("departed-owner", "ws1", Role::Owner))
            .await
            .unwrap();
        state
            .workspace_memberships
            .create(&make_membership("member1", "ws1", Role::Developer))
            .await
            .unwrap();

        run_once(&state).await.unwrap();

        assert_eq!(
            ownerless_notifications(&state, "admin1", "ws1").await.len(),
            1,
            "the workspace's TenantAdmin must be notified"
        );
        assert!(
            ownerless_notifications(&state, "member1", "ws1")
                .await
                .is_empty(),
            "plain members are not notified"
        );
        assert!(
            ownerless_notifications(&state, "admin2", "ws1")
                .await
                .is_empty(),
            "TenantAdmins of other tenants must not be notified"
        );
        // The notification is priority rank 2 (needs human action).
        let n = &ownerless_notifications(&state, "admin1", "ws1").await[0];
        assert_eq!(n.priority_rank, 2);
        assert_eq!(n.priority, NotificationPriority::Urgent);
    }

    /// Emitter-path guard (review F1): the ownerless flow's in-app record
    /// must fan out to the recipient's configured channels — a tenant admin
    /// with an email channel configured gets an outbox entry when the job
    /// runs. Fails if notify_tenant_admins persists without
    /// dispatch_to_channels (AppState::create_notification only
    /// SSE-broadcasts; channel fan-out is call-site responsibility).
    #[tokio::test]
    async fn ownerless_notification_fans_out_to_configured_channels() {
        let state = test_state();
        let ws = make_workspace("ws-chan");
        state.workspaces.create(&ws).await.unwrap();
        state
            .users
            .create(&make_user("admin-chan", "t1", GlobalRole::TenantAdmin))
            .await
            .unwrap();
        // The workspace's Owner left (membership row remains, user record
        // gone) → ownerless by departure.
        state
            .workspace_memberships
            .create(&make_membership("departed-owner", "ws-chan", Role::Owner))
            .await
            .unwrap();
        // Email channel configured (Immediate, all bands).
        state
            .user_channel_prefs
            .upsert(
                &Id::new("admin-chan"),
                &gyre_domain::NotificationChannels {
                    in_app: true,
                    email: gyre_domain::EmailConfig {
                        enabled: true,
                        digest: gyre_domain::DigestFrequency::Immediate,
                        min_priority: gyre_domain::NotificationPriority::Low,
                        type_overrides: Default::default(),
                    },
                    webhook: None,
                    slack: None,
                },
            )
            .await
            .unwrap();

        run_once(&state).await.unwrap();

        // In-app record exists …
        assert_eq!(
            ownerless_notifications(&state, "admin-chan", "ws-chan")
                .await
                .len(),
            1,
            "the tenant admin must get the in-app WorkspaceOwnerless record"
        );
        // … and the configured email channel fired (WorkspaceOwnerless is
        // Urgent — above any threshold).
        let outbox = state
            .kv_store
            .kv_list(crate::notification_dispatcher::EMAIL_OUTBOX_NS)
            .await
            .unwrap();
        assert!(
            outbox.iter().any(|(k, _)| k.starts_with("admin-chan:")),
            "ownerless notification must fan out to the configured email channel"
        );
    }

    #[tokio::test]
    async fn workspace_with_active_owner_is_not_tracked() {
        let state = test_state();
        let ws = make_workspace("ws2");
        state.workspaces.create(&ws).await.unwrap();
        state
            .users
            .create(&make_user("owner2", "t1", GlobalRole::Member))
            .await
            .unwrap();
        state
            .workspace_memberships
            .create(&make_membership("owner2", "ws2", Role::Owner))
            .await
            .unwrap();

        run_once(&state).await.unwrap();

        assert!(
            ownerless_record(&state, &Id::new("ws2"))
                .await
                .unwrap()
                .is_none(),
            "a workspace with an active Owner must not be tracked as ownerless"
        );
    }

    #[tokio::test]
    async fn owner_membership_of_deleted_user_is_not_active() {
        let state = test_state();
        let ws = make_workspace("ws3");
        state.workspaces.create(&ws).await.unwrap();
        state
            .users
            .create(&make_user("admin3", "t1", GlobalRole::TenantAdmin))
            .await
            .unwrap();
        // Owner membership whose user record does not exist (deactivated /
        // removed from the tenant).
        state
            .workspace_memberships
            .create(&make_membership("ghost", "ws3", Role::Owner))
            .await
            .unwrap();

        assert!(
            !has_active_owner(&state, &Id::new("ws3")).await.unwrap(),
            "an Owner membership whose user is gone is not an active Owner"
        );

        run_once(&state).await.unwrap();
        assert!(
            ownerless_record(&state, &Id::new("ws3"))
                .await
                .unwrap()
                .is_some(),
            "the workspace must be tracked as ownerless"
        );
    }

    #[tokio::test]
    async fn never_owned_workspace_is_not_tracked_or_notified() {
        // user-management.md §Ownership Transfer & Reclamation frames the
        // ownerless flow as "when a workspace Owner leaves". A workspace
        // with no Owner membership at all — e.g. one created by the system
        // principal or an agent, which hold no user identity — was never
        // owned and must not be flagged or generate TenantAdmin
        // notification noise.
        let state = test_state();
        let ws = make_workspace("ws8");
        state.workspaces.create(&ws).await.unwrap();
        state
            .users
            .create(&make_user("admin8", "t1", GlobalRole::TenantAdmin))
            .await
            .unwrap();
        // A Developer membership alone is not evidence an owner existed.
        state
            .workspace_memberships
            .create(&make_membership("member8", "ws8", Role::Developer))
            .await
            .unwrap();

        run_once(&state).await.unwrap();

        assert!(
            ownerless_record(&state, &Id::new("ws8"))
                .await
                .unwrap()
                .is_none(),
            "a never-owned workspace must not be recorded as ownerless"
        );
        assert!(
            ownerless_notifications(&state, "admin8", "ws8")
                .await
                .is_empty(),
            "a never-owned workspace must not notify TenantAdmins"
        );
    }

    #[tokio::test]
    async fn job_re_runs_do_not_duplicate_notifications() {
        let state = test_state();
        let ws = make_workspace("ws4");
        state.workspaces.create(&ws).await.unwrap();
        state
            .users
            .create(&make_user("admin4", "t1", GlobalRole::TenantAdmin))
            .await
            .unwrap();
        // The workspace's Owner departed (membership row remains, user
        // record gone).
        state
            .workspace_memberships
            .create(&make_membership("departed-owner", "ws4", Role::Owner))
            .await
            .unwrap();

        run_once(&state).await.unwrap();
        run_once(&state).await.unwrap();
        run_once(&state).await.unwrap();

        assert_eq!(
            ownerless_notifications(&state, "admin4", "ws4").await.len(),
            1,
            "active WorkspaceOwnerless notifications must not stack"
        );
    }

    #[tokio::test]
    async fn orphaned_flag_requires_threshold_days() {
        let state = test_state();
        let ws = make_workspace("ws5");
        state.workspaces.create(&ws).await.unwrap();

        // Seed an ownerless record 40 days old.
        let now = now_secs();
        let record = OwnerlessRecord {
            ownerless_since: now - 40 * 24 * 3600,
            notified_at: Some(now),
        };
        state
            .kv_store
            .kv_set(
                OWNERLESS_KV_NAMESPACE,
                "ws5",
                serde_json::to_string(&record).unwrap(),
            )
            .await
            .unwrap();

        // 30-day threshold (spec default): 40 days ownerless → flagged.
        let flagged = orphaned_workspaces_with(&state, 30).await.unwrap();
        assert_eq!(flagged.len(), 1);
        assert_eq!(flagged[0].workspace_id, "ws5");
        assert!(flagged[0].days_ownerless >= 40);

        // 60-day threshold: not yet flagged.
        let flagged = orphaned_workspaces_with(&state, 60).await.unwrap();
        assert!(
            flagged.is_empty(),
            "a workspace ownerless for 40 days must not be flagged under a 60-day threshold"
        );
    }

    #[tokio::test]
    async fn owner_reassignment_clears_tracking() {
        let state = test_state();
        let ws = make_workspace("ws6");
        state.workspaces.create(&ws).await.unwrap();
        state
            .users
            .create(&make_user("admin6", "t1", GlobalRole::TenantAdmin))
            .await
            .unwrap();
        // The workspace's Owner departed → ownerless.
        state
            .workspace_memberships
            .create(&make_membership("departed-owner", "ws6", Role::Owner))
            .await
            .unwrap();

        // Workspace is ownerless by departure → tracked.
        run_once(&state).await.unwrap();
        assert!(ownerless_record(&state, &Id::new("ws6"))
            .await
            .unwrap()
            .is_some());

        // Ownership is reassigned to an existing user.
        state
            .users
            .create(&make_user("newowner6", "t1", GlobalRole::Member))
            .await
            .unwrap();
        state
            .workspace_memberships
            .create(&make_membership("newowner6", "ws6", Role::Owner))
            .await
            .unwrap();
        clear_ownerless(&state, &Id::new("ws6")).await;

        assert!(
            ownerless_record(&state, &Id::new("ws6"))
                .await
                .unwrap()
                .is_none(),
            "reassignment must clear the ownerless record"
        );

        // And the job keeps it clean (no re-tracking).
        run_once(&state).await.unwrap();
        assert!(ownerless_record(&state, &Id::new("ws6"))
            .await
            .unwrap()
            .is_none());
        assert!(
            orphaned_workspaces_with(&state, 0)
                .await
                .unwrap()
                .is_empty(),
            "a reclaimed workspace must not appear in the orphaned list"
        );
    }

    #[tokio::test]
    async fn user_removal_triggers_immediate_tracking() {
        let state = test_state();
        let ws = make_workspace("ws7");
        state.workspaces.create(&ws).await.unwrap();
        state
            .users
            .create(&make_user("admin7", "t1", GlobalRole::TenantAdmin))
            .await
            .unwrap();
        let owner = make_user("owner7", "t1", GlobalRole::Member);
        state.users.create(&owner).await.unwrap();
        state
            .workspace_memberships
            .create(&make_membership("owner7", "ws7", Role::Owner))
            .await
            .unwrap();

        // The workspace is healthy before the removal.
        run_once(&state).await.unwrap();
        assert!(ownerless_record(&state, &Id::new("ws7"))
            .await
            .unwrap()
            .is_none());

        // The owner's account is removed (SCIM deactivate).
        state.users.delete(&owner.id).await.unwrap();
        on_user_removed(&state, &owner.id).await;

        // The transition is recorded and the TenantAdmin notified without
        // waiting for the daily job.
        assert!(ownerless_record(&state, &Id::new("ws7"))
            .await
            .unwrap()
            .is_some());
        assert_eq!(
            ownerless_notifications(&state, "admin7", "ws7").await.len(),
            1,
            "the TenantAdmin must be notified immediately on owner removal"
        );
    }
}
