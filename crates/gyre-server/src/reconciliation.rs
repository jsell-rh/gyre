//! Meta-spec reconciliation controller and conformance sweep
//! (meta-spec-reconciliation.md §6, §10, §11).
//!
//! Two entry points:
//!
//! - `run_reconciliation` — the slow rollout (§6). Triggered when a
//!   workspace's meta-spec set binding moves to a new version of a
//!   meta-spec (via `PUT /api/v1/workspaces/{id}/meta-spec-set`) or when a
//!   registry meta-spec is approved on new content. Only APPROVED specs
//!   reconcile (§6: "a meta-spec change is approved and rolled out") —
//!   binding a Pending spec is not a rollout; the approval transition
//!   owns that wave, so one logical rollout produces exactly one wave.
//!   Computes the blast radius (repos in the workspace bound to the
//!   changed meta-spec) and creates a deduplicated reconciliation task
//!   per affected repo.
//!
//! - `run_conformance_sweep` — steady state (§10). A background job that
//!   periodically sweeps every workspace, comparing the workspace's active
//!   meta-spec set SHA against the meta-spec set SHA recorded in recent
//!   agent provenance (authorization attestation `InputContent`). When code
//!   was produced under a superseded meta-spec set, a drift-review task is
//!   created (deduplicated) and workspace members are notified.
//!
//! The sweep is database queries only — no agent spawns (§10: "The sweep is
//! cheap"). Agent spawning from reconciliation tasks happens through the
//! normal signal chain (`task_type: delegation` → repo orchestrator).

use std::collections::BTreeMap;
use std::sync::Arc;

use gyre_common::{Id, Notification, NotificationType};
use gyre_domain::{
    MergeQueueEntry, RolloutPolicy, RolloutStrategy, TaskPriority, TaskStatus, TaskType,
};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::AppState;

/// Label attached to every reconciliation task (§6 flow step 4).
pub const RECONCILIATION_LABEL: &str = "meta-spec-reconciliation";

/// Drift-review tasks created by the conformance sweep use this label.
pub const DRIFT_REVIEW_LABEL: &str = "meta-spec-drift-review";

/// Name under which the conformance sweep registers in the job registry.
pub const CONFORMANCE_SWEEP_JOB: &str = "meta_spec_conformance_sweep";

/// Default sweep interval: daily (§10 — "default: daily").
pub const SWEEP_INTERVAL_SECS: u64 = 86_400;

/// How far back the sweep inspects provenance records.
const SWEEP_PROVENANCE_WINDOW_SECS: u64 = 30 * 86_400;

// ---------------------------------------------------------------------------
// §11 — Reconciliation wave completion (ReconciliationCompleted semantics)
// ---------------------------------------------------------------------------

/// Check whether a workspace's reconciliation wave is complete and emit
/// `ReconciliationCompleted` if so (meta-spec-reconciliation.md §11:
/// "All reconciliation tasks for a workspace are done").
///
/// Called after any task with the reconciliation label reaches a terminal
/// status (Done or Cancelled). A wave is complete when the workspace has at
/// least one terminal reconciliation task and zero open ones. Wave duration
/// (trigger → last task terminal) is observed on the
/// `gyre_reconciliation_duration_seconds{workspace}` histogram — measured
/// from the most recent wave's task creation, which is task-creation time in
/// the same `run_reconciliation` call as the trigger (zero-latency feedback
/// per forge-advantages.md).
///
/// Multiple hooks can observe the same terminal transition (REST, MCP, and
/// archive writers), and a re-check may run after the wave completes — the
/// emission is deduplicated against persisted `reconciliation_completed`
/// messages keyed on the wave's identity (latest wave start + total task
/// count), so each wave completes and emits exactly once, including a
/// workspace's second and later waves.
pub async fn maybe_emit_reconciliation_completed(state: &Arc<AppState>, workspace_id: &Id) {
    let tasks = match state.tasks.list_by_workspace(workspace_id).await {
        Ok(t) => t,
        Err(e) => {
            warn!(workspace_id = %workspace_id, error = %e, "reconciliation completion check: task query failed");
            return;
        }
    };

    let wave: Vec<&gyre_domain::Task> = tasks
        .iter()
        .filter(|t| t.labels.iter().any(|l| l == RECONCILIATION_LABEL))
        .collect();

    // A wave needs at least one terminal task to be "complete" — a workspace
    // with no reconciliation tasks at all has no wave in flight and must not
    // emit (e.g. a transition of an unrelated task must not fire the event).
    let has_terminal = wave
        .iter()
        .any(|t| matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled));
    if !has_terminal {
        return;
    }
    let has_open = wave
        .iter()
        .any(|t| !matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled));
    if has_open {
        info!(
            workspace_id = %workspace_id,
            open = wave.iter().filter(|t| !matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled)).count(),
            "reconciliation wave still in flight"
        );
        return;
    }

    // Wave identity: (start of the MOST RECENT wave, total reconciliation
    // task count). Every task in one `run_reconciliation` call is stamped
    // with the same `now`, so one wave's tasks share a creation time — but
    // terminal tasks from PRIOR waves must not drag the identity back:
    // after wave 1 completes and emits, wave 2 (new version → new title →
    // new tasks, so creation-time dedup in `run_reconciliation` does not
    // suppress it) also completes, and keying on the earliest task's
    // creation would recompute wave 1's start, match the persisted
    // `wave_started_at`, and silently suppress the wave-2 event, its
    // MetaSpecDrift notifications, and its duration observation.
    // `max(created_at)` is the latest wave's start; the total task count
    // disambiguates successive waves created within the same wall-clock
    // second (the count only grows — tasks are never removed), so a re-check
    // of the same complete state matches exactly once.
    let wave_started_at = wave.iter().map(|t| t.created_at).max().unwrap_or(0);
    let task_count = wave.len() as u64;

    // Dedup against already-emitted completions (persisted Event-tier
    // messages): re-checks and multi-hook fan-in (REST + MCP + archive
    // writers can each observe the same terminal transition) must emit once.
    let already_emitted = state
        .messages
        .list_by_workspace(
            workspace_id,
            Some("reconciliation_completed"),
            None,
            None,
            None,
            Some(50),
        )
        .await
        .map(|msgs| {
            msgs.iter().any(|m| {
                m.payload
                    .as_ref()
                    .and_then(|p| p.get("wave_started_at"))
                    .and_then(|v| v.as_u64())
                    == Some(wave_started_at)
                    && m.payload
                        .as_ref()
                        .and_then(|p| p.get("task_count"))
                        .and_then(|v| v.as_u64())
                        == Some(task_count)
            })
        })
        .unwrap_or(false);
    if already_emitted {
        return;
    }

    // Wave duration: trigger → last task terminal.
    let now = crate::api::now_secs();
    let duration_secs = now.saturating_sub(wave_started_at);
    state
        .metrics
        .reconciliation_duration_seconds
        .with_label_values(&[workspace_id.as_str()])
        .observe(duration_secs as f64);
    info!(
        workspace_id = %workspace_id,
        duration_secs,
        "reconciliation: wave complete — all reconciliation tasks terminal"
    );
    crate::emit_reconciliation_completed(
        state,
        workspace_id.clone(),
        Some(serde_json::json!({
            "workspace_id": workspace_id.to_string(),
            "wave_started_at": wave_started_at,
            "task_count": task_count,
            "duration_secs": duration_secs,
        })),
    )
    .await;
}

/// Check whether a workspace's drift-review work is complete and emit the
/// `meta_spec_drift_resolved` event if so (meta-spec-reconciliation.md §11:
/// `MetaSpecDriftResolved` — drift remediation finished for a workspace).
///
/// Drift is resolved when every drift-review task created by the conformance
/// sweep for this workspace has reached a terminal status.
pub async fn maybe_emit_drift_resolved(state: &Arc<AppState>, workspace_id: &Id) {
    let tasks = match state.tasks.list_by_workspace(workspace_id).await {
        Ok(t) => t,
        Err(e) => {
            warn!(workspace_id = %workspace_id, error = %e, "drift-resolution check: task query failed");
            return;
        }
    };

    let drift_tasks: Vec<&gyre_domain::Task> = tasks
        .iter()
        .filter(|t| t.labels.iter().any(|l| l == DRIFT_REVIEW_LABEL))
        .collect();
    if drift_tasks.is_empty() {
        return;
    }
    let all_terminal = drift_tasks
        .iter()
        .all(|t| matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled));
    if !all_terminal {
        return;
    }

    info!(
        workspace_id = %workspace_id,
        tasks = drift_tasks.len(),
        "meta-spec drift resolved — all drift-review tasks terminal"
    );
    state
        .emit_event(
            Some(workspace_id.clone()),
            gyre_common::message::Destination::Workspace(workspace_id.clone()),
            gyre_common::message::MessageKind::Custom("meta_spec_drift_resolved".to_string()),
            Some(serde_json::json!({
                "workspace_id": workspace_id.to_string(),
                "tasks": drift_tasks.len(),
            })),
        )
        .await;
}

/// Emit the `meta_spec_changed` domain event (§11) — meta-spec content
/// updated in the registry. Fired by the registry update handler when the
/// prompt (content) of a meta-spec changes.
///
/// Emitted once per workspace whose meta-spec set binds the changed spec
/// (persisted Event-tier, Workspace destination — Broadcast messages are
/// never persisted by the message bus, and the binding workspaces are the
/// affected audience) plus once unscoped fallback when no workspace binds it.
pub async fn emit_meta_spec_changed(
    state: &Arc<AppState>,
    workspace_id: Option<&Id>,
    meta_spec: &gyre_domain::MetaSpec,
) {
    let payload = serde_json::json!({
        "meta_spec_id": meta_spec.id.to_string(),
        "name": meta_spec.name,
        "kind": format!("{:?}", meta_spec.kind),
        "version": meta_spec.version,
        "content_hash": meta_spec.content_hash,
    });
    match workspace_id {
        Some(ws_id) => {
            state
                .emit_event(
                    Some(ws_id.clone()),
                    gyre_common::message::Destination::Workspace(ws_id.clone()),
                    gyre_common::message::MessageKind::Custom("meta_spec_changed".to_string()),
                    Some(payload),
                )
                .await;
        }
        None => {
            // Unscoped registry change: fan out to every workspace that binds
            // this meta-spec (same audience resolution as the §6 trigger).
            let workspaces = state.workspaces.list().await.unwrap_or_default();
            let mut emitted = false;
            for ws in &workspaces {
                let binds = state
                    .meta_spec_sets
                    .get(&ws.id)
                    .await
                    .ok()
                    .flatten()
                    .and_then(|j| {
                        serde_json::from_str::<crate::api::meta_specs::MetaSpecSet>(&j).ok()
                    })
                    .map(|s| {
                        s.personas.values().any(|e| e.path == meta_spec.name)
                            || s.principles.iter().any(|e| e.path == meta_spec.name)
                            || s.standards.iter().any(|e| e.path == meta_spec.name)
                            || s.process.iter().any(|e| e.path == meta_spec.name)
                    })
                    .unwrap_or(false);
                if binds {
                    emitted = true;
                    state
                        .emit_event(
                            Some(ws.id.clone()),
                            gyre_common::message::Destination::Workspace(ws.id.clone()),
                            gyre_common::message::MessageKind::Custom(
                                "meta_spec_changed".to_string(),
                            ),
                            Some(payload.clone()),
                        )
                        .await;
                }
            }
            if !emitted {
                // No workspace binds this spec — the change is still an
                // observable registry event. Emit once unscoped (stored
                // without a workspace destination by the message repository).
                state
                    .emit_event(
                        None,
                        gyre_common::message::Destination::Broadcast,
                        gyre_common::message::MessageKind::Custom("meta_spec_changed".to_string()),
                        Some(payload),
                    )
                    .await;
            }
        }
    }
}

/// Emit the `meta_spec_set_updated` domain event (§11) — a workspace
/// meta-spec set binding changed. Fired by the set-update handler on every
/// successful PUT that changes the binding.
pub async fn emit_meta_spec_set_updated(
    state: &Arc<AppState>,
    workspace_id: &Id,
    changed_paths: &[String],
) {
    state
        .emit_event(
            Some(workspace_id.clone()),
            gyre_common::message::Destination::Workspace(workspace_id.clone()),
            gyre_common::message::MessageKind::Custom("meta_spec_set_updated".to_string()),
            Some(serde_json::json!({
                "workspace_id": workspace_id.to_string(),
                "changed": changed_paths,
            })),
        )
        .await;
}

// ---------------------------------------------------------------------------
// §6 — Reconciliation controller
// ---------------------------------------------------------------------------

/// Result summary of one reconciliation run (for logging/events/tests).
#[derive(Debug, Default, Clone)]
pub struct ReconciliationSummary {
    /// Repos for which a reconciliation task was created this run.
    pub tasks_created: usize,
    /// Repos already covered by an existing open reconciliation task.
    pub tasks_skipped: usize,
}

