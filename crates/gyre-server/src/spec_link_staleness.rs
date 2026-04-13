//! Spec link staleness background job (spec-links.md §Automatic Staleness Detection).
//!
//! Runs daily. For each `SpecLinkEntry` in the spec links store:
//! - Checks if the target spec's current SHA in the ledger differs from the link's pinned `target_sha`.
//! - If they differ, marks the link as `"stale"` and sets `stale_since`.
//! - For cross-workspace links (target_display starts with `@`), re-resolves the workspace/repo.
//! - Creates `CrossWorkspaceSpecChange` notifications for workspace Admin/Developer members.

use std::sync::Arc;

use gyre_common::{Id, Notification, NotificationType};
use gyre_domain::WorkspaceRole;
use tracing::{debug, info, warn};

use crate::AppState;

/// Run one pass of the staleness check across all spec links.
pub async fn run_once(state: &Arc<AppState>) -> anyhow::Result<()> {
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Snapshot all links so we don't hold the lock across await points.
    let links: Vec<_> = {
        let store = state.spec_links_store.lock().await;
        store.clone()
    };

    debug!(count = links.len(), "spec_link_staleness: checking links");

    let mut stale_count = 0u32;

    for link in &links {
        // Only check links that have a pinned target_sha and are currently active.
        let Some(pinned_sha) = &link.target_sha else {
            continue;
        };

        if link.status == "stale" || link.status == "broken" {
            // Already known stale/broken — skip.
            continue;
        }

        // Resolve current SHA from the ledger.
        let current_sha = match state.spec_ledger.find_by_path(&link.target_path).await {
            Ok(Some(entry)) => entry.current_sha,
            Ok(None) => {
                // Target spec not in ledger — mark as broken.
                mark_link_status(state, &link.id, "broken", now_secs).await;
                info!(
                    source = %link.source_path,
                    target = %link.target_path,
                    "spec_link_staleness: target not in ledger — marking broken"
                );
                continue;
            }
            Err(e) => {
                warn!(
                    target = %link.target_path,
                    error = %e,
                    "spec_link_staleness: failed to look up target in ledger"
                );
                continue;
            }
        };

        if !current_sha.is_empty() && &current_sha != pinned_sha {
            mark_link_status(state, &link.id, "stale", now_secs).await;
            stale_count += 1;

            info!(
                source = %link.source_path,
                target = %link.target_path,
                link_type = %link.link_type,
                pinned = %pinned_sha,
                current = %current_sha,
                "spec_link_staleness: SHA mismatch — marking stale"
            );

            // Create notifications for workspace members.
            if let Some(ref source_repo_id) = link.source_repo_id {
                notify_workspace_members(
                    state,
                    source_repo_id,
                    &link.source_path,
                    &link.target_path,
                    now_secs,
                )
                .await;
            }
        }
    }

    // Re-resolve cross-workspace links that are currently unresolved.
    resolve_unresolved_cross_workspace_links(state, &links).await;

    info!(
        stale_count,
        total = links.len(),
        "spec_link_staleness: check complete"
    );

    Ok(())
}

/// Mark a link's status and stale_since in the spec links store.
async fn mark_link_status(state: &AppState, link_id: &str, status: &str, now: u64) {
    let mut store = state.spec_links_store.lock().await;
    if let Some(link) = store.iter_mut().find(|l| l.id == link_id) {
        link.status = status.to_string();
        if status == "stale" {
            link.stale_since = Some(now);
        }
    }
}

