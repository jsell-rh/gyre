//! Stale agent detection: marks agents Dead when heartbeat times out.
//! Honors each agent's `disconnected_behavior` setting (BCP graceful degradation).

use gyre_common::{message::MessageKind, Id};
use gyre_domain::{AgentStatus, DisconnectedBehavior};
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::AppState;

const HEARTBEAT_TIMEOUT_SECS: u64 = 60;

/// Run one stale-agent detection cycle. Used by the job framework for manual triggering.
pub async fn run_once(state: &Arc<AppState>) -> anyhow::Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Server-driven orchestrators (spawned_by="system": the task-115 signal
    // chain's workspace/repo orchestrators) have no external process to send
    // `PUT /agents/:id/heartbeat` — the server itself is the liveness
    // source. When one's heartbeat has aged out, refresh it here rather
    // than applying the disconnect behavior: without this, the detector
    // marks it Dead, `restart_orchestrator` spawns an Active replacement
    // that equally never heartbeats, and the pair churns
    // spawn/kill/restart forever while leaking budget (review F3).
    // User-spawned orchestrators (task-093) have their own processes and
    // still age out normally.
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
        if agent.is_orchestrator() && agent.spawned_by.as_deref() == Some("system") {
            agent.heartbeat(now);
            if let Err(e) = state.agents.update(&agent).await {
                warn!(
                    agent_id = %agent.id,
                    "stale-agent detector: could not refresh system orchestrator heartbeat: {e}"
                );
            }
            continue;
        }

        match agent.disconnected_behavior {
            DisconnectedBehavior::Abort => {
                info!(agent_id = %agent.id, agent_name = %agent.name,
                    "aborting stale agent (disconnected_behavior=abort)");
                // Capture pre-death status: an Active agent held a budget
                // slot (incremented at spawn); the Dead transition must
                // release it or every orchestrator restart leaks one
                // `active_agents` slot (review F3).
                let was_active = agent.status == AgentStatus::Active;
                let _ = agent.transition_status(AgentStatus::Dead);
                // agent.failed analytics event (analytics.md §Auto-Emitted
                // Events: "Agent fails or is killed"). Heartbeat timeout is
                // a failure; the admin kill and fail_agent paths are others.
                let ev = gyre_domain::AnalyticsEvent::new(
                    Id::new(uuid::Uuid::new_v4().to_string()),
                    "agent.failed",
                    Some(agent.id.to_string()),
                    serde_json::json!({
                        "task_id": agent.current_task_id.as_ref().map(|id| id.to_string()),
                        "reason": "heartbeat timeout (abort)",
                        "duration_secs": now.saturating_sub(agent.spawned_at),
                    }),
                    now,
                )
                .with_scope(
                    None,
                    None,
                    Some(&agent.workspace_id),
                    agent.repo_id.as_ref(),
                );
                let _ = state.analytics.record(&ev).await;
                let _ = state.agents.update(&agent).await;

                if was_active {
                    crate::api::budget::decrement_active_agents(
                        state,
                        &agent.workspace_id.to_string(),
                    )
                    .await;
                }

                // Agent lifecycle audit event (observability.md §Audit Event Types):
                // the stale detector marked this agent Dead. pid_alive is null
                // when no PID was ever attested (liveness unknown, not assumed).
                let pid_alive =
                    crate::agent_audit::attested_pid_alive(state, &agent.id.to_string()).await;
                crate::agent_audit::emit_dead(state, &agent, agent.last_heartbeat, pid_alive).await;

                // Clean up worktrees
                if let Ok(worktrees) = state.worktrees.find_by_agent(&agent.id).await {
                    for wt in worktrees {
                        if let Ok(Some(repo)) = state.repos.find_by_id(&wt.repository_id).await {
                            // jj workspace teardown (source-control.md §4):
                            // forget the workspace from the shared jj
                            // checkout and remove the working copy dir.
                            crate::api::spawn::cleanup_jj_workspace(
                                state,
                                &agent.id.to_string(),
                                &repo.path,
                                &wt.path,
                                &format!("agent-{}", agent.id),
                            )
                            .await;
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
                            let pre_block_status = task.status.clone();
                            let _ = task.transition_status(TaskStatus::Blocked);
                            task.updated_at = now;
                            let _ = state.tasks.update(&task).await;
                            // task.status_changed analytics event (analytics.md
                            // §Auto-Emitted Events: "Task transitions status") —
                            // the stale-abort auto-block is a production
                            // transition.
                            if task.status != pre_block_status {
                                crate::api::analytics::record_task_status_changed(
                                    state,
                                    &task,
                                    &pre_block_status,
                                )
                                .await;
                            }
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

                // Platform Model §7: heartbeat abort is a kill — revoke the
                // agent's task-scoped ephemeral secrets (teardown; logs
                // `secret.revoked` per secret).
                crate::secret_rotation::revoke_agent_task_secrets(state, &agent).await;

                let ws_id = agent.workspace_id.clone();
                state.emit_telemetry(
                    ws_id.clone(),
                    MessageKind::AgentStatusChanged,
                    Some(serde_json::json!({
                        "agent_id": agent.id.to_string(),
                        "status": "dead",
                        "reason": format!("Agent {} aborted (no heartbeat, abort behavior)", agent.name),
                    })),
                ).await;

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

                // TASK-093 (§3.3): auto-restart dead orchestrators. Dead
                // orchestrators free their scope slot, so a fresh replacement
                // keeps exactly one live orchestrator per scope.
                if agent.is_orchestrator() && agent.restart_on_failure {
                    restart_orchestrator(state, &agent, now).await;
                }
                // TASK-093 (§3.3): when a repo orchestrator dies, escalate
                // to the workspace orchestrator so it can react (e.g. spawn
                // a replacement or reschedule work).
                if agent.orchestrator_type == gyre_domain::OrchestratorType::RepoOrchestrator {
                    escalate_repo_orchestrator_death(state, &agent).await;
                }
            }

            DisconnectedBehavior::Pause => {
                info!(agent_id = %agent.id, agent_name = %agent.name,
                    "stopping stale agent (disconnected_behavior=pause)");
                let _ = agent.transition_status(AgentStatus::Stopped);
                let _ = state.agents.update(&agent).await;

                // Platform Model §7 (task-100 review round-3 F1): Pause is a
                // terminal stop — Stopped has no exit transitions — so the
                // agent's run ends here and its task-scoped ephemeral
                // secrets must be revoked like the sibling Abort arm
                // (teardown; logs `secret.revoked` per secret).
                crate::secret_rotation::revoke_agent_task_secrets(state, &agent).await;

                let ws_id = agent.workspace_id.clone();
                state.emit_telemetry(
                    ws_id,
                    MessageKind::AgentStatusChanged,
                    Some(serde_json::json!({
                        "agent_id": agent.id.to_string(),
                        "status": "stopped",
                        "reason": format!("Agent {} stopped (no heartbeat, pause behavior)", agent.name),
                    })),
                ).await;
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
/// unique name suffix, same scope/tier, new scoped JWT. No task, no worktree.
async fn restart_orchestrator(state: &Arc<AppState>, dead: &gyre_domain::Agent, now: u64) {
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
    replacement.restart_on_failure = true;
    // F2: keep the persona binding across restarts — the replacement must
    // run with the same persona as the orchestrator it replaces.
    replacement.persona_id = dead.persona_id.clone();
    if let Err(e) = replacement.transition_status(AgentStatus::Active) {
        warn!("restart: failed to activate replacement: {e}");
        return;
    }
    if let Err(e) = state.agents.create(&replacement).await {
        warn!(
            "restart: failed to persist replacement for orchestrator {}: {e}",
            dead.id
        );
        return;
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
    match &token {
        Ok(t) => {
            let _ = state
                .kv_store
                .kv_set("agent_tokens", &replacement.id.to_string(), t.clone())
                .await;
        }
        Err(e) => warn!("restart: failed to mint orchestrator JWT: {e}"),
    }

    // Keypair so the replacement can sign DerivedInputs for children.
    crate::api::spawn::bootstrap_agent_keypair(state, &replacement.id.to_string(), now).await;

    // Budget: the dead agent's slot was freed by the Dead transition, claim it.
    crate::api::budget::increment_active_agents(state, &dead.workspace_id.to_string()).await;

    // F3: relaunch the process. A replacement that is only a persisted row
    // is not a running orchestrator; if the launch fails the replacement
    // will be declared Dead by this same detector and restarted again.
    if let Ok(t) = &token {
        if let Some(workspace) = state
            .workspaces
            .find_by_id(&replacement.workspace_id)
            .await
            .ok()
            .flatten()
        {
            let outcome = crate::api::orchestrator::launch_orchestrator_process(
                state,
                &replacement,
                &workspace,
                t,
            )
            .await;
            if outcome.launch_status != "running" {
                warn!(
                    agent_id = %replacement.id,
                    detail = outcome.launch_detail.as_deref().unwrap_or_default(),
                    "restart: orchestrator process launch failed"
                );
            }
        }
    }

    info!(
        agent_id = %replacement.id,
        replaced = %dead.id,
        orchestrator_type = %dead.orchestrator_type,
        "orchestrator auto-restarted (task-093)"
    );
}

/// TASK-093 (§3.3): notify the live workspace orchestrator that a repo
/// orchestrator died (Directed-tier Escalation message).
async fn escalate_repo_orchestrator_death(state: &Arc<AppState>, dead: &gyre_domain::Agent) {
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

    state
        .emit_event(
            Some(dead.workspace_id.clone()),
            Destination::Agent(ws_orch.id.clone()),
            MessageKind::Escalation,
            Some(serde_json::json!({
                "event": "repo_orchestrator_dead",
                "agent_id": dead.id.to_string(),
                "repo_id": dead.repo_id.as_ref().map(|r| r.to_string()),
                "reason": format!("repo orchestrator '{}' died (heartbeat timeout)", dead.name),
            })),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;
    use gyre_common::Id;
    use gyre_domain::{Agent, AgentStatus, DisconnectedBehavior};

    /// analytics.md §Auto-Emitted Events: an agent aborted by heartbeat
    /// timeout ("fails or is killed") must record agent.failed with the
    /// spec-required task_id/reason properties. Fails on the old behavior
    /// where the abort path recorded no analytics event.
    #[tokio::test]
    async fn abort_records_agent_failed_analytics_event() {
        let state = test_state();

        let ws = gyre_domain::Workspace::new(Id::new("ws-1"), Id::new("t1"), "Ws", "ws", 0);
        state.workspaces.create(&ws).await.unwrap();

        let mut agent = gyre_domain::Agent::new(Id::new("stale-agent-1"), "doomed", 1_000);
        agent.disconnected_behavior = DisconnectedBehavior::Abort;
        agent.transition_status(AgentStatus::Active).unwrap();
        state.agents.create(&agent).await.unwrap();

        // In-flight task assigned to the agent — the abort path blocks it.
        let mut task = gyre_domain::Task::new(Id::new("task-stale-1"), "Stale task", 1_000);
        task.transition_status(gyre_domain::TaskStatus::InProgress)
            .ok();
        state.tasks.create(&task).await.unwrap();
        let mut with_task = state.agents.find_by_id(&agent.id).await.unwrap().unwrap();
        with_task.current_task_id = Some(Id::new("task-stale-1"));
        state.agents.update(&with_task).await.unwrap();

        // Age past the heartbeat timeout, no heartbeat ever recorded.
        let mut aged = state.agents.find_by_id(&agent.id).await.unwrap().unwrap();
        aged.spawned_at = aged.spawned_at.saturating_sub(10_000);
        aged.last_heartbeat = None;
        state.agents.update(&aged).await.unwrap();

        run_once(&state).await.unwrap();

        let dead = state.agents.find_by_id(&agent.id).await.unwrap().unwrap();
        assert_eq!(dead.status, AgentStatus::Dead);

        let blocked = state
            .tasks
            .find_by_id(&Id::new("task-stale-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(blocked.status, gyre_domain::TaskStatus::Blocked);

        let events = state
            .analytics
            .query(Some("agent.failed"), None, 10)
            .await
            .unwrap();
        assert_eq!(
            events.len(),
            1,
            "stale abort must record the agent.failed analytics event"
        );
        let ev = &events[0];
        assert_eq!(ev.event_name, "agent.failed");
        assert_eq!(ev.agent_id.as_deref(), Some("stale-agent-1"));
        assert_eq!(ev.properties["reason"], "heartbeat timeout (abort)");
        assert!(
            ev.properties["duration_secs"].as_u64().is_some(),
            "duration_secs must be present"
        );

        // analytics.md §Auto-Emitted Events: task.status_changed — the
        // stale-abort auto-block is a production transition and must record.
        let status_events = state
            .analytics
            .query(Some("task.status_changed"), None, 10)
            .await
            .unwrap();
        assert_eq!(
            status_events.len(),
            1,
            "stale-abort auto-block must record task.status_changed"
        );
        let sev = &status_events[0];
        assert_eq!(sev.properties["task_id"], "task-stale-1");
        assert_eq!(sev.properties["old_status"], "InProgress");
        assert_eq!(sev.properties["new_status"], "Blocked");
    }

    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    #[tokio::test]
    async fn stale_detector_emits_agent_dead_audit_event() {
        // observability.md §Audit Event Types: when the stale detector marks
        // an agent Dead, an AgentDead event must record the last heartbeat
        // time and PID liveness. Without an attestation pid_alive is null
        // (unknown), never fabricated.
        let state = test_state();
        let now = now_secs();
        let mut agent = Agent::new(Id::new("agent-stale-audit"), "stale-audit-agent", now);
        agent.transition_status(AgentStatus::Active).ok();
        agent.last_heartbeat = Some(now - 120);
        agent.disconnected_behavior = DisconnectedBehavior::Abort;
        state.agents.create(&agent).await.unwrap();

        super::run_once(&state).await.unwrap();

        // The agent actually transitioned to Dead.
        let dead = state
            .agents
            .find_by_id(&Id::new("agent-stale-audit"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(dead.status, AgentStatus::Dead);

        let events = crate::agent_audit::recorded_events(&state, "agent_dead").await;
        assert_eq!(events.len(), 1, "exactly one agent_dead event");
        let ev = &events[0];
        assert_eq!(ev.resource_id.as_deref(), Some("agent-stale-audit"));
        assert_eq!(ev.detail["last_heartbeat_at"], now - 120);
        assert!(ev.detail["pid_alive"].is_null(), "no attestation → unknown");
    }

    #[tokio::test]
    async fn stale_detector_reports_pid_liveness_in_agent_dead_event() {
        // With an attested PID, the AgentDead event must carry the measured
        // liveness: a dead PID (an exited child) reports false, proving the
        // value is measured rather than assumed.
        let state = test_state();
        let now = now_secs();
        let mut agent = Agent::new(Id::new("agent-stale-pid"), "stale-pid-agent", now);
        agent.transition_status(AgentStatus::Active).ok();
        agent.last_heartbeat = Some(now - 120);
        agent.disconnected_behavior = DisconnectedBehavior::Abort;
        state.agents.create(&agent).await.unwrap();

        let mut child = std::process::Command::new("true").spawn().unwrap();
        let dead_pid = child.id();
        child.wait().unwrap();
        let att = crate::workload_attestation::attest_agent(
            "agent-stale-pid",
            Some(dead_pid),
            "local",
            "",
        );
        state
            .kv_store
            .kv_set(
                "workload_attestations",
                "agent-stale-pid",
                serde_json::to_string(&att).unwrap(),
            )
            .await
            .unwrap();

        super::run_once(&state).await.unwrap();

        let events = crate::agent_audit::recorded_events(&state, "agent_dead").await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].detail["pid_alive"], false);
    }

    #[tokio::test]
    async fn stale_detector_pause_revokes_task_scoped_secrets() {
        // platform-model.md §7 (task-100 review round-3 F1): the Pause arm
        // of the stale detector terminally stops the agent (Stopped has no
        // exit transitions) — the task-scoped ephemeral secrets must be
        // revoked, mirroring the Abort arm. Pause is the default
        // DisconnectedBehavior, so the default configuration must not leak.
        use gyre_common::{Id, Secret, SecretScope, SecretType};

        let state = test_state();
        let now = now_secs();
        state
            .tenants
            .create(&gyre_domain::Tenant::new(
                Id::new("default"),
                "Default",
                "default",
                now,
            ))
            .await
            .unwrap();
        let ws = gyre_domain::Workspace::new(
            Id::new("ws-pause"),
            Id::new("default"),
            "Pause WS",
            "pause-ws",
            now,
        );
        state.workspaces.create(&ws).await.unwrap();

        let mut agent = Agent::new(Id::new("agent-stale-pause"), "stale-pause-agent", now);
        agent.transition_status(AgentStatus::Active).ok();
        agent.workspace_id = Id::new("ws-pause");
        agent.current_task_id = Some(Id::new("task-pause"));
        agent.last_heartbeat = Some(now - 120);
        agent.disconnected_behavior = DisconnectedBehavior::Pause;
        state.agents.create(&agent).await.unwrap();

        let mk = |id: &str, name: &str, scope: SecretScope, scope_id: String| Secret {
            id: Id::new(id),
            name: name.to_string(),
            scope,
            scope_id,
            secret_type: SecretType::Ephemeral,
            created_by: "system".to_string(),
            created_at: now,
            expires_at: None,
            last_rotated_at: None,
            tenant_id: "default".to_string(),
        };
        state
            .secrets
            .create(
                &mk(
                    "sec-pause-task",
                    "EPHEMERAL_DB",
                    SecretScope::Task,
                    "task-pause".to_string(),
                ),
                b"v1",
            )
            .await
            .unwrap();
        state
            .secrets
            .create(
                &mk(
                    "sec-pause-ws",
                    "SHARED_KEY",
                    SecretScope::Workspace,
                    "ws-pause".to_string(),
                ),
                b"v2",
            )
            .await
            .unwrap();
        state
            .secrets
            .create(
                &mk(
                    "sec-pause-other",
                    "OTHER_TASK",
                    SecretScope::Task,
                    "task-unrelated".to_string(),
                ),
                b"v3",
            )
            .await
            .unwrap();

        super::run_once(&state).await.unwrap();

        let stopped = state
            .agents
            .find_by_id(&Id::new("agent-stale-pause"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stopped.status, AgentStatus::Stopped);

        assert!(
            state
                .secrets
                .find_by_id(&Id::new("sec-pause-task"), "default")
                .await
                .unwrap()
                .is_none(),
            "pause-behavior stale stop must revoke the task-scoped secret"
        );
        assert!(
            state
                .secrets
                .find_by_id(&Id::new("sec-pause-ws"), "default")
                .await
                .unwrap()
                .is_some(),
            "workspace-scoped secrets must survive the stale stop"
        );
        assert!(
            state
                .secrets
                .find_by_id(&Id::new("sec-pause-other"), "default")
                .await
                .unwrap()
                .is_some(),
            "other tasks' secrets must survive the stale stop"
        );
    }
}
