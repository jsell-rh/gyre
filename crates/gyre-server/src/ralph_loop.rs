//! Ralph loop automation (agent-runtime.md §1 Phases 5–7).
//!
//! Phase 5 ends when the agent calls `agent.complete` (REST or MCP — both
//! surfaces create the MR via [`crate::api::spawn::create_mr_for_completion`]).
//! This module owns everything that happens after that:
//!
//! - Phase 6: the MR enters the merge queue automatically (which triggers
//!   gate execution) and a watcher observes the gate results.
//! - Phase 7 pass path: when all required gates pass, the merge processor
//!   merges the queued MR (subject to the workspace's trust policies); the
//!   watcher observes the merge and marks the task `Done`.
//! - Phase 7 fail path: gate failures are written to the agent's inbox as
//!   messages, the MR is closed, and the agent is re-spawned — same persona,
//!   same meta-spec set, same task, fresh context (no conversation history),
//!   iteration counter incremented. At `max_iterations` (repo setting,
//!   default 10) the task is `Blocked`, the agent `Failed`, and a
//!   `TaskBlocked` message is routed to the repo orchestrator's inbox (the
//!   server spawns an orchestrator to process the failure when none is
//!   live — same routing mechanism as `SpecApproved`).

use std::sync::Arc;

use gyre_common::message::{Destination, MessageKind};
use gyre_common::Id;
use gyre_domain::{
    Agent, AgentStatus, AnalyticsEvent, GateStatus, MergeRequest, MrStatus, Task, TaskStatus,
};

use crate::AppState;

/// Default Ralph-loop iteration ceiling when neither the repo nor the
/// workspace overrides it (agent-runtime.md §1 Phase 7: "default: 10").
pub const DEFAULT_MAX_ITERATIONS: u32 = 10;

/// How long the gate watcher polls before giving up (bounded — agent gates
/// run concurrently with a 5-minute internal timeout each; 20 minutes
/// covers a full gate set plus scheduling slack). Overridable for tests
/// via `GYRE_GATE_WATCH_TIMEOUT_SECS`.
const GATE_WATCH_TIMEOUT_SECS: u64 = 1200;

/// How long the watcher waits for the merge processor to merge an
/// all-green MR (subject to trust holds, queue depth, atomic groups).
const MR_MERGE_WATCH_TIMEOUT_SECS: u64 = 3600;

/// Poll interval for both watcher loops. Overridable for tests via
/// `GYRE_RALPH_POLL_MILLIS`.
const WATCH_POLL_MILLIS: u64 = 2000;

fn poll_interval() -> std::time::Duration {
    std::env::var("GYRE_RALPH_POLL_MILLIS")
        .ok()
        .and_then(|v| v.parse().ok())
        .map(std::time::Duration::from_millis)
        .unwrap_or(std::time::Duration::from_millis(WATCH_POLL_MILLIS))
}

