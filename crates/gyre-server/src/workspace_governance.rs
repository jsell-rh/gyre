//! Workspace ownership governance (user-management.md §Ownership Transfer &
//! Reclamation, §Repo Access).
//!
//! Owns:
//! - last-Owner protection helpers (cannot remove or downgrade the final
//!   Owner of a workspace; a voluntary transfer must designate the new Owner
//!   first);
//! - membership-based authorization for member management and repo access
//!   (workspace role → repo permission table, per-repo overrides that can
//!   restrict but never expand);
//! - Tenant Admin notification when a workspace has no active Owner;
//! - the orphaned-workspace detection background job (ownerless workspaces
//!   are flagged after a configurable period, default 30 days).

use std::sync::Arc;

use gyre_common::{Id, NotificationType};
use gyre_domain::{
    effective_repo_role, GlobalRole, RepoPermission, Repository, WorkspaceMembership, WorkspaceRole,
};
use gyre_domain::{AuditEvent, AuditEventType, AuditOutcome};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::auth::AuthenticatedAgent;
use crate::{notifications, AppState};

use super::api::error::ApiError;

/// KV namespace holding per-workspace governance state (ownerless tracking).
pub const GOVERNANCE_KV_NAMESPACE: &str = "workspace_governance";

/// Default period (days) after which an ownerless workspace is flagged
/// orphaned in the admin dashboard (user-management.md §Ownership Transfer &
/// Reclamation: "default: 30 days").
pub const DEFAULT_ORPHAN_PERIOD_DAYS: u64 = 30;

/// Scan interval (seconds) for the orphaned-workspace detection job.
pub const DETECTION_INTERVAL_SECS: u64 = 3600;