/// Run reconciliation for one workspace after its meta-spec set changed.
///
/// `changed_paths` are the meta-spec paths (set-entry `path` values, i.e.
/// registry meta-spec names) whose pinned version changed in this update.
/// For each, every repo in the workspace is a reconciliation candidate
/// (the workspace binding governs all repos in the workspace — §2), and
/// §6 step 2 requires a task per repo: "For each repo, creates a
/// reconciliation task" titled `Align code with updated {kind} {name}
/// v{version}`, scoped to that repo's orchestrator. A task is skipped only
/// when an open task with the same (workspace, repo, title) already exists
/// — a pending task elsewhere never suppresses this repo's reconciliation.
///
/// §7 rollout policy: the workspace's rollout policy governs how the wave
/// proceeds — `Immediate` creates every task at once, `Rolling` creates
/// only the first `max_concurrent_agents` tasks and defers the rest (the
/// follow-up batches are created by `maybe_advance_rolling_rollout` as each
/// batch completes), and `Background` creates every task but leaves them
/// inert in Backlog (no `task_type`, so they cannot trigger agent spawning
/// — the repo orchestrator decides when to engage them). Task priority is
/// the policy's `ReconciliationPriority` mapped onto `TaskPriority`.
///
/// Emits `ReconciliationStarted` (custom Event-tier kind, §11) when work is
/// created and `ReconciliationCompleted` (existing `MessageKind`) via the
/// shared helper — which also creates priority-6 `MetaSpecDrift`
/// notifications for workspace Admins/Developers/Owners.
pub async fn run_reconciliation(
    state: &Arc<AppState>,
    workspace_id: &Id,
    changed_paths: &[String],
) -> ReconciliationSummary {
    let mut summary = ReconciliationSummary::default();
    if changed_paths.is_empty() {
        return summary;
    }

    let now = crate::api::now_secs();
    let ws_id = workspace_id.clone();

    info!(
        workspace_id = %ws_id,
        changed = changed_paths.len(),
        "reconciliation: controller started"
    );

    // §7: the workspace's rollout policy governs how this wave proceeds.
    let policy = get_workspace_rollout_policy(state, &ws_id).await;
    let task_priority = policy.priority.task_priority();

    // Existing open tasks — the dedup set (same pattern as spec-lifecycle).
    let existing_tasks = state.tasks.list().await.unwrap_or_default();

    // All repos in the workspace bound to the meta-spec set (§6 step 1:
    // repos in the affected workspace bound to the changed meta-spec —
    // the set is the workspace-level binding, so every repo in the
    // workspace is affected).
    let repos = match state.repos.list_by_workspace(&ws_id).await {
        Ok(r) => r,
        Err(e) => {
            warn!(workspace_id = %ws_id, error = %e, "reconciliation: failed to list repos");
            return summary;
        }
    };
    if repos.is_empty() {
        info!(workspace_id = %ws_id, "reconciliation: no repos in workspace");
        return summary;
    }

    // Resolve each changed meta-spec path to its registry record (kind,
    // name, new version). The set entry `path` is the registry meta-spec
    // name (the registry is the spec-content source of truth; the set
    // pins `name@sha`).
    //
    // §6 gates reconciliation on an APPROVED rollout: "when a meta-spec
    // change is approved and rolled out". Binding a spec that is still
    // Pending (never approved) is not a rollout — reconciling now would
    // create a task for content no human has approved, and the later
    // approval trigger would fire a second wave for the same logical
    // rollout (double-trigger). The approval transition owns the
    // reconciliation for never-approved content.
    let mut changed_specs: Vec<gyre_domain::MetaSpec> = Vec::new();
    for path in changed_paths {
        match find_meta_spec_by_name(state, path).await {
            Ok(Some(ms)) => {
                if ms.approval_status != gyre_domain::meta_spec::MetaSpecApprovalStatus::Approved {
                    info!(
                        meta_spec = %path,
                        status = ms.approval_status.as_str(),
                        "reconciliation: changed meta-spec is not Approved yet; the approval trigger owns this rollout"
                    );
                    continue;
                }
                changed_specs.push(ms);
            }
            Ok(None) => {
                warn!(
                    meta_spec = %path,
                    "reconciliation: changed meta-spec not found in registry; skipping"
                );
            }
            Err(e) => {
                warn!(meta_spec = %path, error = %e, "reconciliation: registry lookup failed");
            }
        }
    }

    // Build the wave's candidate list (§6 step 2: one task per repo per
    // changed spec, deduplicated against open tasks).
    let mut candidates: Vec<ReconciliationCandidate> = Vec::new();
    for ms in &changed_specs {
        // §6 flow step 4 / task spec: title "Align code with updated {kind} {name} v{version}".
        let title = format!(
            "Align code with updated {} {} v{}",
            ms.kind, ms.name, ms.version
        );
        let description = format!(
            "Auto-created by the meta-spec reconciliation controller \
             (meta-spec-reconciliation.md §6).\nMeta-spec: {} ({}) v{}\n\
             Content hash: {}\nRe-run affected work under the updated rules; \
             no-op outcome (code already conforms) is valid.",
            ms.name, ms.kind, ms.version, ms.content_hash
        );

        for repo in &repos {
            // Dedup: skip only a non-terminal task with the same title in
            // THIS repo. A pending task in another repo or workspace never
            // suppresses this repo's reconciliation (§6 step 2).
            let exists = existing_tasks.iter().any(|t| {
                t.title == title
                    && t.workspace_id == ws_id
                    && t.repo_id == repo.id
                    && !matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled)
            });

            if exists {
                summary.tasks_skipped += 1;
                info!(title, repo_id = %repo.id, "reconciliation: task already exists, skipping");
                continue;
            }

            candidates.push(ReconciliationCandidate {
                repo_id: repo.id.clone(),
                title: title.clone(),
                spec_path: ms.name.clone(),
                description: description.clone(),
            });
        }
    }

    // §7 strategy dispatch.
    match policy.strategy {
        RolloutStrategy::Immediate | RolloutStrategy::Background => {
            // §7 Immediate: "Create all reconciliation tasks immediately.
            // Budget limits still apply." §7 Background: "Create tasks but
            // leave them in Backlog. Reconciliation competes with feature
            // work for agent time. Repo orchestrator decides priority."
            // Mechanical difference: Immediate tasks carry the Delegation
            // task_type (§6 step 5 — the repo-orchestrator spawning signal);
            // Background tasks are created WITHOUT a task_type, so they
            // cannot trigger agent spawning (enforced in spawn_agent_core)
            // and sit in Backlog until the orchestrator explicitly engages
            // them — the orchestrator decides priority, not the controller.
            let delegation = policy.strategy == RolloutStrategy::Immediate;
            for cand in candidates {
                if create_reconciliation_task(
                    state,
                    &ws_id,
                    &cand,
                    task_priority.clone(),
                    delegation,
                    now,
                )
                .await
                {
                    summary.tasks_created += 1;
                }
            }
        }
        RolloutStrategy::Rolling => {
            // §7 Rolling: "Create tasks in batches. Wait for each batch to
            // complete before starting the next. Batch size =
            // max_concurrent_agents." The first batch is created here; the
            // remainder is persisted (kv-backed, restart-safe) and each
            // follow-up batch is created by `maybe_advance_rolling_rollout`
            // when the previous batch reaches terminal status.
            let batch_size = policy.max_concurrent_agents.max(1) as usize;
            let mut iter = candidates.into_iter();
            let first_batch: Vec<ReconciliationCandidate> =
                iter.by_ref().take(batch_size).collect();
            let remaining: Vec<PendingRolloutTask> =
                iter.map(PendingRolloutTask::from_candidate).collect();

            // (repo_id, title) of the tasks this wave just put in flight:
            // any stored deferred entry for the same work is consumed — it
            // is now open, and a stale deferral would re-create it later.
            let mut created_keys: Vec<(String, String)> = Vec::new();
            for cand in first_batch {
                if create_reconciliation_task(
                    state,
                    &ws_id,
                    &cand,
                    task_priority.clone(),
                    true,
                    now,
                )
                .await
                {
                    summary.tasks_created += 1;
                    created_keys.push((cand.repo_id.to_string(), cand.title.clone()));
                }
            }

            if !remaining.is_empty() || !created_keys.is_empty() {
                // Merge with any in-flight remainder: an overlapping wave
                // (another meta-spec change approved while this rolling
                // rollout is draining) must extend the pending work, not
                // overwrite it. `created_keys` prunes stored deferrals of
                // work this wave just put in flight.
                let deferred = remaining.len();
                match merge_pending_rollout(state, &ws_id, remaining, &created_keys).await {
                    Some(total_deferred) => {
                        if deferred > 0 {
                            info!(
                                workspace_id = %ws_id,
                                deferred,
                                total_deferred,
                                batch_size,
                                "reconciliation: rolling rollout — first batch created, remainder deferred"
                            );
                        }
                    }
                    None => {
                        // Read failure aborted the merge: the stored
                        // remainder is untouched, but THIS wave's deferred
                        // remainder was not persisted — operator-visible
                        // so the missing follow-up batches can be
                        // re-triggered.
                        warn!(
                            workspace_id = %ws_id,
                            deferred,
                            "reconciliation: rolling rollout — pending-state read failed, this wave's remainder was NOT deferred; stored remainder untouched"
                        );
                    }
                }
            }
        }
    }

    if summary.tasks_created > 0 {
        // §11 ReconciliationStarted: controller created tasks for a workspace.
        state
            .emit_event(
                Some(ws_id.clone()),
                gyre_common::message::Destination::Workspace(ws_id.clone()),
                gyre_common::message::MessageKind::Custom("reconciliation_started".to_string()),
                Some(serde_json::json!({
                    "workspace_id": ws_id.to_string(),
                    "tasks_created": summary.tasks_created,
                    "tasks_skipped": summary.tasks_skipped,
                    "strategy": policy.strategy.as_str(),
                })),
            )
            .await;
        // ReconciliationCompleted is NOT emitted here: §11 defines it as
        // "all reconciliation tasks for a workspace are done", which is a
        // terminal-status event, not a creation-time event. The wave-start
        // event above is ReconciliationStarted; wave completion is detected
        // by `maybe_emit_reconciliation_completed` when the last open
        // reconciliation task goes terminal (for a rolling rollout that is
        // the LAST batch's completion — `on_reconciliation_task_terminal`
        // advances the rollout before the completion check).
    }

    // §11 metrics: gyre_reconciliation_tasks_total{workspace, status}.
    record_reconciliation_metrics(
        state,
        ws_id.as_str(),
        summary.tasks_created as u64,
        summary.tasks_skipped as u64,
    );
    info!(
        workspace_id = %ws_id,
        created = summary.tasks_created,
        skipped = summary.tasks_skipped,
        strategy = policy.strategy.as_str(),
        "reconciliation: controller finished"
    );

    summary
}

/// One unit of reconciliation work: a task to create for a specific repo
/// under a specific changed meta-spec (title/description carry the §6
/// format; the candidate list is strategy input).
struct ReconciliationCandidate {
    repo_id: Id,
    title: String,
    spec_path: String,
    description: String,
}

/// Create one reconciliation task from a candidate.
///
/// `delegation` sets the §6 step-5 signal (repo-orchestrator spawning);
/// Background-strategy tasks are created without it (§7: "leave them in
/// Backlog... repo orchestrator decides priority"). Returns true when the
/// task was created.
async fn create_reconciliation_task(
    state: &Arc<AppState>,
    ws_id: &Id,
    cand: &ReconciliationCandidate,
    priority: TaskPriority,
    delegation: bool,
    now: u64,
) -> bool {
    let task_id = Id::new(uuid::Uuid::new_v4().to_string());
    let mut task = gyre_domain::Task::new(task_id.clone(), &cand.title, now);
    // §7: task priority is the rollout policy's ReconciliationPriority
    // mapped onto the Task priority the repo orchestrator orders by.
    task.priority = priority;
    task.labels = vec![RECONCILIATION_LABEL.to_string(), "auto-created".to_string()];
    task.description = Some(cand.description.clone());
    task.workspace_id = ws_id.clone();
    task.repo_id = cand.repo_id.clone();
    task.spec_path = Some(cand.spec_path.clone());
    // §6 flow step 5: the repo orchestrator spawns the reconciliation
    // agent — delegation is the signal-chain discriminator for that.
    // Background (§7) passes `delegation = false` to leave the task inert.
    task.task_type = if delegation {
        Some(TaskType::Delegation)
    } else {
        None
    };

    match state.tasks.create(&task).await {
        Err(e) => {
            warn!(title = %cand.title, repo_id = %cand.repo_id, error = %e, "reconciliation: failed to create task");
            false
        }
        Ok(()) => {
            info!(title = %cand.title, repo_id = %cand.repo_id, "reconciliation: created reconciliation task");
            state
                .emit_event(
                    Some(ws_id.clone()),
                    gyre_common::message::Destination::Workspace(ws_id.clone()),
                    gyre_common::message::MessageKind::TaskCreated,
                    Some(serde_json::json!({
                        "task_id": task_id.to_string(),
                        "reason": "meta-spec-reconciliation",
                    })),
                )
                .await;
            true
        }
    }
}

// ---------------------------------------------------------------------------
// §7 — Rollout policy
// ---------------------------------------------------------------------------

/// KV namespace for pending rolling-rollout state. The kv store is
/// port-backed (SQLite/Postgres in DB mode), so a deferred batch survives a
/// restart instead of orphaning the remainder of the wave.
const ROLLOUT_PENDING_NS: &str = "rollout_pending";

/// Load a workspace's effective rollout policy (§7). An unset policy is the
/// spec default ("Rolling strategy, Normal priority, auto_merge: false") —
/// not an error. A corrupt or unreadable row falls back to the same default
/// with a warning: the default is the safe, spec-mandated configuration, and
/// a damaged row must not brick reconciliation for the workspace.
pub async fn get_workspace_rollout_policy(
    state: &Arc<AppState>,
    workspace_id: &Id,
) -> RolloutPolicy {
    match state.rollout_policies.get(workspace_id).await {
        Ok(Some(json)) => match serde_json::from_str::<RolloutPolicy>(&json) {
            Ok(policy) => policy,
            Err(e) => {
                warn!(
                    workspace_id = %workspace_id,
                    error = %e,
                    "rollout policy: corrupt stored policy, using spec default"
                );
                RolloutPolicy::default()
            }
        },
        Ok(None) => RolloutPolicy::default(),
        Err(e) => {
            warn!(
                workspace_id = %workspace_id,
                error = %e,
                "rollout policy: load failed, using spec default"
            );
            RolloutPolicy::default()
        }
    }
}

/// One deferred reconciliation task of a rolling rollout (§7). The task is
/// fully materialized at trigger time (title/description embed the changed
/// meta-spec version), so batch creation does not depend on the registry
/// still holding the same versions when the batch runs.
#[derive(Serialize, Deserialize)]
struct PendingRolloutTask {
    repo_id: String,
    title: String,
    spec_path: String,
    description: String,
}

impl PendingRolloutTask {
    fn from_candidate(c: ReconciliationCandidate) -> Self {
        Self {
            repo_id: c.repo_id.to_string(),
            title: c.title,
            spec_path: c.spec_path,
            description: c.description,
        }
    }

    fn into_candidate(self) -> ReconciliationCandidate {
        ReconciliationCandidate {
            repo_id: Id::new(self.repo_id),
            title: self.title,
            spec_path: self.spec_path,
            description: self.description,
        }
    }
}

/// Persisted remainder of a rolling rollout (§7): the tasks not yet created.
/// The batch size is deliberately NOT captured here — every advance re-reads
/// the live policy, so an admin changing `max_concurrent_agents` (or the
/// strategy) mid-rollout takes effect on the next batch.
#[derive(Serialize, Deserialize)]
struct PendingRollout {
    workspace_id: String,
    remaining: Vec<PendingRolloutTask>,
}

/// Merge deferred rolling-rollout work into the workspace's pending state
/// (§7 Rolling). Returns `Some(count)` with the resulting deferred count,
/// or `None` when the merge was ABORTED because the stored pending state
/// could not be read — in that case nothing is written and the stored
/// remainder is left exactly as it was ("cannot determine state" must not
/// become "state is empty": overwriting with the incoming-only list would
/// silently discard the stored deferred work, the same loss class as the
/// overwrite bug this merge exists to prevent).
///
/// `incoming` are newly deferred tasks. `consumed` lists the (repo_id,
/// title) pairs the caller has resolved out of the pending remainder
/// (created as tasks, deduplicated against open work, or superseded) —
/// their stored entries are dropped. A stored entry with the same
/// (repo_id, title) as an incoming entry is replaced by it: a re-triggered
/// wave supersedes the older deferral of the same work instead of
/// duplicating it. Everything else stored is retained.
///
/// Merging — rather than overwriting — is what keeps overlapping waves
/// lossless. The pending remainder lives under ONE per-workspace key, so
/// an unconditional write would let a second wave triggered while a
/// rolling rollout is in flight (a wide window by design: Rolling waits
/// for each batch) silently discard the first wave's deferred tasks,
/// leaving their repos unreconciled against the superseded spec change
/// with no error or warning. Merged, both waves' remainders are batched
/// together and drained by the normal batch advance.
async fn merge_pending_rollout(
    state: &Arc<AppState>,
    ws_id: &Id,
    incoming: Vec<PendingRolloutTask>,
    consumed: &[(String, String)],
) -> Option<usize> {
    let mut remaining: Vec<PendingRolloutTask> = Vec::new();
    match state
        .kv_store
        .kv_get(ROLLOUT_PENDING_NS, ws_id.as_str())
        .await
    {
        Ok(Some(raw)) => match serde_json::from_str::<PendingRollout>(&raw) {
            Ok(stored) => {
                for t in stored.remaining {
                    let superseded = incoming
                        .iter()
                        .any(|n| n.repo_id == t.repo_id && n.title == t.title)
                        || consumed.iter().any(|c| c.0 == t.repo_id && c.1 == t.title);
                    if !superseded {
                        remaining.push(t);
                    }
                }
            }
            Err(e) => {
                // Unparseable stored state is unrecoverable garbage — it
                // cannot be merged with, so recovering to the incoming
                // remainder (rather than aborting) is the only forward
                // path that keeps the rollout drainable.
                warn!(
                    workspace_id = %ws_id,
                    error = %e,
                    "rolling rollout: corrupt pending state, keeping only the incoming remainder"
                );
            }
        },
        Ok(None) => {}
        Err(e) => {
            // Fail CLOSED: a read failure means the stored remainder is
            // unknown, not empty. Abort the merge without writing — an
            // overwrite here would discard the stored deferred work behind
            // a transient read error that does not affect the write path.
            // The stored state stays live for the next advance; work the
            // caller already created as tasks is deduplicated on that
            // advance, so the rollout self-heals.
            warn!(
                workspace_id = %ws_id,
                error = %e,
                "rolling rollout: failed to read pending state, merge aborted — stored remainder left untouched"
            );
            return None;
        }
    }
    remaining.extend(incoming);

    if remaining.is_empty() {
        let _ = state
            .kv_store
            .kv_remove(ROLLOUT_PENDING_NS, ws_id.as_str())
            .await;
        return Some(0);
    }
    let count = remaining.len();
    let pending = PendingRollout {
        workspace_id: ws_id.to_string(),
        remaining,
    };
    match serde_json::to_string(&pending) {
        Ok(json) => {
            if let Err(e) = state
                .kv_store
                .kv_set(ROLLOUT_PENDING_NS, ws_id.as_str(), json)
                .await
            {
                warn!(workspace_id = %ws_id, error = %e, "rolling rollout: failed to persist pending state");
            }
        }
        Err(e) => {
            warn!(workspace_id = %ws_id, error = %e, "rolling rollout: failed to serialize pending state");
        }
    }
    Some(count)
}

