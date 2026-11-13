//! Trust suggestion background job (HSI §2).
//!
//! Runs daily per workspace. Queries the last 30 days of gate results and MR
//! reverts. When the workspace has 0 gate failures and 0 reverted MRs in that
//! window and the current trust level can still escalate (is not already
//! `Autonomous`), it creates a priority-8 `TrustSuggestion` notification for
//! each Admin/Owner member of the workspace suggesting the next trust level
//! up the ladder (Supervised → Guided → Autonomous). The human decides.
//!
//! Re-creation is suppressed when:
//!   - The user dismissed a TrustSuggestion within the last 30 days (per-user
//!     cooldown, `NotificationRepository::has_recent_dismissal`), or
//!   - An active (unresolved, undismissed) TrustSuggestion already exists for
//!     that (workspace, user) pair.

use std::sync::Arc;

use gyre_common::{Id, Notification, NotificationType};
use gyre_domain::{GateStatus, MrStatus, TrustLevel, WorkspaceRole};
use tracing::{debug, info, warn};

use crate::AppState;

/// Seconds in 30 days.
const THIRTY_DAYS_SECS: u64 = 30 * 24 * 3600;

/// Run one pass of the trust suggestion job across all workspaces.
pub async fn run_once(state: &Arc<AppState>) -> anyhow::Result<()> {
    let workspaces = state.workspaces.list().await?;
    debug!(
        count = workspaces.len(),
        "trust_suggestion: evaluating workspaces"
    );

    for ws in &workspaces {
        if let Err(e) = evaluate_workspace(state, ws).await {
            warn!(workspace_id = %ws.id, error = %e, "trust_suggestion: workspace evaluation failed");
        }
    }
    Ok(())
}

/// Evaluate a single workspace and create TrustSuggestion notifications if warranted.
async fn evaluate_workspace(
    state: &Arc<AppState>,
    ws: &gyre_domain::Workspace,
) -> anyhow::Result<()> {
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let window_start = now_secs.saturating_sub(THIRTY_DAYS_SECS);

    // Autonomous is the top of the trust ladder — nothing to suggest. Custom is
    // off the ladder entirely: there is no next preset level to suggest.
    let suggested_level = match ws.trust_level {
        TrustLevel::Supervised => TrustLevel::Guided,
        TrustLevel::Guided => TrustLevel::Autonomous,
        TrustLevel::Autonomous | TrustLevel::Custom => {
            debug!(
                workspace = %ws.slug,
                trust_level = %ws.trust_level,
                "trust_suggestion: trust level cannot escalate"
            );
            return Ok(());
        }
    };

    // HSI §2: suggest escalation only on a clean 30-day record — 0 gate
    // failures and 0 reverted MRs.
    let gate_failures = count_gate_failures(state, ws, window_start).await?;
    let mr_reverts = count_mr_reverts(state, ws, window_start).await?;

    if gate_failures > 0 || mr_reverts > 0 {
        debug!(
            workspace = %ws.slug,
            gate_failures,
            mr_reverts,
            "trust_suggestion: no escalation warranted"
        );
        return Ok(());
    }

    info!(
        workspace = %ws.slug,
        current = %ws.trust_level,
        suggested = %suggested_level,
        "trust_suggestion: escalation warranted"
    );

    // Notify Admin/Owner members who haven't recently dismissed this suggestion.
    let members = state
        .workspace_memberships
        .list_by_workspace(&ws.id)
        .await?;

    let now_i64 = now_secs as i64;

    for member in &members {
        if !matches!(member.role, WorkspaceRole::Admin | WorkspaceRole::Owner) {
            continue;
        }

        // Suppress if user dismissed a TrustSuggestion for this workspace in the last 30 days.
        let recently_dismissed = state
            .notifications
            .has_recent_dismissal(
                &ws.id,
                &member.user_id,
                NotificationType::TrustSuggestion.as_str(),
                30,
            )
            .await
            .unwrap_or(false);

        if recently_dismissed {
            debug!(
                workspace = %ws.slug,
                user = %member.user_id,
                "trust_suggestion: suppressed (recently dismissed)"
            );
            continue;
        }

        // Suppress if an active TrustSuggestion already exists for this user/workspace.
        let existing = state
            .notifications
            .list_for_user(
                &member.user_id,
                Some(&ws.id),
                Some(8),
                Some(8),
                None,
                None,
                &[],
                20,
                0,
            )
            .await
            .unwrap_or_default();

        let already_active = existing
            .iter()
            .any(|n| n.notification_type == NotificationType::TrustSuggestion && n.is_active());

        if already_active {
            debug!(
                workspace = %ws.slug,
                user = %member.user_id,
                "trust_suggestion: suppressed (active suggestion exists)"
            );
            continue;
        }

        // Create the notification (HSI §2): priority 8, one per Admin member,
        // workspace-scoped (repo_id None).
        let id = Id::new(uuid::Uuid::new_v4().to_string());
        let mut notif = Notification::new(
            id,
            ws.id.clone(),
            member.user_id.clone(),
            NotificationType::TrustSuggestion,
            "Consider increasing trust level",
            ws.tenant_id.to_string(),
            now_i64,
        );
        notif.body = Some(
            serde_json::json!({
                "gate_failures": gate_failures,
                "mr_reverts": mr_reverts,
                "period_days": 30,
                "current_level": ws.trust_level.to_string(),
                "suggested_level": suggested_level.to_string(),
            })
            .to_string(),
        );
        notif.source.entity_type = "workspace".to_string();
        notif.source.entity_id = ws.id.to_string();

        if let Err(e) = state.create_notification(&notif).await {
            warn!(
                workspace = %ws.slug,
                user = %member.user_id,
                error = %e,
                "trust_suggestion: failed to create notification"
            );
        } else {
            info!(
                workspace = %ws.slug,
                user = %member.user_id,
                "trust_suggestion: notification created"
            );
        }
    }

    Ok(())
}

