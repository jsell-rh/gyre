//! Abandoned branch detection background job (HSI §8, priority 9).
//!
//! Runs daily per workspace. Flags `spec-edit/*` branches where the MR has had
//! no activity for >7 days (`updated_at` older than 7 days). Creates priority-9
//! `AbandonedBranch` notifications for each Admin/Developer member of the workspace.
//!
//! Suppresses re-creation if:
//!   - The user dismissed an AbandonedBranch notification for this MR within 30 days, or
//!   - An active (unresolved, undismissed) AbandonedBranch notification already exists
//!     for that (MR, user) pair.

use std::sync::Arc;

use gyre_common::{Id, Notification, NotificationType};
use gyre_domain::{MrStatus, WorkspaceRole};
use tracing::{debug, info, warn};

use crate::AppState;

/// Seconds in 7 days.
const SEVEN_DAYS_SECS: u64 = 7 * 24 * 3600;

/// Run one pass of the abandoned branch check across all workspaces.
pub async fn run_once(state: &Arc<AppState>) -> anyhow::Result<()> {
    let workspaces = state.workspaces.list().await?;
    debug!(
        count = workspaces.len(),
        "abandoned_branch_check: evaluating workspaces"
    );

    for ws in &workspaces {
        if let Err(e) = evaluate_workspace(state, ws).await {
            warn!(workspace_id = %ws.id, error = %e, "abandoned_branch_check: workspace evaluation failed");
        }
    }
    Ok(())
}

