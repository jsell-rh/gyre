//! Speculative merge background job (M13.5).
//!
//! Runs every 60 seconds. For each active agent branch, attempts a speculative
//! merge against the target branch (main). On conflict, emits a `SpeculativeConflict`
//! domain event. On clean merge, stores the result and emits `SpeculativeMergeClean`.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use tracing::{info, warn};

use crate::AppState;

/// Result of a speculative merge attempt for a given branch.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpeculativeResult {
    pub repo_id: String,
    pub branch: String,
    pub status: SpeculativeStatus,
    pub conflicting_files: Vec<String>,
    /// The other branch that conflicts with this one (if any).
    pub conflicting_branch: Option<String>,
    /// The agent_id working on the conflicting branch (if any).
    pub conflicting_agent_id: Option<String>,
    /// Whether the conflict is order-dependent or order-independent.
    /// `None` when status is not `Conflict`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflict_type: Option<ConflictType>,
    pub detected_at: u64,
}

/// Distinguishes "conflicts regardless of merge order" from
/// "conflicts only if merged in this specific order" (merge-dependencies.md §Speculative Merge).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ConflictType {
    /// The branch conflicts directly with the target branch (main) — merge order does not matter.
    OrderIndependent,
    /// The branch conflicts because a previously speculated branch introduced conflicting changes.
    OrderDependent,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SpeculativeStatus {
    Clean,
    Conflict,
    /// Unable to attempt merge (e.g. branch not found).
    Skipped,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Run one iteration of the speculative merge job.
///
/// Collects all active agent branches, builds a dependency graph from their
/// associated MRs, and speculates in dependency order — skipping branches
/// whose dependency MRs haven't been speculated clean yet.
/// Atomic group members are speculated as a single unit.
pub async fn run_once(state: &Arc<AppState>) -> Result<()> {
    // Get all active agents.
    let active_agents = state
        .agents
        .list_by_status(&gyre_domain::AgentStatus::Active)
        .await?;

    info!(
        agent_count = active_agents.len(),
        "speculative merge: checking {} active agents",
        active_agents.len()
    );

    // Collect all (repo_id, branch, agent_id) tuples to process.
    struct BranchInfo {
        repo_id: String,
        repo_path: String,
        branch: String,
        agent_id: String,
        // MR dependency target IDs (branches whose MRs must be speculated clean first).
        dep_mr_ids: HashSet<String>,
        atomic_group: Option<String>,
    }

    let mut branches: Vec<BranchInfo> = Vec::new();
    // Map from MR ID → branch name for dependency resolution.
    let mut mr_id_to_branch: HashMap<String, String> = HashMap::new();

    for agent in &active_agents {
        let worktrees = state
            .worktrees
            .find_by_agent(&agent.id)
            .await
            .unwrap_or_default();

        for wt in &worktrees {
            let repo = match state.repos.find_by_id(&wt.repository_id).await {
                Ok(Some(r)) => r,
                _ => continue,
            };

            let branch = wt.branch.clone();
            if branch == "main" || branch == "master" {
                continue;
            }

            // Find the MR for this branch to get dependency info.
            let mrs = state
                .merge_requests
                .list_by_repo(&wt.repository_id)
                .await
                .unwrap_or_default();
            let mr = mrs.iter().find(|m| {
                m.source_branch == branch
                    && matches!(
                        m.status,
                        gyre_domain::MrStatus::Open | gyre_domain::MrStatus::Approved
                    )
            });

            let (dep_mr_ids, atomic_group) = if let Some(mr) = mr {
                mr_id_to_branch.insert(mr.id.to_string(), branch.clone());
                let deps: HashSet<String> = mr
                    .depends_on
                    .iter()
                    .map(|d| d.target_mr_id.to_string())
                    .collect();
                (deps, mr.atomic_group.clone())
            } else {
                (HashSet::new(), None)
            };

            branches.push(BranchInfo {
                repo_id: wt.repository_id.to_string(),
                repo_path: repo.path.clone(),
                branch,
                agent_id: agent.id.to_string(),
                dep_mr_ids,
                atomic_group,
            });
        }
    }

    // Build dependency order: resolve MR ID deps to branch names.
    // Track which MR IDs have been speculated clean (or are already merged).
    let mut speculated_mr_ids: HashSet<String> = HashSet::new();

    // Pre-populate with already-merged MR deps (F1 fix): if a dependency MR
    // is already merged, it is fully satisfied and should not block dependents.
    {
        let all_dep_ids: HashSet<String> = branches
            .iter()
            .flat_map(|bi| bi.dep_mr_ids.iter().cloned())
            .collect();
        for dep_id in &all_dep_ids {
            if let Ok(Some(dep_mr)) = state
                .merge_requests
                .find_by_id(&gyre_common::Id::new(dep_id))
                .await
            {
                if dep_mr.status == gyre_domain::MrStatus::Merged {
                    speculated_mr_ids.insert(dep_id.clone());
                }
            }
        }
    }

    // Process in waves: each wave, speculate branches whose deps are all satisfied.
    // Track branches speculated clean for conflict type classification.
    let mut speculated_clean_branches: HashSet<String> = HashSet::new();
    let mut remaining: VecDeque<usize> = (0..branches.len()).collect();
    let mut progress = true;

    while progress && !remaining.is_empty() {
        progress = false;
        let mut still_remaining = VecDeque::new();

        while let Some(idx) = remaining.pop_front() {
            let bi = &branches[idx];

            // Check if all dependency MRs have been speculated clean.
            let deps_satisfied = bi
                .dep_mr_ids
                .iter()
                .all(|dep_id| speculated_mr_ids.contains(dep_id));

            if !deps_satisfied {
                still_remaining.push_back(idx);
                continue;
            }

            // For atomic group members, check if all other group members
            // are also ready (deps satisfied). If not, defer.
            if let Some(ref group) = bi.atomic_group {
                let group_ready = branches.iter().enumerate().all(|(j, bj)| {
                    if j == idx {
                        return true;
                    }
                    if bj.atomic_group.as_deref() != Some(group) {
                        return true;
                    }
                    bj.dep_mr_ids
                        .iter()
                        .all(|dep_id| speculated_mr_ids.contains(dep_id))
                });
                if !group_ready {
                    still_remaining.push_back(idx);
                    continue;
                }
            }

            progress = true;

            // Attempt speculative merge.
            let result = speculate_branch(
                state,
                &bi.repo_id,
                &bi.repo_path,
                &bi.branch,
                &bi.agent_id,
                &speculated_clean_branches,
            )
            .await;

            if result.status == SpeculativeStatus::Clean {
                speculated_clean_branches.insert(bi.branch.clone());
                // Mark associated MR IDs as speculated clean.
                for (mr_id, branch) in &mr_id_to_branch {
                    if branch == &bi.branch {
                        speculated_mr_ids.insert(mr_id.clone());
                    }
                }
            }

            // Store and emit events.
            store_and_emit(state, &bi.repo_id, &bi.branch, result).await;
        }

        remaining = still_remaining;
    }

    // Any remaining branches have unsatisfied dependency order — skip them.
    for idx in remaining {
        let bi = &branches[idx];
        info!(
            repo_id = %bi.repo_id,
            branch = %bi.branch,
            "speculative merge: skipped (unsatisfied dependency order)"
        );
        let result = SpeculativeResult {
            repo_id: bi.repo_id.clone(),
            branch: bi.branch.clone(),
            status: SpeculativeStatus::Skipped,
            conflicting_files: vec![],
            conflicting_branch: None,
            conflicting_agent_id: None,
            conflict_type: None,
            detected_at: now_secs(),
        };
        store_and_emit(state, &bi.repo_id, &bi.branch, result).await;
    }

    Ok(())
}

/// Attempt a speculative merge for a single branch against main.
///
/// `speculated_clean_branches` contains branches already speculated clean in
/// this wave — used to classify conflicts as order-dependent vs order-independent
/// (merge-dependencies.md §Speculative Merge Integration).
async fn speculate_branch(
    state: &Arc<AppState>,
    repo_id: &str,
    repo_path: &str,
    branch: &str,
    agent_id: &str,
    speculated_clean_branches: &HashSet<String>,
) -> SpeculativeResult {
    let can_merge = state.git_ops.can_merge(repo_path, branch, "main").await;

    match can_merge {
        Ok(true) => {
            info!(
                repo_id = %repo_id,
                branch = %branch,
                "speculative merge: clean"
            );
            SpeculativeResult {
                repo_id: repo_id.to_string(),
                branch: branch.to_string(),
                status: SpeculativeStatus::Clean,
                conflicting_files: vec![],
                conflicting_branch: None,
                conflicting_agent_id: None,
                conflict_type: None,
                detected_at: now_secs(),
            }
        }
        Ok(false) => {
            warn!(
                repo_id = %repo_id,
                branch = %branch,
                "speculative merge: conflict detected"
            );

            let (conflicting_branch, conflicting_agent_id) =
                find_conflicting_agent(state, repo_id, branch, agent_id.to_string()).await;

            // Classify the conflict: if the conflicting branch was speculated
            // clean in a prior wave, the conflict is order-dependent (it arises
            // from the interaction of two branches). Otherwise it conflicts
            // directly with the target branch — order-independent.
            let conflict_type = if let Some(ref cb) = conflicting_branch {
                if speculated_clean_branches.contains(cb) {
                    ConflictType::OrderDependent
                } else {
                    ConflictType::OrderIndependent
                }
            } else {
                ConflictType::OrderIndependent
            };

            SpeculativeResult {
                repo_id: repo_id.to_string(),
                branch: branch.to_string(),
                status: SpeculativeStatus::Conflict,
                conflicting_files: vec![],
                conflicting_branch,
                conflicting_agent_id,
                conflict_type: Some(conflict_type),
                detected_at: now_secs(),
            }
        }
        Err(e) => {
            warn!(
                repo_id = %repo_id,
                branch = %branch,
                error = %e,
                "speculative merge: skipped (error)"
            );
            SpeculativeResult {
                repo_id: repo_id.to_string(),
                branch: branch.to_string(),
                status: SpeculativeStatus::Skipped,
                conflicting_files: vec![],
                conflicting_branch: None,
                conflicting_agent_id: None,
                conflict_type: None,
                detected_at: now_secs(),
            }
        }
    }
}

/// Store a speculative merge result and emit the corresponding domain event.
async fn store_and_emit(
    state: &Arc<AppState>,
    repo_id: &str,
    branch: &str,
    result: SpeculativeResult,
) {
    {
        let mut results = state.speculative_results.lock().await;
        results.insert((repo_id.to_string(), branch.to_string()), result.clone());
    }

    let ws_id = state
        .repos
        .find_by_id(&gyre_common::Id::new(repo_id))
        .await
        .ok()
        .flatten()
        .map(|r| r.workspace_id)
        .unwrap_or_else(|| gyre_common::Id::new("default"));

    match result.status {
        SpeculativeStatus::Clean => {
            state
                .emit_event(
                    Some(ws_id.clone()),
                    gyre_common::message::Destination::Workspace(ws_id),
                    gyre_common::message::MessageKind::SpeculativeMergeClean,
                    Some(serde_json::json!({"repo_id": repo_id, "branch": branch})),
                )
                .await;
        }
        SpeculativeStatus::Conflict => {
            state
                .emit_event(
                    Some(ws_id.clone()),
                    gyre_common::message::Destination::Workspace(ws_id),
                    gyre_common::message::MessageKind::SpeculativeConflict,
                    Some(serde_json::json!({
                        "repo_id": repo_id,
                        "branch": branch,
                        "conflicting_files": result.conflicting_files,
                    })),
                )
                .await;
        }
        SpeculativeStatus::Skipped => {}
    }
}

/// Find another active agent whose branch conflicts with `branch` in the same repo.
/// Returns `(conflicting_branch, conflicting_agent_id)` or `(None, None)`.
async fn find_conflicting_agent(
    state: &Arc<AppState>,
    repo_id: &str,
    branch: &str,
    exclude_agent_id: String,
) -> (Option<String>, Option<String>) {
    let active_agents = match state
        .agents
        .list_by_status(&gyre_domain::AgentStatus::Active)
        .await
    {
        Ok(a) => a,
        Err(_) => return (None, None),
    };

    for other_agent in active_agents {
        if other_agent.id.to_string() == exclude_agent_id {
            continue;
        }

        let worktrees = match state.worktrees.find_by_agent(&other_agent.id).await {
            Ok(w) => w,
            Err(_) => continue,
        };

        for wt in worktrees {
            if wt.repository_id.as_str() != repo_id {
                continue;
            }
            if wt.branch == branch || wt.branch == "main" || wt.branch == "master" {
                continue;
            }

            // Check if the two branches have overlapping commits against main.
            // We use touched-paths from agent_commits as a proxy.
            let our_commits = state
                .agent_commits
                .find_by_agent(&gyre_common::Id::new(exclude_agent_id.clone()))
                .await
                .unwrap_or_default();
            let their_commits = state
                .agent_commits
                .find_by_agent(&other_agent.id)
                .await
                .unwrap_or_default();

            // If both agents have commits in the same repo, assume potential conflict.
            let our_in_repo = our_commits
                .iter()
                .any(|c| c.repository_id.as_str() == repo_id);
            let their_in_repo = their_commits
                .iter()
                .any(|c| c.repository_id.as_str() == repo_id);

            if our_in_repo && their_in_repo {
                return (Some(wt.branch.clone()), Some(other_agent.id.to_string()));
            }
        }
    }

    (None, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;

    #[tokio::test]
    async fn run_once_with_no_agents() {
        let state = test_state();
        let result = run_once(&state).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn speculative_result_serializes() {
        let r = SpeculativeResult {
            repo_id: "r1".to_string(),
            branch: "feat/x".to_string(),
            status: SpeculativeStatus::Clean,
            conflicting_files: vec![],
            conflicting_branch: None,
            conflicting_agent_id: None,
            conflict_type: None,
            detected_at: 1000,
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("clean"));
        // conflict_type should be absent for Clean results (skip_serializing_if).
        assert!(!json.contains("conflict_type"));
    }

    #[tokio::test]
    async fn conflict_result_serializes() {
        let r = SpeculativeResult {
            repo_id: "r1".to_string(),
            branch: "feat/y".to_string(),
            status: SpeculativeStatus::Conflict,
            conflicting_files: vec!["src/lib.rs".to_string()],
            conflicting_branch: Some("feat/z".to_string()),
            conflicting_agent_id: Some("agent-1".to_string()),
            conflict_type: Some(ConflictType::OrderIndependent),
            detected_at: 2000,
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("conflict"));
        assert!(json.contains("src/lib.rs"));
        assert!(json.contains("order_independent"));
    }

    #[tokio::test]
    async fn order_dependent_conflict_serializes() {
        let r = SpeculativeResult {
            repo_id: "r1".to_string(),
            branch: "feat/a".to_string(),
            status: SpeculativeStatus::Conflict,
            conflicting_files: vec![],
            conflicting_branch: Some("feat/b".to_string()),
            conflicting_agent_id: None,
            conflict_type: Some(ConflictType::OrderDependent),
            detected_at: 3000,
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("order_dependent"));
    }

    // ── Speculative merge dependency-aware behavior tests (TASK-028 F2) ──

    /// Helper: set up a repo, agent, worktree, and MR for speculative merge tests.
    async fn setup_branch(
        state: &std::sync::Arc<crate::AppState>,
        agent_id: &str,
        repo_id: &str,
        branch: &str,
        mr_id: &str,
    ) {
        use gyre_common::Id;
        use gyre_domain::{Agent, AgentStatus, AgentWorktree, MergeRequest, Repository};

        // Create repo if it doesn't exist.
        if state
            .repos
            .find_by_id(&Id::new(repo_id))
            .await
            .ok()
            .flatten()
            .is_none()
        {
            let repo = Repository::new(
                Id::new(repo_id),
                Id::new("ws-test"),
                repo_id,
                &format!("/tmp/{repo_id}"),
                1000,
            );
            state.repos.create(&repo).await.unwrap();
        }

        // Create agent if it doesn't exist.
        if state
            .agents
            .find_by_id(&Id::new(agent_id))
            .await
            .ok()
            .flatten()
            .is_none()
        {
            let mut agent = Agent::new(Id::new(agent_id), agent_id, 1000);
            agent.status = AgentStatus::Active;
            state.agents.create(&agent).await.unwrap();
        }

        // Create worktree.
        let wt = AgentWorktree::new(
            Id::new(&format!("wt-{agent_id}-{branch}")),
            Id::new(agent_id),
            Id::new(repo_id),
            None,
            branch,
            &format!("/tmp/worktrees/{agent_id}/{branch}"),
            1000,
        );
        state.worktrees.create(&wt).await.unwrap();

        // Create MR.
        let mr = MergeRequest::new(
            Id::new(mr_id),
            Id::new(repo_id),
            &format!("MR for {branch}"),
            branch,
            "main",
            1000,
        );
        state.merge_requests.create(&mr).await.unwrap();
    }

    #[tokio::test]
    async fn branch_with_unsatisfied_deps_is_skipped() {
        use gyre_common::Id;
        use gyre_domain::{DependencySource, MergeRequestDependency};

        let state = test_state();

        // Set up two branches: branch-a (no deps) and branch-b (depends on mr-a).
        setup_branch(&state, "agent-a", "repo1", "feat/a", "mr-a").await;
        setup_branch(&state, "agent-b", "repo1", "feat/b", "mr-b").await;

        // Set branch-b to depend on mr-a.
        let mut mr_b = state
            .merge_requests
            .find_by_id(&Id::new("mr-b"))
            .await
            .unwrap()
            .unwrap();
        mr_b.depends_on = vec![MergeRequestDependency::new(
            Id::new("mr-a"),
            DependencySource::Explicit,
        )];
        state.merge_requests.update(&mr_b).await.unwrap();

        // Run speculative merge.
        let result = run_once(&state).await;
        assert!(result.is_ok());

        // Both should be speculated: branch-a is clean (no deps), branch-b's dep
        // (mr-a) was speculated clean, so branch-b should also be processed.
        let results = state.speculative_results.lock().await;
        let result_a = results.get(&("repo1".to_string(), "feat/a".to_string()));
        let result_b = results.get(&("repo1".to_string(), "feat/b".to_string()));

        assert!(result_a.is_some(), "branch-a should have been speculated");
        assert_eq!(result_a.unwrap().status, SpeculativeStatus::Clean);

        assert!(
            result_b.is_some(),
            "branch-b should have been speculated (deps satisfied)"
        );
        assert_eq!(result_b.unwrap().status, SpeculativeStatus::Clean);
    }

    #[tokio::test]
    async fn branch_with_circular_dep_is_skipped() {
        use gyre_common::Id;
        use gyre_domain::{DependencySource, MergeRequestDependency};

        let state = test_state();

        // Set up two branches with circular dependency: a depends on b, b depends on a.
        setup_branch(&state, "agent-c1", "repo2", "feat/c1", "mr-c1").await;
        setup_branch(&state, "agent-c2", "repo2", "feat/c2", "mr-c2").await;

        let mut mr_c1 = state
            .merge_requests
            .find_by_id(&Id::new("mr-c1"))
            .await
            .unwrap()
            .unwrap();
        mr_c1.depends_on = vec![MergeRequestDependency::new(
            Id::new("mr-c2"),
            DependencySource::Explicit,
        )];
        state.merge_requests.update(&mr_c1).await.unwrap();

        let mut mr_c2 = state
            .merge_requests
            .find_by_id(&Id::new("mr-c2"))
            .await
            .unwrap()
            .unwrap();
        mr_c2.depends_on = vec![MergeRequestDependency::new(
            Id::new("mr-c1"),
            DependencySource::Explicit,
        )];
        state.merge_requests.update(&mr_c2).await.unwrap();

        let result = run_once(&state).await;
        assert!(result.is_ok());

        // Both branches should be skipped — neither dep can ever be satisfied.
        let results = state.speculative_results.lock().await;
        let result_c1 = results.get(&("repo2".to_string(), "feat/c1".to_string()));
        let result_c2 = results.get(&("repo2".to_string(), "feat/c2".to_string()));

        assert!(result_c1.is_some(), "branch-c1 should have a result");
        assert_eq!(result_c1.unwrap().status, SpeculativeStatus::Skipped);

        assert!(result_c2.is_some(), "branch-c2 should have a result");
        assert_eq!(result_c2.unwrap().status, SpeculativeStatus::Skipped);
    }

    #[tokio::test]
    async fn already_merged_dep_does_not_block_dependent() {
        use gyre_common::Id;
        use gyre_domain::{DependencySource, MergeRequestDependency, MrStatus};

        let state = test_state();

        // branch-d depends on mr-merged, which is already merged.
        setup_branch(&state, "agent-d", "repo3", "feat/d", "mr-d").await;

        // Create the merged MR (no associated active agent/worktree).
        let mut mr_merged = gyre_domain::MergeRequest::new(
            Id::new("mr-merged"),
            Id::new("repo3"),
            "Already merged",
            "feat/merged",
            "main",
            500,
        );
        mr_merged.status = MrStatus::Merged;
        state.merge_requests.create(&mr_merged).await.unwrap();

        // Set branch-d to depend on mr-merged.
        let mut mr_d = state
            .merge_requests
            .find_by_id(&Id::new("mr-d"))
            .await
            .unwrap()
            .unwrap();
        mr_d.depends_on = vec![MergeRequestDependency::new(
            Id::new("mr-merged"),
            DependencySource::Explicit,
        )];
        state.merge_requests.update(&mr_d).await.unwrap();

        let result = run_once(&state).await;
        assert!(result.is_ok());

        // branch-d should be speculated clean — the merged dep is pre-populated
        // as satisfied (F1 fix).
        let results = state.speculative_results.lock().await;
        let result_d = results.get(&("repo3".to_string(), "feat/d".to_string()));
        assert!(result_d.is_some(), "branch-d should have been speculated");
        assert_eq!(
            result_d.unwrap().status,
            SpeculativeStatus::Clean,
            "branch-d should be clean because its dependency is already merged"
        );
    }

    #[tokio::test]
    async fn atomic_group_members_speculated_together() {
        use gyre_common::Id;
        use gyre_domain::{DependencySource, MergeRequestDependency};

        let state = test_state();

        // Two branches in the same atomic group, plus a dep outside the group.
        setup_branch(&state, "agent-g1", "repo4", "feat/g1", "mr-g1").await;
        setup_branch(&state, "agent-g2", "repo4", "feat/g2", "mr-g2").await;
        setup_branch(&state, "agent-dep", "repo4", "feat/dep", "mr-dep").await;

        // Put g1 and g2 in the same atomic group.
        let mut mr_g1 = state
            .merge_requests
            .find_by_id(&Id::new("mr-g1"))
            .await
            .unwrap()
            .unwrap();
        mr_g1.atomic_group = Some("migration-bundle".to_string());
        state.merge_requests.update(&mr_g1).await.unwrap();

        let mut mr_g2 = state
            .merge_requests
            .find_by_id(&Id::new("mr-g2"))
            .await
            .unwrap()
            .unwrap();
        mr_g2.atomic_group = Some("migration-bundle".to_string());
        // g2 depends on mr-dep (outside the group).
        mr_g2.depends_on = vec![MergeRequestDependency::new(
            Id::new("mr-dep"),
            DependencySource::Explicit,
        )];
        state.merge_requests.update(&mr_g2).await.unwrap();

        let result = run_once(&state).await;
        assert!(result.is_ok());

        // All three should be speculated clean: mr-dep first (no deps),
        // then g1 and g2 together (g2's dep on mr-dep is satisfied).
        let results = state.speculative_results.lock().await;
        assert_eq!(
            results
                .get(&("repo4".to_string(), "feat/dep".to_string()))
                .unwrap()
                .status,
            SpeculativeStatus::Clean,
        );
        assert_eq!(
            results
                .get(&("repo4".to_string(), "feat/g1".to_string()))
                .unwrap()
                .status,
            SpeculativeStatus::Clean,
        );
        assert_eq!(
            results
                .get(&("repo4".to_string(), "feat/g2".to_string()))
                .unwrap()
                .status,
            SpeculativeStatus::Clean,
        );
    }

    #[tokio::test]
    async fn dep_on_nonexistent_mr_blocks_branch() {
        use gyre_common::Id;
        use gyre_domain::{DependencySource, MergeRequestDependency};

        let state = test_state();

        // Branch depends on an MR that doesn't exist in the system.
        setup_branch(&state, "agent-e", "repo5", "feat/e", "mr-e").await;

        let mut mr_e = state
            .merge_requests
            .find_by_id(&Id::new("mr-e"))
            .await
            .unwrap()
            .unwrap();
        mr_e.depends_on = vec![MergeRequestDependency::new(
            Id::new("mr-nonexistent"),
            DependencySource::Explicit,
        )];
        state.merge_requests.update(&mr_e).await.unwrap();

        let result = run_once(&state).await;
        assert!(result.is_ok());

        // Branch should be skipped — dep can never be satisfied.
        let results = state.speculative_results.lock().await;
        let result_e = results.get(&("repo5".to_string(), "feat/e".to_string()));
        assert!(result_e.is_some());
        assert_eq!(result_e.unwrap().status, SpeculativeStatus::Skipped);
    }

    #[tokio::test]
    async fn dependency_order_is_respected() {
        use gyre_common::Id;
        use gyre_domain::{DependencySource, MergeRequestDependency};

        let state = test_state();

        // Chain: feat/z1 -> feat/z2 -> feat/z3 (z3 depends on z2, z2 depends on z1).
        setup_branch(&state, "agent-z1", "repo6", "feat/z1", "mr-z1").await;
        setup_branch(&state, "agent-z2", "repo6", "feat/z2", "mr-z2").await;
        setup_branch(&state, "agent-z3", "repo6", "feat/z3", "mr-z3").await;

        let mut mr_z2 = state
            .merge_requests
            .find_by_id(&Id::new("mr-z2"))
            .await
            .unwrap()
            .unwrap();
        mr_z2.depends_on = vec![MergeRequestDependency::new(
            Id::new("mr-z1"),
            DependencySource::Explicit,
        )];
        state.merge_requests.update(&mr_z2).await.unwrap();

        let mut mr_z3 = state
            .merge_requests
            .find_by_id(&Id::new("mr-z3"))
            .await
            .unwrap()
            .unwrap();
        mr_z3.depends_on = vec![MergeRequestDependency::new(
            Id::new("mr-z2"),
            DependencySource::Explicit,
        )];
        state.merge_requests.update(&mr_z3).await.unwrap();

        let result = run_once(&state).await;
        assert!(result.is_ok());

        // All three should be speculated clean — z1 first, then z2, then z3.
        let results = state.speculative_results.lock().await;
        assert_eq!(
            results
                .get(&("repo6".to_string(), "feat/z1".to_string()))
                .unwrap()
                .status,
            SpeculativeStatus::Clean,
        );
        assert_eq!(
            results
                .get(&("repo6".to_string(), "feat/z2".to_string()))
                .unwrap()
                .status,
            SpeculativeStatus::Clean,
        );
        assert_eq!(
            results
                .get(&("repo6".to_string(), "feat/z3".to_string()))
                .unwrap()
                .status,
            SpeculativeStatus::Clean,
        );
    }
}