/// The orphan flagging period in days, configurable via
/// `GYRE_ORPHANED_WORKSPACE_DAYS`.
pub fn orphan_period_days() -> u64 {
    std::env::var("GYRE_ORPHANED_WORKSPACE_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_ORPHAN_PERIOD_DAYS)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------------------
// Governance KV state
// ---------------------------------------------------------------------------

/// Durable per-workspace governance state, stored via the KV port so it
/// survives restarts (never a bare in-process map — see
/// check-in-memory-state-stores.sh).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GovernanceState {
    /// When the workspace was first detected with no active Owner.
    pub ownerless_since: Option<u64>,
    /// When Tenant Admins were last notified about the missing Owner.
    pub last_notified_at: Option<u64>,
    /// When the workspace was flagged orphaned (ownerless ≥ period).
    pub orphaned_since: Option<u64>,
}

async fn load_governance_state(state: &AppState, workspace_id: &Id) -> GovernanceState {
    state
        .kv_store
        .kv_get(GOVERNANCE_KV_NAMESPACE, workspace_id.as_str())
        .await
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

async fn save_governance_state(state: &AppState, workspace_id: &Id, gs: &GovernanceState) {
    if let Err(e) = state
        .kv_store
        .kv_set(
            GOVERNANCE_KV_NAMESPACE,
            workspace_id.as_str(),
            serde_json::to_string(gs).unwrap_or_default(),
        )
        .await
    {
        warn!(workspace_id = %workspace_id, error = %e, "failed to persist workspace governance state");
    }
}

// ---------------------------------------------------------------------------
// Owner accounting
// ---------------------------------------------------------------------------

/// Count active Owner memberships in a workspace. A membership is active
/// once accepted; a pending Owner invitation does not exercise ownership.
pub async fn active_owner_count(state: &AppState, workspace_id: &Id) -> anyhow::Result<usize> {
    let members = state
        .workspace_memberships
        .list_by_workspace(workspace_id)
        .await?;
    Ok(members
        .iter()
        .filter(|m| m.role == WorkspaceRole::Owner && m.is_active())
        .count())
}

/// Notify every Tenant Admin of the workspace's tenant that the workspace
/// has no active Owner (user-management.md §Ownership Transfer & Reclamation
/// step 1). Fire-and-forget: a notification is a courtesy side effect and
/// must not fail the triggering request.
pub async fn notify_tenant_admins_no_owner(
    state: &AppState,
    workspace: &gyre_domain::Workspace,
    reason: &str,
) {
    let tenant_id = workspace.tenant_id.clone();
    let admins: Vec<_> = match state.users.list().await {
        Ok(users) => users
            .into_iter()
            .filter(|u| {
                u.tenant_id.as_ref() == Some(&tenant_id) && u.global_role == GlobalRole::TenantAdmin
            })
            .collect(),
        Err(e) => {
            warn!(workspace_id = %workspace.id, error = %e, "failed to list users for ownerless notification");
            return;
        }
    };
    if admins.is_empty() {
        warn!(
            workspace_id = %workspace.id,
            tenant_id = %workspace.tenant_id,
            "workspace has no active Owner but tenant has no TenantAdmin to notify"
        );
        return;
    }
    let admin_count = admins.len();
    for admin in &admins {
        notifications::notify_rich(
            state,
            workspace.id.clone(),
            admin.id.clone(),
            NotificationType::WorkspaceOwnerless,
            format!("Workspace '{}' has no active Owner", workspace.name),
            workspace.tenant_id.to_string(),
            Some(
                serde_json::json!({
                    "workspace_id": workspace.id.to_string(),
                    "workspace_name": workspace.name,
                    "reason": reason,
                })
                .to_string(),
            ),
            Some(workspace.id.to_string()),
            None,
        )
        .await;
    }
    info!(
        workspace_id = %workspace.id,
        admins = admin_count,
        "notified Tenant Admins: workspace has no active Owner"
    );
}

/// Record the ownerless transition for a workspace that currently has no
/// active Owner: stamps `ownerless_since` (first detection) and notifies the
/// Tenant Admins immediately. The orphan-flagging after the configured
/// period is handled by the detection job.
pub async fn note_workspace_ownerless(state: &AppState, workspace: &gyre_domain::Workspace) {
    match active_owner_count(state, &workspace.id).await {
        Ok(n) if n > 0 => {}
        Ok(_) => {
            let mut gs = load_governance_state(state, &workspace.id).await;
            if gs.ownerless_since.is_none() {
                let now = now_secs();
                gs.ownerless_since = Some(now);
                gs.last_notified_at = Some(now);
                save_governance_state(state, &workspace.id, &gs).await;
                notify_tenant_admins_no_owner(state, workspace, "owner-left").await;
            }
        }
        Err(e) => {
            warn!(workspace_id = %workspace.id, error = %e, "failed to count owners for ownerless check");
        }
    }
}

/// Audit-log a membership governance action (role change, ownership transfer,
/// member removal) with the previous state and reason
/// (user-management.md §Ownership Transfer & Reclamation step 4).
pub async fn record_membership_audit(
    state: &AppState,
    auth: &AuthenticatedAgent,
    workspace_id: &Id,
    event_kind: &str,
    detail: serde_json::Value,
) {
    let event = AuditEvent::new(
        Id::new(uuid::Uuid::new_v4().to_string()),
        AuditEventType::Custom(event_kind.to_string()),
        None,
        Some(Id::new(auth.agent_id.clone())),
        None,
        Some(workspace_id.clone()),
        None,
        "workspace_member".to_string(),
        None,
        AuditOutcome::Success,
        detail,
        None,
        None,
        now_secs(),
    );
    if let Err(e) = state.audit.record(&event).await {
        warn!(workspace_id = %workspace_id, error = %e, "failed to record membership audit event");
    }
}

// ---------------------------------------------------------------------------
// Membership-based authorization
// ---------------------------------------------------------------------------

/// The caller's active membership in a workspace, if any.
/// Pending (unaccepted) invitations do not authorize anything
/// (user-management.md §Workspace Invitation Flow).
pub async fn caller_membership(
    state: &AppState,
    auth: &AuthenticatedAgent,
    workspace_id: &Id,
) -> Option<WorkspaceMembership> {
    let user_id = auth
        .user_id
        .clone()
        .unwrap_or_else(|| Id::new(auth.agent_id.clone()));
    match state
        .workspace_memberships
        .find_by_user_and_workspace(&user_id, workspace_id)
        .await
    {
        Ok(Some(m)) if m.is_active() => Some(m),
        _ => None,
    }
}

/// Require the caller to hold at least `min_role` in the workspace.
///
/// Allowed paths:
/// - system token (superuser escape hatch, consistent with ABAC middleware);
/// - global Admin role (tenant administration);
/// - an active workspace membership with role ≥ `min_role`;
/// - bootstrap: a workspace with zero active memberships can be joined by
///   any authenticated caller (the first member; workspace creation seeds
///   the creator as Owner, so this only covers externally-created rows).
pub async fn require_workspace_role(
    state: &AppState,
    auth: &AuthenticatedAgent,
    workspace_id: &Id,
    min_role: WorkspaceRole,
) -> Result<Option<WorkspaceMembership>, ApiError> {
    if auth.agent_id == "system" {
        return Ok(None);
    }
    if auth.roles.contains(&gyre_domain::UserRole::Admin) {
        return Ok(None);
    }
    if let Some(m) = caller_membership(state, auth, workspace_id).await {
        if m.role.rank() >= min_role.rank() {
            return Ok(Some(m));
        }
        return Err(ApiError::Forbidden(format!(
            "workspace role {} is not permitted to perform this action (requires {})",
            m.role.as_str(),
            min_role.as_str()
        )));
    }
    // Bootstrap: no active members at all — the first member can be added.
    let members = state
        .workspace_memberships
        .list_by_workspace(workspace_id)
        .await
        .map_err(ApiError::Internal)?;
    if !members.iter().any(|m| m.is_active()) {
        return Ok(None);
    }
    Err(ApiError::Forbidden(
        "caller is not an active member of this workspace".to_string(),
    ))
}

/// Effective repo role for a member: the more restrictive of their workspace
/// role and any per-repo override (user-management.md §Repo Access).
pub async fn effective_role_for_repo(
    state: &AppState,
    membership: &WorkspaceMembership,
    repo: &Repository,
) -> WorkspaceRole {
    match state
        .repo_access_overrides
        .find_by_repo_and_user(&repo.id, &membership.user_id)
        .await
    {
        Ok(Some(o)) => effective_repo_role(membership.role, o.role),
        _ => membership.role,
    }
}

/// Check a repo permission for the caller, derived from workspace membership
/// per the user-management.md §Repo Access table, with per-repo overrides
/// applied (restrict-only).
///
/// Bypass paths (documented, not silent):
/// - system token — superuser escape hatch;
/// - global Admin role — tenant administration;
/// - Agent role — agents are authorized by the spawn-time scoping and the
///   per-repo ABAC policy machinery (abac-policy-engine.md), not by human
///   workspace membership.
pub async fn check_repo_access_scope(
    state: &AppState,
    repo: &Repository,
    auth: &AuthenticatedAgent,
    perm: RepoPermission,
) -> Result<(), ApiError> {
    if auth.agent_id == "system" {
        return Ok(());
    }
    if auth.roles.contains(&gyre_domain::UserRole::Admin) {
        return Ok(());
    }
    if auth.roles.contains(&gyre_domain::UserRole::Agent) {
        return Ok(());
    }
    let membership = caller_membership(state, auth, &repo.workspace_id)
        .await
        .ok_or_else(|| {
            ApiError::Forbidden("caller is not an active member of this workspace".to_string())
        })?;
    let effective = effective_role_for_repo(state, &membership, repo).await;
    if effective.allows_repo_permission(perm) {
        Ok(())
    } else {
        Err(ApiError::Forbidden(format!(
            "workspace role {} does not grant repo permission '{}'",
            effective.as_str(),
            perm.as_str()
        )))
    }
}

/// Check a workspace-level permission (no repo/override dimension yet, e.g.
/// creating a repo inside the workspace).
pub async fn check_workspace_membership_scope(
    state: &AppState,
    workspace_id: &Id,
    auth: &AuthenticatedAgent,
    perm: RepoPermission,
) -> Result<(), ApiError> {
    if auth.agent_id == "system" {
        return Ok(());
    }
    if auth.roles.contains(&gyre_domain::UserRole::Admin) {
        return Ok(());
    }
    if auth.roles.contains(&gyre_domain::UserRole::Agent) {
        return Ok(());
    }
    let membership = caller_membership(state, auth, workspace_id)
        .await
        .ok_or_else(|| {
            ApiError::Forbidden("caller is not an active member of this workspace".to_string())
        })?;
    if membership.role.allows_repo_permission(perm) {
        Ok(())
    } else {
        Err(ApiError::Forbidden(format!(
            "workspace role {} does not grant permission '{}'",
            membership.role.as_str(),
            perm.as_str()
        )))
    }
}

// ---------------------------------------------------------------------------
// Orphaned-workspace detection job
// ---------------------------------------------------------------------------

/// One pass of the orphaned-workspace detection: for every workspace with no
/// active Owner, stamp the ownerless transition (notifying Tenant Admins on
/// first detection) and flag the workspace orphaned once the configured
/// period (default 30 days) has elapsed. Workspaces that regain an Owner
/// have their governance state cleared.
pub async fn run_once(state: &Arc<AppState>) -> anyhow::Result<()> {
    let workspaces = state.workspaces.list().await?;
    let period_secs = orphan_period_days() * 24 * 3600;
    let now = now_secs();

    for ws in &workspaces {
        if let Err(e) = evaluate_workspace(state, ws, now, period_secs).await {
            warn!(workspace_id = %ws.id, error = %e, "orphaned_workspace_detector: evaluation failed");
        }
    }
    Ok(())
}

async fn evaluate_workspace(
    state: &Arc<AppState>,
    ws: &gyre_domain::Workspace,
    now: u64,
    period_secs: u64,
) -> anyhow::Result<()> {
    let owners = active_owner_count(state, &ws.id).await?;

    if owners > 0 {
        // Workspace has an active Owner: clear any stale governance state so
        // a re-owned workspace is never reported orphaned.
        let gs = load_governance_state(state, &ws.id).await;
        if gs.ownerless_since.is_some() {
            save_governance_state(state, &ws.id, &GovernanceState::default()).await;
            info!(workspace_id = %ws.id, "workspace regained an active Owner; cleared orphan tracking");
        }
        return Ok(());
    }

    let mut gs = load_governance_state(state, &ws.id).await;
    match gs.ownerless_since {
        None => {
            // First detection: stamp and notify the Tenant Admins.
            gs.ownerless_since = Some(now);
            gs.last_notified_at = Some(now);
            save_governance_state(state, &ws.id, &gs).await;
            notify_tenant_admins_no_owner(state, ws, "no-active-owner-detected").await;
        }
        Some(since) => {
            let elapsed = now.saturating_sub(since);
            if gs.orphaned_since.is_none() && elapsed >= period_secs {
                gs.orphaned_since = Some(now);
                gs.last_notified_at = Some(now);
                save_governance_state(state, &ws.id, &gs).await;
                notify_tenant_admins_no_owner(state, ws, "orphaned").await;
                info!(
                    workspace_id = %ws.id,
                    ownerless_days = elapsed / 86400,
                    "workspace flagged orphaned (no active Owner beyond the configured period)"
                );
            }
        }
    }
    Ok(())
}

/// Whether a workspace is currently flagged orphaned: either the detection
/// job set `orphaned_since`, or the ownerless period has demonstrably
/// elapsed since `ownerless_since`.
pub fn is_orphaned(gs: &GovernanceState, now: u64, period_secs: u64) -> bool {
    match gs.ownerless_since {
        Some(since) => gs.orphaned_since.is_some() || now.saturating_sub(since) >= period_secs,
        None => false,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;
    use gyre_domain::{User, Workspace};

    fn ws(id: &str) -> Workspace {
        Workspace::new(
            Id::new(id),
            Id::new("tenant-1"),
            "Test Workspace",
            "test-workspace",
            1000,
        )
    }

    fn member(
        ws_id: &str,
        user_id: &str,
        role: WorkspaceRole,
        accepted: bool,
    ) -> WorkspaceMembership {
        let mut m = WorkspaceMembership::new(
            Id::new(format!("m-{user_id}")),
            Id::new(user_id),
            Id::new(ws_id),
            role,
            Id::new("inviter"),
            1000,
        );
        if accepted {
            m.accept(1500);
        }
        m
    }

    fn tenant_admin(id: &str) -> User {
        let mut u = User::new_sso(Id::new(id), "ext", id, "Admin User", 1000);
        u.tenant_id = Some(Id::new("tenant-1"));
        u.global_role = GlobalRole::TenantAdmin;
        u
    }

    #[tokio::test]
    async fn ownerless_workspace_notifies_tenant_admins_once() {
        let state = test_state();
        let workspace = ws("ws-a");
        state.workspaces.create(&workspace).await.unwrap();
        state.users.create(&tenant_admin("admin-1")).await.unwrap();
        // A pending (unaccepted) Owner invitation does not exercise ownership.
        state
            .workspace_memberships
            .create(&member("ws-a", "ghost-owner", WorkspaceRole::Owner, false))
            .await
            .unwrap();

        run_once(&state).await.unwrap();

        let gs = load_governance_state(&state, &workspace.id).await;
        assert!(gs.ownerless_since.is_some(), "ownerless transition stamped");
        let notifs = state
            .notifications
            .list_for_user(&Id::new("admin-1"), None, None, None, None, 100, 0)
            .await
            .unwrap();
        assert_eq!(
            notifs.len(),
            1,
            "Tenant Admin notified exactly once: {notifs:?}"
        );
        assert_eq!(
            notifs[0].notification_type,
            NotificationType::WorkspaceOwnerless
        );

        // Second pass must not re-notify while inside the period.
        run_once(&state).await.unwrap();
        let notifs = state
            .notifications
            .list_for_user(&Id::new("admin-1"), None, None, None, None, 100, 0)
            .await
            .unwrap();
        assert_eq!(
            notifs.len(),
            1,
            "no duplicate notification within the period"
        );
    }

    #[tokio::test]
    async fn owned_workspace_is_not_flagged() {
        let state = test_state();
        let workspace = ws("ws-b");
        state.workspaces.create(&workspace).await.unwrap();
        state
            .workspace_memberships
            .create(&member("ws-b", "owner-1", WorkspaceRole::Owner, true))
            .await
            .unwrap();

        run_once(&state).await.unwrap();

        let gs = load_governance_state(&state, &workspace.id).await;
        assert!(
            gs.ownerless_since.is_none(),
            "owned workspace must not be tracked"
        );
    }

    #[tokio::test]
    async fn ownerless_beyond_period_is_flagged_orphaned() {
        let state = test_state();
        let workspace = ws("ws-c");
        state.workspaces.create(&workspace).await.unwrap();
        state.users.create(&tenant_admin("admin-2")).await.unwrap();

        // Pre-seed governance state: ownerless for 31 days already.
        let mut gs = GovernanceState {
            ownerless_since: Some(1_000_000),
            last_notified_at: Some(1_000_000),
            orphaned_since: None,
        };
        // 31 days later than ownerless_since.
        let now = gs.ownerless_since.unwrap() + 31 * 24 * 3600;
        state
            .kv_store
            .kv_set(
                GOVERNANCE_KV_NAMESPACE,
                workspace.id.as_str(),
                serde_json::to_string(&gs).unwrap(),
            )
            .await
            .unwrap();

        evaluate_workspace(&state, &workspace, now, 30 * 24 * 3600)
            .await
            .unwrap();

        gs = load_governance_state(&state, &workspace.id).await;
        assert!(
            gs.orphaned_since.is_some(),
            "workspace flagged orphaned after 30 days"
        );
        assert!(is_orphaned(&gs, now, 30 * 24 * 3600));
        // Orphaning re-notifies the Tenant Admins.
        let notifs = state
            .notifications
            .list_for_user(&Id::new("admin-2"), None, None, None, None, 100, 0)
            .await
            .unwrap();
        assert_eq!(notifs.len(), 1, "orphaning must notify Tenant Admins");

        // Inside the period: not orphaned.
        assert!(!is_orphaned(
            &GovernanceState {
                ownerless_since: Some(1_000_000),
                last_notified_at: None,
                orphaned_since: None,
            },
            1_000_000 + 29 * 24 * 3600,
            30 * 24 * 3600
        ));
    }

    #[tokio::test]
    async fn regained_owner_clears_tracking() {
        let state = test_state();
        let workspace = ws("ws-d");
        state.workspaces.create(&workspace).await.unwrap();
        state
            .kv_store
            .kv_set(
                GOVERNANCE_KV_NAMESPACE,
                workspace.id.as_str(),
                serde_json::to_string(&GovernanceState {
                    ownerless_since: Some(500),
                    last_notified_at: Some(500),
                    orphaned_since: Some(900),
                })
                .unwrap(),
            )
            .await
            .unwrap();
        state
            .workspace_memberships
            .create(&member("ws-d", "new-owner", WorkspaceRole::Owner, true))
            .await
            .unwrap();

        run_once(&state).await.unwrap();

        let gs = load_governance_state(&state, &workspace.id).await;
        assert!(gs.ownerless_since.is_none() && gs.orphaned_since.is_none());
    }
}