/// Evaluate a single workspace: find abandoned spec-edit branches and notify members.
async fn evaluate_workspace(
    state: &Arc<AppState>,
    ws: &gyre_domain::Workspace,
) -> anyhow::Result<()> {
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let cutoff = now_secs.saturating_sub(SEVEN_DAYS_SECS);

    // Fetch all MRs for this workspace.
    let all_mrs = state.merge_requests.list_by_workspace(&ws.id).await?;

    // Filter to open MRs on spec-edit/* branches with no activity for >7 days.
    let abandoned: Vec<_> = all_mrs
        .iter()
        .filter(|mr| {
            mr.source_branch.starts_with("spec-edit/")
                && mr.status == MrStatus::Open
                && mr.updated_at < cutoff
        })
        .collect();

    if abandoned.is_empty() {
        debug!(workspace = %ws.slug, "abandoned_branch_check: no abandoned branches");
        return Ok(());
    }

    info!(
        workspace = %ws.slug,
        count = abandoned.len(),
        "abandoned_branch_check: found abandoned branches"
    );

    // Fetch Admin/Developer members who will receive notifications.
    let members = state
        .workspace_memberships
        .list_by_workspace(&ws.id)
        .await?;

    let target_members: Vec<_> = members
        .iter()
        .filter(|m| matches!(m.role, WorkspaceRole::Admin | WorkspaceRole::Developer))
        .collect();

    if target_members.is_empty() {
        debug!(workspace = %ws.slug, "abandoned_branch_check: no Admin/Developer members");
        return Ok(());
    }

    let now_i64 = now_secs as i64;

    for mr in &abandoned {
        let mr_id_str = mr.id.to_string();

        for member in &target_members {
            // Suppress if user dismissed an AbandonedBranch for this MR within 30 days.
            let recently_dismissed = state
                .notifications
                .has_recent_dismissal(
                    &ws.id,
                    &member.user_id,
                    NotificationType::AbandonedBranch.as_str(),
                    30,
                )
                .await
                .unwrap_or(false);

            if recently_dismissed {
                debug!(
                    workspace = %ws.slug,
                    user = %member.user_id,
                    mr = %mr.id,
                    "abandoned_branch_check: suppressed (recently dismissed)"
                );
                continue;
            }

            // Suppress if an active AbandonedBranch notification already exists for this MR/user.
            let existing = state
                .notifications
                .list_for_user(
                    &member.user_id,
                    Some(&ws.id),
                    Some(9),
                    Some(9),
                    Some(NotificationType::AbandonedBranch.as_str()),
                    100,
                    0,
                )
                .await
                .unwrap_or_default();

            let already_active = existing.iter().any(|n| {
                n.notification_type == NotificationType::AbandonedBranch
                    && n.is_active()
                    && n.entity_ref.as_deref() == Some(&mr_id_str)
            });

            if already_active {
                debug!(
                    workspace = %ws.slug,
                    user = %member.user_id,
                    mr = %mr.id,
                    "abandoned_branch_check: suppressed (active notification exists)"
                );
                continue;
            }

            // Create the notification.
            let id = Id::new(uuid::Uuid::new_v4().to_string());
            let title = format!(
                "Abandoned branch: '{}' — MR '{}' has had no activity for >7 days",
                mr.source_branch, mr.title
            );
            let mut notif = Notification::new(
                id,
                ws.id.clone(),
                member.user_id.clone(),
                NotificationType::AbandonedBranch,
                title,
                ws.tenant_id.to_string(),
                now_i64,
            );
            notif.body = Some(
                serde_json::json!({
                    "source_branch": mr.source_branch,
                    "mr_title": mr.title,
                    "mr_id": mr.id.to_string(),
                    "repository_id": mr.repository_id.to_string(),
                    "updated_at": mr.updated_at
                })
                .to_string(),
            );
            notif.entity_ref = Some(mr_id_str.clone());
            notif.repo_id = Some(mr.repository_id.to_string());

            if let Err(e) = state.notifications.create(&notif).await {
                warn!(
                    workspace = %ws.slug,
                    user = %member.user_id,
                    mr = %mr.id,
                    error = %e,
                    "abandoned_branch_check: failed to create notification"
                );
            } else {
                info!(
                    workspace = %ws.slug,
                    user = %member.user_id,
                    mr = %mr.id,
                    "abandoned_branch_check: notification created"
                );
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;
    use gyre_domain::{MergeRequest, MrStatus, WorkspaceMembership, WorkspaceRole};

    fn make_mr(
        id: &str,
        ws_id: &str,
        source_branch: &str,
        status: MrStatus,
        updated_at: u64,
    ) -> MergeRequest {
        MergeRequest {
            id: Id::new(id),
            repository_id: Id::new("repo1"),
            title: format!("MR {id}"),
            source_branch: source_branch.to_string(),
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
            created_at: updated_at,
            updated_at,
            reverted_at: None,
            revert_mr_id: None,
        }
    }

    fn make_workspace(id: &str) -> gyre_domain::Workspace {
        gyre_domain::Workspace {
            id: Id::new(id),
            tenant_id: Id::new("tenant1"),
            name: format!("ws-{id}"),
            slug: format!("ws-{id}"),
            description: None,
            budget: None,
            max_repos: None,
            max_agents_per_repo: None,
            trust_level: gyre_domain::TrustLevel::Supervised,
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

    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    #[tokio::test]
    async fn creates_notification_for_abandoned_spec_edit_branch() {
        let state = test_state();
        let now = now_secs();

        let ws = make_workspace("ws1");
        state.workspaces.create(&ws).await.unwrap();

        let membership = make_membership("user1", "ws1", WorkspaceRole::Admin);
        state
            .workspace_memberships
            .create(&membership)
            .await
            .unwrap();

        // Open spec-edit MR with no activity for >7 days.
        let mr = make_mr(
            "mr1",
            "ws1",
            "spec-edit/my-spec-slug-001",
            MrStatus::Open,
            now - SEVEN_DAYS_SECS - 1000,
        );
        state.merge_requests.create(&mr).await.unwrap();

        run_once(&state).await.unwrap();

        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user1"),
                Some(&Id::new("ws1")),
                Some(9),
                Some(9),
                None,
                10,
                0,
            )
            .await
            .unwrap();

        assert!(
            notifs
                .iter()
                .any(|n| n.notification_type == NotificationType::AbandonedBranch
                    && n.entity_ref.as_deref() == Some("mr1")
                    && n.repo_id.as_deref() == Some("repo1")),
            "expected AbandonedBranch notification with entity_ref=mr1 and repo_id=repo1"
        );
    }

    #[tokio::test]
    async fn notifies_both_admin_and_developer_members() {
        let state = test_state();
        let now = now_secs();

        let ws = make_workspace("ws2");
        state.workspaces.create(&ws).await.unwrap();

        let admin = make_membership("admin_user", "ws2", WorkspaceRole::Admin);
        let dev = make_membership("dev_user", "ws2", WorkspaceRole::Developer);
        let viewer = make_membership("viewer_user", "ws2", WorkspaceRole::Viewer);
        state
            .workspace_memberships
            .create(&admin)
            .await
            .unwrap();
        state
            .workspace_memberships
            .create(&dev)
            .await
            .unwrap();
        state
            .workspace_memberships
            .create(&viewer)
            .await
            .unwrap();

        let mr = make_mr(
            "mr2",
            "ws2",
            "spec-edit/some-spec-002",
            MrStatus::Open,
            now - SEVEN_DAYS_SECS - 500,
        );
        state.merge_requests.create(&mr).await.unwrap();

        run_once(&state).await.unwrap();

        // Admin should get notified.
        let admin_notifs = state
            .notifications
            .list_for_user(
                &Id::new("admin_user"),
                Some(&Id::new("ws2")),
                Some(9),
                Some(9),
                None,
                10,
                0,
            )
            .await
            .unwrap();
        assert!(
            admin_notifs
                .iter()
                .any(|n| n.notification_type == NotificationType::AbandonedBranch),
            "Admin should receive AbandonedBranch notification"
        );

        // Developer should get notified.
        let dev_notifs = state
            .notifications
            .list_for_user(
                &Id::new("dev_user"),
                Some(&Id::new("ws2")),
                Some(9),
                Some(9),
                None,
                10,
                0,
            )
            .await
            .unwrap();
        assert!(
            dev_notifs
                .iter()
                .any(|n| n.notification_type == NotificationType::AbandonedBranch),
            "Developer should receive AbandonedBranch notification"
        );

        // Viewer should NOT get notified.
        let viewer_notifs = state
            .notifications
            .list_for_user(
                &Id::new("viewer_user"),
                Some(&Id::new("ws2")),
                Some(9),
                Some(9),
                None,
                10,
                0,
            )
            .await
            .unwrap();
        assert!(
            !viewer_notifs
                .iter()
                .any(|n| n.notification_type == NotificationType::AbandonedBranch),
            "Viewer should NOT receive AbandonedBranch notification"
        );
    }

    #[tokio::test]
    async fn does_not_create_notification_for_non_spec_edit_branch() {
        let state = test_state();
        let now = now_secs();

        let ws = make_workspace("ws3");
        state.workspaces.create(&ws).await.unwrap();

        let membership = make_membership("user3", "ws3", WorkspaceRole::Admin);
        state
            .workspace_memberships
            .create(&membership)
            .await
            .unwrap();

        // Regular branch — not spec-edit/.
        let mr = make_mr(
            "mr3",
            "ws3",
            "feat/some-feature",
            MrStatus::Open,
            now - SEVEN_DAYS_SECS - 1000,
        );
        state.merge_requests.create(&mr).await.unwrap();

        run_once(&state).await.unwrap();

        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user3"),
                Some(&Id::new("ws3")),
                Some(9),
                Some(9),
                None,
                10,
                0,
            )
            .await
            .unwrap();

        assert!(
            !notifs
                .iter()
                .any(|n| n.notification_type == NotificationType::AbandonedBranch),
            "should not create notification for non-spec-edit branch"
        );
    }

    #[tokio::test]
    async fn does_not_create_notification_for_merged_mr() {
        let state = test_state();
        let now = now_secs();

        let ws = make_workspace("ws4");
        state.workspaces.create(&ws).await.unwrap();

        let membership = make_membership("user4", "ws4", WorkspaceRole::Admin);
        state
            .workspace_memberships
            .create(&membership)
            .await
            .unwrap();

        // Merged MR — should be excluded even though branch is spec-edit/ and old.
        let mr = make_mr(
            "mr4",
            "ws4",
            "spec-edit/done-spec-004",
            MrStatus::Merged,
            now - SEVEN_DAYS_SECS - 1000,
        );
        state.merge_requests.create(&mr).await.unwrap();

        run_once(&state).await.unwrap();

        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user4"),
                Some(&Id::new("ws4")),
                Some(9),
                Some(9),
                None,
                10,
                0,
            )
            .await
            .unwrap();

        assert!(
            !notifs
                .iter()
                .any(|n| n.notification_type == NotificationType::AbandonedBranch),
            "should not create notification for merged MR"
        );
    }

    #[tokio::test]
    async fn does_not_create_notification_for_closed_mr() {
        let state = test_state();
        let now = now_secs();

        let ws = make_workspace("ws5");
        state.workspaces.create(&ws).await.unwrap();

        let membership = make_membership("user5", "ws5", WorkspaceRole::Admin);
        state
            .workspace_memberships
            .create(&membership)
            .await
            .unwrap();

        let mr = make_mr(
            "mr5",
            "ws5",
            "spec-edit/closed-spec-005",
            MrStatus::Closed,
            now - SEVEN_DAYS_SECS - 1000,
        );
        state.merge_requests.create(&mr).await.unwrap();

        run_once(&state).await.unwrap();

        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user5"),
                Some(&Id::new("ws5")),
                Some(9),
                Some(9),
                None,
                10,
                0,
            )
            .await
            .unwrap();

        assert!(
            !notifs
                .iter()
                .any(|n| n.notification_type == NotificationType::AbandonedBranch),
            "should not create notification for closed MR"
        );
    }

    #[tokio::test]
    async fn does_not_create_notification_when_updated_within_seven_days() {
        let state = test_state();
        let now = now_secs();

        let ws = make_workspace("ws6");
        state.workspaces.create(&ws).await.unwrap();

        let membership = make_membership("user6", "ws6", WorkspaceRole::Admin);
        state
            .workspace_memberships
            .create(&membership)
            .await
            .unwrap();

        // Active MR — updated within 7 days.
        let mr = make_mr(
            "mr6",
            "ws6",
            "spec-edit/active-spec-006",
            MrStatus::Open,
            now - 3600, // 1 hour ago
        );
        state.merge_requests.create(&mr).await.unwrap();

        run_once(&state).await.unwrap();

        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user6"),
                Some(&Id::new("ws6")),
                Some(9),
                Some(9),
                None,
                10,
                0,
            )
            .await
            .unwrap();

        assert!(
            !notifs
                .iter()
                .any(|n| n.notification_type == NotificationType::AbandonedBranch),
            "should not create notification when MR updated within 7 days"
        );
    }

    #[tokio::test]
    async fn suppresses_when_user_recently_dismissed() {
        let state = test_state();
        let now = now_secs();
        let now_i64 = now as i64;

        let ws = make_workspace("ws7");
        state.workspaces.create(&ws).await.unwrap();

        let membership = make_membership("user7", "ws7", WorkspaceRole::Admin);
        state
            .workspace_memberships
            .create(&membership)
            .await
            .unwrap();

        let mr = make_mr(
            "mr7",
            "ws7",
            "spec-edit/dismissed-spec-007",
            MrStatus::Open,
            now - SEVEN_DAYS_SECS - 1000,
        );
        state.merge_requests.create(&mr).await.unwrap();

        // Create a pre-existing dismissed AbandonedBranch notification (within 30 days).
        let dismissed_notif = Notification {
            id: Id::new("notif-dismissed"),
            workspace_id: Id::new("ws7"),
            user_id: Id::new("user7"),
            notification_type: NotificationType::AbandonedBranch,
            priority: 9,
            title: "Old abandoned branch notification".to_string(),
            body: None,
            entity_ref: Some("mr7".to_string()),
            repo_id: None,
            resolved_at: None,
            dismissed_at: Some(now_i64 - 86400), // dismissed 1 day ago
            created_at: now_i64 - 172800,         // created 2 days ago
            tenant_id: "tenant1".to_string(),
        };
        state.notifications.create(&dismissed_notif).await.unwrap();

        run_once(&state).await.unwrap();

        // Only the pre-existing dismissed notification should exist — no new one.
        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user7"),
                Some(&Id::new("ws7")),
                Some(9),
                Some(9),
                None,
                10,
                0,
            )
            .await
            .unwrap();

        let active_abandoned = notifs
            .iter()
            .filter(|n| {
                n.notification_type == NotificationType::AbandonedBranch && n.is_active()
            })
            .count();

        assert_eq!(
            active_abandoned, 0,
            "should not create new notification when user recently dismissed"
        );
    }

    #[tokio::test]
    async fn does_not_duplicate_when_active_notification_exists() {
        let state = test_state();
        let now = now_secs();
        let now_i64 = now as i64;

        let ws = make_workspace("ws8");
        state.workspaces.create(&ws).await.unwrap();

        let membership = make_membership("user8", "ws8", WorkspaceRole::Admin);
        state
            .workspace_memberships
            .create(&membership)
            .await
            .unwrap();

        let mr = make_mr(
            "mr8",
            "ws8",
            "spec-edit/dup-spec-008",
            MrStatus::Open,
            now - SEVEN_DAYS_SECS - 1000,
        );
        state.merge_requests.create(&mr).await.unwrap();

        // Create a pre-existing active AbandonedBranch notification for same MR/user.
        let existing_notif = Notification {
            id: Id::new("notif-existing"),
            workspace_id: Id::new("ws8"),
            user_id: Id::new("user8"),
            notification_type: NotificationType::AbandonedBranch,
            priority: 9,
            title: "Already notified about abandoned branch".to_string(),
            body: None,
            entity_ref: Some("mr8".to_string()),
            repo_id: Some("repo1".to_string()),
            resolved_at: None,
            dismissed_at: None,
            created_at: now_i64 - 86400, // created 1 day ago
            tenant_id: "tenant1".to_string(),
        };
        state.notifications.create(&existing_notif).await.unwrap();

        run_once(&state).await.unwrap();

        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user8"),
                Some(&Id::new("ws8")),
                Some(9),
                Some(9),
                None,
                100,
                0,
            )
            .await
            .unwrap();

        let abandoned_count = notifs
            .iter()
            .filter(|n| {
                n.notification_type == NotificationType::AbandonedBranch
                    && n.entity_ref.as_deref() == Some("mr8")
            })
            .count();

        assert_eq!(
            abandoned_count, 1,
            "should not duplicate — expected exactly 1 AbandonedBranch for mr8, got {abandoned_count}"
        );
    }
}