/// Count gate failures for the workspace within the window (HSI §2: gate
/// results for the last 30 days whose status is not `Passed` —
/// a non-passing result, including an in-flight Pending/Running one, is
/// conservative evidence against escalating trust).
///
/// Gate results are scoped per-MR, so the workspace window is the union of
/// results across all of the workspace's MRs; each result's own timestamp
/// (finish time, falling back to start time for still-running gates) decides
/// whether it falls inside the window.
async fn count_gate_failures(
    state: &Arc<AppState>,
    ws: &gyre_domain::Workspace,
    window_start: u64,
) -> anyhow::Result<u64> {
    let mrs = state.merge_requests.list_by_workspace(&ws.id).await?;

    let mut failures = 0u64;
    for mr in &mrs {
        let results = state.gate_results.list_by_mr_id(mr.id.as_str()).await?;
        for result in &results {
            let ts = result.finished_at.or(result.started_at);
            if ts.is_some_and(|t| t >= window_start) && result.status != GateStatus::Passed {
                failures += 1;
            }
        }
    }
    Ok(failures)
}

/// Count MR reverts for the workspace within the window (HSI §2: MRs with
/// status `Reverted` in the last 30 days, by revert time).
async fn count_mr_reverts(
    state: &Arc<AppState>,
    ws: &gyre_domain::Workspace,
    window_start: u64,
) -> anyhow::Result<u64> {
    let mrs = state.merge_requests.list_by_workspace(&ws.id).await?;

    Ok(mrs
        .iter()
        .filter(|mr| {
            mr.status == MrStatus::Reverted
                && mr
                    .reverted_at
                    .or(Some(mr.updated_at))
                    .is_some_and(|t| t >= window_start)
        })
        .count() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;
    use gyre_domain::{
        GateResult, MergeRequest, MrStatus, TrustLevel, WorkspaceMembership, WorkspaceRole,
    };

    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    fn make_mr(id: &str, ws_id: &str, status: MrStatus, created_at: u64) -> MergeRequest {
        MergeRequest {
            id: Id::new(id),
            repository_id: Id::new("repo1"),
            title: format!("MR {id}"),
            source_branch: format!("feat/{id}"),
            target_branch: "main".to_string(),
            status,
            author_agent_id: None,
            reviewers: vec![],
            diff_stats: None,
            has_conflicts: None,
            spec_ref: None,
            depends_on: vec![],
            atomic_group: None,
            workspace_id: Id::new(ws_id),
            created_at,
            updated_at: created_at,
            reverted_at: None,
            revert_commit_sha: None,
            merge_commit_sha: None,
        }
    }

    /// Reverted MR — `reverted_at` inside the 30-day window.
    fn make_reverted_mr(id: &str, ws_id: &str, reverted_at: u64) -> MergeRequest {
        let mut mr = make_mr(id, ws_id, MrStatus::Merged, reverted_at - 1000);
        mr.revert(format!("revert-{id}"), reverted_at).unwrap();
        mr
    }

    fn make_gate_result(id: &str, mr_id: &str, passed: bool, ts: u64) -> GateResult {
        GateResult {
            id: Id::new(id),
            gate_id: Id::new("gate1"),
            mr_id: Id::new(mr_id),
            status: if passed {
                GateStatus::Passed
            } else {
                GateStatus::Failed
            },
            output: None,
            started_at: Some(ts),
            finished_at: Some(ts + 100),
        }
    }

    fn make_workspace(id: &str, trust: TrustLevel) -> gyre_domain::Workspace {
        gyre_domain::Workspace {
            id: Id::new(id),
            tenant_id: Id::new("default"),
            name: format!("ws-{id}"),
            slug: format!("ws-{id}"),
            description: None,
            budget: None,
            max_repos: None,
            max_agents_per_repo: None,
            trust_level: trust,
            llm_model: None,
            created_at: 1_000_000,
            compute_target_id: None,
        }
    }

    fn make_membership(user_id: &str, ws_id: &str, role: WorkspaceRole) -> WorkspaceMembership {
        WorkspaceMembership {
            id: Id::new(uuid::Uuid::new_v4().to_string()),
            user_id: Id::new(user_id),
            workspace_id: Id::new(ws_id),
            role,
            invited_by: Id::new("admin"),
            accepted: true,
            accepted_at: Some(1_000_000),
            created_at: 1_000_000,
        }
    }

    async fn trust_suggestions_for(
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
                Some(NotificationType::TrustSuggestion.as_str()),
                None,
                &[],
                50,
                0,
            )
            .await
            .unwrap()
            .into_iter()
            .filter(|n| n.notification_type == NotificationType::TrustSuggestion)
            .collect()
    }

    #[tokio::test]
    async fn suggestion_created_when_record_clean() {
        let state = test_state();
        let now = now_secs();

        // Supervised workspace with an Admin member.
        let ws = make_workspace("ws1", TrustLevel::Supervised);
        state.workspaces.create(&ws).await.unwrap();
        state
            .workspace_memberships
            .create(&make_membership("user1", "ws1", WorkspaceRole::Admin))
            .await
            .unwrap();

        // Merged MRs within the 30-day window with all gates passing.
        let mr1 = make_mr("mr1", "ws1", MrStatus::Merged, now - 1000);
        let mr2 = make_mr("mr2", "ws1", MrStatus::Merged, now - 2000);
        state.merge_requests.create(&mr1).await.unwrap();
        state.merge_requests.create(&mr2).await.unwrap();
        state
            .gate_results
            .save(&make_gate_result("gr1", "mr1", true, now - 900))
            .await
            .unwrap();
        state
            .gate_results
            .save(&make_gate_result("gr2", "mr2", true, now - 1900))
            .await
            .unwrap();

        run_once(&state).await.unwrap();

        let notifs = trust_suggestions_for(&state, "user1", "ws1").await;
        assert_eq!(notifs.len(), 1, "expected exactly one TrustSuggestion");

        let n = &notifs[0];
        assert_eq!(n.priority_rank, 8);
        assert_eq!(n.title, "Consider increasing trust level");
        assert_eq!(
            n.source.repo_id, None,
            "workspace-level suggestion has no repo_id"
        );
        assert!(n.is_active());

        let body: serde_json::Value = serde_json::from_str(n.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["gate_failures"], 0);
        assert_eq!(body["mr_reverts"], 0);
        assert_eq!(body["period_days"], 30);
        assert_eq!(body["current_level"], "Supervised");
        assert_eq!(body["suggested_level"], "Guided");
    }

    #[tokio::test]
    async fn suggestion_created_for_workspace_with_no_activity() {
        // 0 gate failures and 0 reverted MRs in 30 days — even with no MRs at
        // all, the counts are 0/0 and the spec's criteria are met.
        let state = test_state();

        let ws = make_workspace("ws9", TrustLevel::Guided);
        state.workspaces.create(&ws).await.unwrap();
        state
            .workspace_memberships
            .create(&make_membership("user9", "ws9", WorkspaceRole::Admin))
            .await
            .unwrap();

        run_once(&state).await.unwrap();

        let notifs = trust_suggestions_for(&state, "user9", "ws9").await;
        assert_eq!(
            notifs.len(),
            1,
            "clean (empty) record should suggest escalation"
        );

        let body: serde_json::Value =
            serde_json::from_str(notifs[0].body.as_deref().unwrap()).unwrap();
        assert_eq!(body["current_level"], "Guided");
        assert_eq!(body["suggested_level"], "Autonomous");
    }

    #[tokio::test]
    async fn no_suggestion_when_gate_failure_in_window() {
        let state = test_state();
        let now = now_secs();

        let ws = make_workspace("ws2", TrustLevel::Supervised);
        state.workspaces.create(&ws).await.unwrap();
        state
            .workspace_memberships
            .create(&make_membership("user2", "ws2", WorkspaceRole::Admin))
            .await
            .unwrap();

        let mr1 = make_mr("mr10", "ws2", MrStatus::Merged, now - 1000);
        state.merge_requests.create(&mr1).await.unwrap();
        // One passing and one failing gate result, both inside the window.
        state
            .gate_results
            .save(&make_gate_result("gr10", "mr10", true, now - 900))
            .await
            .unwrap();
        state
            .gate_results
            .save(&make_gate_result("gr11", "mr10", false, now - 800))
            .await
            .unwrap();

        run_once(&state).await.unwrap();

        let notifs = trust_suggestions_for(&state, "user2", "ws2").await;
        assert!(
            notifs.is_empty(),
            "gate failure in window must suppress the suggestion"
        );
    }

    #[tokio::test]
    async fn gate_failure_outside_window_is_ignored() {
        let state = test_state();
        let now = now_secs();

        let ws = make_workspace("ws4", TrustLevel::Supervised);
        state.workspaces.create(&ws).await.unwrap();
        state
            .workspace_memberships
            .create(&make_membership("user4", "ws4", WorkspaceRole::Admin))
            .await
            .unwrap();

        let mr1 = make_mr("mr30", "ws4", MrStatus::Merged, now - 1000);
        state.merge_requests.create(&mr1).await.unwrap();
        // Failure older than 30 days — outside the window, must not count.
        state
            .gate_results
            .save(&make_gate_result("gr30", "mr30", false, now - 31 * 86400))
            .await
            .unwrap();

        run_once(&state).await.unwrap();

        let notifs = trust_suggestions_for(&state, "user4", "ws4").await;
        assert_eq!(
            notifs.len(),
            1,
            "failure outside the 30-day window must not suppress the suggestion"
        );
    }

    #[tokio::test]
    async fn no_suggestion_when_mr_reverted_in_window() {
        let state = test_state();
        let now = now_secs();

        let ws = make_workspace("ws5", TrustLevel::Supervised);
        state.workspaces.create(&ws).await.unwrap();
        state
            .workspace_memberships
            .create(&make_membership("user5", "ws5", WorkspaceRole::Admin))
            .await
            .unwrap();

        // A reverted MR (revert inside the window) with otherwise passing gates.
        state
            .merge_requests
            .create(&make_reverted_mr("mr40", "ws5", now - 500))
            .await
            .unwrap();
        let mr2 = make_mr("mr41", "ws5", MrStatus::Merged, now - 1000);
        state.merge_requests.create(&mr2).await.unwrap();
        state
            .gate_results
            .save(&make_gate_result("gr40", "mr40", true, now - 400))
            .await
            .unwrap();
        state
            .gate_results
            .save(&make_gate_result("gr41", "mr41", true, now - 900))
            .await
            .unwrap();

        run_once(&state).await.unwrap();

        let notifs = trust_suggestions_for(&state, "user5", "ws5").await;
        assert!(
            notifs.is_empty(),
            "MR revert in window must suppress the suggestion"
        );
    }

    #[tokio::test]
    async fn no_suggestion_for_autonomous_workspace() {
        let state = test_state();
        let now = now_secs();

        // Autonomous workspaces should never get a suggestion.
        let ws = make_workspace("ws3", TrustLevel::Autonomous);
        state.workspaces.create(&ws).await.unwrap();
        state
            .workspace_memberships
            .create(&make_membership("user3", "ws3", WorkspaceRole::Owner))
            .await
            .unwrap();

        // Perfectly passing MRs.
        let mr = make_mr("mr20", "ws3", MrStatus::Merged, now - 500);
        state.merge_requests.create(&mr).await.unwrap();
        state
            .gate_results
            .save(&make_gate_result("gr20", "mr20", true, now - 400))
            .await
            .unwrap();

        run_once(&state).await.unwrap();

        let notifs = trust_suggestions_for(&state, "user3", "ws3").await;
        assert!(
            notifs.is_empty(),
            "Autonomous workspace should not receive trust suggestions"
        );
    }

    #[tokio::test]
    async fn one_notification_per_admin_member() {
        let state = test_state();
        let now = now_secs();

        let ws = make_workspace("ws6", TrustLevel::Supervised);
        state.workspaces.create(&ws).await.unwrap();
        state
            .workspace_memberships
            .create(&make_membership("admin1", "ws6", WorkspaceRole::Admin))
            .await
            .unwrap();
        state
            .workspace_memberships
            .create(&make_membership("admin2", "ws6", WorkspaceRole::Admin))
            .await
            .unwrap();
        state
            .workspace_memberships
            .create(&make_membership("dev1", "ws6", WorkspaceRole::Developer))
            .await
            .unwrap();
        state
            .workspace_memberships
            .create(&make_membership("viewer1", "ws6", WorkspaceRole::Viewer))
            .await
            .unwrap();

        let mr = make_mr("mr50", "ws6", MrStatus::Merged, now - 1000);
        state.merge_requests.create(&mr).await.unwrap();
        state
            .gate_results
            .save(&make_gate_result("gr50", "mr50", true, now - 900))
            .await
            .unwrap();

        run_once(&state).await.unwrap();

        // Each admin gets their own notification.
        assert_eq!(
            trust_suggestions_for(&state, "admin1", "ws6").await.len(),
            1
        );
        assert_eq!(
            trust_suggestions_for(&state, "admin2", "ws6").await.len(),
            1
        );
        // Non-admin members do not.
        assert!(trust_suggestions_for(&state, "dev1", "ws6")
            .await
            .is_empty());
        assert!(trust_suggestions_for(&state, "viewer1", "ws6")
            .await
            .is_empty());
    }

    #[tokio::test]
    async fn dismissal_cooldown_suppresses_new_suggestion() {
        let state = test_state();
        let now = now_secs();

        let ws = make_workspace("ws7", TrustLevel::Supervised);
        state.workspaces.create(&ws).await.unwrap();
        state
            .workspace_memberships
            .create(&make_membership("user7", "ws7", WorkspaceRole::Admin))
            .await
            .unwrap();

        let mr = make_mr("mr60", "ws7", MrStatus::Merged, now - 1000);
        state.merge_requests.create(&mr).await.unwrap();
        state
            .gate_results
            .save(&make_gate_result("gr60", "mr60", true, now - 900))
            .await
            .unwrap();

        // First run creates the suggestion.
        run_once(&state).await.unwrap();
        let notifs = trust_suggestions_for(&state, "user7", "ws7").await;
        assert_eq!(notifs.len(), 1);

        // The admin dismisses it.
        state
            .notifications
            .dismiss(&notifs[0].id, &Id::new("user7"))
            .await
            .unwrap();

        // Second run: dismissed < 30 days ago → no new notification.
        run_once(&state).await.unwrap();

        let after = trust_suggestions_for(&state, "user7", "ws7").await;
        assert_eq!(after.len(), 1, "no new notification during the cooldown");
        assert!(
            after[0].dismissed_at.is_some(),
            "the only notification is the dismissed one"
        );
        assert!(!after[0].is_active());
    }

    #[tokio::test]
    async fn active_suggestion_not_duplicated_on_rerun() {
        let state = test_state();
        let now = now_secs();

        let ws = make_workspace("ws8", TrustLevel::Supervised);
        state.workspaces.create(&ws).await.unwrap();
        state
            .workspace_memberships
            .create(&make_membership("user8", "ws8", WorkspaceRole::Admin))
            .await
            .unwrap();

        let mr = make_mr("mr70", "ws8", MrStatus::Merged, now - 1000);
        state.merge_requests.create(&mr).await.unwrap();
        state
            .gate_results
            .save(&make_gate_result("gr70", "mr70", true, now - 900))
            .await
            .unwrap();

        run_once(&state).await.unwrap();
        // Daily re-run while the first suggestion is still active (neither
        // resolved nor dismissed) must not stack duplicates.
        run_once(&state).await.unwrap();

        let notifs = trust_suggestions_for(&state, "user8", "ws8").await;
        assert_eq!(notifs.len(), 1, "active suggestion must not be duplicated");
    }

    /// HSI §2 requires `trust_suggestion_check` to run daily per workspace.
    /// The job was registered in the `JobRegistry` (jobs.rs) but never
    /// spawned, so in a default deployment the handler never executed and
    /// no TrustSuggestion notification was ever created automatically —
    /// while the unit tests above stayed green because they call `run_once`
    /// directly. The binary's `main` is not exercisable from lib tests, so
    /// this regression test reads the shipped `main.rs` bytes (same
    /// include_str! approach as spec_registry's starter-manifest test) and
    /// fails when the `jobs::spawn_job` call for this job is removed —
    /// audit-only registration cannot stand in for the specced daily
    /// mechanism again.
    #[test]
    fn job_is_scheduled_in_main() {
        let main_src = include_str!("main.rs");
        let spawned = main_src.split("jobs::spawn_job(").skip(1).any(|call| {
            // Confine the search to this call's argument list (up to
            // the closing `);`) so a job name mentioned elsewhere in
            // main.rs cannot satisfy the check.
            let args = call.split(");").next().unwrap_or(call);
            args.contains("\"trust_suggestion_check\"")
        });
        assert!(
            spawned,
            "trust_suggestion_check is registered in the JobRegistry but never spawned: \
             add jobs::spawn_job(.., \"trust_suggestion_check\", ..) in main.rs — without \
             it the daily HSI §2 mechanism does not run in a default deployment"
        );
    }
}
