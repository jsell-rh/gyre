//! Trust suggestion background job (HSI §2).
//!
//! Runs daily per workspace. Evaluates trust escalation criteria over the last 30 days:
//!   - Supervised → Guided:   >80% gate pass rate, <5% revert rate
//!   - Guided    → Autonomous: >95% gate pass rate, <2% revert rate
//!
//! When criteria are met and the current trust level is below the suggested level,
//! creates a priority-8 `TrustSuggestion` notification for each Admin/Owner member
//! of the workspace. Suppresses re-creation if:
//!   - The user dismissed a TrustSuggestion within the last 30 days, or
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

    // Determine the suggested trust level (None = no escalation warranted).
    let suggested = suggested_trust_level(state, ws, window_start).await?;

    let Some(suggested_level) = suggested else {
        debug!(workspace = %ws.slug, "trust_suggestion: no escalation warranted");
        return Ok(());
    };

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
            .list_for_user(&member.user_id, Some(&ws.id), Some(8), Some(8), None, 20, 0)
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

        // Create the notification.
        let id = Id::new(uuid::Uuid::new_v4().to_string());
        let title = format!(
            "Trust suggestion: consider upgrading workspace '{}' from {} to {}",
            ws.slug, ws.trust_level, suggested_level
        );
        let mut notif = Notification::new(
            id,
            ws.id.clone(),
            member.user_id.clone(),
            NotificationType::TrustSuggestion,
            title,
            ws.tenant_id.to_string(),
            now_i64,
        );
        notif.body = Some(
            serde_json::json!({
                "current_trust": ws.trust_level.to_string(),
                "suggested_trust": suggested_level.to_string(),
                "window_days": 30
            })
            .to_string(),
        );
        notif.entity_ref = Some(ws.id.to_string());

        if let Err(e) = state.notifications.create(&notif).await {
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

/// Returns the suggested next trust level if escalation criteria are met, otherwise None.
///
/// Criteria (HSI §2):
///   Supervised → Guided:    >80% gate pass rate AND <5% revert rate (30-day window)
///   Guided    → Autonomous: >95% gate pass rate AND <2% revert rate (30-day window)
async fn suggested_trust_level(
    state: &Arc<AppState>,
    ws: &gyre_domain::Workspace,
    window_start: u64,
) -> anyhow::Result<Option<TrustLevel>> {
    let target = match ws.trust_level {
        TrustLevel::Supervised => TrustLevel::Guided,
        TrustLevel::Guided => TrustLevel::Autonomous,
        // Autonomous and Custom do not escalate further.
        TrustLevel::Autonomous | TrustLevel::Custom => return Ok(None),
    };

    let (gate_pass_threshold, revert_threshold) = match &target {
        TrustLevel::Guided => (0.80_f64, 0.05_f64),
        TrustLevel::Autonomous => (0.95_f64, 0.02_f64),
        _ => return Ok(None),
    };

    // Fetch all MRs for this workspace.
    let all_mrs = state.merge_requests.list_by_workspace(&ws.id).await?;

    // Filter to MRs created within the 30-day window that have reached a terminal state.
    let recent_terminal: Vec<_> = all_mrs
        .iter()
        .filter(|mr| {
            mr.created_at >= window_start
                && matches!(
                    mr.status,
                    MrStatus::Merged | MrStatus::Closed | MrStatus::Reverted
                )
        })
        .collect();

    if recent_terminal.is_empty() {
        debug!(workspace = %ws.slug, "trust_suggestion: no terminal MRs in window");
        return Ok(None);
    }

    // Revert rate: Reverted / (Merged + Reverted). Closed MRs don't count as reverts.
    let merged_count = recent_terminal
        .iter()
        .filter(|mr| matches!(mr.status, MrStatus::Merged | MrStatus::Reverted))
        .count();

    let revert_count = recent_terminal
        .iter()
        .filter(|mr| matches!(mr.status, MrStatus::Reverted))
        .count();

    let revert_rate = if merged_count > 0 {
        revert_count as f64 / merged_count as f64
    } else {
        0.0
    };

    // Gate pass rate: compute across all gate results for recent MRs.
    let mut total_gates: usize = 0;
    let mut passed_gates: usize = 0;

    for mr in &recent_terminal {
        let results = state.gate_results.list_by_mr_id(mr.id.as_str()).await?;
        for result in &results {
            // Only count finished gate evaluations.
            if result.finished_at.is_some() {
                total_gates += 1;
                if result.status == GateStatus::Passed {
                    passed_gates += 1;
                }
            }
        }
    }

    let gate_pass_rate = if total_gates > 0 {
        passed_gates as f64 / total_gates as f64
    } else {
        // No gate data: be conservative, assume criteria not met.
        0.0
    };

    debug!(
        workspace = %ws.slug,
        gate_pass_rate,
        revert_rate,
        merged_count,
        revert_count,
        total_gates,
        passed_gates,
        "trust_suggestion: metrics computed"
    );

    if gate_pass_rate > gate_pass_threshold && revert_rate < revert_threshold {
        Ok(Some(target))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;
    use gyre_domain::{
        GateResult, MergeRequest, MrStatus, TrustLevel, WorkspaceMembership, WorkspaceRole,
    };

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
            revert_mr_id: None,
        }
    }

    fn make_gate_result(id: &str, mr_id: &str, passed: bool) -> GateResult {
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
            started_at: Some(1_000_000),
            finished_at: Some(1_001_000),
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

    #[tokio::test]
    async fn suggestion_created_when_criteria_met() {
        let state = test_state();
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Set up workspace with Supervised trust level.
        let ws = make_workspace("ws1", TrustLevel::Supervised);
        state.workspaces.create(&ws).await.unwrap();

        // Add an Admin member who will receive the notification.
        let membership = make_membership("user1", "ws1", WorkspaceRole::Admin);
        state
            .workspace_memberships
            .create(&membership)
            .await
            .unwrap();

        // Add merged MRs within the 30-day window with all gates passing.
        let mr1 = make_mr("mr1", "ws1", MrStatus::Merged, now_secs - 1000);
        let mr2 = make_mr("mr2", "ws1", MrStatus::Merged, now_secs - 2000);
        let mr3 = make_mr("mr3", "ws1", MrStatus::Merged, now_secs - 3000);
        state.merge_requests.create(&mr1).await.unwrap();
        state.merge_requests.create(&mr2).await.unwrap();
        state.merge_requests.create(&mr3).await.unwrap();

        // All gate results pass → >80% gate pass rate.
        let gr1 = make_gate_result("gr1", "mr1", true);
        let gr2 = make_gate_result("gr2", "mr2", true);
        let gr3 = make_gate_result("gr3", "mr3", true);
        state.gate_results.save(&gr1).await.unwrap();
        state.gate_results.save(&gr2).await.unwrap();
        state.gate_results.save(&gr3).await.unwrap();

        // Run the job.
        run_once(&state).await.unwrap();

        // Expect a TrustSuggestion notification for user1.
        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user1"),
                Some(&Id::new("ws1")),
                Some(8),
                Some(8),
                None,
                10,
                0,
            )
            .await
            .unwrap();

        assert!(
            notifs
                .iter()
                .any(|n| n.notification_type == NotificationType::TrustSuggestion),
            "expected TrustSuggestion notification"
        );
    }

    #[tokio::test]
    async fn no_suggestion_when_gate_pass_rate_too_low() {
        let state = test_state();
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let ws = make_workspace("ws2", TrustLevel::Supervised);
        state.workspaces.create(&ws).await.unwrap();

        let membership = make_membership("user2", "ws2", WorkspaceRole::Admin);
        state
            .workspace_memberships
            .create(&membership)
            .await
            .unwrap();

        let mr1 = make_mr("mr10", "ws2", MrStatus::Merged, now_secs - 1000);
        let mr2 = make_mr("mr11", "ws2", MrStatus::Merged, now_secs - 2000);
        state.merge_requests.create(&mr1).await.unwrap();
        state.merge_requests.create(&mr2).await.unwrap();

        // One pass, one fail → 50% pass rate (below 80% threshold).
        let gr1 = make_gate_result("gr10", "mr10", true);
        let gr2 = make_gate_result("gr11", "mr11", false);
        state.gate_results.save(&gr1).await.unwrap();
        state.gate_results.save(&gr2).await.unwrap();

        run_once(&state).await.unwrap();

        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user2"),
                Some(&Id::new("ws2")),
                Some(8),
                Some(8),
                None,
                10,
                0,
            )
            .await
            .unwrap();

        assert!(
            !notifs
                .iter()
                .any(|n| n.notification_type == NotificationType::TrustSuggestion),
            "should not create TrustSuggestion when gate pass rate is too low"
        );
    }

    #[tokio::test]
    async fn no_suggestion_for_autonomous_workspace() {
        let state = test_state();
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Autonomous workspaces should never get a suggestion.
        let ws = make_workspace("ws3", TrustLevel::Autonomous);
        state.workspaces.create(&ws).await.unwrap();

        let membership = make_membership("user3", "ws3", WorkspaceRole::Owner);
        state
            .workspace_memberships
            .create(&membership)
            .await
            .unwrap();

        // Add perfectly passing MRs.
        let mr = make_mr("mr20", "ws3", MrStatus::Merged, now_secs - 500);
        state.merge_requests.create(&mr).await.unwrap();
        let gr = make_gate_result("gr20", "mr20", true);
        state.gate_results.save(&gr).await.unwrap();

        run_once(&state).await.unwrap();

        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user3"),
                Some(&Id::new("ws3")),
                Some(8),
                Some(8),
                None,
                10,
                0,
            )
            .await
            .unwrap();

        assert!(
            !notifs
                .iter()
                .any(|n| n.notification_type == NotificationType::TrustSuggestion),
            "Autonomous workspace should not receive trust suggestions"
        );
    }
}