/// Re-resolve unresolved cross-workspace links (target_display starts with `@`).
async fn resolve_unresolved_cross_workspace_links(
    state: &AppState,
    links: &[crate::spec_registry::SpecLinkEntry],
) {
    for link in links {
        if link.status != "unresolved" {
            continue;
        }
        let Some(ref display) = link.target_display else {
            continue;
        };
        if !display.starts_with('@') {
            continue;
        }

        let parsed = crate::spec_registry::parse_cross_workspace_target(display);
        let resolved_repo_id = match &parsed {
            crate::spec_registry::CrossWorkspaceTarget::CrossWorkspace {
                workspace_slug,
                repo_name,
                ..
            } => {
                // Try to resolve workspace → repo.
                let ws = state
                    .workspaces
                    .find_by_slug(&Id::new("default"), workspace_slug)
                    .await
                    .ok()
                    .flatten();
                if let Some(ws) = ws {
                    state
                        .repos
                        .find_by_name_and_workspace(&ws.id, repo_name)
                        .await
                        .ok()
                        .flatten()
                        .map(|r| r.id.to_string())
                } else {
                    None
                }
            }
            crate::spec_registry::CrossWorkspaceTarget::CrossRepo { repo_name, .. } => {
                // For cross-repo same-workspace links, we'd need the source workspace.
                // Without it, we can't resolve. Leave as unresolved.
                debug!(
                    link_id = %link.id,
                    repo_name = %repo_name,
                    "spec_link_staleness: cross-repo link needs workspace context to resolve"
                );
                None
            }
            _ => None,
        };

        if let Some(repo_id) = resolved_repo_id {
            let mut store = state.spec_links_store.lock().await;
            if let Some(entry) = store.iter_mut().find(|l| l.id == link.id) {
                entry.target_repo_id = Some(repo_id);
                entry.status = "active".to_string();
                info!(
                    link_id = %link.id,
                    "spec_link_staleness: cross-workspace link resolved"
                );
            }
        }
    }
}

