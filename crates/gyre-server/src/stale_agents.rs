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
                // source-control.md §Agent-to-Commit Tracking: the Dead
                // transition terminates the agent's open session — records
                // the worktree's real current branch and dirty state, and
                // stamps ended_at. Must run BEFORE the worktree cleanup
                // below, or the dirty-state observation would have nothing
                // left to observe. Best-effort: never fails the kill.
                crate::agent_sessions::end_session(state, &agent).await;
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

                // message-bus.md §Edge Cases (orphaned agents): the detector
                // marks the heartbeated-out agent Dead and bulk-acks its
                // unacked Directed inbox with reason "agent_orphaned" —
                // retained for audit, never redelivered, and reclaimable by
                // the dead-inbox retention job (expire_acked_inboxes) once
                // GYRE_DEAD_INBOX_TTL_SECS elapses. Without this, unacked
                // Directed messages to dead agents accumulate indefinitely.
                crate::api::spawn::acknowledge_agent_inbox(state, &agent.id, "agent_orphaned")
                    .await;

                if was_active {
                    crate::api::budget::decrement_active_agents(
                        state,
                        &agent.workspace_id.to_string(),
                    )
                    .await;
                    // Preview-agent slot bookkeeping: this abort released
                    // the slot, and the preview-owned release (process
                    // exit, DELETE, GC) must not release it a second time
                    // (exactly-once; decrement_active is MAX(0, n-1)).
                    // No-op for normal agents.
                    crate::api::meta_specs::mark_preview_slot_released(
                        state,
                        &agent.id.to_string(),
                    )
                    .await;
                    // agent-runtime.md §4: wall-clock compute time
                    // (spawn → dead).
                    crate::api::budget::record_compute_minutes_for_agent(state, &agent).await;
                }

                // Agent lifecycle audit event (observability.md §Audit Event Types):
                // the stale detector marked this agent Dead. pid_alive is null
                // when no PID was ever attested (liveness unknown, not assumed).
                let pid_alive =
                    crate::agent_audit::attested_pid_alive(state, &agent.id.to_string()).await;
                crate::agent_audit::emit_dead(state, &agent, agent.last_heartbeat, pid_alive).await;

                // Clean up worktrees (agent-workflow.md §Parallel Work via
                // Worktrees — the shared terminal teardown: jj workspace
                // forget + working-copy removal, plain-git worktree removal,
                // agent_worktrees record delete).
                crate::api::spawn::cleanup_agent_worktrees(state, &agent.id).await;

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

                // analytics.md §Auto-Emitted Events: agent.failed fires when
                // an agent "fails or is killed". Pause is a terminal stop —
                // Stopped has no exit transitions — so the run ends here on
                // heartbeat loss, exactly like the sibling Abort arm (task-146
                // review F2).
                crate::api::analytics::record_agent_failed(
                    state,
                    &agent,
                    "heartbeat timeout (pause)",
                )
                .await;

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
/// unique name suffix, same scope/tier/lifecycle config, new scoped JWT. No
/// task, no worktree. Returns None (leaving the orchestrator dead) when a
/// live orchestrator already holds the scope, the workspace spawn budget is
/// exhausted, or persistence fails. The Arc handle relaunches the replacement
/// process (launch_orchestrator_process) like the spawn cores do.
async fn restart_orchestrator(
    state: &Arc<AppState>,
    dead: &gyre_domain::Agent,
    now: u64,
) -> Option<gyre_domain::Agent> {
    // Exactly-one-live (§3.2), the same check the spawn cores run: never
    // mint a replacement while a live orchestrator holds the scope. Without
    // this, a second entry into the shared death handling (a second terminal
    // transition — stop after fail, fail on an agent the stale detector
    // already killed) spawned a second live replacement with its own scoped
    // JWT. A scope-listing failure is conservative: no replacement rather
    // than a guessed-free scope.
    match state.agents.list_by_workspace(&dead.workspace_id).await {
        Ok(peers) => {
            let occupied = peers.iter().any(|a| {
                a.orchestrator_type == dead.orchestrator_type
                    && match dead.orchestrator_type {
                        gyre_domain::OrchestratorType::RepoOrchestrator => {
                            a.repo_id == dead.repo_id
                        }
                        _ => true,
                    }
                    && crate::api::orchestrator::is_live(a)
            });
            if occupied {
                warn!(
                    agent_id = %dead.id,
                    workspace_id = %dead.workspace_id,
                    "restart: live orchestrator already holds the scope; skipping replacement"
                );
                return None;
            }
        }
        Err(e) => {
            warn!(
                agent_id = %dead.id,
                "restart: cannot verify scope occupancy, leaving orchestrator unreplaced: {e}"
            );
            return None;
        }
    }

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
    // F2: keep the persona binding across restarts — the replacement must
    // run with the same persona as the orchestrator it replaces.
    replacement.persona_id = dead.persona_id.clone();
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
    // §1 Token Scoping: tenant and repo name resolved from the live stores
    // so the replacement's scope list is minted from real hierarchy context.
    // An unresolvable workspace means the tenant scope cannot be determined —
    // skip the re-mint (the replacement cannot authenticate until a token
    // exists) and log, rather than fabricating a "default" tenant identity
    // (task-097 F3 class).
    let tenant_id = match state
        .workspaces
        .find_by_id(&dead.workspace_id)
        .await
        .ok()
        .flatten()
    {
        Some(ws) => ws.tenant_id.to_string(),
        None => {
            warn!(
                agent_id = %replacement.id,
                workspace_id = %dead.workspace_id,
                "restart: tenant scope cannot be determined (workspace unresolvable); \
                 skipping orchestrator JWT re-mint"
            );
            // The replacement row exists (Active, budget claimed) but cannot
            // authenticate — report it as the replacement so escalation stays
            // truthful about scope occupancy.
            return Some(replacement);
        }
    };
    let repo_name = if let Some(rid) = dead.repo_id.as_ref() {
        state
            .repos
            .find_by_id(rid)
            .await
            .ok()
            .flatten()
            .map(|r| r.name)
    } else {
        None
    };
    // F2: the replacement keeps the dead orchestrator's persona binding —
    // the persona claim names the tier's persona slug.
    let persona = match dead.orchestrator_type {
        gyre_domain::OrchestratorType::WorkspaceOrchestrator => "workspace-orchestrator",
        gyre_domain::OrchestratorType::RepoOrchestrator => "repo-orchestrator",
        gyre_domain::OrchestratorType::Worker => "worker",
    };
    let token = state.agent_signing_key.mint_orchestrator(
        &replacement.id.to_string(),
        dead.spawned_by.as_deref().unwrap_or("system"),
        &state.base_url,
        state.agent_jwt_ttl_secs,
        &tenant_id,
        &dead.workspace_id.to_string(),
        dead.repo_id.as_ref().map(|r| r.to_string()).as_deref(),
        repo_name.as_deref(),
        &dead.orchestrator_type.to_string(),
        persona,
    );
    // Register the replacement's token so the auth middleware accepts it —
    // an unstored JWT leaves the restarted orchestrator unable to authenticate.
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
    Some(replacement)
}

