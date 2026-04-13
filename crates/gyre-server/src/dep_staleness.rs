//! Dependency staleness background job (dependency-graph.md §Version Drift Tracking).
//!
//! Runs daily. For each `DependencyEdge`:
//! - Resolves the target repo's current version (latest semver tag).
//! - Computes `version_drift` and updates `target_version_current`.
//! - If drift exceeds workspace policy `max_version_drift`, sets status to `Stale`.
//! - Checks if `last_verified_at` is older than `stale_dependency_alert_days`.
//! - Creates auto-tasks for stale dependencies if `auto_create_update_tasks` is enabled.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use gyre_common::Id;
use gyre_domain::{DependencyStatus, TaskPriority};
use tracing::{debug, info, warn};

use crate::AppState;

/// Run one pass of the dependency staleness check.
pub async fn run_once(state: &Arc<AppState>) -> anyhow::Result<()> {
    run_once_with(state, |path| {
        Box::pin(async move { crate::version_compute::latest_semver_tag(&path).await })
    })
    .await
}

/// Run one pass of the dependency staleness check with an injectable version
/// resolver. The `resolve_version` closure maps a repo path to its current
/// semver version (if any). Production callers use `latest_semver_tag`; tests
/// can inject controlled responses.
pub async fn run_once_with<F, Fut>(state: &Arc<AppState>, resolve_version: F) -> anyhow::Result<()>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = Option<String>>,
{
    let now = crate::api::now_secs();

    let all_edges = state.dependencies.list_all().await?;

    if all_edges.is_empty() {
        debug!("dep_staleness: no dependency edges to check");
        return Ok(());
    }

    debug!(count = all_edges.len(), "dep_staleness: checking edges");

    // Cache workspace policies so we don't re-fetch for each edge.
    let mut policy_cache: HashMap<String, gyre_domain::DependencyPolicy> = HashMap::new();

    // Cache repo → workspace_id mapping.
    let all_repos = state.repos.list().await.unwrap_or_default();
    let repo_workspace: HashMap<String, String> = all_repos
        .iter()
        .map(|r| (r.id.to_string(), r.workspace_id.to_string()))
        .collect();

    let mut stale_count = 0u32;
    let mut task_count = 0u32;

    for edge in &all_edges {
        // Skip edges that are already in Breaking or Orphaned status — those are
        // managed by other systems.
        if edge.status == DependencyStatus::Breaking || edge.status == DependencyStatus::Orphaned {
            continue;
        }

        // Look up the workspace for this edge's source repo.
        let workspace_id = match repo_workspace.get(&edge.source_repo_id.to_string()) {
            Some(ws) => ws.clone(),
            None => continue,
        };

        // Get or cache the workspace's dependency policy.
        let policy = match policy_cache.get(&workspace_id) {
            Some(p) => p.clone(),
            None => {
                let p = state
                    .dependency_policies
                    .get_for_workspace(&Id::new(&workspace_id))
                    .await
                    .unwrap_or_default();
                policy_cache.insert(workspace_id.clone(), p.clone());
                p
            }
        };

        // Resolve target repo's current version from its latest semver tag.
        let target_repo = all_repos
            .iter()
            .find(|r| r.id.as_str() == edge.target_repo_id.as_str());

        let target_version = if let Some(repo) = target_repo {
            resolve_version(repo.path.clone()).await
        } else {
            None
        };

        let mut updated_edge = edge.clone();
        updated_edge.target_version_current = target_version.clone();

        // Only update last_verified_at when the target version has actually changed,
        // indicating the dependency was genuinely updated. Unconditionally resetting
        // causes time-based staleness to revert after one cycle (TASK-021 F2).
        let version_changed =
            target_version.is_some() && target_version != edge.target_version_current;
        if version_changed {
            updated_edge.last_verified_at = now;
        }

        // Compute version drift if we have both pinned and current versions.
        let drift = if let (Some(pinned), Some(current)) = (&edge.version_pinned, &target_version) {
            crate::version_compute::compute_version_drift(pinned, current)
        } else {
            None
        };
        updated_edge.version_drift = drift;

        // Determine if this edge should be marked stale.
        let mut is_stale = false;

        // Check version drift threshold.
        if policy.max_version_drift > 0 {
            if let Some(d) = drift {
                if d > policy.max_version_drift {
                    is_stale = true;
                    debug!(
                        source = %edge.source_repo_id,
                        target = %edge.target_repo_id,
                        drift = d,
                        threshold = policy.max_version_drift,
                        "dep_staleness: version drift exceeds threshold"
                    );
                }
            }
        }

        // Check time-based staleness. Use updated_edge.last_verified_at (not the
        // stored edge value) so that a version change within this cycle correctly
        // resets the clock and prevents a stale-on-arrival false positive.
        if policy.stale_dependency_alert_days > 0 {
            let threshold_secs = policy.stale_dependency_alert_days as u64 * 86400;
            if now.saturating_sub(updated_edge.last_verified_at) > threshold_secs {
                is_stale = true;
                debug!(
                    source = %edge.source_repo_id,
                    target = %edge.target_repo_id,
                    days_since = (now - edge.last_verified_at) / 86400,
                    "dep_staleness: dependency not updated within alert threshold"
                );
            }
        }

        if is_stale && updated_edge.status != DependencyStatus::Stale {
            updated_edge.status = DependencyStatus::Stale;
            stale_count += 1;

            // Create auto-task if policy allows.
            if policy.auto_create_update_tasks {
                let source_name = all_repos
                    .iter()
                    .find(|r| r.id.as_str() == edge.source_repo_id.as_str())
                    .map(|r| r.name.as_str())
                    .unwrap_or(edge.source_repo_id.as_str());
                let target_name = target_repo
                    .map(|r| r.name.as_str())
                    .unwrap_or(edge.target_repo_id.as_str());

                let title = if let (Some(pinned), Some(current)) =
                    (&edge.version_pinned, &target_version)
                {
                    format!("Update {target_name} dependency from {pinned} to {current}")
                } else {
                    format!("Update stale dependency on {target_name}")
                };

                let task_id = Id::new(uuid::Uuid::new_v4().to_string());
                let mut task = gyre_domain::Task::new(task_id, &title, now);
                task.priority = TaskPriority::Medium;
                task.labels = vec![
                    "dependency-update".to_string(),
                    "stale-dependency".to_string(),
                    "auto-created".to_string(),
                ];
                task.description = Some(format!(
                    "Dependency on '{target_name}' in repo '{source_name}' is stale. \
                     Pinned version: {}. Current version: {}. Drift: {} versions.",
                    edge.version_pinned.as_deref().unwrap_or("unknown"),
                    target_version.as_deref().unwrap_or("unknown"),
                    drift
                        .map(|d| d.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                ));
                task.workspace_id = Id::new(&workspace_id);
                task.repo_id = edge.source_repo_id.clone();

                if let Err(e) = state.tasks.create(&task).await {
                    warn!("dep_staleness: failed to create task: {e}");
                } else {
                    task_count += 1;
                }
            }
        } else if !is_stale && updated_edge.status == DependencyStatus::Stale {
            // Edge was stale but is no longer — revert to Active.
            updated_edge.status = DependencyStatus::Active;
        }

        // Persist the updated edge.
        if let Err(e) = state.dependencies.save(&updated_edge).await {
            warn!("dep_staleness: failed to update edge: {e}");
        }
    }

    if stale_count > 0 || task_count > 0 {
        info!(
            newly_stale = stale_count,
            tasks_created = task_count,
            "dep_staleness: check complete"
        );
    } else {
        debug!("dep_staleness: no new stale dependencies found");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;
    use gyre_domain::{DependencyEdge, DependencyType, DetectionMethod, Repository};

    fn create_edge(
        source_repo_id: &str,
        target_repo_id: &str,
        version_pinned: Option<&str>,
        last_verified_at: u64,
    ) -> DependencyEdge {
        let mut edge = DependencyEdge::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            Id::new(source_repo_id),
            Id::new(target_repo_id),
            DependencyType::Code,
            "Cargo.toml",
            "target-crate",
            DetectionMethod::CargoToml,
            last_verified_at,
        );
        edge.version_pinned = version_pinned.map(|s| s.to_string());
        edge
    }

    async fn create_repo(state: &Arc<AppState>, id: &str, name: &str, ws_id: &str) -> Id {
        let repo = Repository::new(
            Id::new(id),
            Id::new(ws_id),
            name,
            format!("/tmp/{name}.git"),
            0,
        );
        state.repos.create(&repo).await.unwrap();
        repo.id
    }

    #[tokio::test]
    async fn test_time_based_staleness_detection() {
        let state = test_state();

        // Create workspace.
        let ws = gyre_domain::Workspace::new(
            Id::new("ws-stale"),
            Id::new("tenant-1"),
            "stale-ws",
            "stale-ws",
            0,
        );
        state.workspaces.create(&ws).await.unwrap();

        // Set policy: stale_dependency_alert_days = 30 (so 30 * 86400 = 2592000 secs).
        let policy = gyre_domain::DependencyPolicy {
            stale_dependency_alert_days: 30,
            auto_create_update_tasks: false,
            ..Default::default()
        };
        state
            .dependency_policies
            .set_for_workspace(&Id::new("ws-stale"), &policy)
            .await
            .unwrap();

        create_repo(&state, "repo-s1", "repo-s1", "ws-stale").await;
        create_repo(&state, "repo-t1", "repo-t1", "ws-stale").await;

        // Edge with last_verified_at far in the past (0 = epoch).
        let edge = create_edge("repo-s1", "repo-t1", None, 0);
        state.dependencies.save(&edge).await.unwrap();

        // Run the staleness check.
        run_once(&state).await.unwrap();

        // The edge should now be marked Stale.
        let updated = state
            .dependencies
            .find_by_id(&edge.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, DependencyStatus::Stale);
    }

    #[tokio::test]
    async fn test_active_dependency_not_marked_stale() {
        let state = test_state();

        let ws = gyre_domain::Workspace::new(
            Id::new("ws-active"),
            Id::new("tenant-1"),
            "active-ws",
            "active-ws",
            0,
        );
        state.workspaces.create(&ws).await.unwrap();

        create_repo(&state, "repo-a2", "repo-a2", "ws-active").await;
        create_repo(&state, "repo-b2", "repo-b2", "ws-active").await;

        // Edge verified very recently (now).
        let now = crate::api::now_secs();
        let edge = create_edge("repo-a2", "repo-b2", None, now);
        state.dependencies.save(&edge).await.unwrap();

        run_once(&state).await.unwrap();

        let updated = state
            .dependencies
            .find_by_id(&edge.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, DependencyStatus::Active);
    }

    #[tokio::test]
    async fn test_auto_task_creation_on_staleness() {
        let state = test_state();

        let ws = gyre_domain::Workspace::new(
            Id::new("ws-task"),
            Id::new("tenant-1"),
            "task-ws",
            "task-ws",
            0,
        );
        state.workspaces.create(&ws).await.unwrap();

        // Policy: short alert window + auto-create tasks.
        let policy = gyre_domain::DependencyPolicy {
            stale_dependency_alert_days: 1,
            auto_create_update_tasks: true,
            ..Default::default()
        };
        state
            .dependency_policies
            .set_for_workspace(&Id::new("ws-task"), &policy)
            .await
            .unwrap();

        let source_id = create_repo(&state, "repo-src", "repo-src", "ws-task").await;
        create_repo(&state, "repo-tgt", "repo-tgt", "ws-task").await;

        // Edge with old last_verified_at.
        let edge = create_edge("repo-src", "repo-tgt", Some("1.0.0"), 0);
        state.dependencies.save(&edge).await.unwrap();

        run_once(&state).await.unwrap();

        // Verify task was created for the source repo.
        let tasks = state.tasks.list_by_repo(&source_id).await.unwrap();
        assert_eq!(tasks.len(), 1);
        assert!(tasks[0].title.contains("stale dependency"));
        assert!(tasks[0].labels.contains(&"stale-dependency".to_string()));
        assert!(tasks[0].labels.contains(&"auto-created".to_string()));
    }

    #[tokio::test]
    async fn test_no_task_when_auto_create_disabled() {
        let state = test_state();

        let ws = gyre_domain::Workspace::new(
            Id::new("ws-noauto"),
            Id::new("tenant-1"),
            "noauto-ws",
            "noauto-ws",
            0,
        );
        state.workspaces.create(&ws).await.unwrap();

        // auto_create_update_tasks = false.
        let policy = gyre_domain::DependencyPolicy {
            stale_dependency_alert_days: 1,
            auto_create_update_tasks: false,
            ..Default::default()
        };
        state
            .dependency_policies
            .set_for_workspace(&Id::new("ws-noauto"), &policy)
            .await
            .unwrap();

        let source_id = create_repo(&state, "repo-na-src", "repo-na-src", "ws-noauto").await;
        create_repo(&state, "repo-na-tgt", "repo-na-tgt", "ws-noauto").await;

        let edge = create_edge("repo-na-src", "repo-na-tgt", None, 0);
        state.dependencies.save(&edge).await.unwrap();

        run_once(&state).await.unwrap();

        // Edge is stale but no task should be created.
        let updated = state
            .dependencies
            .find_by_id(&edge.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, DependencyStatus::Stale);

        let tasks = state.tasks.list_by_repo(&source_id).await.unwrap();
        assert!(tasks.is_empty());
    }

    #[tokio::test]
    async fn test_breaking_edges_skipped() {
        let state = test_state();

        let ws = gyre_domain::Workspace::new(
            Id::new("ws-brk"),
            Id::new("tenant-1"),
            "break-ws",
            "break-ws",
            0,
        );
        state.workspaces.create(&ws).await.unwrap();

        create_repo(&state, "repo-brk-s", "repo-brk-s", "ws-brk").await;
        create_repo(&state, "repo-brk-t", "repo-brk-t", "ws-brk").await;

        // Edge with Breaking status should be skipped.
        let mut edge = create_edge("repo-brk-s", "repo-brk-t", None, 0);
        edge.status = DependencyStatus::Breaking;
        state.dependencies.save(&edge).await.unwrap();

        run_once(&state).await.unwrap();

        // Status should remain Breaking, not changed to Stale.
        let updated = state
            .dependencies
            .find_by_id(&edge.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, DependencyStatus::Breaking);
    }

    #[tokio::test]
    async fn test_stale_reverts_to_active_when_no_longer_stale() {
        let state = test_state();

        let ws = gyre_domain::Workspace::new(
            Id::new("ws-revert"),
            Id::new("tenant-1"),
            "revert-ws",
            "revert-ws",
            0,
        );
        state.workspaces.create(&ws).await.unwrap();

        // Policy with high thresholds so nothing triggers.
        let policy = gyre_domain::DependencyPolicy {
            max_version_drift: 100,
            stale_dependency_alert_days: 0, // disabled
            auto_create_update_tasks: false,
            ..Default::default()
        };
        state
            .dependency_policies
            .set_for_workspace(&Id::new("ws-revert"), &policy)
            .await
            .unwrap();

        create_repo(&state, "repo-rv-s", "repo-rv-s", "ws-revert").await;
        create_repo(&state, "repo-rv-t", "repo-rv-t", "ws-revert").await;

        // Edge is currently Stale but conditions no longer warrant it.
        let now = crate::api::now_secs();
        let mut edge = create_edge("repo-rv-s", "repo-rv-t", None, now);
        edge.status = DependencyStatus::Stale;
        state.dependencies.save(&edge).await.unwrap();

        run_once(&state).await.unwrap();

        let updated = state
            .dependencies
            .find_by_id(&edge.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, DependencyStatus::Active);
    }

    /// Multi-cycle persistence test (checklist item 71): verifies that a time-based
    /// stale edge remains Stale across subsequent job runs when the underlying
    /// condition has not changed. Catches the F2 bug where unconditional
    /// `last_verified_at = now` caused stale edges to revert to Active.
    #[tokio::test]
    async fn test_time_based_staleness_persists_across_cycles() {
        let state = test_state();

        let ws = gyre_domain::Workspace::new(
            Id::new("ws-persist"),
            Id::new("tenant-1"),
            "persist-ws",
            "persist-ws",
            0,
        );
        state.workspaces.create(&ws).await.unwrap();

        let policy = gyre_domain::DependencyPolicy {
            stale_dependency_alert_days: 30,
            auto_create_update_tasks: false,
            ..Default::default()
        };
        state
            .dependency_policies
            .set_for_workspace(&Id::new("ws-persist"), &policy)
            .await
            .unwrap();

        create_repo(&state, "repo-p-s", "repo-p-s", "ws-persist").await;
        create_repo(&state, "repo-p-t", "repo-p-t", "ws-persist").await;

        // Edge with last_verified_at far in the past (0 = epoch).
        let edge = create_edge("repo-p-s", "repo-p-t", None, 0);
        state.dependencies.save(&edge).await.unwrap();

        // Cycle 1: edge should become Stale.
        run_once(&state).await.unwrap();
        let after_cycle1 = state
            .dependencies
            .find_by_id(&edge.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(after_cycle1.status, DependencyStatus::Stale);

        // Cycle 2: edge should REMAIN Stale — the underlying condition hasn't changed.
        run_once(&state).await.unwrap();
        let after_cycle2 = state
            .dependencies
            .find_by_id(&edge.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            after_cycle2.status,
            DependencyStatus::Stale,
            "stale status must persist across job cycles when condition unchanged"
        );
    }

    /// Multi-cycle recovery test: verifies that a stale edge reverts to Active
    /// when the dependency is genuinely updated (version changes). Uses injectable
    /// version resolution to simulate a version bump between cycles.
    #[tokio::test]
    async fn test_time_based_staleness_recovery_on_version_change() {
        use std::sync::atomic::{AtomicU32, Ordering};

        let state = test_state();

        let ws = gyre_domain::Workspace::new(
            Id::new("ws-recover"),
            Id::new("tenant-1"),
            "recover-ws",
            "recover-ws",
            0,
        );
        state.workspaces.create(&ws).await.unwrap();

        let policy = gyre_domain::DependencyPolicy {
            stale_dependency_alert_days: 30,
            max_version_drift: 100, // high threshold so drift doesn't trigger staleness
            auto_create_update_tasks: false,
            ..Default::default()
        };
        state
            .dependency_policies
            .set_for_workspace(&Id::new("ws-recover"), &policy)
            .await
            .unwrap();

        create_repo(&state, "repo-rc-s", "repo-rc-s", "ws-recover").await;
        create_repo(&state, "repo-rc-t", "repo-rc-t", "ws-recover").await;

        // Edge with old last_verified_at and a known pinned version.
        let mut edge = create_edge("repo-rc-s", "repo-rc-t", Some("1.0.0"), 0);
        edge.target_version_current = Some("1.0.0".to_string());
        state.dependencies.save(&edge).await.unwrap();

        // Track how many times the resolver is called to return different versions.
        let call_count = Arc::new(AtomicU32::new(0));
        let call_count_clone = call_count.clone();

        // Cycle 1: resolver returns "1.0.0" (same as current → no version change
        // → last_verified_at stays at 0 → time-based staleness triggers).
        run_once_with(&state, |_path| {
            let cc = call_count_clone.clone();
            async move {
                let n = cc.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    // Cycle 1: version unchanged.
                    Some("1.0.0".to_string())
                } else {
                    // Cycle 2: version bumped — simulates a real dependency update.
                    Some("2.0.0".to_string())
                }
            }
        })
        .await
        .unwrap();

        let after_cycle1 = state
            .dependencies
            .find_by_id(&edge.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(after_cycle1.status, DependencyStatus::Stale);
        // last_verified_at should NOT have been updated (version didn't change).
        assert_eq!(after_cycle1.last_verified_at, 0);

        // Cycle 2: resolver returns "2.0.0" (different from stored "1.0.0"
        // → version_changed = true → last_verified_at updated to now
        // → time-based check: now - now = 0 < threshold → not stale → reverts to Active).
        run_once_with(&state, |_path| async { Some("2.0.0".to_string()) })
            .await
            .unwrap();

        let after_cycle2 = state
            .dependencies
            .find_by_id(&edge.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            after_cycle2.status,
            DependencyStatus::Active,
            "edge should revert to Active after dependency version changes"
        );
        // last_verified_at should have been updated to now.
        assert_ne!(after_cycle2.last_verified_at, 0);
        // target_version_current should reflect the new version.
        assert_eq!(
            after_cycle2.target_version_current,
            Some("2.0.0".to_string())
        );
    }
}