/// Create `CrossWorkspaceSpecChange` notifications for Admin/Developer members
/// of the workspace that owns the source spec.
async fn notify_workspace_members(
    state: &AppState,
    source_repo_id: &str,
    source_path: &str,
    target_path: &str,
    now_secs: u64,
) {
    // Resolve the workspace that owns the source repo.
    let repo = match state.repos.find_by_id(&Id::new(source_repo_id)).await {
        Ok(Some(r)) => r,
        _ => return,
    };
    let workspace_id = &repo.workspace_id;

    // Resolve tenant_id from workspace.
    let tenant_id = match state.workspaces.find_by_id(workspace_id).await {
        Ok(Some(ws)) => ws.tenant_id.to_string(),
        _ => return,
    };

    let members = match state
        .workspace_memberships
        .list_by_workspace(workspace_id)
        .await
    {
        Ok(m) => m,
        Err(e) => {
            warn!(
                workspace_id = %workspace_id,
                error = %e,
                "spec_link_staleness: failed to list workspace members"
            );
            return;
        }
    };

    let now_i64 = now_secs as i64;

    for member in &members {
        if !matches!(member.role, WorkspaceRole::Admin | WorkspaceRole::Developer) {
            continue;
        }

        let id = Id::new(uuid::Uuid::new_v4().to_string());
        let title = format!(
            "Spec link stale: '{}' → '{}' — target spec has changed",
            source_path, target_path
        );
        let mut notif = Notification::new(
            id,
            workspace_id.clone(),
            member.user_id.clone(),
            NotificationType::CrossWorkspaceSpecChange,
            title,
            &tenant_id,
            now_i64,
        );
        notif.body = Some(
            serde_json::json!({
                "source_path": source_path,
                "target_path": target_path,
                "link_status": "stale",
            })
            .to_string(),
        );
        notif.entity_ref = Some(source_path.to_string());

        if let Err(e) = state.notifications.create(&notif).await {
            warn!(
                user = %member.user_id,
                error = %e,
                "spec_link_staleness: failed to create notification"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;
    use crate::spec_registry::{SpecLinkEntry, SpecLinkType};
    use gyre_domain::{ApprovalStatus, SpecLedgerEntry, WorkspaceMembership, WorkspaceRole};

    fn make_ledger_entry(path: &str, sha: &str, status: ApprovalStatus) -> SpecLedgerEntry {
        SpecLedgerEntry {
            path: path.to_string(),
            title: format!("Spec {path}"),
            owner: "user:test".to_string(),
            kind: None,
            current_sha: sha.to_string(),
            approval_mode: "human_only".to_string(),
            approval_status: status,
            linked_tasks: vec![],
            linked_mrs: vec![],
            drift_status: "clean".to_string(),
            created_at: 1_000_000,
            updated_at: 1_000_000,
            repo_id: Some("repo1".to_string()),
            workspace_id: Some("ws1".to_string()),
        }
    }

    fn make_link(
        id: &str,
        source: &str,
        target: &str,
        link_type: SpecLinkType,
        target_sha: Option<&str>,
    ) -> SpecLinkEntry {
        SpecLinkEntry {
            id: id.to_string(),
            source_path: source.to_string(),
            source_repo_id: Some("repo1".to_string()),
            link_type,
            target_path: target.to_string(),
            target_repo_id: None,
            target_display: None,
            target_sha: target_sha.map(|s| s.to_string()),
            reason: None,
            status: "active".to_string(),
            created_at: 1_000_000,
            stale_since: None,
        }
    }

    fn make_workspace(id: &str) -> gyre_domain::Workspace {
        gyre_domain::Workspace {
            id: Id::new(id),
            tenant_id: Id::new("default"),
            name: format!("ws-{id}"),
            slug: format!("ws-{id}"),
            description: None,
            budget: None,
            max_repos: None,
            max_agents_per_repo: None,
            trust_level: gyre_domain::TrustLevel::Guided,
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

    fn make_repo(id: &str, ws_id: &str) -> gyre_domain::Repository {
        gyre_domain::Repository {
            id: Id::new(id),
            name: format!("repo-{id}"),
            path: format!("/repos/{id}"),
            workspace_id: Id::new(ws_id),
            default_branch: "main".to_string(),
            is_mirror: false,
            mirror_url: None,
            mirror_interval_secs: None,
            last_mirror_sync: None,
            description: None,
            status: gyre_domain::RepoStatus::Active,
            created_at: 1_000_000,
            updated_at: 1_000_000,
        }
    }

    #[tokio::test]
    async fn staleness_job_detects_sha_mismatch() {
        let state = test_state();

        // Set up target spec with current SHA different from pinned.
        let target = make_ledger_entry(
            "system/target.md",
            "current_sha_456",
            ApprovalStatus::Approved,
        );
        state.spec_ledger.save(&target).await.unwrap();

        // Set up a link with a pinned SHA that no longer matches.
        let link = make_link(
            "link1",
            "system/source.md",
            "system/target.md",
            SpecLinkType::DependsOn,
            Some("old_sha_123"),
        );
        {
            let mut store = state.spec_links_store.lock().await;
            store.push(link);
        }

        run_once(&state).await.unwrap();

        // Verify the link is now stale.
        let store = state.spec_links_store.lock().await;
        let updated = store.iter().find(|l| l.id == "link1").unwrap();
        assert_eq!(updated.status, "stale");
        assert!(updated.stale_since.is_some());
    }

    #[tokio::test]
    async fn staleness_job_skips_matching_sha() {
        let state = test_state();

        // Target spec SHA matches pinned SHA.
        let target =
            make_ledger_entry("system/target.md", "matching_sha", ApprovalStatus::Approved);
        state.spec_ledger.save(&target).await.unwrap();

        let link = make_link(
            "link2",
            "system/source.md",
            "system/target.md",
            SpecLinkType::DependsOn,
            Some("matching_sha"),
        );
        {
            let mut store = state.spec_links_store.lock().await;
            store.push(link);
        }

        run_once(&state).await.unwrap();

        // Link should still be active.
        let store = state.spec_links_store.lock().await;
        let updated = store.iter().find(|l| l.id == "link2").unwrap();
        assert_eq!(updated.status, "active");
        assert!(updated.stale_since.is_none());
    }

    #[tokio::test]
    async fn staleness_job_marks_missing_target_as_broken() {
        let state = test_state();

        // No target spec in ledger.
        let link = make_link(
            "link3",
            "system/source.md",
            "system/nonexistent.md",
            SpecLinkType::Extends,
            Some("some_sha"),
        );
        {
            let mut store = state.spec_links_store.lock().await;
            store.push(link);
        }

        run_once(&state).await.unwrap();

        let store = state.spec_links_store.lock().await;
        let updated = store.iter().find(|l| l.id == "link3").unwrap();
        assert_eq!(updated.status, "broken");
    }

    #[tokio::test]
    async fn staleness_job_creates_notifications_for_workspace_members() {
        let state = test_state();

        // Set up workspace + repo + members.
        let ws = make_workspace("ws1");
        state.workspaces.create(&ws).await.unwrap();

        let repo = make_repo("repo1", "ws1");
        state.repos.create(&repo).await.unwrap();

        let admin = make_membership("admin1", "ws1", WorkspaceRole::Admin);
        let dev = make_membership("dev1", "ws1", WorkspaceRole::Developer);
        let viewer = make_membership("viewer1", "ws1", WorkspaceRole::Viewer);
        state.workspace_memberships.create(&admin).await.unwrap();
        state.workspace_memberships.create(&dev).await.unwrap();
        state.workspace_memberships.create(&viewer).await.unwrap();

        // Set up stale link scenario.
        let target = make_ledger_entry("system/target.md", "new_sha", ApprovalStatus::Approved);
        state.spec_ledger.save(&target).await.unwrap();

        let link = make_link(
            "link4",
            "system/source.md",
            "system/target.md",
            SpecLinkType::DependsOn,
            Some("old_sha"),
        );
        {
            let mut store = state.spec_links_store.lock().await;
            store.push(link);
        }

        run_once(&state).await.unwrap();

        // Admin and Developer should get notifications; Viewer should not.
        let admin_notifs = state
            .notifications
            .list_for_user(
                &Id::new("admin1"),
                Some(&Id::new("ws1")),
                None,
                None,
                None,
                10,
                0,
            )
            .await
            .unwrap();
        assert!(
            admin_notifs
                .iter()
                .any(|n| n.notification_type == NotificationType::CrossWorkspaceSpecChange),
            "Admin should receive stale link notification"
        );

        let dev_notifs = state
            .notifications
            .list_for_user(
                &Id::new("dev1"),
                Some(&Id::new("ws1")),
                None,
                None,
                None,
                10,
                0,
            )
            .await
            .unwrap();
        assert!(
            dev_notifs
                .iter()
                .any(|n| n.notification_type == NotificationType::CrossWorkspaceSpecChange),
            "Developer should receive stale link notification"
        );

        let viewer_notifs = state
            .notifications
            .list_for_user(
                &Id::new("viewer1"),
                Some(&Id::new("ws1")),
                None,
                None,
                None,
                10,
                0,
            )
            .await
            .unwrap();
        assert!(
            !viewer_notifs
                .iter()
                .any(|n| n.notification_type == NotificationType::CrossWorkspaceSpecChange),
            "Viewer should NOT receive stale link notification"
        );
    }

    #[tokio::test]
    async fn staleness_job_skips_already_stale_links() {
        let state = test_state();

        let target = make_ledger_entry("system/target.md", "new_sha", ApprovalStatus::Approved);
        state.spec_ledger.save(&target).await.unwrap();

        let mut link = make_link(
            "link5",
            "system/source.md",
            "system/target.md",
            SpecLinkType::DependsOn,
            Some("old_sha"),
        );
        link.status = "stale".to_string();
        link.stale_since = Some(999_000);
        {
            let mut store = state.spec_links_store.lock().await;
            store.push(link);
        }

        run_once(&state).await.unwrap();

        // stale_since should remain unchanged (not re-stamped).
        let store = state.spec_links_store.lock().await;
        let updated = store.iter().find(|l| l.id == "link5").unwrap();
        assert_eq!(updated.status, "stale");
        assert_eq!(updated.stale_since, Some(999_000));
    }

    #[tokio::test]
    async fn staleness_job_skips_links_without_target_sha() {
        let state = test_state();

        // Link without a pinned SHA should be skipped entirely.
        let link = make_link(
            "link6",
            "system/source.md",
            "system/target.md",
            SpecLinkType::References,
            None,
        );
        {
            let mut store = state.spec_links_store.lock().await;
            store.push(link);
        }

        run_once(&state).await.unwrap();

        let store = state.spec_links_store.lock().await;
        let updated = store.iter().find(|l| l.id == "link6").unwrap();
        assert_eq!(updated.status, "active");
    }
}