/// Create the next batch of a rolling rollout when the previous batch has
/// completed (§7 Rolling: "Wait for each batch to complete before starting
/// the next").
///
/// A batch is in flight while the workspace has any non-terminal
/// reconciliation task CARRYING a spawning signal (task_type) — the
/// batch boundary is "no open active reconciliation work", so an
/// earlier wave's still-running tasks also hold the next batch back (a
/// workspace still reconciling is not ready for more). §7 Background
/// tasks (no task_type, parked in Backlog by design) do NOT count: they
/// are inert until the repo orchestrator engages them and must not stall
/// the rollout's batches.
///
/// The policy is re-read on every advance: if the admin switched the
/// workspace to a non-Rolling strategy mid-rollout, the remainder is flushed
/// at once — the admin's latest configuration governs (§7: admins configure
/// rollout policies).
///
/// Called from `on_reconciliation_task_terminal` after any reconciliation
/// task reaches a terminal status, BEFORE the wave-completion check, so a
/// follow-up batch suppresses the completion event until the whole rollout
/// is done (§11: "all reconciliation tasks for a workspace are done").
pub async fn maybe_advance_rolling_rollout(state: &Arc<AppState>, workspace_id: &Id) {
    // Cheap early exit: no pending state → no rolling rollout in flight.
    let raw = match state
        .kv_store
        .kv_get(ROLLOUT_PENDING_NS, workspace_id.as_str())
        .await
    {
        Ok(Some(json)) => json,
        _ => return,
    };
    let mut pending: PendingRollout = match serde_json::from_str(&raw) {
        Ok(p) => p,
        Err(e) => {
            warn!(
                workspace_id = %workspace_id,
                error = %e,
                "rolling rollout: corrupt pending state, dropping it"
            );
            let _ = state
                .kv_store
                .kv_remove(ROLLOUT_PENDING_NS, workspace_id.as_str())
                .await;
            return;
        }
    };
    if pending.remaining.is_empty() {
        let _ = state
            .kv_store
            .kv_remove(ROLLOUT_PENDING_NS, workspace_id.as_str())
            .await;
        return;
    }

    // A batch is in flight while any non-terminal reconciliation task
    // carrying a spawning signal (task_type) exists. §7 Background tasks
    // are created WITHOUT a task_type and sit in Backlog by design —
    // until the repo orchestrator engages them, potentially forever —
    // so they must not hold the rollout's next batch hostage. The gate
    // counts the rollout's own in-flight work only, consistent with
    // `run_reconciliation`, which starts a new wave's first batch
    // without any in-flight check.
    let open_reconciliation = state
        .tasks
        .list_by_workspace(workspace_id)
        .await
        .map(|tasks| {
            tasks.iter().any(|t| {
                t.labels.iter().any(|l| l == RECONCILIATION_LABEL)
                    && t.task_type.is_some()
                    && !matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled)
            })
        })
        .unwrap_or(false);
    if open_reconciliation {
        return;
    }

    // Re-read the policy: the admin's live configuration governs every
    // advance — a mid-rollout switch to a non-Rolling strategy flushes the
    // rest at once, and a batch-size change applies to the next batch.
    let policy = get_workspace_rollout_policy(state, workspace_id).await;
    let batch_size = if policy.strategy == RolloutStrategy::Rolling {
        policy.max_concurrent_agents.max(1) as usize
    } else {
        usize::MAX
    };
    let task_priority = policy.priority.task_priority();
    // §7 Background: tasks are created WITHOUT the delegation signal —
    // they sit in Backlog and cannot trigger agent spawning; the repo
    // orchestrator decides priority. The flush path must derive the flag
    // from the LIVE policy (re-read above), not from the strategy the
    // rollout started under: a mid-rollout switch to Background must
    // produce inert tasks, exactly like `run_reconciliation`'s own
    // Background branch.
    let delegation = policy.strategy != RolloutStrategy::Background;
    let now = crate::api::now_secs();

    // Existing open tasks — same dedup rule as `run_reconciliation`.
    let existing_tasks = state.tasks.list().await.unwrap_or_default();

    let mut created: usize = 0;
    let mut kept: Vec<PendingRolloutTask> = Vec::new();
    // Every entry of the loaded snapshot is resolved by this advance
    // (created, deduplicated away, or re-deferred via `kept`). Passing the
    // snapshot keys as `consumed` makes the final merge retain ONLY
    // entries a concurrent wave registered after this snapshot was loaded
    // (plus `kept`) — a plain overwrite would discard them.
    let consumed: Vec<(String, String)> = pending
        .remaining
        .iter()
        .map(|t| (t.repo_id.clone(), t.title.clone()))
        .collect();
    for pending_task in std::mem::take(&mut pending.remaining) {
        // Already in flight (e.g. a concurrent wave created the same
        // task): drop from the remainder instead of duplicating it.
        let covered = existing_tasks.iter().any(|t| {
            t.title == pending_task.title
                && t.workspace_id == *workspace_id
                && t.repo_id.as_str() == pending_task.repo_id
                && !matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled)
        });
        if covered {
            continue;
        }
        if created < batch_size {
            let cand = pending_task.into_candidate();
            if create_reconciliation_task(
                state,
                workspace_id,
                &cand,
                task_priority.clone(),
                delegation,
                now,
            )
            .await
            {
                created += 1;
            }
        } else {
            kept.push(pending_task);
        }
    }

    match merge_pending_rollout(state, workspace_id, kept, &consumed).await {
        Some(0) => {
            info!(
                workspace_id = %workspace_id,
                created,
                "rolling rollout: no deferred work left"
            );
        }
        Some(deferred) => {
            info!(
                workspace_id = %workspace_id,
                created,
                deferred,
                "rolling rollout: next batch created"
            );
        }
        None => {
            // Read failure aborted the merge: the stored pre-advance
            // snapshot is untouched. Entries this advance already created
            // as tasks are still in that snapshot and are deduplicated on
            // the next advance, so the rollout self-heals — but the log
            // makes the anomaly operator-visible.
            warn!(
                workspace_id = %workspace_id,
                created,
                "rolling rollout: pending-state read failed on advance; stored remainder retained (already-created work is deduplicated next advance)"
            );
        }
    }

    if created > 0 {
        record_reconciliation_metrics(state, workspace_id.as_str(), created as u64, 0);
    }
}

/// Shared terminal-transition hook for reconciliation / drift-review tasks.
///
/// Called by every surface that moves a task to a terminal status (REST task
/// update, REST repo deletion, MCP `update_task`). Order matters: the
/// rolling rollout advances FIRST so a follow-up batch keeps the wave open,
/// then the §11 wave-completion events fire only when nothing remains.
pub async fn on_reconciliation_task_terminal(state: &Arc<AppState>, workspace_id: &Id) {
    maybe_advance_rolling_rollout(state, workspace_id).await;
    maybe_emit_reconciliation_completed(state, workspace_id).await;
    maybe_emit_drift_resolved(state, workspace_id).await;
}

/// §7 auto_merge: when the workspace's rollout policy sets `auto_merge`, a
/// reconciliation MR merges automatically once its gates pass.
///
/// Enforcement point: MR creation (agent completion — the §6 step-6 "MRs are
/// opened automatically" path). The MR is enqueued into the merge queue,
/// which triggers gate execution; the merge processor merges a queued entry
/// only after all gates pass (merge_processor step 4b). Gate passage is
/// still required — only the merge trigger is automatic. With the default
/// `auto_merge: false`, the MR waits for a human/agent enqueue.
pub async fn maybe_auto_enqueue_reconciliation_mr(
    state: &Arc<AppState>,
    mr: &gyre_domain::MergeRequest,
    task: &gyre_domain::Task,
) {
    if !task.labels.iter().any(|l| l == RECONCILIATION_LABEL) {
        return;
    }
    let policy = get_workspace_rollout_policy(state, &mr.workspace_id).await;
    if !policy.auto_merge {
        return;
    }

    let entry = MergeQueueEntry::new(
        Id::new(uuid::Uuid::new_v4().to_string()),
        mr.id.clone(),
        // Same default priority as an explicit POST /merge-queue enqueue.
        50,
        crate::api::now_secs(),
    );
    if let Err(e) = state.merge_queue.enqueue(&entry).await {
        warn!(
            mr_id = %mr.id,
            error = %e,
            "rollout policy auto_merge: failed to enqueue reconciliation MR"
        );
        return;
    }
    // Trigger quality-gate execution for the enqueued MR (same as the
    // explicit enqueue handler) — the merge processor merges once they pass.
    crate::gate_executor::trigger_gates_for_mr(
        state.clone(),
        mr.id.clone(),
        mr.repository_id.clone(),
    )
    .await;
    info!(
        mr_id = %mr.id,
        workspace_id = %mr.workspace_id,
        "rollout policy auto_merge: reconciliation MR enqueued — merges when gates pass"
    );
}

/// Diff two meta-spec sets and return the paths whose pinned version changed.
///
/// A path is "changed" when its SHA differs between the old and new set, or
/// when it appears only in the new set (newly bound). Removals are not
/// reconciliation triggers (unbinding a spec does not invalidate code
/// produced under it while it was active — no new version to align to).
pub fn diff_meta_spec_set(
    old: &crate::api::meta_specs::MetaSpecSet,
    new: &crate::api::meta_specs::MetaSpecSet,
) -> Vec<String> {
    use crate::api::meta_specs::MetaSpecSet;

    fn entries(set: &MetaSpecSet) -> BTreeMap<String, String> {
        let mut m: BTreeMap<String, String> = set
            .personas
            .values()
            .map(|e| (e.path.clone(), e.sha.clone()))
            .collect();
        // Two personas may pin the same spec path — keep the first pin.
        for e in set
            .principles
            .iter()
            .chain(&set.standards)
            .chain(&set.process)
        {
            m.entry(e.path.clone()).or_insert_with(|| e.sha.clone());
        }
        m
    }

    let old_entries = entries(old);
    let new_entries = entries(new);

    new_entries
        .into_iter()
        .filter(|(path, new_sha)| match old_entries.get(path) {
            Some(old_sha) => old_sha != new_sha,
            None => true,
        })
        .map(|(path, _)| path)
        .collect()
}

/// Look up a registry meta-spec by name (set entries pin `name@sha`).
async fn find_meta_spec_by_name(
    state: &Arc<AppState>,
    name: &str,
) -> anyhow::Result<Option<gyre_domain::MetaSpec>> {
    use gyre_ports::MetaSpecFilter;
    let all = state.meta_specs.list(&MetaSpecFilter::default()).await?;
    Ok(all.into_iter().find(|ms| ms.name == name))
}

// ---------------------------------------------------------------------------
// §10 — Conformance sweep
// ---------------------------------------------------------------------------

/// Result summary of one conformance sweep (for logging/events/tests).
#[derive(Debug, Default, Clone)]
pub struct SweepSummary {
    /// Workspaces scanned.
    pub workspaces_scanned: usize,
    /// Drifted (repo, set-sha) pairs found.
    pub drift_detected: usize,
    /// Drift-review tasks created this sweep.
    pub tasks_created: usize,
    /// Drift already covered by an existing open drift-review task.
    pub tasks_skipped: usize,
}

/// Run one conformance sweep pass across all workspaces (§10).
///
/// For each workspace with a bound meta-spec set, compare the active set
/// SHA (SHA-256 of the set JSON — same algorithm as
/// `compute_meta_spec_set_sha`) against the `meta_spec_set_sha` recorded in
/// recent authorization provenance (`chain_attestations` for the workspace's
/// repos). Provenance recording a different SHA means the code was produced
/// under a superseded meta-spec set: ensure a drift-review task exists and
/// notify workspace members (`MetaSpecDrift`).
///
/// Database queries only — no agent spawns (§10).
pub async fn run_conformance_sweep(state: &Arc<AppState>) -> anyhow::Result<SweepSummary> {
    let mut summary = SweepSummary::default();
    let now = crate::api::now_secs();

    let workspaces = state.workspaces.list().await?;
    summary.workspaces_scanned = workspaces.len();

    let existing_tasks = state.tasks.list().await.unwrap_or_default();

    for ws in &workspaces {
        // Desired state: the workspace's current meta-spec set SHA.
        let current_sha =
            crate::compute_meta_spec_set_sha(state.meta_spec_sets.as_ref(), &ws.id).await;
        if current_sha.is_empty() {
            // No set bound — nothing to conform to (§10: only repos whose
            // workspace binds a meta-spec set are in scope).
            continue;
        }

        let repos = match state.repos.list_by_workspace(&ws.id).await {
            Ok(r) => r,
            Err(e) => {
                warn!(workspace_id = %ws.id, error = %e, "sweep: failed to list repos");
                continue;
            }
        };

        // Drift review is per workspace+set-sha: one task covers all code
        // produced under the superseded set in this workspace.
        let mut drifted_repos: Vec<String> = Vec::new();
        let since = now.saturating_sub(SWEEP_PROVENANCE_WINDOW_SECS);

        for repo in &repos {
            // Actual state: provenance-recorded meta-spec set SHA for code
            // in this repo (§6 controller comparison: desired vs actual).
            let attestations = match state
                .chain_attestations
                .find_by_repo(repo.id.as_str(), since, now)
                .await
            {
                Ok(a) => a,
                Err(e) => {
                    warn!(repo_id = %repo.id, error = %e, "sweep: provenance query failed");
                    continue;
                }
            };

            let repo_drifted = attestations.iter().any(|att| {
                let recorded = match &att.input {
                    gyre_common::AttestationInput::Signed(si) => {
                        si.content.meta_spec_set_sha.clone()
                    }
                    gyre_common::AttestationInput::Derived(_) => String::new(),
                };
                !recorded.is_empty() && recorded != current_sha
            });

            if repo_drifted {
                drifted_repos.push(repo.id.to_string());
            }
        }

        if drifted_repos.is_empty() {
            continue;
        }

        summary.drift_detected += drifted_repos.len();

        // §10: "ensure a reconciliation task exists (create if missing)".
        // Dedup on (workspace, title) — the title embeds the active set
        // SHA, so a new set version yields a new task, but the workspace
        // id (not the slug, which need not be globally unique) scopes the
        // dedup: two workspaces with the same slug and same active set
        // SHA never suppress each other's drift review.
        let title = format!(
            "Review meta-spec drift in workspace {} (active set {})",
            ws.slug, current_sha
        );
        let exists = existing_tasks.iter().any(|t| {
            t.title == title
                && t.workspace_id == ws.id
                && !matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled)
        });

        if exists {
            summary.tasks_skipped += 1;
            info!(workspace_id = %ws.id, "sweep: drift-review task already exists");
        } else {
            let task_id = Id::new(uuid::Uuid::new_v4().to_string());
            let mut task = gyre_domain::Task::new(task_id, &title, now);
            task.priority = TaskPriority::Medium;
            task.labels = vec![DRIFT_REVIEW_LABEL.to_string(), "auto-created".to_string()];
            task.description = Some(format!(
                "Auto-created by the meta-spec conformance sweep \
                 (meta-spec-reconciliation.md §10).\nActive meta-spec set SHA: \
                 {}\nRepos with provenance under a superseded set: {}\n\
                 Verify code conforms to the active meta-spec set and reconcile drift.",
                current_sha,
                drifted_repos.join(", ")
            ));
            task.workspace_id = ws.id.clone();
            task.repo_id = Id::new(drifted_repos[0].clone());

            match state.tasks.create(&task).await {
                Err(e) => warn!(title, error = %e, "sweep: failed to create drift-review task"),
                Ok(()) => {
                    summary.tasks_created += 1;
                    info!(title, "sweep: created drift-review task");
                }
            }
        }

        // §11 MetaSpecDriftDetected: conformance sweep found code under a
        // superseded meta-spec.
        state
            .emit_event(
                Some(ws.id.clone()),
                gyre_common::message::Destination::Workspace(ws.id.clone()),
                gyre_common::message::MessageKind::Custom("meta_spec_drift_detected".to_string()),
                Some(serde_json::json!({
                    "workspace_id": ws.id.to_string(),
                    "active_meta_spec_set_sha": current_sha,
                    "drifted_repos": drifted_repos,
                })),
            )
            .await;

        // §11 metrics: gyre_meta_spec_drift_total{workspace, repo}.
        record_drift_metrics(state, ws.id.as_str(), &drifted_repos);

        // MetaSpecDrift notifications to workspace members (priority 6).
        notify_drift_members(state, ws, &current_sha, &drifted_repos, now).await;
    }

    if summary.drift_detected > 0 {
        info!(
            workspaces = summary.workspaces_scanned,
            drift = summary.drift_detected,
            created = summary.tasks_created,
            skipped = summary.tasks_skipped,
            "sweep: conformance sweep found drift"
        );
    } else {
        info!(
            workspaces = summary.workspaces_scanned,
            "sweep: no meta-spec drift detected"
        );
    }

    Ok(summary)
}