/// TASK-093 (§3.3, F6): shared orchestrator death handling. Called from the
/// stale-agent Abort path, the fail handler, and the stop handler so every
/// terminal transition of an orchestrator gets the same treatment: restart a
/// replacement when `restart_on_failure` is set (subject to the spawn budget)
/// and escalate repo-tier deaths to the live workspace orchestrator.
///
/// Exactly-once contract: callers must invoke this only on the FIRST terminal
/// transition of the agent — the fail/stop handlers early-return on an
/// already-terminal agent, and `run_once` skips terminal agents, so the
/// shared handling (replacement spawn, budget increment, escalation) runs
/// exactly once per death. `restart_orchestrator` additionally refuses to
/// mint a replacement while a live orchestrator holds the scope (§3.2), so a
/// stray second entry cannot produce two live orchestrators.
pub(crate) async fn handle_orchestrator_death(
    state: &Arc<AppState>,
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
    state: &Arc<AppState>,
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
            dead.workspace_id.clone(),
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

    /// analytics.md §Auto-Emitted Events: an agent stopped by heartbeat
    /// timeout with pause behavior is equally "fails or is killed" — Pause
    /// is a terminal stop (Stopped has no exit transitions), so the run
    /// ends there and agent.failed must record with the spec-required
    /// task_id/reason properties (task-146 review F2). Fails on the old
    /// behavior where the Pause arm recorded no analytics event.
    #[tokio::test]
    async fn pause_records_agent_failed_analytics_event() {
        let state = test_state();

        let ws = gyre_domain::Workspace::new(Id::new("ws-2"), Id::new("t1"), "Ws", "ws", 0);
        state.workspaces.create(&ws).await.unwrap();

        let mut agent = gyre_domain::Agent::new(Id::new("stale-agent-2"), "paused", 1_000);
        // The agent must actually live in ws-2: the event's workspace
        // attribution comes from agent.workspace_id, so without this the
        // scoping assertion below would compare against a fabricated
        // bootstrap-"default" scope and could never pass.
        agent.workspace_id = Id::new("ws-2");
        agent.disconnected_behavior = DisconnectedBehavior::Pause;
        agent.transition_status(AgentStatus::Active).unwrap();
        state.agents.create(&agent).await.unwrap();

        // In-flight task assigned to the agent — the event must carry it.
        let mut task = gyre_domain::Task::new(Id::new("task-stale-2"), "Stale pause task", 1_000);
        task.transition_status(gyre_domain::TaskStatus::InProgress)
            .ok();
        state.tasks.create(&task).await.unwrap();
        let mut with_task = state.agents.find_by_id(&agent.id).await.unwrap().unwrap();
        with_task.current_task_id = Some(Id::new("task-stale-2"));
        state.agents.update(&with_task).await.unwrap();

        // Age past the heartbeat timeout, no heartbeat ever recorded.
        let mut aged = state.agents.find_by_id(&agent.id).await.unwrap().unwrap();
        aged.spawned_at = aged.spawned_at.saturating_sub(10_000);
        aged.last_heartbeat = None;
        state.agents.update(&aged).await.unwrap();

        run_once(&state).await.unwrap();

        let stopped = state.agents.find_by_id(&agent.id).await.unwrap().unwrap();
        assert_eq!(stopped.status, AgentStatus::Stopped);

        let events = state
            .analytics
            .query(Some("agent.failed"), None, 10)
            .await
            .unwrap();
        assert_eq!(
            events.len(),
            1,
            "stale pause must record the agent.failed analytics event"
        );
        let ev = &events[0];
        assert_eq!(ev.event_name, "agent.failed");
        assert_eq!(ev.agent_id.as_deref(), Some("stale-agent-2"));
        assert_eq!(ev.properties["task_id"], "task-stale-2");
        assert_eq!(ev.properties["reason"], "heartbeat timeout (pause)");
        assert!(
            ev.properties["duration_secs"].as_u64().is_some(),
            "duration_secs must be present"
        );
        assert_eq!(
            ev.workspace_id.as_deref(),
            Some("ws-2"),
            "event must be scoped to the workspace for filtered queries"
        );
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

    /// message-bus.md §Edge Cases (orphaned agents): when the stale detector
    /// marks a heartbeated-out agent Dead, its unacked Directed messages are
    /// bulk-acked with reason "agent_orphaned" — count_unacked drops to zero,
    /// the rows remain stored for audit, and the dead-inbox retention expiry
    /// reclaims exactly those rows (expire_acked_inboxes matches only the
    /// agent_completed/agent_orphaned reasons, so the deletion count also
    /// proves the recorded reason). Fails on the old behavior where the Dead
    /// transition never called acknowledge_all.
    #[tokio::test]
    async fn stale_abort_bulk_acks_directed_inbox_with_agent_orphaned() {
        use gyre_common::message::{Destination, Message, MessageKind, MessageOrigin};

        let state = test_state();
        let now = now_secs();

        let mut agent = Agent::new(Id::new("agent-stale-inbox"), "stale-inbox-agent", now);
        agent.transition_status(AgentStatus::Active).ok();
        agent.last_heartbeat = Some(now - 120);
        agent.disconnected_behavior = DisconnectedBehavior::Abort;
        state.agents.create(&agent).await.unwrap();

        // Aged past a 1s retention cutoff so the reclamation probe can use a
        // short dead-inbox window without touching process-global env vars.
        let created_at = (now * 1000).saturating_sub(10_000);
        let directed = |id: &str, target: &str| Message {
            id: Id::new(id.to_string()),
            tenant_id: Id::new("default"),
            from: MessageOrigin::Server,
            workspace_id: Some(Id::new("ws-1")),
            to: Destination::Agent(Id::new(target.to_string())),
            kind: MessageKind::TaskAssignment,
            payload: None,
            created_at,
            signature: None,
            key_id: None,
            acknowledged: false,
        };

        // Two unacked Directed messages in the orphaned agent's inbox.
        state
            .messages
            .store(&directed("orphan-msg-1", "agent-stale-inbox"))
            .await
            .unwrap();
        state
            .messages
            .store(&directed("orphan-msg-2", "agent-stale-inbox"))
            .await
            .unwrap();
        // A live agent's unacked inbox — must survive untouched.
        state
            .messages
            .store(&directed("orphan-live-msg", "agent-stale-live"))
            .await
            .unwrap();

        assert_eq!(
            state
                .messages
                .count_unacked(&Id::new("agent-stale-inbox"))
                .await
                .unwrap(),
            2,
            "precondition: two unacked Directed messages before the Dead transition"
        );

        run_once(&state).await.unwrap();

        let dead = state
            .agents
            .find_by_id(&Id::new("agent-stale-inbox"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(dead.status, AgentStatus::Dead);

        // Orphan cleanup: zero unacked, rows retained for audit.
        assert_eq!(
            state
                .messages
                .count_unacked(&Id::new("agent-stale-inbox"))
                .await
                .unwrap(),
            0,
            "the Dead transition must bulk-ack the orphaned agent's Directed inbox"
        );
        for msg_id in ["orphan-msg-1", "orphan-msg-2"] {
            let msg = state
                .messages
                .find_by_id(&Id::new(msg_id))
                .await
                .unwrap()
                .unwrap_or_else(|| panic!("{msg_id} must remain stored for audit"));
            assert!(msg.acknowledged, "{msg_id} must be acknowledged");
        }

        // Reclamation: with the dead-inbox retention window (1s here)
        // elapsed, expire_acked_inboxes deletes exactly the two
        // agent_orphaned rows.
        let deleted = state
            .messages
            .expire_acked_inboxes(now * 1000 - 1_000)
            .await
            .unwrap();
        assert_eq!(
            deleted, 2,
            "dead-inbox retention must reclaim exactly the orphaned agent's inbox"
        );
        assert!(state
            .messages
            .find_by_id(&Id::new("orphan-msg-1"))
            .await
            .unwrap()
            .is_none());
        assert!(state
            .messages
            .find_by_id(&Id::new("orphan-msg-2"))
            .await
            .unwrap()
            .is_none());

        // Untouched: the live agent's unacked inbox.
        let live = state
            .messages
            .find_by_id(&Id::new("orphan-live-msg"))
            .await
            .unwrap()
            .expect("live agent's unacked Directed message must survive");
        assert!(!live.acknowledged);
    }
}