fn gate_watch_timeout() -> std::time::Duration {
    std::env::var("GYRE_GATE_WATCH_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .map(std::time::Duration::from_secs)
        .unwrap_or(std::time::Duration::from_secs(GATE_WATCH_TIMEOUT_SECS))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Phase 6 hook: called by both completion surfaces right after the MR is
/// created. Puts the MR on the merge queue (which triggers gate execution
/// — test, lint, spec review, trace capture per the repo's gate config)
/// and starts the Ralph-loop watcher.
pub async fn on_agent_mr_created(state: &Arc<AppState>, agent: &Agent, mr: &MergeRequest) {
    // Only agent-authored MRs are Ralph-loop candidates; a human-authored
    // MR keeps its human-driven queue lifecycle.
    if mr.author_agent_id.is_none() {
        return;
    }
    let mr_id = mr.id.clone();
    match crate::api::merge_queue::enqueue_core(state, &mr.id, 50).await {
        Ok((_entry, _created)) => {
            tracing::info!(
                mr_id = %mr.id,
                agent_id = %agent.id,
                "ralph-loop: MR enqueued; gates triggered (agent-runtime.md §1 Phase 6)"
            );
        }
        // A Closed/Reverted MR cannot re-enter the queue (task-095 R3-F2):
        // nothing to watch.
        Err(e) => {
            tracing::warn!(
                mr_id = %mr.id,
                agent_id = %agent.id,
                "ralph-loop: could not enqueue completion MR: {e:?}"
            );
            return;
        }
    }
    let state = Arc::clone(state);
    let agent_id = agent.id.clone();
    tokio::spawn(async move {
        watch_mr_gates(state, mr_id, agent_id).await;
    });
}

/// Watch an MR's gate results until they are all terminal, then dispatch
/// the Phase 7 pass/fail paths. Bounded by [`GATE_WATCH_TIMEOUT_SECS`] —
/// a watcher never loops forever.
async fn watch_mr_gates(state: Arc<AppState>, mr_id: Id, agent_id: Id) {
    let deadline = std::time::Instant::now() + gate_watch_timeout();
    loop {
        tokio::time::sleep(poll_interval()).await;

        let results = state
            .gate_results
            .list_by_mr_id(mr_id.as_str())
            .await
            .unwrap_or_default();

        // No gate results yet: either the repo has no gates (trivial pass —
        // check_gates_for_mr returns Ok(true)) or the gate records have not
        // been created yet (enqueue triggered them synchronously, so this
        // window is one poll at most). Fall through to the verdict check.
        let all_terminal = results
            .iter()
            .all(|r| !matches!(r.status, GateStatus::Pending | GateStatus::Running));
        if !results.is_empty() && !all_terminal {
            if std::time::Instant::now() > deadline {
                tracing::warn!(
                    mr_id = %mr_id,
                    "ralph-loop: gate watch timed out; leaving MR to the merge processor"
                );
                return;
            }
            continue;
        }

        match crate::gate_executor::check_gates_for_mr(&state, &mr_id).await {
            // Gates still running (a result flipped to Pending between the
            // terminal scan and the verdict) — keep watching.
            Ok(false) => continue,
            Ok(true) => {
                // Phase 7 pass path: the merge processor merges the queued
                // MR (subject to the workspace's trust policies — a
                // Supervised workspace holds it for human review, which is
                // the platform trust model, not a Ralph-loop defect). Watch
                // for the merge and complete the task.
                watch_mr_merge(state, mr_id, agent_id).await;
                return;
            }
            Err(reason) => {
                tracing::info!(
                    mr_id = %mr_id,
                    agent_id = %agent_id,
                    reason = %reason,
                    "ralph-loop: gate failure — re-spawning agent (agent-runtime.md §1 Phase 7)"
                );
                on_gates_failed(&state, &mr_id, &agent_id).await;
                return;
            }
        }
    }
}

/// Phase 7 pass path tail: wait for the merge processor to merge the MR,
/// then mark the task `Done` (the spec's "MR merges, task Completed").
async fn watch_mr_merge(state: Arc<AppState>, mr_id: Id, agent_id: Id) {
    let deadline =
        std::time::Instant::now() + std::time::Duration::from_secs(MR_MERGE_WATCH_TIMEOUT_SECS);
    loop {
        tokio::time::sleep(poll_interval()).await;
        let Ok(Some(mr)) = state.merge_requests.find_by_id(&mr_id).await else {
            return;
        };
        match mr.status {
            MrStatus::Merged => {
                complete_task_on_merge(&state, &mr, &agent_id).await;
                return;
            }
            // A human (or the recovery protocol) closed the MR — the Ralph
            // loop stops; the task stays in Review for the human decision.
            MrStatus::Closed | MrStatus::Reverted => {
                tracing::info!(
                    mr_id = %mr_id,
                    status = ?mr.status,
                    "ralph-loop: MR closed without merge; leaving task in Review"
                );
                return;
            }
            _ => {
                if std::time::Instant::now() > deadline {
                    tracing::warn!(
                        mr_id = %mr_id,
                        "ralph-loop: merge watch timed out (trust hold or queue depth); task stays in Review"
                    );
                    return;
                }
            }
        }
    }
}

/// Task → Done when the agent's MR merged (Review → Done is the valid
/// terminal transition; emits TaskTransitioned for observability).
async fn complete_task_on_merge(state: &Arc<AppState>, mr: &MergeRequest, agent_id: &Id) {
    let Some(agent) = state.agents.find_by_id(agent_id).await.ok().flatten() else {
        return;
    };
    let Some(task_id) = agent.current_task_id.clone() else {
        return;
    };
    let Ok(Some(mut task)) = state.tasks.find_by_id(&task_id).await else {
        return;
    };
    if task.status == TaskStatus::Done {
        return;
    }
    // The persisted transition runs from the status loaded above to Done
    // (task-146 review R4): the in-memory InProgress→Review hop below only
    // satisfies the domain's transition graph — the row this path persists
    // changes from `old_status` (the loaded status) to Done.
    let old_status = task.status.clone();
    // Navigate to Review first (an InProgress task at merge time — e.g. the
    // MR merged while a re-spawn had returned the task to InProgress —
    // needs InProgress → Review → Done).
    if task.status == TaskStatus::InProgress {
        let _ = task.transition_status(TaskStatus::Review);
    }
    if task.transition_status(TaskStatus::Done).is_err() {
        tracing::warn!(
            task_id = %task.id,
            status = ?task.status,
            "ralph-loop: could not transition merged task to Done"
        );
        return;
    }
    task.updated_at = now_secs();
    if let Err(e) = state.tasks.update(&task).await {
        tracing::warn!(task_id = %task.id, "ralph-loop: task Done persist failed: {e}");
        return;
    }
    // Auto-track the Review→Done terminal transition (analytics.md
    // §Auto-Emitted Events — every persisted task status transition
    // records a task.status_changed event; task-146 review R4: this
    // pass-path tail previously persisted Done without it).
    crate::api::analytics::record_task_status_changed(state, &task, &old_status).await;
    state
        .emit_event(
            Some(task.workspace_id.clone()),
            Destination::Workspace(task.workspace_id.clone()),
            MessageKind::TaskTransitioned,
            Some(serde_json::json!({
                "task_id": task.id.to_string(),
                "status": "done",
                "mr_id": mr.id.to_string(),
            })),
        )
        .await;
    tracing::info!(
        task_id = %task.id,
        mr_id = %mr.id,
        "ralph-loop: gates passed, MR merged, task Done (agent-runtime.md §1 Phase 7)"
    );
}

/// Phase 7 fail path: gate failures → agent inbox, then re-spawn or block.
async fn on_gates_failed(state: &Arc<AppState>, mr_id: &Id, agent_id: &Id) {
    let now = now_secs();

    let Some(mut agent) = state.agents.find_by_id(agent_id).await.ok().flatten() else {
        tracing::warn!(mr_id = %mr_id, "ralph-loop: author agent vanished; nothing to re-spawn");
        return;
    };
    let Some(mr) = state.merge_requests.find_by_id(mr_id).await.ok().flatten() else {
        return;
    };
    let Some(task_id) = agent.current_task_id.clone() else {
        tracing::warn!(agent_id = %agent_id, "ralph-loop: agent has no task; nothing to re-spawn");
        return;
    };
    let Some(mut task) = state.tasks.find_by_id(&task_id).await.ok().flatten() else {
        return;
    };

    // 1. Task remains InProgress (Phase 7 step 1) — move it back from the
    //    Review state the completion handler set.
    if task.status == TaskStatus::Review {
        let old_status = task.status.clone();
        let _ = task.transition_status(TaskStatus::InProgress);
        task.updated_at = now;
        let _ = state.tasks.update(&task).await;
        // Auto-track the re-open transition (analytics.md
        // §Auto-Emitted Events — every persisted task status transition
        // records a task.status_changed event).
        crate::api::analytics::record_task_status_changed(state, &task, &old_status).await;
    }

    // 2. Gate failure details → the agent's inbox, as messages the next
    //    session reads on startup (gyre_message_poll). One message per
    //    failed gate, carrying the gate's real recorded output.
    let failed = state
        .gate_results
        .list_by_mr_id(mr_id.as_str())
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r.status == GateStatus::Failed)
        .collect::<Vec<_>>();
    for r in &failed {
        let gate_name = state
            .quality_gates
            .find_by_id(r.gate_id.as_str())
            .await
            .ok()
            .flatten()
            .map(|g| g.name)
            .unwrap_or_else(|| r.gate_id.to_string());
        state
            .emit_event(
                Some(agent.workspace_id.clone()),
                Destination::Agent(agent.id.clone()),
                MessageKind::GateFailure,
                Some(serde_json::json!({
                    "mr_id": mr.id.to_string(),
                    "gate_name": gate_name,
                    "gate_type": "gate_failure",
                    "status": "Failed",
                    "output": r.output.clone().unwrap_or_default(),
                    "spec_ref": mr.spec_ref.clone(),
                    "iteration": agent.iteration,
                })),
            )
            .await;
    }

    // 3. Close the failed MR so the next completion creates a fresh one
    //    (the completion path is idempotent on non-closed MRs — a re-spawn
    //    completing against the old Open MR would silently reuse it).
    let mut closed = mr.clone();
    if closed.transition_status(MrStatus::Closed).is_ok() {
        closed.updated_at = now;
        let _ = state.merge_requests.update(&closed).await;
        // Auto-track mr.closed (analytics.md §Auto-Emitted Events): the
        // ralph loop closes the failed MR without merge so the next
        // completion creates a fresh one — same event shape as the MR
        // close endpoint, with the gate-failure cause as the reason.
        let ev = AnalyticsEvent::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            "mr.closed",
            mr.author_agent_id.as_ref().map(|id| id.to_string()),
            serde_json::json!({
                "mr_id": mr.id.to_string(),
                "repo_id": mr.repository_id.to_string(),
                "reason": "gate_failure",
            }),
            now,
        )
        .with_scope(None, None, Some(&mr.workspace_id), Some(&mr.repository_id));
        let _ = state.analytics.record(&ev).await;
    }

    // 4. Iteration accounting against the repo's ceiling.
    let iteration = agent.iteration.saturating_add(1);
    let repo = state.repos.find_by_id(&task.repo_id).await.ok().flatten();
    let max_iterations = repo
        .as_ref()
        .and_then(|r| r.max_agent_iterations)
        .unwrap_or(DEFAULT_MAX_ITERATIONS);

    if iteration >= max_iterations {
        agent.iteration = iteration;
        let _ = agent.transition_status(AgentStatus::Failed);
        let _ = state.agents.update(&agent).await;
        let old_status = task.status.clone();
        let _ = task.transition_status(TaskStatus::Blocked);
        task.updated_at = now;
        let _ = state.tasks.update(&task).await;
        // Auto-track the terminal transitions (analytics.md
        // §Auto-Emitted Events): agent.failed on the iteration ceiling,
        // task.status_changed on the Block.
        crate::api::analytics::record_agent_failed(
            state,
            &agent,
            &format!("max iterations reached ({max_iterations}) with failing gates"),
        )
        .await;
        crate::api::analytics::record_task_status_changed(state, &task, &old_status).await;
        // The agent's token was already revoked at completion; make sure a
        // terminal agent cannot re-authenticate.
        let _ = state
            .kv_store
            .kv_remove("agent_tokens", agent.id.as_str())
            .await;
        notify_task_blocked(
            state,
            &task,
            repo.as_ref(),
            format!("max iterations reached ({max_iterations}) with failing gates"),
        )
        .await;
        tracing::warn!(
            task_id = %task.id,
            agent_id = %agent.id,
            iteration,
            max_iterations,
            "ralph-loop: task Blocked, agent Failed (agent-runtime.md §1 Phase 7)"
        );
        return;
    }

    // 5. Re-spawn: same agent entity (same persona, same meta-spec set,
    //    same task), iteration incremented, FRESH context — a new process
    //    with a new JWT and no conversation history, reading the gate
    //    failure messages from its inbox.
    agent.iteration = iteration;
    if let Err(e) = agent.transition_status(AgentStatus::Active) {
        tracing::warn!(agent_id = %agent.id, "ralph-loop: re-activation failed: {e}");
        return;
    }
    if let Err(e) = state.agents.update(&agent).await {
        tracing::warn!(agent_id = %agent.id, "ralph-loop: agent persist failed: {e}");
        return;
    }
    if let Err(e) = relaunch_agent_session(state, &agent, &task).await {
        // The re-spawn launch is a `ComputeTarget::spawn_process()` call,
        // and the Phase 7 spawn-failure policy is not path-scoped — route
        // it through the same retry/notification ladder the first-spawn
        // path uses (review F1, task-118).
        respawn_with_retry(state, &agent, &task, repo.as_ref(), e).await;
    }
}