/// Create priority-6 MetaSpecDrift notifications for Admin/Developer/Owner
/// members of a drifted workspace.
async fn notify_drift_members(
    state: &Arc<AppState>,
    ws: &gyre_domain::Workspace,
    active_sha: &str,
    drifted_repos: &[String],
    now: u64,
) {
    let members = match state.workspace_memberships.list_by_workspace(&ws.id).await {
        Ok(m) => m,
        Err(e) => {
            warn!(workspace_id = %ws.id, error = %e, "sweep: failed to list members");
            return;
        }
    };

    for member in &members {
        if !matches!(
            member.role,
            gyre_domain::WorkspaceRole::Admin
                | gyre_domain::WorkspaceRole::Developer
                | gyre_domain::WorkspaceRole::Owner
        ) {
            continue;
        }
        let mut notif = Notification::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            ws.id.clone(),
            member.user_id.clone(),
            NotificationType::MetaSpecDrift,
            format!(
                "Meta-spec drift detected in workspace '{}' — code was produced under a superseded meta-spec set",
                ws.slug
            ),
            ws.tenant_id.to_string(),
            now as i64,
        );
        notif.body = Some(
            serde_json::json!({
                "active_meta_spec_set_sha": active_sha,
                "drifted_repos": drifted_repos,
            })
            .to_string(),
        );
        notif.source.entity_type = "workspace".to_string();
        notif.source.entity_id = ws.id.to_string();
        if let Err(e) = state.notifications.create(&notif).await {
            warn!(
                user = %member.user_id,
                error = %e,
                "sweep: failed to create MetaSpecDrift notification"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// §11 — Prometheus metrics
// ---------------------------------------------------------------------------

/// Record reconciliation metrics (§11 Observability):
/// `gyre_reconciliation_tasks_total{workspace, status}` — incremented at
/// task creation/skip time in `run_reconciliation`.
fn record_reconciliation_metrics(state: &AppState, workspace_id: &str, created: u64, skipped: u64) {
    if created > 0 {
        state
            .metrics
            .reconciliation_tasks_total
            .with_label_values(&[workspace_id, "created"])
            .inc_by(created as f64);
    }
    if skipped > 0 {
        state
            .metrics
            .reconciliation_tasks_total
            .with_label_values(&[workspace_id, "skipped"])
            .inc_by(skipped as f64);
    }
}

/// Record drift metrics (§11 Observability):
/// `gyre_meta_spec_drift_total{workspace, repo}` — incremented per drifted
/// repo at sweep time.
fn record_drift_metrics(state: &AppState, workspace_id: &str, drifted_repos: &[String]) {
    for repo in drifted_repos {
        state
            .metrics
            .meta_spec_drift_total
            .with_label_values(&[workspace_id, repo])
            .inc();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::meta_specs::MetaSpecPinnedEntry;
    use crate::mem::test_state;
    use gyre_common::message::MessageKind;
    use gyre_domain::meta_spec::{MetaSpec, MetaSpecApprovalStatus, MetaSpecKind, MetaSpecScope};
    use gyre_domain::{Repository, Workspace, WorkspaceMembership, WorkspaceRole};

    fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    fn sha256_hex(s: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(s.as_bytes());
        hex::encode(h.finalize())
    }

    async fn make_workspace(state: &Arc<AppState>, id: &str) -> gyre_domain::Workspace {
        let ws = Workspace {
            id: Id::new(id),
            tenant_id: Id::new("default"),
            name: format!("ws-{id}"),
            slug: id.to_string(),
            description: None,
            budget: None,
            max_repos: None,
            max_agents_per_repo: None,
            trust_level: gyre_domain::TrustLevel::Autonomous,
            llm_model: None,
            created_at: 1_000_000,
            compute_target_id: None,
        };
        state.workspaces.create(&ws).await.unwrap();
        ws
    }

    async fn make_repo(state: &Arc<AppState>, id: &str, ws_id: &str) -> Repository {
        let repo = Repository::new(
            Id::new(id),
            Id::new(ws_id),
            format!("repo-{id}"),
            format!("/tmp/does-not-matter/{id}"),
            now(),
        );
        state.repos.create(&repo).await.unwrap();
        repo
    }

    async fn make_meta_spec(state: &Arc<AppState>, name: &str, version: u32) -> MetaSpec {
        let prompt = format!("prompt for {name} v{version}");
        let ms = MetaSpec {
            id: Id::new(uuid::Uuid::new_v4().to_string()),
            kind: MetaSpecKind::Persona,
            name: name.to_string(),
            scope: MetaSpecScope::Global,
            scope_id: None,
            prompt,
            version,
            content_hash: sha256_hex(&format!("prompt for {name} v{version}")),
            required: false,
            approval_status: MetaSpecApprovalStatus::Approved,
            approved_by: Some("admin".to_string()),
            approved_at: Some(now()),
            approved_content_hash: Some(sha256_hex(&format!("prompt for {name} v{version}"))),
            created_by: "admin".to_string(),
            created_at: now(),
            updated_at: now(),
        };
        state.meta_specs.create(&ms).await.unwrap();
        ms
    }

    fn pin(path: &str, sha: &str) -> MetaSpecPinnedEntry {
        MetaSpecPinnedEntry {
            path: path.to_string(),
            sha: sha.to_string(),
        }
    }

    fn make_set(
        ws_id: &str,
        entries: Vec<MetaSpecPinnedEntry>,
    ) -> crate::api::meta_specs::MetaSpecSet {
        // Distinct persona keys per entry: a single reused key would make the
        // map collapse to the last entry, silently dropping the others (the
        // set-diff test would then pass by disappearance, not comparison).
        let mut personas = std::collections::BTreeMap::new();
        for (i, e) in entries.into_iter().enumerate() {
            personas.insert(format!("role-{i}"), e);
        }
        crate::api::meta_specs::MetaSpecSet {
            workspace_id: ws_id.to_string(),
            personas,
            principles: vec![],
            standards: vec![],
            process: vec![],
        }
    }

    async fn put_set(
        state: &Arc<AppState>,
        ws_id: &str,
        set: &crate::api::meta_specs::MetaSpecSet,
    ) {
        let json = serde_json::to_string(set).unwrap();
        state
            .meta_spec_sets
            .upsert(&Id::new(ws_id), &json)
            .await
            .unwrap();
    }

    async fn open_tasks_with_label(state: &Arc<AppState>, label: &str) -> Vec<gyre_domain::Task> {
        state
            .tasks
            .list()
            .await
            .unwrap()
            .into_iter()
            .filter(|t| t.labels.iter().any(|l| l == label))
            .collect()
    }

    // -- §6: reconciliation creates tasks for affected repos ----------------

    #[tokio::test(flavor = "multi_thread")]
    async fn reconciliation_creates_task_for_bound_repo() {
        let state = test_state();
        let ws = make_workspace(&state, "ws-recon-1").await;
        let _repo = make_repo(&state, "repo-1", "ws-recon-1").await;
        let ms = make_meta_spec(&state, "backend-developer", 4).await;

        let summary = run_reconciliation(&state, &ws.id, std::slice::from_ref(&ms.name)).await;

        assert_eq!(summary.tasks_created, 1);
        let tasks = open_tasks_with_label(&state, RECONCILIATION_LABEL).await;
        assert_eq!(tasks.len(), 1);
        let t = &tasks[0];
        assert_eq!(
            t.title,
            "Align code with updated meta:persona backend-developer v4"
        );
        assert_eq!(t.priority, TaskPriority::Medium);
        assert!(t.labels.iter().any(|l| l == "auto-created"));
        assert_eq!(t.workspace_id.as_str(), "ws-recon-1");
        assert_eq!(t.task_type, Some(TaskType::Delegation));
        assert_eq!(t.spec_path.as_deref(), Some("backend-developer"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn reconciliation_no_repos_creates_nothing() {
        let state = test_state();
        let ws = make_workspace(&state, "ws-recon-empty").await;
        let ms = make_meta_spec(&state, "backend-developer", 2).await;

        let summary = run_reconciliation(&state, &ws.id, std::slice::from_ref(&ms.name)).await;
        assert_eq!(summary.tasks_created, 0);
        assert_eq!(
            open_tasks_with_label(&state, RECONCILIATION_LABEL)
                .await
                .len(),
            0
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn reconciliation_unknown_spec_creates_nothing() {
        let state = test_state();
        let ws = make_workspace(&state, "ws-recon-unknown").await;
        let _repo = make_repo(&state, "repo-u", "ws-recon-unknown").await;

        let summary = run_reconciliation(&state, &ws.id, &["no-such-spec".to_string()]).await;
        assert_eq!(summary.tasks_created, 0);
    }

    // -- §6: deduplication ---------------------------------------------------

    #[tokio::test(flavor = "multi_thread")]
    async fn reconciliation_deduplicates_open_tasks() {
        let state = test_state();
        let ws = make_workspace(&state, "ws-recon-dup").await;
        let _repo = make_repo(&state, "repo-d", "ws-recon-dup").await;
        let ms = make_meta_spec(&state, "backend-developer", 4).await;

        let first = run_reconciliation(&state, &ws.id, std::slice::from_ref(&ms.name)).await;
        assert_eq!(first.tasks_created, 1);

        // Re-run with the same version → no new task.
        let second = run_reconciliation(&state, &ws.id, std::slice::from_ref(&ms.name)).await;
        assert_eq!(second.tasks_created, 0);
        assert_eq!(second.tasks_skipped, 1);
        assert_eq!(
            open_tasks_with_label(&state, RECONCILIATION_LABEL)
                .await
                .len(),
            1
        );

        // New version → new task (different title).
        state
            .meta_specs
            .update(&MetaSpec {
                version: 5,
                ..ms.clone()
            })
            .await
            .unwrap();
        let third = run_reconciliation(&state, &ws.id, std::slice::from_ref(&ms.name)).await;
        assert_eq!(third.tasks_created, 1);
        assert_eq!(
            open_tasks_with_label(&state, RECONCILIATION_LABEL)
                .await
                .len(),
            2
        );
    }

    // -- §7: rollout policy --------------------------------------------------

    async fn set_policy(state: &Arc<AppState>, ws_id: &str, policy: &gyre_domain::RolloutPolicy) {
        let json = serde_json::to_string(policy).unwrap();
        state
            .rollout_policies
            .upsert(&Id::new(ws_id), &json)
            .await
            .unwrap();
    }

    async fn pending_rollout_state(
        state: &Arc<AppState>,
        ws_id: &str,
    ) -> Option<serde_json::Value> {
        state
            .kv_store
            .kv_get("rollout_pending", ws_id)
            .await
            .unwrap()
            .map(|raw| serde_json::from_str(&raw).unwrap())
    }

    /// Drive every open reconciliation task in a workspace to Done through
    /// the legal transition chain (the same chain the REST/MCP task writers
    /// enforce), then run the shared terminal hook.
    async fn finish_all_reconciliation_tasks(state: &Arc<AppState>, ws_id: &str) {
        let tasks = state
            .tasks
            .list_by_workspace(&Id::new(ws_id))
            .await
            .unwrap();
        for mut t in tasks {
            if !t.labels.iter().any(|l| l == RECONCILIATION_LABEL) {
                continue;
            }
            if matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled) {
                continue;
            }
            if t.status == TaskStatus::Backlog {
                t.transition_status(TaskStatus::InProgress).unwrap();
            }
            if t.status == TaskStatus::Blocked {
                t.transition_status(TaskStatus::InProgress).unwrap();
            }
            if t.status == TaskStatus::InProgress {
                t.transition_status(TaskStatus::Review).unwrap();
            }
            t.transition_status(TaskStatus::Done).unwrap();
            state.tasks.update(&t).await.unwrap();
        }
    }

    /// Drive only the ACTIVE reconciliation tasks (those carrying a
    /// task_type — the rollout's own in-flight work) to Done, leaving
    /// inert §7 Background tasks (no task_type, parked in Backlog)
    /// untouched. Used by the Background-history stall regression: the
    /// parked tasks must stay open to exercise the gate.
    async fn finish_active_reconciliation_tasks(state: &Arc<AppState>, ws_id: &Id) {
        let tasks = state.tasks.list_by_workspace(ws_id).await.unwrap();
        for mut t in tasks {
            if !t.labels.iter().any(|l| l == RECONCILIATION_LABEL) {
                continue;
            }
            if t.task_type.is_none() {
                continue; // inert Background task: leave parked
            }
            if matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled) {
                continue;
            }
            if t.status == TaskStatus::Backlog {
                t.transition_status(TaskStatus::InProgress).unwrap();
            }
            if t.status == TaskStatus::Blocked {
                t.transition_status(TaskStatus::InProgress).unwrap();
            }
            if t.status == TaskStatus::InProgress {
                t.transition_status(TaskStatus::Review).unwrap();
            }
            t.transition_status(TaskStatus::Done).unwrap();
            state.tasks.update(&t).await.unwrap();
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn unset_policy_is_the_spec_default() {
        let state = test_state();
        let ws = make_workspace(&state, "ws-policy-default").await;

        let policy = get_workspace_rollout_policy(&state, &ws.id).await;
        assert_eq!(policy.strategy, gyre_domain::RolloutStrategy::Rolling);
        assert_eq!(policy.priority, gyre_domain::ReconciliationPriority::Normal);
        assert!(!policy.auto_merge);
        assert!(policy.max_concurrent_agents >= 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn immediate_strategy_creates_all_tasks_at_once() {
        let state = test_state();
        let ws = make_workspace(&state, "ws-imm").await;
        for i in 0..3 {
            make_repo(&state, &format!("repo-imm-{i}"), "ws-imm").await;
        }
        let ms = make_meta_spec(&state, "backend-developer", 4).await;
        set_policy(
            &state,
            "ws-imm",
            &gyre_domain::RolloutPolicy {
                strategy: gyre_domain::RolloutStrategy::Immediate,
                max_concurrent_agents: 1, // would batch if the strategy were Rolling
                priority: gyre_domain::ReconciliationPriority::Normal,
                auto_merge: false,
            },
        )
        .await;

        let summary = run_reconciliation(&state, &ws.id, std::slice::from_ref(&ms.name)).await;

        // All 3 tasks exist immediately — no deferral.
        assert_eq!(summary.tasks_created, 3);
        let tasks = open_tasks_with_label(&state, RECONCILIATION_LABEL).await;
        assert_eq!(tasks.len(), 3);
        assert!(tasks
            .iter()
            .all(|t| t.task_type == Some(TaskType::Delegation)));
        assert!(pending_rollout_state(&state, "ws-imm").await.is_none());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rolling_strategy_creates_batches_and_advances_on_completion() {
        let state = test_state();
        let ws = make_workspace(&state, "ws-roll").await;
        for i in 0..3 {
            make_repo(&state, &format!("repo-roll-{i}"), "ws-roll").await;
        }
        let ms = make_meta_spec(&state, "backend-developer", 4).await;
        set_policy(
            &state,
            "ws-roll",
            &gyre_domain::RolloutPolicy {
                strategy: gyre_domain::RolloutStrategy::Rolling,
                max_concurrent_agents: 2,
                priority: gyre_domain::ReconciliationPriority::Normal,
                auto_merge: false,
            },
        )
        .await;

        let summary = run_reconciliation(&state, &ws.id, std::slice::from_ref(&ms.name)).await;

        // First batch only: 2 of 3 tasks, the third deferred.
        assert_eq!(summary.tasks_created, 2);
        let tasks = open_tasks_with_label(&state, RECONCILIATION_LABEL).await;
        assert_eq!(tasks.len(), 2);
        let pending = pending_rollout_state(&state, "ws-roll").await;
        let pending = pending.expect("rolling rollout must persist the deferred remainder");
        assert_eq!(pending["remaining"].as_array().unwrap().len(), 1);

        // While the first batch is open, advancing creates nothing.
        maybe_advance_rolling_rollout(&state, &ws.id).await;
        assert_eq!(
            open_tasks_with_label(&state, RECONCILIATION_LABEL)
                .await
                .len(),
            2
        );

        // Batch 1 completes → the terminal hook creates batch 2.
        finish_all_reconciliation_tasks(&state, "ws-roll").await;
        on_reconciliation_task_terminal(&state, &ws.id).await;
        let tasks = open_tasks_with_label(&state, RECONCILIATION_LABEL).await;
        // 2 terminal + 1 open from batch 2.
        assert_eq!(tasks.len(), 3);
        assert_eq!(
            tasks
                .iter()
                .filter(|t| !matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled))
                .count(),
            1,
            "batch 2 must create exactly the deferred task"
        );
        let still_pending = pending_rollout_state(&state, "ws-roll").await;
        assert!(
            still_pending.is_none(),
            "creating the final batch consumes the deferred remainder — nothing left to track"
        );

        // While batch 2 is open, no completion event yet (§11 fires only
        // when ALL reconciliation tasks are done).
        let completed = count_completed_messages(&state, &ws.id).await;
        assert_eq!(completed, 0, "wave not complete while batch 2 is open");

        // Batch 2 completes → rollout finished: no pending state, wave
        // completion fires exactly once.
        finish_all_reconciliation_tasks(&state, "ws-roll").await;
        on_reconciliation_task_terminal(&state, &ws.id).await;
        assert!(
            pending_rollout_state(&state, "ws-roll").await.is_none(),
            "finished rollout must clear its pending state"
        );
        assert_eq!(
            count_completed_messages(&state, &ws.id).await,
            1,
            "ReconciliationCompleted must fire exactly once, at the end of the rollout"
        );

        // No further batches appear.
        on_reconciliation_task_terminal(&state, &ws.id).await;
        assert_eq!(
            open_tasks_with_label(&state, RECONCILIATION_LABEL)
                .await
                .len(),
            3
        );
    }

    async fn count_completed_messages(state: &Arc<AppState>, ws_id: &Id) -> usize {
        state
            .messages
            .list_by_workspace(
                ws_id,
                Some("reconciliation_completed"),
                None,
                None,
                None,
                Some(50),
            )
            .await
            .unwrap()
            .len()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn background_strategy_leaves_tasks_inert_in_backlog() {
        let state = test_state();
        let ws = make_workspace(&state, "ws-bg").await;
        for i in 0..2 {
            make_repo(&state, &format!("repo-bg-{i}"), "ws-bg").await;
        }
        let ms = make_meta_spec(&state, "backend-developer", 4).await;
        set_policy(
            &state,
            "ws-bg",
            &gyre_domain::RolloutPolicy {
                strategy: gyre_domain::RolloutStrategy::Background,
                max_concurrent_agents: 5,
                priority: gyre_domain::ReconciliationPriority::Normal,
                auto_merge: false,
            },
        )
        .await;

        let summary = run_reconciliation(&state, &ws.id, std::slice::from_ref(&ms.name)).await;

        // §7 Background: tasks are created but left in Backlog, WITHOUT the
        // delegation signal — they cannot trigger agent spawning until the
        // repo orchestrator explicitly engages them (spawn_agent_core
        // rejects tasks without a task_type).
        assert_eq!(summary.tasks_created, 2);
        let tasks = open_tasks_with_label(&state, RECONCILIATION_LABEL).await;
        assert_eq!(tasks.len(), 2);
        assert!(tasks.iter().all(|t| t.status == TaskStatus::Backlog));
        assert!(
            tasks.iter().all(|t| t.task_type.is_none()),
            "Background tasks must not carry the Delegation spawning signal"
        );
        assert!(pending_rollout_state(&state, "ws-bg").await.is_none());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rollout_policy_priority_maps_to_task_priority() {
        let state = test_state();
        let ws = make_workspace(&state, "ws-prio").await;
        make_repo(&state, "repo-prio", "ws-prio").await;
        let ms = make_meta_spec(&state, "backend-developer", 4).await;
        set_policy(
            &state,
            "ws-prio",
            &gyre_domain::RolloutPolicy {
                strategy: gyre_domain::RolloutStrategy::Immediate,
                max_concurrent_agents: 5,
                priority: gyre_domain::ReconciliationPriority::High,
                auto_merge: false,
            },
        )
        .await;

        run_reconciliation(&state, &ws.id, std::slice::from_ref(&ms.name)).await;

        let tasks = open_tasks_with_label(&state, RECONCILIATION_LABEL).await;
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].priority, TaskPriority::High);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rolling_strategy_switch_flushes_remaining_on_advance() {
        // An admin switching the policy mid-rollout governs: the deferred
        // remainder is created at once on the next advance.
        let state = test_state();
        let ws = make_workspace(&state, "ws-switch").await;
        for i in 0..3 {
            make_repo(&state, &format!("repo-sw-{i}"), "ws-switch").await;
        }
        let ms = make_meta_spec(&state, "backend-developer", 4).await;
        set_policy(
            &state,
            "ws-switch",
            &gyre_domain::RolloutPolicy {
                strategy: gyre_domain::RolloutStrategy::Rolling,
                max_concurrent_agents: 1,
                priority: gyre_domain::ReconciliationPriority::Normal,
                auto_merge: false,
            },
        )
        .await;

        let summary = run_reconciliation(&state, &ws.id, std::slice::from_ref(&ms.name)).await;
        assert_eq!(summary.tasks_created, 1);
        assert_eq!(
            pending_rollout_state(&state, "ws-switch").await.unwrap()["remaining"]
                .as_array()
                .unwrap()
                .len(),
            2
        );

        // Admin switches to immediate; batch 1 completes.
        set_policy(
            &state,
            "ws-switch",
            &gyre_domain::RolloutPolicy {
                strategy: gyre_domain::RolloutStrategy::Immediate,
                max_concurrent_agents: 1,
                priority: gyre_domain::ReconciliationPriority::Normal,
                auto_merge: false,
            },
        )
        .await;
        finish_all_reconciliation_tasks(&state, "ws-switch").await;
        on_reconciliation_task_terminal(&state, &ws.id).await;

        // The whole remainder flushed at once — not in batches of 1.
        let open = open_tasks_with_label(&state, RECONCILIATION_LABEL)
            .await
            .into_iter()
            .filter(|t| !matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled))
            .count();
        assert_eq!(open, 2, "strategy switch must flush the deferred remainder");
        assert!(pending_rollout_state(&state, "ws-switch").await.is_none());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rolling_batch_size_change_applies_to_next_batch() {
        // The admin's live configuration governs every advance: a
        // max_concurrent_agents change takes effect on the next batch.
        let state = test_state();
        let ws = make_workspace(&state, "ws-bsize").await;
        for i in 0..4 {
            make_repo(&state, &format!("repo-bs-{i}"), "ws-bsize").await;
        }
        let ms = make_meta_spec(&state, "backend-developer", 4).await;
        set_policy(
            &state,
            "ws-bsize",
            &gyre_domain::RolloutPolicy {
                strategy: gyre_domain::RolloutStrategy::Rolling,
                max_concurrent_agents: 1,
                priority: gyre_domain::ReconciliationPriority::Normal,
                auto_merge: false,
            },
        )
        .await;

        let summary = run_reconciliation(&state, &ws.id, std::slice::from_ref(&ms.name)).await;
        assert_eq!(summary.tasks_created, 1);

        // Admin widens the batch; batch 1 completes.
        set_policy(
            &state,
            "ws-bsize",
            &gyre_domain::RolloutPolicy {
                strategy: gyre_domain::RolloutStrategy::Rolling,
                max_concurrent_agents: 3,
                priority: gyre_domain::ReconciliationPriority::Normal,
                auto_merge: false,
            },
        )
        .await;
        finish_all_reconciliation_tasks(&state, "ws-bsize").await;
        on_reconciliation_task_terminal(&state, &ws.id).await;

        // The whole remaining 3-task remainder is created (new batch size),
        // not just 1 (trigger-time size).
        let open = open_tasks_with_label(&state, RECONCILIATION_LABEL)
            .await
            .into_iter()
            .filter(|t| !matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled))
            .count();
        assert_eq!(open, 3, "batch-size change must apply to the next batch");
        assert!(pending_rollout_state(&state, "ws-bsize").await.is_none());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rolling_strategy_switch_to_background_flushes_inert_tasks() {
        // F1 regression (review fd31a1cb): the flush path in
        // `maybe_advance_rolling_rollout` used to hardcode the delegation
        // signal, so a mid-rollout switch to Background created the
        // deferred remainder WITH `task_type: Delegation` — those tasks
        // can trigger agent spawning, contradicting §7 Background ("leave
        // them in Backlog... repo orchestrator decides priority") and
        // disagreeing with `run_reconciliation`'s own Background branch.
        // The flag must be derived from the re-read LIVE policy.
        let state = test_state();
        let ws = make_workspace(&state, "ws-sw-bg").await;
        for i in 0..3 {
            make_repo(&state, &format!("repo-swb-{i}"), "ws-sw-bg").await;
        }
        let ms = make_meta_spec(&state, "backend-developer", 4).await;
        set_policy(
            &state,
            "ws-sw-bg",
            &gyre_domain::RolloutPolicy {
                strategy: gyre_domain::RolloutStrategy::Rolling,
                max_concurrent_agents: 1,
                priority: gyre_domain::ReconciliationPriority::Normal,
                auto_merge: false,
            },
        )
        .await;

        let summary = run_reconciliation(&state, &ws.id, std::slice::from_ref(&ms.name)).await;
        assert_eq!(summary.tasks_created, 1);
        assert_eq!(
            pending_rollout_state(&state, "ws-sw-bg").await.unwrap()["remaining"]
                .as_array()
                .unwrap()
                .len(),
            2
        );

        // Admin switches to Background; batch 1 completes.
        set_policy(
            &state,
            "ws-sw-bg",
            &gyre_domain::RolloutPolicy {
                strategy: gyre_domain::RolloutStrategy::Background,
                max_concurrent_agents: 1,
                priority: gyre_domain::ReconciliationPriority::Normal,
                auto_merge: false,
            },
        )
        .await;
        finish_all_reconciliation_tasks(&state, "ws-sw-bg").await;
        on_reconciliation_task_terminal(&state, &ws.id).await;

        // The whole remainder flushed at once, and every flushed task is
        // inert: Backlog, no delegation signal — identical to what a
        // fresh Background rollout would have created.
        let open = open_tasks_with_label(&state, RECONCILIATION_LABEL)
            .await
            .into_iter()
            .filter(|t| !matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled))
            .collect::<Vec<_>>();
        assert_eq!(open.len(), 2, "Background switch must flush the remainder");
        assert!(
            open.iter().all(|t| t.status == TaskStatus::Backlog),
            "Background-flushed tasks must sit in Backlog"
        );
        assert!(
            open.iter().all(|t| t.task_type.is_none()),
            "Background-flushed tasks must not carry the Delegation spawning signal"
        );
        assert!(pending_rollout_state(&state, "ws-sw-bg").await.is_none());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn background_history_does_not_stall_rolling_rollout() {
        // Regression (review c9dc5a11): the next-batch gate in
        // `maybe_advance_rolling_rollout` used to count ANY non-terminal
        // reconciliation-labeled task as "batch in flight" — including
        // §7 Background tasks, which are created WITHOUT a task_type and
        // sit in Backlog by design until the repo orchestrator engages
        // them (potentially forever). A workspace with Background
        // history therefore stalled a later Rolling rollout indefinitely:
        // batch 1 completed, but the advance saw the inert Backlog tasks
        // as in-flight work and never created the next batch (and the
        // strategy-switch flush, living inside the same advance, never
        // ran either). The gate must count only the rollout's own
        // in-flight work — reconciliation tasks carrying a task_type.
        let state = test_state();
        let ws = make_workspace(&state, "ws-bg-hist").await;
        for i in 0..3 {
            make_repo(&state, &format!("repo-bg-hist-{i}"), "ws-bg-hist").await;
        }
        // Background wave parks 3 inert Backlog tasks (no task_type).
        let bg_spec = make_meta_spec(&state, "backend-developer", 4).await;
        set_policy(
            &state,
            "ws-bg-hist",
            &gyre_domain::RolloutPolicy {
                strategy: gyre_domain::RolloutStrategy::Background,
                max_concurrent_agents: 1,
                priority: gyre_domain::ReconciliationPriority::Normal,
                auto_merge: false,
            },
        )
        .await;
        let bg_summary =
            run_reconciliation(&state, &ws.id, std::slice::from_ref(&bg_spec.name)).await;
        assert_eq!(bg_summary.tasks_created, 3);
        let parked = open_tasks_with_label(&state, RECONCILIATION_LABEL).await;
        assert_eq!(parked.len(), 3);
        assert!(parked.iter().all(|t| t.task_type.is_none()));

        // Admin switches to Rolling; a new wave (different meta-spec, so
        // distinct titles) starts under it: batch 1 created, 2 deferred.
        set_policy(
            &state,
            "ws-bg-hist",
            &gyre_domain::RolloutPolicy {
                strategy: gyre_domain::RolloutStrategy::Rolling,
                max_concurrent_agents: 1,
                priority: gyre_domain::ReconciliationPriority::Normal,
                auto_merge: false,
            },
        )
        .await;
        let roll_spec = make_meta_spec(&state, "code-reviewer", 2).await;
        let summary =
            run_reconciliation(&state, &ws.id, std::slice::from_ref(&roll_spec.name)).await;
        assert_eq!(summary.tasks_created, 1);
        assert_eq!(
            pending_rollout_state(&state, "ws-bg-hist").await.unwrap()["remaining"]
                .as_array()
                .unwrap()
                .len(),
            2
        );

        // Finish ONLY the rolling batch-1 task; the inert Background
        // tasks stay open in Backlog (the stall condition).
        finish_active_reconciliation_tasks(&state, &ws.id).await;
        on_reconciliation_task_terminal(&state, &ws.id).await;

        // §7 Rolling: "Wait for each batch to complete before starting the
        // next" — batch 1 completed, so the next batch must be created
        // despite the 3 inert Background tasks still being open.
        let open = open_tasks_with_label(&state, RECONCILIATION_LABEL)
            .await
            .into_iter()
            .filter(|t| !matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled))
            .collect::<Vec<_>>();
        let active = open.iter().filter(|t| t.task_type.is_some()).count();
        let inert = open.iter().filter(|t| t.task_type.is_none()).count();
        assert_eq!(
            active, 1,
            "batch 2 must be created after batch 1 completes despite Background history"
        );
        assert_eq!(
            inert, 3,
            "the parked Background tasks must remain untouched in Backlog"
        );
        assert_eq!(
            pending_rollout_state(&state, "ws-bg-hist").await.unwrap()["remaining"]
                .as_array()
                .unwrap()
                .len(),
            1
        );

        // Draining the remaining batches finishes the rollout (batch size
        // 1 → two more advances), each proceeding despite the parked
        // Background tasks. Batch 2 completes → the last batch (3) is
        // created and nothing remains deferred.
        finish_active_reconciliation_tasks(&state, &ws.id).await;
        on_reconciliation_task_terminal(&state, &ws.id).await;
        assert!(
            pending_rollout_state(&state, "ws-bg-hist").await.is_none(),
            "batch 2 completion must create the final batch and drain the remainder"
        );
        let active = open_tasks_with_label(&state, RECONCILIATION_LABEL)
            .await
            .into_iter()
            .filter(|t| !matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled))
            .filter(|t| t.task_type.is_some())
            .count();
        assert_eq!(
            active, 1,
            "batch 3 (the last) must be created after batch 2 completes"
        );

        // Batch 3 completes → the rollout is done: no deferred work left,
        // and only the inert Background tasks remain open (their
        // engagement is the repo orchestrator's call, §7).
        finish_active_reconciliation_tasks(&state, &ws.id).await;
        on_reconciliation_task_terminal(&state, &ws.id).await;
        assert!(pending_rollout_state(&state, "ws-bg-hist").await.is_none());
        let open = open_tasks_with_label(&state, RECONCILIATION_LABEL)
            .await
            .into_iter()
            .filter(|t| !matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled))
            .collect::<Vec<_>>();
        assert_eq!(open.len(), 3);
        assert!(open.iter().all(|t| t.task_type.is_none()));
    }

    /// KvJsonStore wrapper whose `kv_get` fails while `fail_reads` is set —
    /// used to exercise the fail-closed pending-state merge: a read
    /// failure must abort the merge (stored remainder untouched), never
    /// overwrite the stored state with an incoming-only list.
    struct FlakyReadKvStore {
        inner: crate::mem::MemKvStore,
        fail_reads: std::sync::atomic::AtomicBool,
    }

    impl FlakyReadKvStore {
        fn new() -> Self {
            Self {
                inner: crate::mem::MemKvStore::default(),
                fail_reads: std::sync::atomic::AtomicBool::new(false),
            }
        }
    }

    #[async_trait::async_trait]
    impl gyre_ports::KvJsonStore for FlakyReadKvStore {
        async fn kv_set(&self, namespace: &str, key: &str, value: String) -> anyhow::Result<()> {
            self.inner.kv_set(namespace, key, value).await
        }

        async fn kv_get(&self, namespace: &str, key: &str) -> anyhow::Result<Option<String>> {
            if self.fail_reads.load(std::sync::atomic::Ordering::SeqCst) {
                Err(anyhow::anyhow!("injected kv read failure"))
            } else {
                self.inner.kv_get(namespace, key).await
            }
        }

        async fn kv_remove(&self, namespace: &str, key: &str) -> anyhow::Result<()> {
            self.inner.kv_remove(namespace, key).await
        }

        async fn kv_list(&self, namespace: &str) -> anyhow::Result<Vec<(String, String)>> {
            self.inner.kv_list(namespace).await
        }

        async fn kv_clear(&self, namespace: &str) -> anyhow::Result<()> {
            self.inner.kv_clear(namespace).await
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn merge_pending_rollout_read_failure_leaves_stored_state_untouched() {
        // F2 regression (review fd31a1cb): `merge_pending_rollout` used to
        // fail OPEN on a pending-state read error — it warned "keeping
        // only the incoming remainder" and then WROTE that incoming-only
        // list, silently discarding the stored deferred remainder behind
        // a transient read failure that did not affect the write path
        // (the same silent-work-loss class as the overwrite bug the merge
        // exists to prevent). A read failure must ABORT the merge: the
        // stored state stays exactly as it was.
        let kv = Arc::new(FlakyReadKvStore::new());
        let state = crate::mem::test_state_with_kv_store(kv.clone());
        let ws = make_workspace(&state, "ws-kvfail").await;
        for i in 0..4 {
            make_repo(&state, &format!("repo-kv-{i}"), "ws-kvfail").await;
        }
        let persona = make_meta_spec(&state, "backend-developer", 4).await;
        let reviewer = make_meta_spec(&state, "code-reviewer", 2).await;
        set_policy(
            &state,
            "ws-kvfail",
            &gyre_domain::RolloutPolicy {
                strategy: gyre_domain::RolloutStrategy::Rolling,
                max_concurrent_agents: 1,
                priority: gyre_domain::ReconciliationPriority::Normal,
                auto_merge: false,
            },
        )
        .await;

        // Wave A: persona v4 → 1 task + 3 deferred (stored).
        let a = run_reconciliation(&state, &ws.id, std::slice::from_ref(&persona.name)).await;
        assert_eq!(a.tasks_created, 1);
        assert_eq!(
            pending_rollout_state(&state, "ws-kvfail").await.unwrap()["remaining"]
                .as_array()
                .unwrap()
                .len(),
            3
        );

        // Wave B triggers while kv reads fail: its first batch is still
        // created (the write path is unaffected), but the merge of its
        // deferred remainder into the stored state must ABORT on the read
        // failure instead of overwriting the stored remainder.
        kv.fail_reads
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let b = run_reconciliation(&state, &ws.id, std::slice::from_ref(&reviewer.name)).await;
        assert_eq!(b.tasks_created, 1);
        kv.fail_reads
            .store(false, std::sync::atomic::Ordering::SeqCst);

        // Stored state is EXACTLY wave A's remainder — not overwritten
        // with wave B's incoming-only list, not merged.
        let pending = pending_rollout_state(&state, "ws-kvfail")
            .await
            .expect("aborted merge must leave the stored remainder in place");
        let remaining = pending["remaining"].as_array().unwrap();
        assert_eq!(remaining.len(), 3);
        assert!(
            remaining
                .iter()
                .all(|e| e["title"].as_str().unwrap().contains("backend-developer")),
            "stored remainder must still be wave A's deferrals"
        );
        assert!(
            !remaining
                .iter()
                .any(|e| e["title"].as_str().unwrap().contains("code-reviewer")),
            "wave B's remainder must NOT have replaced the stored state"
        );

        // The retained state is live, not garbage: the rollout keeps
        // draining from it (finish wave B's open task, advance → next
        // batch created from wave A's stored remainder).
        finish_all_reconciliation_tasks(&state, "ws-kvfail").await;
        on_reconciliation_task_terminal(&state, &ws.id).await;
        let after = pending_rollout_state(&state, "ws-kvfail").await;
        assert_eq!(
            after.as_ref().unwrap()["remaining"]
                .as_array()
                .unwrap()
                .len(),
            2,
            "the untouched stored remainder must still drive the next batch"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn overlapping_rolling_waves_retain_both_remainders() {
        // Regression (review dde37e8e): the pending remainder lives under
        // one per-workspace key, so persisting a second wave's remainder
        // while a rolling rollout is in flight used to overwrite the first
        // wave's deferred tasks — silent work loss, repos left
        // unreconciled. The persist must MERGE: both waves' remainders are
        // retained and drained by the normal batch advance.
        let state = test_state();
        let ws = make_workspace(&state, "ws-overlap").await;
        for i in 0..4 {
            make_repo(&state, &format!("repo-ov-{i}"), "ws-overlap").await;
        }
        let persona = make_meta_spec(&state, "backend-developer", 4).await;
        let reviewer = make_meta_spec(&state, "code-reviewer", 2).await;
        set_policy(
            &state,
            "ws-overlap",
            &gyre_domain::RolloutPolicy {
                strategy: gyre_domain::RolloutStrategy::Rolling,
                max_concurrent_agents: 1,
                priority: gyre_domain::ReconciliationPriority::Normal,
                auto_merge: false,
            },
        )
        .await;

        // Wave A: persona v4 → 1 task + 3 deferred.
        let a = run_reconciliation(&state, &ws.id, std::slice::from_ref(&persona.name)).await;
        assert_eq!(a.tasks_created, 1);

        // Wave B triggers while wave A's rollout is in flight (its first
        // batch is still open): reviewer v2 → 1 task + 3 deferred. Must
        // extend, not clobber, wave A's remainder.
        let b = run_reconciliation(&state, &ws.id, std::slice::from_ref(&reviewer.name)).await;
        assert_eq!(b.tasks_created, 1);

        let pending = pending_rollout_state(&state, "ws-overlap")
            .await
            .expect("overlapping waves must keep a pending remainder");
        let remaining = pending["remaining"].as_array().unwrap();
        assert_eq!(
            remaining.len(),
            6,
            "both waves' deferred remainders must be retained (3 + 3)"
        );
        let count_for = |spec: &str| {
            remaining
                .iter()
                .filter(|e| e["title"].as_str().unwrap().contains(spec))
                .count()
        };
        assert_eq!(count_for("backend-developer"), 3);
        assert_eq!(count_for("code-reviewer"), 3);

        // Drive the whole (now merged) rollout to completion: batch after
        // batch, one task per advance (batch size 1).
        for _ in 0..20 {
            if pending_rollout_state(&state, "ws-overlap").await.is_none() {
                break;
            }
            finish_all_reconciliation_tasks(&state, "ws-overlap").await;
            on_reconciliation_task_terminal(&state, &ws.id).await;
        }
        // Finish the final batch the loop left open.
        finish_all_reconciliation_tasks(&state, "ws-overlap").await;
        on_reconciliation_task_terminal(&state, &ws.id).await;
        assert!(
            pending_rollout_state(&state, "ws-overlap").await.is_none(),
            "the merged rollout must fully drain"
        );

        // EVERY repo must have received BOTH waves' reconciliation tasks —
        // this is the exact assertion the overwrite bug failed (persona
        // tasks were 1/4).
        let tasks = open_tasks_with_label(&state, RECONCILIATION_LABEL).await;
        let persona_tasks = tasks
            .iter()
            .filter(|t| t.title.contains("backend-developer"))
            .count();
        let reviewer_tasks = tasks
            .iter()
            .filter(|t| t.title.contains("code-reviewer"))
            .count();
        assert_eq!(
            persona_tasks, 4,
            "every repo must receive the persona wave's task"
        );
        assert_eq!(
            reviewer_tasks, 4,
            "every repo must receive the reviewer wave's task"
        );
        assert_eq!(
            count_completed_messages(&state, &ws.id).await,
            1,
            "the merged rollout completes exactly once, at the very end"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn retriggered_rolling_wave_supersedes_not_duplicates() {
        // A re-trigger of the SAME spec+version while its remainder is
        // deferred replaces the matching deferred entries instead of
        // duplicating them — the merge must be additive across waves but
        // idempotent within one.
        let state = test_state();
        let ws = make_workspace(&state, "ws-retrig").await;
        for i in 0..3 {
            make_repo(&state, &format!("repo-rt-{i}"), "ws-retrig").await;
        }
        let ms = make_meta_spec(&state, "backend-developer", 4).await;
        set_policy(
            &state,
            "ws-retrig",
            &gyre_domain::RolloutPolicy {
                strategy: gyre_domain::RolloutStrategy::Rolling,
                max_concurrent_agents: 1,
                priority: gyre_domain::ReconciliationPriority::Normal,
                auto_merge: false,
            },
        )
        .await;

        let first = run_reconciliation(&state, &ws.id, std::slice::from_ref(&ms.name)).await;
        assert_eq!(first.tasks_created, 1); // batch 1
        assert_eq!(
            pending_rollout_state(&state, "ws-retrig").await.unwrap()["remaining"]
                .as_array()
                .unwrap()
                .len(),
            2
        );

        // Re-trigger the same version: the open task's repo dedupes
        // (skipped); one deferred repo's task is created as the re-trigger's
        // first batch — which must CONSUME that repo's stored deferral (the
        // work is now in flight; a stale deferral would re-create it after
        // it goes terminal) — and the other re-defers, superseding its
        // stored entry. The remainder must shrink to exactly that one
        // deferral, never duplicating anything. (Repo iteration order from
        // the mem store is not deterministic, so identify the repos by
        // observed task state, not by id order.)
        let open_before: Vec<String> = open_tasks_with_label(&state, RECONCILIATION_LABEL)
            .await
            .iter()
            .map(|t| t.repo_id.as_str().to_string())
            .collect();
        assert_eq!(
            open_before.len(),
            1,
            "wave A must have exactly one open task"
        );

        let second = run_reconciliation(&state, &ws.id, std::slice::from_ref(&ms.name)).await;
        assert_eq!(second.tasks_created, 1); // one deferred repo, as batch 1
        assert_eq!(second.tasks_skipped, 1); // the already-open repo

        let open_after: Vec<String> = open_tasks_with_label(&state, RECONCILIATION_LABEL)
            .await
            .iter()
            .map(|t| t.repo_id.as_str().to_string())
            .collect();
        let newly_in_flight: Vec<&String> = open_after
            .iter()
            .filter(|r| !open_before.contains(r))
            .collect();
        assert_eq!(
            newly_in_flight.len(),
            1,
            "the re-trigger put exactly one repo in flight"
        );

        let remaining = pending_rollout_state(&state, "ws-retrig").await.unwrap()["remaining"]
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(
            remaining.len(),
            1,
            "a re-triggered wave must not duplicate deferred entries"
        );
        let deferred_repo = remaining[0]["repo_id"].as_str().unwrap();
        assert!(
            !open_before.contains(&deferred_repo.to_string()),
            "the already-open repo is skipped, never re-deferred"
        );
        assert_ne!(
            deferred_repo,
            newly_in_flight[0].as_str(),
            "the stored deferral for work just put in flight must be consumed"
        );

        // Drain: exactly one task per repo at the end.
        for _ in 0..10 {
            if pending_rollout_state(&state, "ws-retrig").await.is_none() {
                break;
            }
            finish_all_reconciliation_tasks(&state, "ws-retrig").await;
            on_reconciliation_task_terminal(&state, &ws.id).await;
        }
        finish_all_reconciliation_tasks(&state, "ws-retrig").await;
        on_reconciliation_task_terminal(&state, &ws.id).await;
        let tasks = open_tasks_with_label(&state, RECONCILIATION_LABEL).await;
        assert_eq!(
            tasks.len(),
            3,
            "each repo ends with exactly one reconciliation task"
        );
    }

    // -- §7: auto_merge ------------------------------------------------------

    async fn make_reconciliation_mr(
        state: &Arc<AppState>,
        ws_id: &str,
        repo_id: &str,
    ) -> (gyre_domain::MergeRequest, gyre_domain::Task) {
        let mut task = gyre_domain::Task::new(Id::new("task-mr-auto"), "Align code", now());
        task.labels = vec![RECONCILIATION_LABEL.to_string()];
        task.workspace_id = Id::new(ws_id);
        task.repo_id = Id::new(repo_id);
        state.tasks.create(&task).await.unwrap();

        let mut mr = gyre_domain::MergeRequest::new(
            Id::new("mr-auto"),
            Id::new(repo_id),
            "Reconcile backend-developer v4",
            "reconcile/abc/spec",
            "main",
            now(),
        );
        mr.workspace_id = Id::new(ws_id);
        state.merge_requests.create(&mr).await.unwrap();
        (mr, task)
    }

    async fn queued_entries_for(state: &Arc<AppState>, mr_id: &str) -> usize {
        state
            .merge_queue
            .list_queue()
            .await
            .unwrap()
            .into_iter()
            .filter(|e| e.merge_request_id.as_str() == mr_id)
            .count()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn auto_merge_enqueues_reconciliation_mr_when_enabled() {
        let state = test_state();
        make_workspace(&state, "ws-am-on").await;
        let repo = make_repo(&state, "repo-am-on", "ws-am-on").await;
        set_policy(
            &state,
            "ws-am-on",
            &gyre_domain::RolloutPolicy {
                strategy: gyre_domain::RolloutStrategy::Rolling,
                max_concurrent_agents: 5,
                priority: gyre_domain::ReconciliationPriority::Normal,
                auto_merge: true,
            },
        )
        .await;
        let (mr, task) = make_reconciliation_mr(&state, "ws-am-on", repo.id.as_str()).await;

        maybe_auto_enqueue_reconciliation_mr(&state, &mr, &task).await;

        assert_eq!(
            queued_entries_for(&state, mr.id.as_str()).await,
            1,
            "auto_merge must enqueue the reconciliation MR (it then merges when gates pass)"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn auto_merge_disabled_leaves_mr_unenqueued() {
        let state = test_state();
        make_workspace(&state, "ws-am-off").await;
        let repo = make_repo(&state, "repo-am-off", "ws-am-off").await;
        // Default policy: auto_merge = false.
        let (mr, task) = make_reconciliation_mr(&state, "ws-am-off", repo.id.as_str()).await;

        maybe_auto_enqueue_reconciliation_mr(&state, &mr, &task).await;

        assert_eq!(
            queued_entries_for(&state, mr.id.as_str()).await,
            0,
            "default policy must leave the MR for a human/agent-triggered enqueue"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn auto_merge_ignores_non_reconciliation_mrs() {
        let state = test_state();
        make_workspace(&state, "ws-am-other").await;
        let repo = make_repo(&state, "repo-am-other", "ws-am-other").await;
        set_policy(
            &state,
            "ws-am-other",
            &gyre_domain::RolloutPolicy {
                strategy: gyre_domain::RolloutStrategy::Rolling,
                max_concurrent_agents: 5,
                priority: gyre_domain::ReconciliationPriority::Normal,
                auto_merge: true,
            },
        )
        .await;
        let (mr, mut task) = make_reconciliation_mr(&state, "ws-am-other", repo.id.as_str()).await;
        task.labels = vec!["feature".to_string()];

        maybe_auto_enqueue_reconciliation_mr(&state, &mr, &task).await;

        assert_eq!(
            queued_entries_for(&state, mr.id.as_str()).await,
            0,
            "auto_merge applies to reconciliation MRs only"
        );
    }

    // -- §6: set diffing (trigger condition) ---------------------------------

    #[tokio::test(flavor = "multi_thread")]
    async fn set_diff_detects_version_changes() {
        let old = make_set("ws", vec![pin("backend-developer", "a1")]);
        let new = make_set("ws", vec![pin("backend-developer", "a1")]);
        assert!(
            diff_meta_spec_set(&old, &new).is_empty(),
            "same set → no changes"
        );

        let new = make_set("ws", vec![pin("backend-developer", "b2")]);
        assert_eq!(
            diff_meta_spec_set(&old, &new),
            vec!["backend-developer".to_string()]
        );

        // Newly bound entry counts as changed; the identical
        // backend-developer pin (a1 → a1) does not.
        let new = make_set(
            "ws",
            vec![pin("backend-developer", "a1"), pin("security", "c3")],
        );
        assert_eq!(diff_meta_spec_set(&old, &new), vec!["security".to_string()]);

        // Removal is not a trigger.
        let new = make_set("ws", vec![]);
        assert!(diff_meta_spec_set(&old, &new).is_empty());
    }

    // -- §10: conformance sweep detects drift --------------------------------

    fn sample_attestation(
        repo_id: &str,
        ws_id: &str,
        meta_sha: &str,
        created_at: u64,
    ) -> gyre_common::Attestation {
        use gyre_common::attestation::*;
        let content = InputContent {
            spec_path: "specs/system/x.md".to_string(),
            spec_sha: "a".repeat(40),
            workspace_id: ws_id.to_string(),
            repo_id: repo_id.to_string(),
            persona_constraints: vec![],
            meta_spec_set_sha: meta_sha.to_string(),
            scope: ScopeConstraint::default(),
        };
        let si = SignedInput {
            content,
            output_constraints: vec![],
            valid_until: created_at + 3600,
            expected_generation: None,
            signature: vec![1, 2, 3],
            key_binding: gyre_common::KeyBinding {
                public_key: vec![1],
                user_identity: "user:admin".to_string(),
                issuer: "https://gyre.local".to_string(),
                trust_anchor_id: "ta".to_string(),
                issued_at: created_at,
                expires_at: created_at + 3600,
                user_signature: vec![1],
                platform_countersign: vec![1],
            },
        };
        gyre_common::Attestation {
            id: sha256_hex(&format!("{repo_id}-{meta_sha}-{created_at}")),
            input: AttestationInput::Signed(si),
            output: AttestationOutput {
                content_hash: vec![1],
                commit_sha: format!("{:040x}", created_at),
                agent_signature: None,
                gate_results: vec![],
            },
            metadata: gyre_common::attestation::AttestationMetadata {
                created_at,
                workspace_id: ws_id.to_string(),
                repo_id: repo_id.to_string(),
                task_id: "task-1".to_string(),
                agent_id: "agent-1".to_string(),
                chain_depth: 0,
            },
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn sweep_detects_drift_and_creates_deduped_task() {
        let state = test_state();
        let ws = make_workspace(&state, "ws-sweep-1").await;
        let repo = make_repo(&state, "repo-s1", "ws-sweep-1").await;

        // Bind a meta-spec set — this is the desired state.
        let set = make_set("ws-sweep-1", vec![pin("backend-developer", "a1")]);
        put_set(&state, "ws-sweep-1", &set).await;
        let active_sha =
            crate::compute_meta_spec_set_sha(state.meta_spec_sets.as_ref(), &ws.id).await;
        assert!(!active_sha.is_empty());

        // Provenance under an OLD set SHA → drift.
        let old_att = sample_attestation(repo.id.as_str(), "ws-sweep-1", "deadbeef", now());
        state.chain_attestations.save(&old_att).await.unwrap();

        // Member to notify.
        let membership = WorkspaceMembership {
            id: Id::new(uuid::Uuid::new_v4().to_string()),
            user_id: Id::new("user-sweep"),
            workspace_id: ws.id.clone(),
            role: WorkspaceRole::Admin,
            invited_by: Id::new("admin"),
            accepted: true,
            accepted_at: Some(now()),
            created_at: now(),
        };
        state
            .workspace_memberships
            .create(&membership)
            .await
            .unwrap();

        let summary = run_conformance_sweep(&state).await.unwrap();
        assert_eq!(summary.drift_detected, 1);
        assert_eq!(summary.tasks_created, 1);

        let tasks = open_tasks_with_label(&state, DRIFT_REVIEW_LABEL).await;
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].workspace_id.as_str(), "ws-sweep-1");

        // MetaSpecDrift notification created for the admin member.
        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user-sweep"),
                Some(&ws.id),
                Some(6),
                Some(6),
                None,
                10,
                0,
            )
            .await
            .unwrap();
        assert!(
            notifs
                .iter()
                .any(|n| n.notification_type == NotificationType::MetaSpecDrift),
            "expected MetaSpecDrift notification"
        );

        // meta_spec_drift_detected event persisted (Event tier is stored).
        let msgs = state
            .messages
            .list_by_workspace(
                &ws.id,
                Some("meta_spec_drift_detected"),
                None,
                None,
                None,
                Some(50),
            )
            .await
            .unwrap();
        assert!(
            msgs.iter()
                .any(|m| m.kind == MessageKind::Custom("meta_spec_drift_detected".to_string())),
            "expected meta_spec_drift_detected event message"
        );

        // Second sweep: same drift → task deduplicated, but drift still
        // counted and notification refreshed.
        let second = run_conformance_sweep(&state).await.unwrap();
        assert_eq!(second.drift_detected, 1);
        assert_eq!(second.tasks_created, 0);
        assert_eq!(second.tasks_skipped, 1);
        assert_eq!(
            open_tasks_with_label(&state, DRIFT_REVIEW_LABEL)
                .await
                .len(),
            1
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn sweep_clean_when_provenance_matches_current_set() {
        let state = test_state();
        let ws = make_workspace(&state, "ws-sweep-2").await;
        let repo = make_repo(&state, "repo-s2", "ws-sweep-2").await;

        let set = make_set("ws-sweep-2", vec![pin("backend-developer", "a1")]);
        put_set(&state, "ws-sweep-2", &set).await;
        let active_sha =
            crate::compute_meta_spec_set_sha(state.meta_spec_sets.as_ref(), &ws.id).await;

        // Provenance under the CURRENT set SHA → no drift.
        let att = sample_attestation(repo.id.as_str(), "ws-sweep-2", &active_sha, now());
        state.chain_attestations.save(&att).await.unwrap();

        let summary = run_conformance_sweep(&state).await.unwrap();
        assert_eq!(summary.drift_detected, 0);
        assert_eq!(summary.tasks_created, 0);
        assert_eq!(
            open_tasks_with_label(&state, DRIFT_REVIEW_LABEL)
                .await
                .len(),
            0
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn sweep_skips_workspaces_without_set() {
        let state = test_state();
        let _ws = make_workspace(&state, "ws-sweep-3").await;
        let repo = make_repo(&state, "repo-s3", "ws-sweep-3").await;

        // No set bound; provenance with a non-empty SHA exists but there is
        // no desired state to compare against.
        let att = sample_attestation(repo.id.as_str(), "ws-sweep-3", "deadbeef", now());
        state.chain_attestations.save(&att).await.unwrap();

        let summary = run_conformance_sweep(&state).await.unwrap();
        assert_eq!(summary.drift_detected, 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn sweep_ignores_empty_provenance_sha() {
        let state = test_state();
        let _ws = make_workspace(&state, "ws-sweep-4").await;
        let repo = make_repo(&state, "repo-s4", "ws-sweep-4").await;

        let set = make_set("ws-sweep-4", vec![pin("backend-developer", "a1")]);
        put_set(&state, "ws-sweep-4", &set).await;

        // Provenance recorded before any set was bound (empty SHA) — cannot
        // assert drift against an unknown baseline.
        let att = sample_attestation(repo.id.as_str(), "ws-sweep-4", "", now());
        state.chain_attestations.save(&att).await.unwrap();

        let summary = run_conformance_sweep(&state).await.unwrap();
        assert_eq!(summary.drift_detected, 0);
    }

    // -- End-to-end: set update triggers reconciliation ----------------------

    #[tokio::test(flavor = "multi_thread")]
    async fn set_update_via_api_triggers_reconciliation() {
        use axum::{body::Body, Router};
        use http::{Request, StatusCode};
        use tower::ServiceExt;

        let state = test_state();
        // Create the workspace via the API (member role wiring for
        // notifications comes from the membership below).
        let app: Router = crate::api::api_router().with_state(state.clone());

        let ws_body = serde_json::json!({"name": "ws-e2e", "slug": "ws-e2e"});
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(ws_body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let ws_json: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        let ws_id = ws_json["id"].as_str().unwrap().to_string();

        make_repo(&state, "repo-e2e", &ws_id).await;
        make_meta_spec(&state, "backend-developer", 4).await;

        // PUT the set with a first binding.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/workspaces/{ws_id}/meta-spec-set"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "personas": { "backend": { "path": "backend-developer", "sha": "a1" } },
                            "principles": [], "standards": [], "process": []
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // First binding → reconciliation task created.
        let tasks = open_tasks_with_label(&state, RECONCILIATION_LABEL).await;
        assert_eq!(tasks.len(), 1, "set binding should trigger reconciliation");
        assert_eq!(tasks[0].workspace_id.as_str(), ws_id);

        // Re-PUT the identical set → no new task (no version change).
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/workspaces/{ws_id}/meta-spec-set"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "personas": { "backend": { "path": "backend-developer", "sha": "a1" } },
                            "principles": [], "standards": [], "process": []
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            open_tasks_with_label(&state, RECONCILIATION_LABEL)
                .await
                .len(),
            1,
            "identical re-PUT must not create a second task"
        );
    }

    // -- §10/§9: identical set re-PUT must not flip the set SHA -----------

    /// `compute_meta_spec_set_sha` hashes the stored JSON bytes. The sweep
    /// (§10) compares that hash against provenance records, and the §9 merge
    /// gate compares `agent.meta_spec_set_sha == input.meta_spec_set_sha`.
    /// If serializing the same set twice produced different bytes (e.g. a
    /// HashMap's per-instance key order), an identical re-PUT would rewrite
    /// the stored JSON under a new SHA and false-drift every provenance
    /// record. This test pins the canonical-serialization invariant.
    #[tokio::test(flavor = "multi_thread")]
    async fn identical_reput_keeps_set_sha_stable() {
        use axum::{body::Body, Router};
        use http::{Request, StatusCode};
        use tower::ServiceExt;

        let state = test_state();
        let app: Router = crate::api::api_router().with_state(state.clone());

        // Create a workspace via the API.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"name": "ws-sha", "slug": "ws-sha"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let ws_json: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        let ws_id = ws_json["id"].as_str().unwrap().to_string();
        let ws_id = Id::new(&ws_id);

        // Multiple personas: HashMap ordering is per-instance random, so a
        // multi-entry set is what exposes nondeterministic serialization.
        let put_body = serde_json::json!({
            "personas": {
                "backend":  {"path": "backend-developer", "sha": "a1"},
                "frontend": {"path": "frontend-developer", "sha": "b2"},
                "sre":      {"path": "sre-operator", "sha": "c3"}
            },
            "principles": [], "standards": [], "process": []
        });
        let put = |app: Router, ws: Id| {
            let body = put_body.to_string();
            async move {
                app.oneshot(
                    Request::builder()
                        .method("PUT")
                        .uri(format!("/api/v1/workspaces/{}/meta-spec-set", ws.as_str()))
                        .header("authorization", "Bearer test-token")
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap()
            }
        };

        let r1 = put(app.clone(), ws_id.clone()).await;
        assert_eq!(r1.status(), StatusCode::OK);
        let sha1 = crate::compute_meta_spec_set_sha(state.meta_spec_sets.as_ref(), &ws_id).await;
        assert!(!sha1.is_empty());

        // Identical re-PUT: same entries (deliberately supplied in the same
        // JSON object order; key order in the REQUEST must not matter
        // either, since deserialization into BTreeMap canonicalizes).
        let r2 = put(app.clone(), ws_id.clone()).await;
        assert_eq!(r2.status(), StatusCode::OK);
        let sha2 = crate::compute_meta_spec_set_sha(state.meta_spec_sets.as_ref(), &ws_id).await;
        assert_eq!(sha1, sha2, "identical re-PUT must not change the set SHA");

        // Re-PUT with the SAME entries in a DIFFERENT request key order:
        // still identical after deserialization → same SHA.
        let reordered = serde_json::json!({
            "personas": {
                "sre":      {"path": "sre-operator", "sha": "c3"},
                "backend":  {"path": "backend-developer", "sha": "a1"},
                "frontend": {"path": "frontend-developer", "sha": "b2"}
            },
            "principles": [], "standards": [], "process": []
        });
        let r3 = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!(
                        "/api/v1/workspaces/{}/meta-spec-set",
                        ws_id.as_str()
                    ))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(reordered.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r3.status(), StatusCode::OK);
        let sha3 = crate::compute_meta_spec_set_sha(state.meta_spec_sets.as_ref(), &ws_id).await;
        assert_eq!(
            sha1, sha3,
            "request key order must not leak into the stored set SHA"
        );

        // A genuinely different set (one pinned SHA changed) must change
        // the SHA — the invariant must not be trivially "SHA never moves".
        let changed = serde_json::json!({
            "personas": {
                "backend":  {"path": "backend-developer", "sha": "a9"},
                "frontend": {"path": "frontend-developer", "sha": "b2"},
                "sre":      {"path": "sre-operator", "sha": "c3"}
            },
            "principles": [], "standards": [], "process": []
        });
        let r4 = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!(
                        "/api/v1/workspaces/{}/meta-spec-set",
                        ws_id.as_str()
                    ))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(changed.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r4.status(), StatusCode::OK);
        let sha4 = crate::compute_meta_spec_set_sha(state.meta_spec_sets.as_ref(), &ws_id).await;
        assert_ne!(sha1, sha4, "a changed pin must change the set SHA");
    }

    // -- §6 step 2: task per repo, scoped per workspace ----------------------

    #[tokio::test(flavor = "multi_thread")]
    async fn reconciliation_creates_task_per_repo() {
        let state = test_state();
        let ws = make_workspace(&state, "ws-recon-multi").await;
        let _r1 = make_repo(&state, "repo-m1", "ws-recon-multi").await;
        let _r2 = make_repo(&state, "repo-m2", "ws-recon-multi").await;
        let _r3 = make_repo(&state, "repo-m3", "ws-recon-multi").await;
        let ms = make_meta_spec(&state, "backend-developer", 4).await;

        let s = run_reconciliation(&state, &ws.id, std::slice::from_ref(&ms.name)).await;
        let tasks = open_tasks_with_label(&state, RECONCILIATION_LABEL).await;
        assert_eq!(s.tasks_created, 3, "§6 step 2: task per affected repo");
        assert_eq!(tasks.len(), 3);
        let mut repo_ids: Vec<String> = tasks
            .iter()
            .map(|t| t.repo_id.as_str().to_string())
            .collect();
        repo_ids.sort();
        assert_eq!(
            repo_ids,
            vec![
                "repo-m1".to_string(),
                "repo-m2".to_string(),
                "repo-m3".to_string()
            ],
            "each repo gets its own task"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn reconciliation_two_workspaces_same_spec_not_suppressed() {
        let state = test_state();
        let ws1 = make_workspace(&state, "ws-recon-w1").await;
        let _repo1 = make_repo(&state, "repo-pa", "ws-recon-w1").await;
        let ws2 = make_workspace(&state, "ws-recon-w2").await;
        let _repo2 = make_repo(&state, "repo-pb", "ws-recon-w2").await;
        let ms = make_meta_spec(&state, "backend-developer", 4).await;

        let s1 = run_reconciliation(&state, &ws1.id, std::slice::from_ref(&ms.name)).await;
        assert_eq!(s1.tasks_created, 1);
        let s2 = run_reconciliation(&state, &ws2.id, std::slice::from_ref(&ms.name)).await;
        assert_eq!(
            s2.tasks_created, 1,
            "second workspace must get its own reconciliation task"
        );
        assert_eq!(s2.tasks_skipped, 0);

        let tasks = open_tasks_with_label(&state, RECONCILIATION_LABEL).await;
        assert_eq!(tasks.len(), 2);
        assert_eq!(
            tasks
                .iter()
                .filter(|t| t.workspace_id.as_str() == "ws-recon-w1")
                .count(),
            1
        );
        assert_eq!(
            tasks
                .iter()
                .filter(|t| t.workspace_id.as_str() == "ws-recon-w2")
                .count(),
            1
        );
    }

    // -- §11: ReconciliationCompleted fires on wave completion, not creation -

    #[tokio::test(flavor = "multi_thread")]
    async fn reconciliation_completed_fires_when_all_tasks_terminal() {
        let state = test_state();
        // Drain the bus with the real dispatcher + notification bridge: the
        // p6 MetaSpecDrift notification is created asynchronously by
        // NotificationBridge consuming the ReconciliationCompleted event.
        crate::message_dispatcher::spawn_message_consumer(state.clone()).await;
        let ws = make_workspace(&state, "ws-wave").await;
        make_repo(&state, "repo-w1", "ws-wave").await;
        make_repo(&state, "repo-w2", "ws-wave").await;
        let ms = make_meta_spec(&state, "backend-developer", 4).await;

        // Member so the completion notification path is exercised end-to-end.
        let membership = WorkspaceMembership {
            id: Id::new(uuid::Uuid::new_v4().to_string()),
            user_id: Id::new("user-wave"),
            workspace_id: ws.id.clone(),
            role: WorkspaceRole::Admin,
            invited_by: Id::new("admin"),
            accepted: true,
            accepted_at: Some(now()),
            created_at: now(),
        };
        state
            .workspace_memberships
            .create(&membership)
            .await
            .unwrap();

        // Two reconciliation tasks created (one per repo).
        run_reconciliation(&state, &ws.id, std::slice::from_ref(&ms.name)).await;

        let count_completed = |ws: gyre_domain::Workspace| {
            let state = state.clone();
            async move {
                state
                    .messages
                    .list_by_workspace(
                        &ws.id,
                        Some("reconciliation_completed"),
                        None,
                        None,
                        None,
                        Some(50),
                    )
                    .await
                    .unwrap()
                    .len()
            }
        };

        // NOT fired at creation time (§11: completion = all tasks done).
        assert_eq!(
            count_completed(ws.clone()).await,
            0,
            "no completion event while tasks are open"
        );

        // Terminal-transition the first task (Backlog → Cancelled) — one task
        // still open, still no completion.
        let tasks = open_tasks_with_label(&state, RECONCILIATION_LABEL).await;
        assert_eq!(tasks.len(), 2);
        let mut t1 = state.tasks.find_by_id(&tasks[0].id).await.unwrap().unwrap();
        t1.transition_status(TaskStatus::Cancelled).unwrap();
        state.tasks.update(&t1).await.unwrap();
        maybe_emit_reconciliation_completed(&state, &ws.id).await;
        assert_eq!(
            count_completed(ws.clone()).await,
            0,
            "one task still open — wave not complete"
        );

        // Terminal-transition the second task — wave complete, event fires.
        let mut t2 = state.tasks.find_by_id(&tasks[1].id).await.unwrap().unwrap();
        t2.transition_status(TaskStatus::InProgress).unwrap();
        t2.transition_status(TaskStatus::Review).unwrap();
        t2.transition_status(TaskStatus::Done).unwrap();
        state.tasks.update(&t2).await.unwrap();
        maybe_emit_reconciliation_completed(&state, &ws.id).await;
        assert_eq!(
            count_completed(ws.clone()).await,
            1,
            "all tasks terminal — ReconciliationCompleted must fire exactly once"
        );

        // MetaSpecDrift notification for the admin member — created by the
        // NotificationBridge consuming the ReconciliationCompleted event, so
        // poll until the async dispatcher has drained it.
        let mut notifs = Vec::new();
        for _ in 0..100 {
            notifs = state
                .notifications
                .list_for_user(
                    &Id::new("user-wave"),
                    Some(&ws.id),
                    Some(6),
                    Some(6),
                    None,
                    10,
                    0,
                )
                .await
                .unwrap();
            if notifs
                .iter()
                .any(|n| n.notification_type == NotificationType::MetaSpecDrift)
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(
            notifs
                .iter()
                .any(|n| n.notification_type == NotificationType::MetaSpecDrift),
            "expected MetaSpecDrift notification on wave completion"
        );

        // Idempotency: a re-check of the complete wave must not re-emit.
        maybe_emit_reconciliation_completed(&state, &ws.id).await;
        assert_eq!(
            count_completed(ws.clone()).await,
            1,
            "idempotent: re-checking a complete wave must not re-emit"
        );
    }

    /// A SECOND reconciliation wave must emit its own ReconciliationCompleted.
    /// Regression for the wave-identity bug: identity was
    /// min(created_at) over ALL reconciliation tasks — including terminal
    /// tasks from prior waves — so after wave 1 completed and emitted, wave
    /// 2's completion recomputed wave 1's start, matched the persisted
    /// wave_started_at, and suppressed the wave-2 event. §11 requires the
    /// event for every wave ("all reconciliation tasks for a workspace are
    /// done" — true again when the second wave's tasks all go terminal).
    #[tokio::test(flavor = "multi_thread")]
    async fn second_wave_emits_its_own_completion() {
        let state = test_state();
        let ws = make_workspace(&state, "ws-wave2").await;
        make_repo(&state, "repo-w2", "ws-wave2").await;
        let ms = make_meta_spec(&state, "backend-developer", 4).await;

        let count_completed = || {
            let state = state.clone();
            let ws_id = ws.id.clone();
            async move {
                state
                    .messages
                    .list_by_workspace(
                        &ws_id,
                        Some("reconciliation_completed"),
                        None,
                        None,
                        None,
                        Some(50),
                    )
                    .await
                    .unwrap()
                    .len()
            }
        };
        let complete_all_open = || {
            let state = state.clone();
            let ws_id = ws.id.clone();
            async move {
                let tasks = state
                    .tasks
                    .list_by_workspace(&ws_id)
                    .await
                    .unwrap()
                    .into_iter()
                    .filter(|t| {
                        t.labels.iter().any(|l| l == RECONCILIATION_LABEL)
                            && !matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled)
                    })
                    .collect::<Vec<_>>();
                for mut t in tasks {
                    t.transition_status(TaskStatus::InProgress).unwrap();
                    t.transition_status(TaskStatus::Review).unwrap();
                    t.transition_status(TaskStatus::Done).unwrap();
                    state.tasks.update(&t).await.unwrap();
                }
            }
        };

        // Wave 1: v4 → one task → complete → emit.
        run_reconciliation(&state, &ws.id, std::slice::from_ref(&ms.name)).await;
        complete_all_open().await;
        maybe_emit_reconciliation_completed(&state, &ws.id).await;
        assert_eq!(count_completed().await, 1, "wave 1 must emit exactly once");

        // Wave 2: same spec bumped to v5 (new title → new task). Created
        // within the same wall-clock second as wave 1 in the worst case —
        // the identity must still distinguish the waves.
        state
            .meta_specs
            .update(&MetaSpec {
                version: 5,
                ..ms.clone()
            })
            .await
            .unwrap();
        run_reconciliation(&state, &ws.id, std::slice::from_ref(&ms.name)).await;
        let open = open_tasks_with_label(&state, RECONCILIATION_LABEL)
            .await
            .into_iter()
            .filter(|t| !matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled))
            .count();
        assert_eq!(open, 1, "wave 2 must create a new open task");

        maybe_emit_reconciliation_completed(&state, &ws.id).await;
        assert_eq!(
            count_completed().await,
            1,
            "wave 2 open — no completion while in flight"
        );

        complete_all_open().await;
        maybe_emit_reconciliation_completed(&state, &ws.id).await;
        assert_eq!(
            count_completed().await,
            2,
            "wave 2 completion must emit its own ReconciliationCompleted"
        );

        // Idempotency preserved: re-checking the complete state must not
        // emit a third event for the same wave.
        maybe_emit_reconciliation_completed(&state, &ws.id).await;
        assert_eq!(
            count_completed().await,
            2,
            "re-check must not re-emit wave 2"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn no_completion_event_for_workspace_without_reconciliation_tasks() {
        let state = test_state();
        let ws = make_workspace(&state, "ws-nowave").await;

        // An unrelated terminal task must not fire the completion event.
        let mut task = gyre_domain::Task::new(Id::new("unrelated-1"), "Unrelated", now());
        task.workspace_id = ws.id.clone();
        task.transition_status(TaskStatus::InProgress).unwrap();
        task.transition_status(TaskStatus::Review).unwrap();
        task.transition_status(TaskStatus::Done).unwrap();
        state.tasks.create(&task).await.unwrap();

        maybe_emit_reconciliation_completed(&state, &ws.id).await;
        let msgs = state
            .messages
            .list_by_workspace(
                &ws.id,
                Some("reconciliation_completed"),
                None,
                None,
                None,
                Some(50),
            )
            .await
            .unwrap();
        assert!(
            msgs.is_empty(),
            "no reconciliation tasks — no completion event"
        );
    }

    // -- §11: MetaSpecDriftResolved on drift-review task completion ---------

    #[tokio::test(flavor = "multi_thread")]
    async fn drift_resolved_fires_when_all_drift_review_tasks_terminal() {
        let state = test_state();
        let ws = make_workspace(&state, "ws-resolve").await;

        // One terminal drift-review task: drift resolved.
        let mut task = gyre_domain::Task::new(
            Id::new("drift-task-1"),
            "Review meta-spec drift in workspace ws-resolve (active set abc)",
            now(),
        );
        task.labels = vec![DRIFT_REVIEW_LABEL.to_string()];
        task.workspace_id = ws.id.clone();
        task.transition_status(TaskStatus::InProgress).unwrap();
        task.transition_status(TaskStatus::Review).unwrap();
        task.transition_status(TaskStatus::Done).unwrap();
        state.tasks.create(&task).await.unwrap();

        // Open drift-review task in another workspace must not resolve this one.
        let ws2 = make_workspace(&state, "ws-resolve-2").await;
        let mut open_task = gyre_domain::Task::new(
            Id::new("drift-task-2"),
            "Review meta-spec drift in workspace ws-resolve-2 (active set abc)",
            now(),
        );
        open_task.labels = vec![DRIFT_REVIEW_LABEL.to_string()];
        open_task.workspace_id = ws2.id.clone();
        state.tasks.create(&open_task).await.unwrap();

        maybe_emit_drift_resolved(&state, &ws.id).await;
        let msgs = state
            .messages
            .list_by_workspace(
                &ws.id,
                Some("meta_spec_drift_resolved"),
                None,
                None,
                None,
                Some(50),
            )
            .await
            .unwrap();
        assert_eq!(
            msgs.len(),
            1,
            "all drift-review tasks terminal → drift resolved event"
        );

        // ws2 still has an open drift-review task — no event.
        let msgs2 = state
            .messages
            .list_by_workspace(
                &ws2.id,
                Some("meta_spec_drift_resolved"),
                None,
                None,
                None,
                Some(50),
            )
            .await
            .unwrap();
        assert!(
            msgs2.is_empty(),
            "open drift-review task — drift not resolved"
        );
    }

    // -- §11: MetaSpecSetUpdated / MetaSpecChanged events --------------------

    #[tokio::test(flavor = "multi_thread")]
    async fn set_update_emits_meta_spec_set_updated_event() {
        use axum::{body::Body, Router};
        use http::{Request, StatusCode};
        use tower::ServiceExt;

        let state = test_state();
        let app: Router = crate::api::api_router().with_state(state.clone());

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"name": "ws-setevent", "slug": "ws-setevent"})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let ws_json: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        let ws_id = ws_json["id"].as_str().unwrap().to_string();

        // Bind a set — changed_paths is non-empty (new binding) → event fires.
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/workspaces/{ws_id}/meta-spec-set"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "personas": { "backend": { "path": "backend-developer", "sha": "a1" } },
                            "principles": [], "standards": [], "process": []
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let msgs = state
            .messages
            .list_by_workspace(
                &Id::new(&ws_id),
                Some("meta_spec_set_updated"),
                None,
                None,
                None,
                Some(50),
            )
            .await
            .unwrap();
        assert_eq!(
            msgs.len(),
            1,
            "set binding change must emit meta_spec_set_updated"
        );
        let payload = msgs[0].payload.as_ref().unwrap();
        assert_eq!(
            payload["changed"].as_array().map(|a| a.len()).unwrap_or(0),
            1,
            "event payload must name the changed spec"
        );

        // Identical re-PUT: no version change → no additional event.
        let app2: Router = crate::api::api_router().with_state(state.clone());
        let resp = app2
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/workspaces/{ws_id}/meta-spec-set"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "personas": { "backend": { "path": "backend-developer", "sha": "a1" } },
                            "principles": [], "standards": [], "process": []
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let msgs = state
            .messages
            .list_by_workspace(
                &Id::new(&ws_id),
                Some("meta_spec_set_updated"),
                None,
                None,
                None,
                Some(50),
            )
            .await
            .unwrap();
        assert_eq!(
            msgs.len(),
            1,
            "identical re-PUT must not emit another event"
        );
    }
}
