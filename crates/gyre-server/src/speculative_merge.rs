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
    pub detected_at: u64,
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
                mr_id_to_branch
                    .insert(mr.id.to_string(), branch.clone());
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
    // Track which branches have been speculated clean.
    let mut speculated_clean: HashSet<String> = HashSet::new(); // branch names
    let mut speculated_mr_ids: HashSet<String> = HashSet::new(); // MR IDs speculated clean

    // Process in waves: each wave, speculate branches whose deps are all satisfied.
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
            )
            .await;

            if result.status == SpeculativeStatus::Clean {
                speculated_clean.insert(bi.branch.clone());
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
            detected_at: now_secs(),
        };
        store_and_emit(state, &bi.repo_id, &bi.branch, result).await;
    }

    Ok(())
}

/// Attempt a speculative merge for a single branch against main.
async fn speculate_branch(
    state: &Arc<AppState>,
    repo_id: &str,
    repo_path: &str,
    branch: &str,
    agent_id: &str,
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

            SpeculativeResult {
                repo_id: repo_id.to_string(),
                branch: branch.to_string(),
                status: SpeculativeStatus::Conflict,
                conflicting_files: vec![],
                conflicting_branch,
                conflicting_agent_id,
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
        results.insert(
            (repo_id.to_string(), branch.to_string()),
            result.clone(),
        );
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
            detected_at: 1000,
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("clean"));
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
            detected_at: 2000,
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("conflict"));
        assert!(json.contains("src/lib.rs"));
    }
}