/// Phase 7 spawn-failure policy on the re-spawn path (agent-runtime.md
/// §1 Phase 7; review F1, task-118): "If `ComputeTarget::spawn_process()`
/// fails … Retries up to 3 times with exponential backoff (1s, 5s, 30s).
/// If all retries fail: task marked Blocked, orchestrator notified,
/// priority-1 notification created for workspace admins." The first
/// failure (`first_error`) plus 3 backoff retries is the same attempt
/// ladder [`crate::signal_chain::retry_worker_spawn`] applies to first
/// spawns. Each attempt keeps the claim fresh
/// ([`crate::signal_chain::touch_task`]) so the scheduler's stale
/// worker-claim recovery never releases the task mid-retry (the agent is
/// Active — the recovery's own guard — but the touch keeps the two
/// mechanisms from racing on a status flip).
///
/// Terminal state: agent `Failed` with the freshly-minted re-spawn JWT
/// removed (agent JWT auth validates token presence + signature, not
/// agent status — a terminal agent must not keep a valid token), task
/// `Blocked`, `TaskBlocked` to the repo orchestrator, and the priority-1
/// `ComputeTargetUnavailable` notification for workspace admins.
async fn respawn_with_retry(
    state: &Arc<AppState>,
    agent: &Agent,
    task: &Task,
    repo: Option<&gyre_domain::Repository>,
    first_error: anyhow::Error,
) {
    let mut reason = format!("{first_error:#}");
    for attempt in 1..=crate::signal_chain::SPAWN_RETRY_BACKOFF_SECS.len() {
        let backoff = crate::signal_chain::SPAWN_RETRY_BACKOFF_SECS[attempt - 1];
        tracing::warn!(
            agent_id = %agent.id,
            task_id = %task.id,
            attempt,
            backoff_secs = backoff,
            reason = %reason,
            "ralph-loop: re-spawn launch failed; retrying with backoff"
        );
        crate::signal_chain::touch_task(state, &task.id).await;
        tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;
        match relaunch_agent_session(state, agent, task).await {
            Ok(()) => return,
            Err(e) => reason = format!("{e:#}"),
        }
    }

    // All retries exhausted — the Phase 7 terminal spawn-failure state.
    let now = now_secs();
    let mut failed = agent.clone();
    let _ = failed.transition_status(AgentStatus::Failed);
    let _ = state.agents.update(&failed).await;
    // The re-spawn minted a fresh JWT before the failed launch; a terminal
    // agent must not keep it (same teardown as the max-iterations path and
    // the scheduler's `teardown_failed_spawn`).
    let _ = state
        .kv_store
        .kv_remove("agent_tokens", agent.id.as_str())
        .await;

    // Freshest task row: the watcher snapshot predates the backoff awaits,
    // and the whole-row persist must not overwrite a concurrent mutation
    // (cancellation, spec rejection).
    let blocked = match state.tasks.find_by_id(&task.id).await {
        Ok(Some(mut t)) => {
            let old_status = t.status.clone();
            let _ = t.transition_status(TaskStatus::Blocked);
            t.updated_at = now;
            let _ = state.tasks.update(&t).await;
            // Auto-track the Block (analytics.md §Auto-Emitted Events —
            // every persisted task status transition records).
            crate::api::analytics::record_task_status_changed(state, &t, &old_status).await;
            t
        }
        _ => task.clone(),
    };
    // Auto-track the terminal agent death (analytics.md §Auto-Emitted
    // Events: agent.failed covers "fails or is killed").
    crate::api::analytics::record_agent_failed(
        state,
        &failed,
        &format!("re-spawn failed after all retries: {reason}"),
    )
    .await;
    notify_task_blocked(
        state,
        &blocked,
        repo,
        format!("re-spawn failed after all retries: {reason}"),
    )
    .await;
    if let Some(repo) = repo {
        crate::signal_chain::notify_compute_target_unavailable(state, repo, &blocked, &reason)
            .await;
    }
    tracing::error!(
        task_id = %task.id,
        agent_id = %agent.id,
        reason = %reason,
        "ralph-loop: re-spawn failed after all retries; task Blocked, agent Failed (agent-runtime.md §1 Phase 7)"
    );
}

