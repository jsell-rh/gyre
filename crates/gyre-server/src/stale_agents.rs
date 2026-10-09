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

                // Budget symmetry (task-093 F4): the aborted agent no longer
                // counts against the workspace concurrency limit. Runs for
                // every aborted agent, not just orchestrators, because the
                // budget tracks all active agents.
                crate::api::budget::decrement_active_agents(state, &agent.workspace_id.to_string())
                    .await;

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

                // TASK-093 (§3.3, F6): orchestrator death handling — restart
                // (when restart_on_failure is set) and escalate to the
                // workspace orchestrator (repo tier). Shared with the
                // fail/stop handlers so every terminal transition of an
                // orchestrator gets identical treatment.
                handle_orchestrator_death(state, &agent, now, "heartbeat timeout (abort)").await;
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
/// TASK-093 (§3.3): spawn a replacement for a dead orchestrator. Fresh id,
/// unique name suffix, same scope/tier/lifecycle config, new scoped JWT. No
/// task, no worktree. Returns None (leaving the orchestrator dead) when the
/// workspace spawn budget is exhausted or persistence fails.
async fn restart_orchestrator(
    state: &AppState,
    dead: &gyre_domain::Agent,
    now: u64,
) -> Option<gyre_domain::Agent> {
    // Budget symmetry (task-093 F4): a replacement must pass the same spawn
    // budget check as a fresh spawn. If the workspace is at its limit, leave
    // the orchestrator dead and let the escalation surface it — restarting
    // unconditionally would both exceed the configured limit and spin (die →
    // restart → die) without a slot.
    if let Err(e) =
        crate::api::budget::check_spawn_budget(state, &dead.workspace_id.to_string()).await
    {
        warn!(
            agent_id = %dead.id,
            workspace_id = %dead.workspace_id,
            "restart: budget exhausted, leaving dead orchestrator unreplaced: {e}"
        );
        return None;
    }

    // Unique replacement name: append a restart counter suffix.
    let base = dead.name.split("-restart-").next().unwrap_or(&dead.name);
    let mut n = 1;
    let mut name = format!("{base}-restart-{n}");
    while let Ok(Some(_)) = state.agents.find_by_name(&name).await {
        n += 1;
        name = format!("{base}-restart-{n}");
    }

    let mut replacement =
        gyre_domain::Agent::new(Id::new(uuid::Uuid::new_v4().to_string()), name, now);
    replacement.parent_id = dead.parent_id.clone();
    replacement.spawned_by = dead.spawned_by.clone();
    replacement.workspace_id = dead.workspace_id.clone();
    replacement.repo_id = dead.repo_id.clone();
    replacement.orchestrator_type = dead.orchestrator_type.clone();
    // Inherit the full lifecycle configuration (task-093 F5): the replacement
    // must behave exactly like the agent it replaces — including how it should
    // itself be treated on disconnect — or the second death silently degrades
    // to Pause.
    replacement.disconnected_behavior = dead.disconnected_behavior.clone();
    replacement.restart_on_failure = true;
    if let Err(e) = replacement.transition_status(AgentStatus::Active) {
        warn!("restart: failed to activate replacement: {e}");
        return None;
    }
    if let Err(e) = state.agents.create(&replacement).await {
        warn!(
            "restart: failed to persist replacement for orchestrator {}: {e}",
            dead.id
        );
        return None;
    }

    // Scoped JWT for the replacement (same tier and scope as the dead one).
    let token = state.agent_signing_key.mint_orchestrator(
        &replacement.id.to_string(),
        dead.spawned_by.as_deref().unwrap_or("system"),
        &state.base_url,
        state.agent_jwt_ttl_secs,
        &dead.workspace_id.to_string(),
        dead.repo_id.as_ref().map(|r| r.to_string()).as_deref(),
        &dead.orchestrator_type.to_string(),
    );
    match token {
        Ok(t) => {
            let _ = state
                .kv_store
                .kv_set("agent_tokens", &replacement.id.to_string(), t)
                .await;
        }
        Err(e) => warn!("restart: failed to mint orchestrator JWT: {e}"),
    }

    // Keypair so the replacement can sign DerivedInputs for children.
    crate::api::spawn::bootstrap_agent_keypair(state, &replacement.id.to_string(), now).await;

    // Budget: the dead agent's slot was freed by the Dead transition, claim it.
    crate::api::budget::increment_active_agents(state, &dead.workspace_id.to_string()).await;

    info!(
        agent_id = %replacement.id,
        replaced = %dead.id,
        orchestrator_type = %dead.orchestrator_type,
        "orchestrator auto-restarted (task-093)"
    );
    Some(replacement)
}

