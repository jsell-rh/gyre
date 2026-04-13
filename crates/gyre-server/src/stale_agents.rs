//! Stale agent detection: marks agents Dead when heartbeat times out.
//! Honors each agent's `disconnected_behavior` setting (BCP graceful degradation).

use gyre_common::{message::MessageKind, Id};
use gyre_domain::{AgentStatus, DisconnectedBehavior};
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::AppState;

const HEARTBEAT_TIMEOUT_SECS: u64 = 60;

/// Run one stale-agent detection cycle. Used by the job framework for manual triggering.
pub async fn run_once(state: &AppState) -> anyhow::Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let agents = state.agents.list().await?;
    for mut agent in agents {
        // Skip agents in terminal states.
        if matches!(
            agent.status,
            AgentStatus::Dead | AgentStatus::Stopped | AgentStatus::Failed
        ) {
            continue;
        }
        if agent.is_alive(now, HEARTBEAT_TIMEOUT_SECS) {
            continue;
        }

        match agent.disconnected_behavior {
            DisconnectedBehavior::Abort => {
                info!(agent_id = %agent.id, agent_name = %agent.name,
                    "aborting stale agent (disconnected_behavior=abort)");
                let _ = agent.transition_status(AgentStatus::Dead);
                let _ = state.agents.update(&agent).await;

                // Clean up worktrees
                if let Ok(worktrees) = state.worktrees.find_by_agent(&agent.id).await {
                    for wt in worktrees {
                        if let Ok(Some(repo)) = state.repos.find_by_id(&wt.repository_id).await {
                            if let Err(e) =
                                state.git_ops.remove_worktree(&repo.path, &wt.path).await
                            {
                                warn!("remove_worktree failed for agent {}: {e}", agent.id);
                            }
                        }
                        let _ = state.worktrees.delete(&wt.id).await;
                    }
                }

                // Block the assigned task
                if let Some(task_id) = &agent.current_task_id {
                    if let Ok(Some(mut task)) = state.tasks.find_by_id(task_id).await {
                        use gyre_domain::TaskStatus;
                        if task.status == TaskStatus::InProgress {
                            let _ = task.transition_status(TaskStatus::Blocked);
                            task.updated_at = now;
                            let _ = state.tasks.update(&task).await;
                        }
                    }
                }

                // HSI §4: Clean up interrogation ABAC policies for dead agents.
                let agent_id_str = agent.id.to_string();
                crate::api::spawn::cleanup_interrogation_policies(state, &agent_id_str).await;
                let _ = state
                    .kv_store
                    .kv_remove("interrogation_context", &agent_id_str)
                    .await;

                let ws_id = agent.workspace_id.clone();
                state.emit_telemetry(
                    ws_id.clone(),
                    MessageKind::AgentStatusChanged,
                    Some(serde_json::json!({
                        "agent_id": agent.id.to_string(),
                        "status": "dead",
                        "reason": format!("Agent {} aborted (no heartbeat, abort behavior)", agent.name),
                    })),
                );

                // Notify the spawning user that the agent was abandoned (HSI §2).
                if let Some(ref spawned_by) = agent.spawned_by {
                    crate::notifications::notify(
                        state,
                        ws_id,
                        Id::new(spawned_by.clone()),
                        gyre_common::NotificationType::AbandonedBranch,
                        format!("Agent '{}' was abandoned (heartbeat timeout)", agent.name),
                        "default",
                    )
                    .await;
                }
            }

            DisconnectedBehavior::Pause => {
                info!(agent_id = %agent.id, agent_name = %agent.name,
                    "stopping stale agent (disconnected_behavior=pause)");
                let _ = agent.transition_status(AgentStatus::Stopped);
                let _ = state.agents.update(&agent).await;

                let ws_id = agent.workspace_id.clone();
                state.emit_telemetry(
                    ws_id,
                    MessageKind::AgentStatusChanged,
                    Some(serde_json::json!({
                        "agent_id": agent.id.to_string(),
                        "status": "stopped",
                        "reason": format!("Agent {} stopped (no heartbeat, pause behavior)", agent.name),
                    })),
                );
            }

            DisconnectedBehavior::ContinueOffline => {
                // Leave agent running; log a warning only.
                warn!(agent_id = %agent.id, agent_name = %agent.name,
                    "agent heartbeat timed out but disconnected_behavior=continue_offline; leaving running");
            }
        }
    }
    Ok(())
}

pub fn spawn_stale_agent_detector(state: Arc<AppState>) {
    const CHECK_INTERVAL_SECS: u64 = 30;

    tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(tokio::time::Duration::from_secs(CHECK_INTERVAL_SECS));
        loop {
            interval.tick().await;
            if let Err(e) = run_once(&state).await {
                error!("stale agent check failed: {e}");
            }
        }
    });
}
