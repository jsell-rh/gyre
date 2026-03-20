//! Speculative merge background job (M13.5).
//!
//! Runs every 60 seconds. For each active agent branch, attempts a speculative
//! merge against the target branch (main). On conflict, emits a `SpeculativeConflict`
//! domain event. On clean merge, stores the result and emits `SpeculativeMergeClean`.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, warn};

use crate::domain_events::DomainEvent;
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
/// For each active agent that has a worktree, attempt `can_merge` against "main".
/// Record the result in `state.speculative_results` and emit domain events.
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

    for agent in &active_agents {
        // Find worktrees for this agent.
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
            let repo_id = wt.repository_id.to_string();

            // Skip main/master — no speculative merge needed.
            if branch == "main" || branch == "master" {
                continue;
            }

            // Attempt speculative merge via can_merge.
            let can_merge = state.git_ops.can_merge(&repo.path, &branch, "main").await;

            let result = match can_merge {
                Ok(true) => {
                    info!(
                        repo_id = %repo_id,
                        branch = %branch,
                        "speculative merge: clean"
                    );
                    SpeculativeResult {
                        repo_id: repo_id.clone(),
                        branch: branch.clone(),
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

                    // Try to find which other branch conflicts by checking other active
                    // agents' branches that touch overlapping paths.
                    let (conflicting_branch, conflicting_agent_id) =
                        find_conflicting_agent(state, &repo_id, &branch, agent.id.to_string())
                            .await;

                    SpeculativeResult {
                        repo_id: repo_id.clone(),
                        branch: branch.clone(),
                        status: SpeculativeStatus::Conflict,
                        // We don't have per-file conflict detail from can_merge; provide empty list.
                        // A full implementation would use git merge-tree output.
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
                        repo_id: repo_id.clone(),
                        branch: branch.clone(),
                        status: SpeculativeStatus::Skipped,
                        conflicting_files: vec![],
                        conflicting_branch: None,
                        conflicting_agent_id: None,
                        detected_at: now_secs(),
                    }
                }
            };

            // Store the result.
            {
                let mut results = state.speculative_results.lock().await;
                results.insert((repo_id.clone(), branch.clone()), result.clone());
            }

            // Emit domain event.
            match result.status {
                SpeculativeStatus::Clean => {
                    let _ = state
                        .event_tx
                        .send(DomainEvent::SpeculativeMergeClean { repo_id, branch });
                }
                SpeculativeStatus::Conflict => {
                    let _ = state.event_tx.send(DomainEvent::SpeculativeConflict {
                        repo_id,
                        branch,
                        conflicting_files: result.conflicting_files,
                    });
                }
                SpeculativeStatus::Skipped => {}
            }
        }
    }

    Ok(())
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