/// TASK-093 (§3.3, F6): shared orchestrator death handling. Called from the
/// stale-agent Abort path, the fail handler, and the stop handler so every
/// terminal transition of an orchestrator gets the same treatment: restart a
/// replacement when `restart_on_failure` is set (subject to the spawn budget)
/// and escalate repo-tier deaths to the live workspace orchestrator.
pub(crate) async fn handle_orchestrator_death(
    state: &AppState,
    dead: &gyre_domain::Agent,
    now: u64,
    cause: &str,
) {
    let mut replacement = None;
    if dead.is_orchestrator() && dead.restart_on_failure {
        replacement = restart_orchestrator(state, dead, now).await;
    }
    if dead.orchestrator_type == gyre_domain::OrchestratorType::RepoOrchestrator {
        escalate_repo_orchestrator_death(state, dead, replacement.as_ref(), cause).await;
    }
}

/// TASK-093 (§3.3, F8): notify the live workspace orchestrator that a repo
/// orchestrator died (Directed-tier Escalation message). When a replacement
/// was already spawned the message is informational and names the
/// replacement, so the recipient does not react as if the repo is
/// unorchestrated.
async fn escalate_repo_orchestrator_death(
    state: &AppState,
    dead: &gyre_domain::Agent,
    replacement: Option<&gyre_domain::Agent>,
    cause: &str,
) {
    use gyre_common::message::Destination;

    let peers = match state.agents.list_by_workspace(&dead.workspace_id).await {
        Ok(p) => p,
        Err(e) => {
            warn!("escalate: failed to list workspace agents: {e}");
            return;
        }
    };
    let Some(ws_orch) = peers.iter().find(|a| {
        a.orchestrator_type == gyre_domain::OrchestratorType::WorkspaceOrchestrator
            && !matches!(
                a.status,
                AgentStatus::Dead | AgentStatus::Stopped | AgentStatus::Failed
            )
            && a.id != dead.id
    }) else {
        info!(
            repo_id = ?dead.repo_id,
            "no live workspace orchestrator to escalate repo orchestrator death to"
        );
        return;
    };

    let mut payload = serde_json::json!({
        "event": "repo_orchestrator_dead",
        "agent_id": dead.id.to_string(),
        "repo_id": dead.repo_id.as_ref().map(|r| r.to_string()),
        "reason": format!("repo orchestrator '{}' died ({cause})", dead.name),
    });
    if let Some(repl) = replacement {
        payload["replacement_agent_id"] = serde_json::json!(repl.id.to_string());
        payload["informational"] = serde_json::json!(true);
    }

    state
        .emit_event(
            Some(dead.workspace_id.clone()),
            Destination::Agent(ws_orch.id.clone()),
            MessageKind::Escalation,
            Some(payload),
        )
        .await;
}

pub fn spawn_stale_agent_detector(state: Arc<AppState>) {
    const CHECK_INTERVAL_SECS: u64 = 30;

    tokio::spawn(async move {
        // Record every cycle in the job registry so /healthz can detect a
        // dead detector loop (business-continuity.md §2).
        state
            .job_registry
            .mark_scheduled("stale_agent_detector")
            .await;
        let mut interval =
            tokio::time::interval(tokio::time::Duration::from_secs(CHECK_INTERVAL_SECS));
        loop {
            interval.tick().await;
            let started_at = crate::jobs::now_secs();
            let result = run_once(&state).await;
            state
                .job_registry
                .record_cycle("stale_agent_detector", started_at, &result)
                .await;
            if let Err(e) = result {
                error!("stale agent check failed: {e}");
            }
        }
    });
}