/// Deliver the `TaskBlocked` message to the repo orchestrator's inbox
/// (agent-runtime.md §1 Phase 7): same routing mechanism as `SpecApproved`
/// — if no orchestrator is active, the server spawns one to process the
/// failure. Shared by the max-iterations path and the worker scheduler's
/// spawn-failure path.
pub(crate) async fn notify_task_blocked(
    state: &Arc<AppState>,
    task: &Task,
    repo: Option<&gyre_domain::Repository>,
    reason: String,
) {
    // Best-effort agent attribution for the payload (the failed attempt's
    // agent record, when one exists and is still linked to the task).
    let (agent_id, iteration) = match task.assigned_to.as_ref() {
        Some(aid) => match state.agents.find_by_id(aid).await {
            Ok(Some(a)) => (a.id.to_string(), a.iteration),
            _ => (aid.to_string(), 0),
        },
        None => (String::new(), 0),
    };
    let payload = serde_json::json!({
        "task_id": task.id.to_string(),
        "repo_id": task.repo_id.to_string(),
        "reason": reason,
        "agent_id": agent_id,
        "iteration": iteration,
    });

    let orchestrator = match repo {
        Some(repo) => crate::signal_chain::ensure_repo_orchestrator_for_failure(state, repo).await,
        None => None,
    };
    let destination = match orchestrator {
        Some(o) => Destination::Agent(o.id.clone()),
        // No orchestrator could be ensured (budget, spawn failure): the
        // message still goes on the bus workspace-destined so the failure
        // is observable.
        None => Destination::Workspace(task.workspace_id.clone()),
    };

    state
        .emit_event(
            Some(task.workspace_id.clone()),
            destination,
            MessageKind::TaskBlocked,
            Some(payload),
        )
        .await;
}

/// Collect every `TaskBlocked` message observable for a workspace, from
/// BOTH bus tiers the routing in [`notify_task_blocked`] uses: the
/// workspace event tier (`list_by_workspace`) when no orchestrator could
/// be ensured, and the repo orchestrator's Directed inbox
/// (`list_unacked`) when one was spawned on demand. Tests must not assert
/// Directed delivery through `list_by_workspace` — the port contract
/// excludes agent-destined messages from that listing (they are accessed
/// via `list_after`/`list_unacked`), so the primary routing is invisible
/// there.
#[cfg(test)]
pub(crate) async fn collect_task_blocked_messages(
    state: &Arc<AppState>,
    workspace_id: &Id,
) -> Vec<gyre_common::message::Message> {
    let mut msgs = state
        .messages
        .list_by_workspace(
            workspace_id,
            Some(MessageKind::TaskBlocked.as_str()),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap_or_default();
    if let Ok(agents) = state.agents.list_by_workspace(workspace_id).await {
        for orch in agents
            .iter()
            .filter(|a| a.orchestrator_type == gyre_domain::OrchestratorType::RepoOrchestrator)
        {
            if let Ok(inbox) = state.messages.list_unacked(&orch.id, 100).await {
                msgs.extend(
                    inbox
                        .into_iter()
                        .filter(|m| m.kind == MessageKind::TaskBlocked),
                );
            }
        }
    }
    msgs
}

/// Re-spawn an agent session on the workspace's compute target
/// (agent-runtime.md §1 Phase 7 re-spawn + §8: system-initiated agents use
/// the same compute target as their workspace).
///
/// The same agent entity is relaunched: fresh JWT, fresh process, the SAME
/// worktree/branch (the code state persists on the branch — the fresh
/// session reasons from the current state of the code, not from
/// conversation history), and the same meta-spec prompt set re-assembled
/// at pinned versions.
pub(crate) async fn relaunch_agent_session(
    state: &Arc<AppState>,
    agent: &Agent,
    task: &Task,
) -> anyhow::Result<()> {
    let repo = state
        .repos
        .find_by_id(&task.repo_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("repo {} not found", task.repo_id))?;
    let workspace = state
        .workspaces
        .find_by_id(&repo.workspace_id)
        .await
        .ok()
        .flatten();

    // The agent's worktree record carries its working directory (jj
    // workspace when provisioned, plain-git worktree otherwise) and branch.
    let worktrees = state.worktrees.find_by_agent(&agent.id).await?;
    let wt = worktrees
        .first()
        .ok_or_else(|| anyhow::anyhow!("agent {} has no worktree to re-spawn", agent.id))?;
    let branch = wt.branch.clone();
    let work_dir = wt.path.clone();

    // Resolve the workspace compute target (same as the first spawn) —
    // used both for the JWT's workload/attestation claims and the launch
    // context below.
    let resolved_ct_entity = match workspace.as_ref() {
        Some(ws) => resolve_workspace_target(state, ws).await?,
        None => None,
    };
    let wl_image_hash: Option<String> = match &resolved_ct_entity {
        Some(e) if e.target_type == gyre_domain::ComputeTargetType::Container => {
            let image = e.config["image"].as_str().unwrap_or("gyre-agent:latest");
            gyre_adapters::compute::ContainerTarget::new(image.to_string())
                .image_digest()
                .await
                .ok()
        }
        _ => None,
    };

    // Same meta-spec set: re-assembled from the registry at the pinned
    // versions (the set SHA is recomputed; a changed set is a changed
    // prompt, which is the reconciliation model, not history drift).
    let prompt_set = crate::prompt_assembly::assemble_prompt_set(state, task).await;

    // Clone URL for the re-spawned process.
    let ws_slug = workspace
        .as_ref()
        .map(|ws| ws.slug.clone())
        .unwrap_or_else(|| repo.workspace_id.to_string());
    let clone_url = format!("{}/git/{}/{}", state.base_url, ws_slug, repo.name);

    // Same persona (agent-runtime.md §1 Phase 7 / AgentLaunchCtx contract):
    // the re-spawned session is styled by the persona bound to the agent
    // row at first spawn. Fail closed when the binding no longer resolves
    // to an Approved persona — silently dropping or substituting the
    // persona prompt would drift the session's behavioral context.
    let persona = match &agent.persona_id {
        Some(persona_id) => {
            let p = state
                .personas
                .find_by_id(persona_id)
                .await?
                .ok_or_else(|| {
                    anyhow::anyhow!("persona {persona_id} bound to agent {} not found", agent.id)
                })?;
            if p.approval_status != gyre_domain::PersonaApprovalStatus::Approved {
                anyhow::bail!(
                    "persona '{}' bound to agent {} is {:?}; a human must approve \
                     it before the agent can be re-spawned",
                    p.slug,
                    agent.id,
                    p.approval_status
                );
            }
            Some(p)
        }
        None => None,
    };

    // Fresh JWT (§1 Token Scoping + task-129 review round 6): minted with
    // the SAME worker scope claims as the first spawn
    // (`mint_worker_scoped`: tenant, `workspace_id`, `repo_id`, and the
    // fine-grained `repo:{name}:write` scope list) so the ABAC subject
    // attribute `subject.repo_scope` (§Attributes, source "Agent OIDC
    // claim") is real on every request the re-spawned session makes —
    // without it the `builtin-agent-repo-scope` containment Deny fails
    // its condition and the agent gains tenant-wide access. Tenant comes
    // from the repo's workspace, resolved fail-closed: an unresolvable
    // workspace means the tenant scope cannot be determined, never a
    // fabricated identity. The persona slug bound to the agent row and
    // the identity-security.md attestation level implied by the compute
    // target mirror the first-spawn mint.
    let ws_tenant = match &workspace {
        Some(ws) => ws.tenant_id.to_string(),
        None => anyhow::bail!(
            "workspace {} for repo {} not found; the tenant scope for the \
             re-spawn JWT cannot be determined",
            repo.workspace_id,
            repo.id
        ),
    };
    let token = state
        .agent_signing_key
        .mint_worker_scoped(
            agent.id.as_str(),
            task.id.as_str(),
            agent.spawned_by.as_deref().unwrap_or("system"),
            &state.base_url,
            state.agent_jwt_ttl_secs,
            &ws_tenant,
            &repo.workspace_id.to_string(),
            &repo.id.to_string(),
            &repo.name,
            persona.as_ref().map(|p| p.slug.as_str()),
            // identity-security.md attestation levels: 1=raw subprocess,
            // 2=CLI-managed, 3=Gyre-managed container with attestation —
            // the same mapping the spawn path applies.
            Some(
                resolved_ct_entity
                    .as_ref()
                    .map_or(1u8, |ct| match ct.target_type {
                        gyre_domain::ComputeTargetType::Container => 3,
                        gyre_domain::ComputeTargetType::Ssh => 2,
                        gyre_domain::ComputeTargetType::Kubernetes => 1,
                    }),
            ),
            wl_image_hash,
        )
        .map_err(|e| anyhow::anyhow!("re-spawn JWT mint: {e}"))?;
    let _ = state
        .kv_store
        .kv_set("agent_tokens", agent.id.as_str(), token.clone())
        .await;

    // analytics.md §Auto-Emitted Events: the re-spawn's `agent.spawned`
    // persona property reports the persona bound to the agent row (the
    // same persona resolved above), not a hardcoded bootstrap label —
    // persona-performance decision signals keyed on this property are
    // garbage otherwise. Captured before `persona` moves into the
    // launch ctx.
    let persona_label = persona
        .as_ref()
        .map(|p| p.slug.clone())
        .unwrap_or_else(|| "default-worker".to_string());

    let launch = crate::api::spawn::launch_agent_process(
        state,
        &crate::api::spawn::AgentLaunchCtx {
            agent: agent.clone(),
            workspace,
            repo: repo.clone(),
            task: task.clone(),
            branch,
            token,
            clone_url,
            prompt_set,
            resolved_ct_entity,
            // The recorded worktree path is the agent's actual working
            // directory; use it for both resolution slots so the launch
            // picks it whenever it exists on disk.
            jj_workspace_path: work_dir.clone(),
            plain_worktree_path: work_dir,
            // Re-spawn carries no request persona; the agent's recorded
            // persona (agent_personas KV) is unchanged by re-spawning.
            persona_name: None,
            persona,
            // Workers only: this path requires a worktree (asserted above),
            // and interrogation agents have none.
            is_interrogation: false,
        },
    )
    .await;

    if !launch.launched {
        anyhow::bail!("compute-target spawn failed for re-spawn");
    }

    // Workload attestation for the new session (same record shape as the
    // first spawn — queryable via GET /agents/:id/workload).
    let att = crate::workload_attestation::attest_agent_with_container(
        agent.id.as_str(),
        launch.pid,
        &launch.compute_target_label,
        "",
        launch.container_id.clone(),
        launch.container_image.clone(),
    );
    if let Ok(json) = serde_json::to_string(&att) {
        let _ = state
            .kv_store
            .kv_set("workload_attestations", agent.id.as_str(), json)
            .await;
    }

    // Budget: the completion path decremented the active-agent counter;
    // the re-spawned session occupies a slot again.
    crate::api::budget::increment_active_agents(state, &repo.workspace_id.to_string()).await;

    // Auto-track agent spawn (analytics.md §Auto-Emitted Events — a
    // re-spawned session is an agent spawn). Same shape as the first-spawn
    // event in spawn_agent_core: the persona bound to the agent row, the
    // launch's resolved compute target, scoped to the workspace/repo.
    let ev = AnalyticsEvent::new(
        Id::new(uuid::Uuid::new_v4().to_string()),
        "agent.spawned",
        Some(agent.id.to_string()),
        serde_json::json!({
            "task_id": task.id.to_string(),
            "compute_target": launch.compute_target_label,
            "persona": persona_label,
            "orchestrator_type": "worker",
            "respawn": true,
            "iteration": agent.iteration,
        }),
        now_secs(),
    )
    .with_scope(None, None, Some(&repo.workspace_id), Some(&repo.id));
    let _ = state.analytics.record(&ev).await;

    tracing::info!(
        agent_id = %agent.id,
        task_id = %task.id,
        iteration = agent.iteration,
        "ralph-loop: agent re-spawned with fresh context (agent-runtime.md §1 Phase 7)"
    );
    Ok(())
}

/// Resolve the compute target for a system-initiated (re-)spawn: the
/// workspace's assignment, else the tenant default. No request-level
/// override exists on this path.
async fn resolve_workspace_target(
    state: &Arc<AppState>,
    workspace: &gyre_domain::Workspace,
) -> anyhow::Result<Option<gyre_domain::ComputeTargetEntity>> {
    if let Some(ct_id) = &workspace.compute_target_id {
        if let Some(e) = state.compute_targets.get_by_id(ct_id).await? {
            return Ok(Some(e));
        }
    }
    state
        .compute_targets
        .get_default_for_tenant(&workspace.tenant_id)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;
    use gyre_domain::{
        AgentWorktree, GateResult, GateStatus, GateType, MergeRequest, MrStatus, QualityGate,
    };

    #[test]
    fn default_max_iterations_matches_spec() {
        // agent-runtime.md §1 Phase 7: "default: 10".
        assert_eq!(DEFAULT_MAX_ITERATIONS, 10);
    }

    /// Fixture: workspace ws-1, repo r-1, an Idle agent (iteration 0) with a
    /// worktree, its task in Review, an Open MR, one failed gate result.
    /// `max_agent_iterations` configures the repo's Ralph-loop ceiling.
    async fn loop_state(max_agent_iterations: Option<u32>) -> Arc<AppState> {
        let state = test_state();
        let ws = gyre_domain::Workspace::new(Id::new("ws-1"), Id::new("t1"), "Ws", "ws", 0);
        state.workspaces.create(&ws).await.unwrap();

        // The repo-orchestrator persona the failure-routing path resolves
        // (approved, tenant-scoped to t1 — the spawn core fails closed
        // without one, task-099 F2).
        let mut persona = gyre_domain::Persona::new(
            Id::new("repo-orchestrator-persona"),
            "repo-orchestrator",
            "repo-orchestrator",
            gyre_domain::PersonaScope::Tenant(Id::new("t1")),
            "test orchestrator persona",
            0,
        );
        persona.approval_status = gyre_domain::PersonaApprovalStatus::Approved;
        state.personas.create(&persona).await.unwrap();

        // A real on-disk path: the re-launched agent process runs with the
        // worktree as its working directory.
        let worktree_path = std::env::temp_dir().join(format!(
            "gyre-ralph-test-wt-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&worktree_path).unwrap();

        let mut repo = gyre_domain::Repository::new(
            Id::new("r-1"),
            Id::new("ws-1"),
            "r-1",
            "/tmp/gyre-ralph-test-r-1",
            0,
        );
        repo.max_agent_iterations = max_agent_iterations;
        state.repos.create(&repo).await.unwrap();

        let mut task = Task::new(Id::new("task-1"), "Implement auth retry", 0);
        task.task_type = Some(gyre_domain::TaskType::Implementation);
        task.workspace_id = Id::new("ws-1");
        task.repo_id = Id::new("r-1");
        // Production parity: the spawn path assigns the spawned agent onto
        // the task (spawn.rs sets task.assigned_to = agent.id before the
        // session starts), so a task an agent is working through review
        // carries its assignee — the task.status_changed re-open event's
        // assigned_to property reports it.
        task.assigned_to = Some(Id::new("agent-1"));
        task.transition_status(TaskStatus::InProgress).unwrap();
        task.transition_status(TaskStatus::Review).unwrap();
        state.tasks.create(&task).await.unwrap();

        let mut agent = Agent::new(Id::new("agent-1"), "worker-1", 0);
        agent.transition_status(AgentStatus::Active).unwrap();
        agent.transition_status(AgentStatus::Idle).unwrap();
        agent.assign_task(task.id.clone());
        agent.workspace_id = Id::new("ws-1");
        agent.spawned_by = Some("system".to_string());
        state.agents.create(&agent).await.unwrap();
        state
            .worktrees
            .create(&AgentWorktree::new(
                Id::new("wt-1"),
                agent.id.clone(),
                Id::new("r-1"),
                Some(task.id.clone()),
                "task/abc",
                worktree_path.to_str().unwrap(),
                0,
            ))
            .await
            .unwrap();

        let mut mr = MergeRequest::new(
            Id::new("mr-1"),
            Id::new("r-1"),
            "Agent 'worker-1': Implement auth retry",
            "task/abc",
            "main",
            0,
        );
        mr.author_agent_id = Some(agent.id.clone());
        mr.workspace_id = Id::new("ws-1");
        state.merge_requests.create(&mr).await.unwrap();

        let gate = QualityGate {
            id: Id::new("gate-1"),
            repo_id: Id::new("r-1"),
            name: "unit tests".to_string(),
            gate_type: GateType::TestCommand,
            command: Some("cargo test".to_string()),
            required_approvals: None,
            persona: None,
            validation_type: None,
            required: true,
            gate_phase: Default::default(),
            position: 0,
            timeout_secs: None,
            created_at: 0,
        };
        state.quality_gates.save(&gate).await.unwrap();
        state
            .gate_results
            .save(&GateResult {
                id: Id::new("gr-1"),
                gate_id: gate.id.clone(),
                mr_id: mr.id.clone(),
                status: GateStatus::Failed,
                output: Some("test auth_retry failed: expected 3 attempts".to_string()),
                started_at: Some(0),
                finished_at: Some(1),
            })
            .await
            .unwrap();
        state
    }

    /// Phase 7 fail path under the ceiling: the agent is re-spawned —
    /// iteration incremented, status Active again, fresh JWT, same task —
    /// with the gate failure details in its inbox, the failed MR closed,
    /// and the task back to InProgress. The process launch is pinned with
    /// GYRE_AGENT_COMMAND=/bin/true (an explicit command wins over container
    /// auto-detection, so the launch is deterministic).
    #[tokio::test]
    async fn gate_failure_respawns_agent_with_fresh_context() {
        // Pin the launch command for THIS test only, and remove it on scope
        // exit (including panic unwind): GYRE_AGENT_COMMAND is process-global,
        // so a leaked value changes every later test's launch behavior
        // (order-dependent pollution — review F3, task-118).
        struct EnvVarRemove(&'static str);
        impl Drop for EnvVarRemove {
            fn drop(&mut self) {
                std::env::remove_var(self.0);
            }
        }
        let _env_guard = EnvVarRemove("GYRE_AGENT_COMMAND");
        std::env::set_var("GYRE_AGENT_COMMAND", "/bin/true");
        let state = loop_state(None).await; // default ceiling: 10

        // Bind an approved persona to the agent row so the re-spawn's
        // `agent.spawned` persona property is observable: analytics.md
        // §Auto-Emitted Events requires the property to report the
        // persona bound to the agent row, not a hardcoded bootstrap label
        // (regression: the emit site hardcoded "default-worker").
        {
            let mut bound = state
                .agents
                .find_by_id(&Id::new("agent-1"))
                .await
                .unwrap()
                .unwrap();
            bound.persona_id = Some(Id::new("repo-orchestrator-persona"));
            state.agents.update(&bound).await.unwrap();
        }

        on_gates_failed(&state, &Id::new("mr-1"), &Id::new("agent-1")).await;

        let agent = state
            .agents
            .find_by_id(&Id::new("agent-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(agent.status, AgentStatus::Active, "agent re-spawned");
        assert_eq!(agent.iteration, 1, "iteration incremented");
        assert_eq!(agent.current_task_id, Some(Id::new("task-1")));

        // Same task, back to InProgress (Phase 7 step 1).
        let task = state
            .tasks
            .find_by_id(&Id::new("task-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(task.status, TaskStatus::InProgress);

        // The failed MR is closed so the next completion creates a fresh one.
        let mr = state
            .merge_requests
            .find_by_id(&Id::new("mr-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(mr.status, MrStatus::Closed);

        // Gate failure details are in the agent's inbox (messages the
        // re-spawned session reads on startup via gyre_message_poll).
        let inbox = state
            .messages
            .list_unacked(&Id::new("agent-1"), 100)
            .await
            .unwrap();
        let failure = inbox
            .iter()
            .find(|m| m.kind == MessageKind::GateFailure)
            .expect("gate failure message in agent inbox");
        assert_eq!(
            failure
                .payload
                .as_ref()
                .and_then(|p| p["gate_name"].as_str()),
            Some("unit tests")
        );
        assert_eq!(
            failure.payload.as_ref().and_then(|p| p["output"].as_str()),
            Some("test auth_retry failed: expected 3 attempts"),
            "the agent receives the gate's REAL recorded output"
        );

        // Fresh JWT: the completion path revoked the old token; the
        // re-spawn mints and stores a new one.
        let token = state
            .kv_store
            .kv_get("agent_tokens", "agent-1")
            .await
            .unwrap();
        assert!(token.is_some(), "re-spawned agent has a fresh token");

        // analytics.md §Auto-Emitted Events: a re-spawned session is an
        // agent spawn — `agent.spawned` must record with the same shape as
        // the first-spawn event (task_id, compute_target, persona), scoped
        // to the workspace/repo, and marked as a re-spawn.
        let events = state
            .analytics
            .query(Some("agent.spawned"), None, 10)
            .await
            .unwrap();
        let ev = events
            .iter()
            .find(|e| e.agent_id.as_deref() == Some("agent-1"))
            .expect("re-spawn must record an agent.spawned event");
        assert_eq!(ev.properties["task_id"], "task-1");
        assert_eq!(ev.properties["respawn"], true);
        assert_eq!(ev.properties["iteration"], 1);
        assert!(
            ev.properties["compute_target"]
                .as_str()
                .is_some_and(|t| !t.is_empty()),
            "compute_target must be a non-empty string"
        );
        assert_eq!(
            ev.properties["persona"], "repo-orchestrator",
            "persona property must report the persona bound to the agent row, not a hardcoded label"
        );
        assert_eq!(ev.workspace_id.as_deref(), Some("ws-1"));
        assert_eq!(ev.repo_id.as_deref(), Some("r-1"));

        // analytics.md §Auto-Emitted Events (review round 2): the ralph
        // loop's MR close and task re-open are real trigger points —
        // mr.closed carries mr_id/repo_id/reason, task.status_changed
        // carries the Review→InProgress re-open.
        let events = state
            .analytics
            .query(Some("mr.closed"), None, 10)
            .await
            .unwrap();
        let ev = events
            .iter()
            .find(|e| e.properties["mr_id"].as_str() == Some("mr-1"))
            .expect("gate-failure MR close must record an mr.closed event");
        assert_eq!(ev.properties["repo_id"], "r-1");
        assert_eq!(ev.properties["reason"], "gate_failure");
        assert_eq!(ev.agent_id.as_deref(), Some("agent-1"));
        assert_eq!(ev.workspace_id.as_deref(), Some("ws-1"));
        assert_eq!(ev.repo_id.as_deref(), Some("r-1"));

        let events = state
            .analytics
            .query(Some("task.status_changed"), None, 10)
            .await
            .unwrap();
        let ev = events
            .iter()
            .find(|e| e.properties["task_id"].as_str() == Some("task-1"))
            .expect("gate-failure re-open must record a task.status_changed event");
        assert_eq!(ev.properties["old_status"], "Review");
        assert_eq!(ev.properties["new_status"], "InProgress");
        assert_eq!(ev.properties["assigned_to"], "agent-1");
    }

    /// Review round 6 (task-129, §Built-In Policies agent-repo-scope): the
    /// re-spawn JWT must carry the SAME worker scope claims as the first
    /// spawn (`mint_worker_scoped`: workspace_id + repo_id). The previous
    /// mint (`mint_with_workload`) hardcoded `repo_id: None`, so a
    /// re-spawned agent's requests had NO `subject.repo_scope` — the
    /// builtin-agent-repo-scope containment Deny (750) failed its
    /// condition and builtin-agent-scoped-access (700) granted
    /// tenant-wide access. Killing direction: with the claim-less mint,
    /// both assertions below fail (claims are None).
    #[tokio::test]
    async fn respawn_jwt_carries_worker_scope_claims() {
        struct EnvVarRemove(&'static str);
        impl Drop for EnvVarRemove {
            fn drop(&mut self) {
                std::env::remove_var(self.0);
            }
        }
        let _env_guard = EnvVarRemove("GYRE_AGENT_COMMAND");
        std::env::set_var("GYRE_AGENT_COMMAND", "/bin/true");
        let state = loop_state(None).await; // default ceiling: 10

        on_gates_failed(&state, &Id::new("mr-1"), &Id::new("agent-1")).await;

        let agent = state
            .agents
            .find_by_id(&Id::new("agent-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(agent.status, AgentStatus::Active, "agent re-spawned");

        let token = state
            .kv_store
            .kv_get("agent_tokens", "agent-1")
            .await
            .unwrap()
            .expect("re-spawned agent has a fresh token");
        let claims = state
            .agent_signing_key
            .validate(&token, &state.base_url)
            .expect("re-spawn JWT must validate against the signing key");
        assert_eq!(
            claims.repo_id.as_deref(),
            Some("r-1"),
            "the re-spawn JWT must carry the task's repo scope — without it, \
             agent-repo-scope containment is not enforced for re-spawned sessions"
        );
        assert_eq!(
            claims.workspace_id.as_deref(),
            Some("ws-1"),
            "the re-spawn JWT must carry the repo's workspace scope, like the \
             first-spawn JWT"
        );
    }

    /// Phase 7 spawn-failure policy on the RE-SPAWN path (review F1,
    /// task-118): a gate failure under the iteration ceiling re-spawns the
    /// agent; when the re-spawn LAUNCH fails (compute target unreachable),
    /// the policy is the same 3-retry exponential backoff (1s/5s/30s) as
    /// the first-spawn path — the task is NOT blocked after one attempt —
    /// and the terminal state carries the priority-1
    /// `ComputeTargetUnavailable` notification for workspace admins and
    /// removes the freshly-minted re-spawn JWT (agent JWT auth validates
    /// token presence + signature, not agent status, so a terminal Failed
    /// agent must not keep a valid token). Deterministic launch failure:
    /// the workspace's compute target is a Kubernetes target — `kubectl
    /// apply` cannot succeed without a cluster in any test environment.
    #[tokio::test(start_paused = true)]
    async fn respawn_failure_retries_then_blocks_task() {
        let state = loop_state(None).await; // ceiling 10; iteration 1 < 10

        // A workspace admin to receive the priority-1 notification.
        state
            .workspace_memberships
            .create(&gyre_domain::WorkspaceMembership::new(
                Id::new("mem-1"),
                Id::new("admin-user"),
                Id::new("ws-1"),
                gyre_domain::WorkspaceRole::Admin,
                Id::new("inviter"),
                0,
            ))
            .await
            .unwrap();

        // Unreachable Kubernetes compute target assigned to the workspace:
        // every re-spawn launch attempt fails deterministically.
        let mut ct = gyre_domain::ComputeTargetEntity::new(
            Id::new("ct-k8s"),
            Id::new("t1"),
            "k8s-unreachable",
            gyre_domain::ComputeTargetType::Kubernetes,
            0,
        );
        ct.config = serde_json::json!({ "image": "gyre-agent:latest" });
        state.compute_targets.create(&ct).await.unwrap();
        let mut ws = state
            .workspaces
            .find_by_id(&Id::new("ws-1"))
            .await
            .unwrap()
            .unwrap();
        ws.compute_target_id = Some(ct.id.clone());
        state.workspaces.update(&ws).await.unwrap();

        let started = tokio::time::Instant::now();
        on_gates_failed(&state, &Id::new("mr-1"), &Id::new("agent-1")).await;

        // The full backoff ladder ran: 1s + 5s + 30s of retry sleep between
        // the four launch attempts (attempt 0 + retries 1-3). A single
        // failed attempt with no retry — the pre-fix behavior — returns in
        // well under a second of paused time.
        assert!(
            started.elapsed() >= std::time::Duration::from_secs(36),
            "three backoff retries (1+5+30s) preceded the terminal state; elapsed {:?}",
            started.elapsed()
        );

        let task = state
            .tasks
            .find_by_id(&Id::new("task-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(task.status, TaskStatus::Blocked);
        let agent = state
            .agents
            .find_by_id(&Id::new("agent-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(agent.status, AgentStatus::Failed);
        assert_eq!(agent.iteration, 1, "the gate failure still counted");

        // The freshly-minted re-spawn JWT did not outlive the agent.
        let token = state
            .kv_store
            .kv_get("agent_tokens", "agent-1")
            .await
            .unwrap();
        assert!(token.is_none(), "failed re-spawn's JWT removed");

        // TaskBlocked message on the bus, routed to the repo
        // orchestrator spawned on demand (or workspace-destined when none
        // could be ensured) — the same routing as the max-iterations path.
        // Directed (orchestrator-inbox) delivery is invisible to
        // list_by_workspace by the port contract, so collect both tiers.
        let msgs = collect_task_blocked_messages(&state, &Id::new("ws-1")).await;
        assert!(
            msgs.iter()
                .any(|m| m.payload.as_ref().and_then(|p| p["task_id"].as_str()) == Some("task-1")),
            "TaskBlocked message emitted: {msgs:?}"
        );

        // Priority-1 notification for the workspace admin.
        let notifs = state
            .notifications
            .list_for_user(&Id::new("admin-user"), None, None, None, None, None, 100, 0)
            .await
            .unwrap();
        assert!(
            notifs.iter().any(|n| {
                n.notification_type == gyre_common::NotificationType::ComputeTargetUnavailable
                    && n.priority_rank == 1
            }),
            "priority-1 ComputeTargetUnavailable notification created: {notifs:?}"
        );

        // analytics.md §Auto-Emitted Events (review round 2): the terminal
        // re-spawn failure records agent.failed and task.status_changed —
        // the spec trigger points the reviewer found silent.
        let events = state
            .analytics
            .query(Some("agent.failed"), None, 10)
            .await
            .unwrap();
        let ev = events
            .iter()
            .find(|e| e.agent_id.as_deref() == Some("agent-1"))
            .expect("terminal re-spawn failure must record an agent.failed event");
        assert_eq!(ev.properties["task_id"], "task-1");
        assert!(
            ev.properties["reason"]
                .as_str()
                .is_some_and(|r| r.starts_with("re-spawn failed after all retries")),
            "reason names the re-spawn failure: {:?}",
            ev.properties["reason"]
        );

        let events = state
            .analytics
            .query(Some("task.status_changed"), None, 10)
            .await
            .unwrap();
        assert!(
            events.iter().any(|e| e.properties["task_id"] == "task-1"
                && e.properties["old_status"] == "InProgress"
                && e.properties["new_status"] == "Blocked"),
            "terminal Block must record a task.status_changed event: {events:?}"
        );
    }

    /// Phase 7 at the ceiling: max iterations → task Blocked, agent Failed,
    /// TaskBlocked message routed to the repo orchestrator (spawned on
    /// demand — none was live).
    #[tokio::test]
    async fn max_iterations_blocks_task_and_fails_agent() {
        let state = loop_state(Some(1)).await; // ceiling reached on iteration 1

        on_gates_failed(&state, &Id::new("mr-1"), &Id::new("agent-1")).await;

        let task = state
            .tasks
            .find_by_id(&Id::new("task-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(task.status, TaskStatus::Blocked);
        let agent = state
            .agents
            .find_by_id(&Id::new("agent-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(agent.status, AgentStatus::Failed);
        assert_eq!(agent.iteration, 1);

        // TaskBlocked message exists, carrying the reason. Directed
        // (orchestrator-inbox) delivery is invisible to list_by_workspace
        // by the port contract, so collect both tiers; the routing itself
        // is asserted precisely below via the orchestrator's inbox.
        let msgs = collect_task_blocked_messages(&state, &Id::new("ws-1")).await;
        assert!(
            msgs.iter().any(|m| {
                m.payload.as_ref().and_then(|p| p["task_id"].as_str()) == Some("task-1")
            }),
            "TaskBlocked message emitted: {msgs:?}"
        );
        // The routing spawned a repo orchestrator to process the failure
        // (none was live) and the message sits in its inbox.
        let orch = state
            .agents
            .list_by_workspace(&Id::new("ws-1"))
            .await
            .unwrap()
            .into_iter()
            .find(|a| a.orchestrator_type == gyre_domain::OrchestratorType::RepoOrchestrator);
        let orch = orch.expect("repo orchestrator spawned to process the failure");
        let inbox = state.messages.list_unacked(&orch.id, 100).await.unwrap();
        assert!(
            inbox.iter().any(|m| m.kind == MessageKind::TaskBlocked
                && m.payload.as_ref().and_then(|p| p["task_id"].as_str()) == Some("task-1")),
            "TaskBlocked delivered to the repo orchestrator inbox"
        );

        // Terminal agent: no token left behind.
        let token = state
            .kv_store
            .kv_get("agent_tokens", "agent-1")
            .await
            .unwrap();
        assert!(token.is_none(), "failed agent's token revoked");

        // analytics.md §Auto-Emitted Events (review round 2): the ceiling
        // records mr.closed, agent.failed, and both task.status_changed
        // transitions (Review→InProgress re-open, InProgress→Blocked).
        let events = state
            .analytics
            .query(Some("mr.closed"), None, 10)
            .await
            .unwrap();
        assert!(
            events.iter().any(
                |e| e.properties["mr_id"] == "mr-1" && e.properties["reason"] == "gate_failure"
            ),
            "ceiling MR close must record an mr.closed event: {events:?}"
        );

        let events = state
            .analytics
            .query(Some("agent.failed"), None, 10)
            .await
            .unwrap();
        let ev = events
            .iter()
            .find(|e| e.agent_id.as_deref() == Some("agent-1"))
            .expect("iteration ceiling must record an agent.failed event");
        assert_eq!(ev.properties["task_id"], "task-1");
        assert!(
            ev.properties["reason"]
                .as_str()
                .is_some_and(|r| r.starts_with("max iterations reached")),
            "reason names the iteration ceiling: {:?}",
            ev.properties["reason"]
        );

        let events = state
            .analytics
            .query(Some("task.status_changed"), None, 10)
            .await
            .unwrap();
        assert!(
            events.iter().any(|e| e.properties["task_id"] == "task-1"
                && e.properties["old_status"] == "InProgress"
                && e.properties["new_status"] == "Blocked"),
            "ceiling Block must record a task.status_changed event: {events:?}"
        );
        assert!(
            events.iter().any(|e| e.properties["task_id"] == "task-1"
                && e.properties["old_status"] == "Review"
                && e.properties["new_status"] == "InProgress"),
            "ceiling re-open must record a task.status_changed event: {events:?}"
        );
    }

    /// Phase 7 pass path tail: when the merged MR's author task is in
    /// Review, the merge completes the task (Done) and emits
    /// TaskTransitioned.
    #[tokio::test]
    async fn merge_completes_task() {
        let state = loop_state(None).await;
        let mr = state
            .merge_requests
            .find_by_id(&Id::new("mr-1"))
            .await
            .unwrap()
            .unwrap();
        let mut merged = mr.clone();
        merged.transition_status(MrStatus::Approved).unwrap();
        merged.transition_status(MrStatus::Merged).unwrap();
        state.merge_requests.update(&merged).await.unwrap();

        complete_task_on_merge(&state, &merged, &Id::new("agent-1")).await;

        let task = state
            .tasks
            .find_by_id(&Id::new("task-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(task.status, TaskStatus::Done);
        let msgs = state
            .messages
            .list_by_workspace(
                &Id::new("ws-1"),
                Some(MessageKind::TaskTransitioned.as_str()),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();
        assert!(msgs
            .iter()
            .any(|m| { m.payload.as_ref().and_then(|p| p["task_id"].as_str()) == Some("task-1") }));

        // task-146 review R4: the Review→Done terminal transition is a
        // persisted production status change — it must record a
        // task.status_changed analytics event like every sibling path.
        let events = state
            .analytics
            .query(Some("task.status_changed"), None, 10)
            .await
            .unwrap();
        assert!(
            events.iter().any(|e| e.properties["task_id"] == "task-1"
                && e.properties["old_status"] == "Review"
                && e.properties["new_status"] == "Done"
                && e.properties["assigned_to"] == "agent-1"),
            "merge completion must record a task.status_changed event: {events:?}"
        );
    }
}
