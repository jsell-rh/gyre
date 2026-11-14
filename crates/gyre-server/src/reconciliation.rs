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

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use gyre_common::{Id, Notification, NotificationType};
use gyre_domain::{
    MergeQueueEntry, RolloutPolicy, RolloutStrategy, TaskPriority, TaskStatus, TaskType,
};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::api::meta_specs::{MetaSpecSet, TenantMetaSpecSet};
use crate::AppState;

/// Current wall-clock in milliseconds. Used for wave ids: successive waves
/// can start within the same second (tests, fast successive rollouts), and
/// the wave id must stay unique and orderable per wave.
fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Label attached to every reconciliation task (§6 flow step 4).
pub const RECONCILIATION_LABEL: &str = "meta-spec-reconciliation";

/// Prefix of the structured provenance label pinned to each reconciliation
/// task at creation: the changed meta-spec (name and version) the task
/// aligns code to. Read back by `maybe_emit_reconciliation_completed` to
/// enrich the `reconciliation_completed` payload with what the wave
/// changed (system-explorer.md §6 key-moment narrative needs it).
const META_SPEC_LABEL_PREFIX: &str = "meta-spec:";

/// Render a meta-spec provenance label (`meta-spec:{name}@v{version}`).
fn meta_spec_label(name: &str, version: u32) -> String {
    format!("{META_SPEC_LABEL_PREFIX}{name}@v{version}")
}

/// Parse a meta-spec provenance label back into `(name, version)`. Only
/// labels written by [`meta_spec_label`] parse; anything else (including
/// `RECONCILIATION_LABEL`, which shares no prefix) yields `None`.
fn parse_meta_spec_label(label: &str) -> Option<(&str, u32)> {
    let rest = label.strip_prefix(META_SPEC_LABEL_PREFIX)?;
    let (name, version) = rest.rsplit_once("@v")?;
    let version = version.parse().ok()?;
    if name.is_empty() {
        return None;
    }
    Some((name, version))
}

/// Prefix of the structured wave-identity label pinned to each
/// reconciliation task at creation: the trigger time of the wave the task
/// belongs to. A rolling rollout's follow-up batches are created later
/// (with a fresh `now`), so creation time alone cannot group a wave's
/// tasks; the wave id lets `maybe_emit_reconciliation_completed` scope its
/// payload to the CURRENT wave instead of the union of every wave that
/// ever ran in the workspace.
const RECON_WAVE_LABEL_PREFIX: &str = "recon-wave:";

/// Render a wave-identity label (`recon-wave:{wave_id}`).
fn recon_wave_label(wave_id: u64) -> String {
    format!("{RECON_WAVE_LABEL_PREFIX}{wave_id}")
}

/// Parse a wave-identity label back into its wave id. Only labels written
/// by [`recon_wave_label`] parse.
fn parse_recon_wave_label(label: &str) -> Option<u64> {
    label.strip_prefix(RECON_WAVE_LABEL_PREFIX)?.parse().ok()
}

/// The wave a task belongs to, from its wave-identity label. `None` for
/// tasks created before the label existed (their specs cannot be
/// attributed to a wave and are left out of wave payloads).
fn task_wave_id(t: &gyre_domain::Task) -> Option<u64> {
    t.labels
        .iter()
        .filter_map(|l| parse_recon_wave_label(l))
        .max()
}

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
    // What the wave changed: the meta-spec provenance labels materialized
    // on the wave's tasks at creation (name + version), scoped to the
    // CURRENT wave by its identity label. Prior waves' terminal tasks
    // carry older wave ids and must not bleed their specs into this
    // wave's payload; tasks predating the labels (either kind) attribute
    // to no wave and are left out, leaving consumers the task_count
    // fallback. Sorted and deduplicated for a stable payload.
    let changed_specs: Vec<serde_json::Value> = {
        let current_wave = wave.iter().filter_map(|t| task_wave_id(t)).max();
        let mut entries: std::collections::BTreeSet<(String, u32)> =
            std::collections::BTreeSet::new();
        for t in &wave {
            if task_wave_id(t) != current_wave {
                continue;
            }
            for label in &t.labels {
                if let Some((name, version)) = parse_meta_spec_label(label) {
                    entries.insert((name.to_string(), version));
                }
            }
        }
        entries
            .into_iter()
            .map(|(name, version)| serde_json::json!({ "name": name, "version": version }))
            .collect()
    };
    crate::emit_reconciliation_completed(
        state,
        workspace_id.clone(),
        Some(serde_json::json!({
            "workspace_id": workspace_id.to_string(),
            "wave_started_at": wave_started_at,
            "task_count": task_count,
            "duration_secs": duration_secs,
            "specs": changed_specs,
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
            // Unscoped registry change: fan out to every workspace whose
            // EFFECTIVE set (own bindings plus inherited tenant defaults, §8)
            // binds this meta-spec (same audience resolution as the §6
            // trigger).
            let workspaces = state.workspaces.list().await.unwrap_or_default();
            let mut emitted = false;
            for ws in &workspaces {
                let binds = resolve_effective_meta_spec_set(state, ws)
                    .await
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
    // Wave identity for this run: millisecond trigger time, shared by every
    // task this wave creates (including deferred rolling batches, which
    // persist it) and read back by the completion event to scope its
    // payload to this wave.
    let wave_id = now_millis();
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
                meta_version: ms.version,
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
                    wave_id,
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
            let remaining: Vec<PendingRolloutTask> = iter
                .map(|c| PendingRolloutTask::from_candidate(c, wave_id))
                .collect();

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
                    wave_id,
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
    /// The changed meta-spec's version at trigger time. Materialized onto
    /// the created task (provenance label) so the wave's completion event
    /// can name what changed without depending on the registry still
    /// holding that version when the wave finishes.
    meta_version: u32,
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
    wave_id: u64,
    now: u64,
) -> bool {
    let task_id = Id::new(uuid::Uuid::new_v4().to_string());
    let mut task = gyre_domain::Task::new(task_id.clone(), &cand.title, now);
    // §7: task priority is the rollout policy's ReconciliationPriority
    // mapped onto the Task priority the repo orchestrator orders by.
    task.priority = priority;
    task.labels = vec![
        RECONCILIATION_LABEL.to_string(),
        // Wave identity: the trigger time of the wave this task belongs to.
        // Follow-up rolling batches are created later (fresh `now`), so the
        // wave id is threaded through the persisted pending state rather
        // than taken from task creation time.
        recon_wave_label(wave_id),
        // Provenance for the wave's completion event: the changed meta-spec
        // (name + version) this task aligns code to, materialized at
        // creation like the title so `maybe_emit_reconciliation_completed`
        // can name what the wave changed without a registry lookup that
        // could since have advanced to a newer version.
        meta_spec_label(&cand.spec_path, cand.meta_version),
        "auto-created".to_string(),
    ];
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
    /// Version of the changed meta-spec when the wave started.
    /// `serde(default)` keeps batches persisted before these fields existed
    /// deserializable; such tasks carry no provenance labels.
    #[serde(default)]
    meta_version: u32,
    /// Trigger time of the wave this deferred task belongs to, so batches
    /// created later still carry the wave's identity label.
    #[serde(default)]
    wave_id: u64,
}

impl PendingRolloutTask {
    fn from_candidate(c: ReconciliationCandidate, wave_id: u64) -> Self {
        Self {
            repo_id: c.repo_id.to_string(),
            title: c.title,
            spec_path: c.spec_path,
            description: c.description,
            meta_version: c.meta_version,
            wave_id,
        }
    }

    fn into_candidate(self) -> ReconciliationCandidate {
        ReconciliationCandidate {
            repo_id: Id::new(self.repo_id),
            title: self.title,
            spec_path: self.spec_path,
            description: self.description,
            meta_version: self.meta_version,
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
            // The deferred task keeps its original wave's identity label —
            // `wave_id` was persisted at wave start, before this batch was
            // deferred, so late-created batch tasks still group with the
            // wave that queued them.
            let wave_id = pending_task.wave_id;
            let cand = pending_task.into_candidate();
            if create_reconciliation_task(
                state,
                workspace_id,
                &cand,
                task_priority.clone(),
                delegation,
                wave_id,
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
// §8 — Tenant-scope meta-spec changes (inheritance + propagation)
// ---------------------------------------------------------------------------

/// All meta-spec paths pinned anywhere in a workspace set (any section).
fn workspace_set_paths(set: &MetaSpecSet) -> BTreeSet<String> {
    let mut paths: BTreeSet<String> = set.personas.values().map(|e| e.path.clone()).collect();
    paths.extend(set.principles.iter().map(|e| e.path.clone()));
    paths.extend(set.standards.iter().map(|e| e.path.clone()));
    paths.extend(set.process.iter().map(|e| e.path.clone()));
    paths
}

/// All meta-spec paths pinned anywhere in a tenant set (any section).
fn tenant_set_paths(set: &TenantMetaSpecSet) -> BTreeSet<String> {
    let mut paths: BTreeSet<String> = set.personas.values().map(|e| e.path.clone()).collect();
    paths.extend(set.principles.iter().map(|e| e.path.clone()));
    paths.extend(set.standards.iter().map(|e| e.path.clone()));
    paths.extend(set.process.iter().map(|e| e.path.clone()));
    paths
}

/// Resolve a workspace's EFFECTIVE meta-spec set (§8): tenant defaults merged
/// with the workspace's own overrides — the workspace pin wins on conflict.
///
/// Merge rule (deterministic, canonical-serializable):
/// - `personas`: tenant entries whose path is pinned anywhere in the
///   workspace set, or whose role key is rebound by the workspace set, are
///   dropped; the workspace's persona map is then overlaid on the survivors.
/// - `principles`/`standards`/`process`: the workspace's entries come first,
///   followed by the tenant entries whose path the workspace does not pin.
///
/// Consistency guarantee: if `!workspace_overrides_path(tenant, ws, P)`, the
/// merged set binds `P` exactly as the tenant set does (no workspace persona
/// key collision removed a tenant pin of `P`, and the workspace pins `P`
/// nowhere) — so "not overridden" always means "inherits the tenant pin".
pub fn merge_meta_spec_sets(tenant: &TenantMetaSpecSet, workspace: &MetaSpecSet) -> MetaSpecSet {
    let ws_paths = workspace_set_paths(workspace);

    let mut personas = BTreeMap::new();
    for (key, entry) in &tenant.personas {
        if ws_paths.contains(&entry.path) || workspace.personas.contains_key(key) {
            continue;
        }
        personas.insert(key.clone(), entry.clone());
    }
    for (key, entry) in &workspace.personas {
        personas.insert(key.clone(), entry.clone());
    }

    let merge_vec =
        |ws_entries: &[crate::api::meta_specs::MetaSpecPinnedEntry],
         t_entries: &[crate::api::meta_specs::MetaSpecPinnedEntry]| {
            let mut out = ws_entries.to_vec();
            for e in t_entries {
                if !ws_paths.contains(&e.path) {
                    out.push(e.clone());
                }
            }
            out
        };

    MetaSpecSet {
        workspace_id: workspace.workspace_id.clone(),
        personas,
        principles: merge_vec(&workspace.principles, &tenant.principles),
        standards: merge_vec(&workspace.standards, &tenant.standards),
        process: merge_vec(&workspace.process, &tenant.process),
    }
}

/// Whether a workspace overrides a tenant-level default for `path` (§8):
/// the workspace's own meta-spec set governs that meta-spec, so tenant
/// propagation skips it (unless the tenant admin forces the change).
///
/// A workspace overrides `path` when its own set pins `path` in any section,
/// OR when it rebinds a tenant persona role whose pin is `path` (the role
/// rebinding drops the tenant's pin of `path` from the effective set — see
/// `merge_meta_spec_sets`).
pub fn workspace_overrides_path(
    tenant: &TenantMetaSpecSet,
    workspace: &MetaSpecSet,
    path: &str,
) -> bool {
    if workspace_set_paths(workspace).contains(path) {
        return true;
    }
    tenant
        .personas
        .iter()
        .any(|(key, e)| e.path == path && workspace.personas.contains_key(key))
}

/// Load a workspace's own meta-spec set (empty set if none is bound).
/// Corrupt stored JSON is an error — silently treating it as an empty set
/// would flip the effective-set SHA and false-drift all provenance.
async fn load_workspace_set(state: &AppState, workspace_id: &Id) -> anyhow::Result<MetaSpecSet> {
    match state.meta_spec_sets.get(workspace_id).await? {
        Some(json) => Ok(serde_json::from_str(&json)?),
        None => Ok(MetaSpecSet::default()),
    }
}

/// Load a tenant's meta-spec set (empty set if none is bound).
async fn load_tenant_set(state: &AppState, tenant_id: &Id) -> anyhow::Result<TenantMetaSpecSet> {
    match state.tenant_meta_spec_sets.get(tenant_id).await? {
        Some(json) => Ok(serde_json::from_str(&json)?),
        None => Ok(TenantMetaSpecSet::default()),
    }
}

/// Resolve a workspace's effective meta-spec set (§8 inheritance): tenant
/// defaults merged with the workspace's own overrides. Public so API
/// handlers and the SHA computation share one resolution rule.
pub async fn resolve_effective_meta_spec_set(
    state: &AppState,
    workspace: &gyre_domain::Workspace,
) -> anyhow::Result<MetaSpecSet> {
    let ws_set = load_workspace_set(state, &workspace.id).await?;
    let tenant_set = load_tenant_set(state, &workspace.tenant_id).await?;
    let mut effective = merge_meta_spec_sets(&tenant_set, &ws_set);
    effective.workspace_id = workspace.id.to_string();
    Ok(effective)
}

/// Diff two tenant meta-spec sets and return the paths whose pinned version
/// changed (same semantics as `diff_meta_spec_set`: new or re-pinned entries
/// are changes; removals are not propagation triggers).
pub fn diff_tenant_meta_spec_set(old: &TenantMetaSpecSet, new: &TenantMetaSpecSet) -> Vec<String> {
    fn entries(set: &TenantMetaSpecSet) -> BTreeMap<String, String> {
        let mut m: BTreeMap<String, String> = set
            .personas
            .values()
            .map(|e| (e.path.clone(), e.sha.clone()))
            .collect();
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

/// Emit the `tenant_meta_spec_set_updated` domain event (§11): a tenant's
/// meta-spec set binding changed. Fired by the tenant set-update handler on
/// every successful PUT that changes the binding. Unscoped broadcast (the
/// audience — the tenant's workspaces — is enumerated by the propagation
/// run that follows; per-workspace events come from `run_reconciliation`).
pub async fn emit_tenant_meta_spec_set_updated(
    state: &Arc<AppState>,
    tenant_id: &Id,
    changed_paths: &[String],
) {
    state
        .emit_event(
            None,
            gyre_common::message::Destination::Broadcast,
            gyre_common::message::MessageKind::Custom("tenant_meta_spec_set_updated".to_string()),
            Some(serde_json::json!({
                "tenant_id": tenant_id.to_string(),
                "changed": changed_paths,
            })),
        )
        .await;
}

/// Result summary of one tenant-scope reconciliation run.
#[derive(Debug, Default, Clone)]
pub struct TenantReconciliationSummary {
    /// Workspaces in the tenant that were evaluated for propagation.
    pub workspaces_scanned: usize,
    /// Workspaces skipped entirely because they override every changed
    /// meta-spec (their own rollout policy governs; §8 step 3).
    pub workspaces_skipped_override: usize,
    /// Workspaces skipped because their own meta-spec set could not be read
    /// (storage error or corrupt JSON). The override gate never fails open:
    /// an unreadable set is not "no overrides", so the workspace is left to
    /// its own §6 rollout until the set is readable again.
    pub workspaces_skipped_error: usize,
    /// Reconciliation tasks created this run (summed across workspaces).
    pub tasks_created: usize,
    /// Repos already covered by an existing open reconciliation task.
    pub tasks_skipped: usize,
}

/// Run tenant-scope reconciliation (§8): propagate changed tenant-level
/// meta-spec pins to the tenant's workspaces.
///
/// For each workspace in the tenant:
/// - paths the workspace overrides (its own set pins them) are skipped —
///   each workspace's rollout policy is applied individually (§8 step 3) —
///   unless `force` (tenant-admin override for urgent compliance-driven
///   changes, §8 step 4);
/// - a workspace whose own set cannot be read (storage error or corrupt
///   JSON) is skipped entirely — the override gate never fails open, so an
///   unreadable set is not treated as "no overrides";
/// - the remaining changed paths go through `run_reconciliation`, which
///   creates the per-repo reconciliation tasks for that workspace
///   (§8 step 2: per-workspace reconciliation plans).
///
/// Only paths the tenant set actually pins propagate: §6 reconciles repos
/// bound to the changed meta-spec, and a tenant-scoped path no set pins
/// binds nothing (the blast radius is empty — §8 step 1).
pub async fn run_tenant_reconciliation(
    state: &Arc<AppState>,
    tenant_id: &Id,
    changed_paths: &[String],
    force: bool,
) -> TenantReconciliationSummary {
    let mut summary = TenantReconciliationSummary::default();
    if changed_paths.is_empty() {
        return summary;
    }

    let tenant_set = load_tenant_set(state, tenant_id).await.unwrap_or_default();
    let bound_paths = tenant_set_paths(&tenant_set);
    // Only tenant-bound paths propagate (see doc comment).
    let propagating: Vec<&String> = changed_paths
        .iter()
        .filter(|p| bound_paths.contains(*p))
        .collect();
    if propagating.is_empty() {
        info!(
            tenant_id = %tenant_id,
            "tenant reconciliation: no changed path is bound by the tenant set; nothing to propagate"
        );
        return summary;
    }

    let workspaces = match state.workspaces.list_by_tenant(tenant_id).await {
        Ok(ws) => ws,
        Err(e) => {
            warn!(tenant_id = %tenant_id, error = %e, "tenant reconciliation: failed to list workspaces");
            return summary;
        }
    };

    info!(
        tenant_id = %tenant_id,
        workspaces = workspaces.len(),
        changed = propagating.len(),
        force,
        "tenant reconciliation: controller started"
    );

    for ws in &workspaces {
        summary.workspaces_scanned += 1;
        // Never fail open the override gate (standing repo rule): a workspace
        // set that cannot be loaded is not "no overrides" — an empty-set
        // fallback would drop the workspace's pins and let tenant
        // propagation (including force) stomp a workspace that chose its own
        // version. Fail closed: skip the workspace; its own §6 rollout
        // governs until the set is readable again.
        let ws_set = match load_workspace_set(state, &ws.id).await {
            Ok(set) => set,
            Err(e) => {
                summary.workspaces_skipped_error += 1;
                warn!(
                    workspace_id = %ws.id,
                    error = %e,
                    "tenant reconciliation: failed to load workspace meta-spec set; skipping workspace (fail closed)"
                );
                continue;
            }
        };

        let mut apply: Vec<String> = Vec::new();
        let mut overridden: Vec<&String> = Vec::new();
        for path in &propagating {
            if !force && workspace_overrides_path(&tenant_set, &ws_set, path) {
                overridden.push(path);
            } else {
                apply.push((*path).clone());
            }
        }

        if !overridden.is_empty() {
            // §8 step 3: the workspace's own rollout policy governs the
            // overridden paths — it reconciles them when IT updates its own
            // binding (the §6 set-diff trigger), not on tenant propagation.
            summary.workspaces_skipped_override += 1;
            info!(
                workspace_id = %ws.id,
                overridden = overridden.len(),
                "tenant reconciliation: workspace overrides changed meta-spec(s); skipping per its rollout policy"
            );
        }

        if apply.is_empty() {
            continue;
        }

        let s = run_reconciliation(state, &ws.id, &apply).await;
        summary.tasks_created += s.tasks_created;
        summary.tasks_skipped += s.tasks_skipped;
    }

    info!(
        tenant_id = %tenant_id,
        scanned = summary.workspaces_scanned,
        skipped_override = summary.workspaces_skipped_override,
        skipped_error = summary.workspaces_skipped_error,
        created = summary.tasks_created,
        dedup_skipped = summary.tasks_skipped,
        "tenant reconciliation: controller finished"
    );

    summary
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
        // Desired state: the workspace's current EFFECTIVE meta-spec set SHA
        // (own bindings plus inherited tenant defaults, §8) — inheritance
        // changes the governing set, so code produced under the pre-inheritance
        // set is drift.
        let current_sha = crate::compute_meta_spec_set_sha(state, &ws.id).await;
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
/// members of a drifted workspace, plus the drifted meta-specs' owners
/// (user-management.md §Who Gets Notified: "Spec drift detected" → Spec
/// owner — the drift is against the workspace's effective meta-spec set,
/// so each pinned spec's ledger owner is a recipient; owner refs resolve
/// via `resolve_owner_ref`, `user:{name}` / `team:{slug}` forms). The
/// member set is the meta-spec-reconciliation.md §10 accountability
/// superset — misses no specced recipient; owners are added on top,
/// deduplicated.
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

    // Recipients: role-based members + the effective set's spec owners.
    let mut recipients: Vec<Id> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for member in &members {
        if !matches!(
            member.role,
            gyre_domain::WorkspaceRole::Admin
                | gyre_domain::WorkspaceRole::Developer
                | gyre_domain::WorkspaceRole::Owner
        ) {
            continue;
        }
        if seen.insert(member.user_id.as_str().to_string()) {
            recipients.push(member.user_id.clone());
        }
    }
    // The effective set's pinned specs are the specs the code drifted
    // from — their owners (ledger `owner` field) are routing recipients.
    if let Ok(effective) = resolve_effective_meta_spec_set(state, ws).await {
        let pinned_paths: Vec<&str> = effective
            .personas
            .values()
            .map(|e| e.path.as_str())
            .chain(effective.principles.iter().map(|e| e.path.as_str()))
            .chain(effective.standards.iter().map(|e| e.path.as_str()))
            .chain(effective.process.iter().map(|e| e.path.as_str()))
            .collect();
        for path in pinned_paths {
            let owner = match state.spec_ledger.find_by_path(path).await {
                Ok(Some(entry)) => entry.owner,
                _ => continue,
            };
            for owner_id in crate::notifications::resolve_owner_ref(state, &ws.id, &owner).await {
                if seen.insert(owner_id.as_str().to_string()) {
                    recipients.push(owner_id);
                }
            }
        }
    }

    for user_id in &recipients {
        let mut notif = Notification::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            ws.id.clone(),
            user_id.clone(),
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
        if let Err(e) = state.create_notification(&notif).await {
            warn!(
                user = %user_id,
                error = %e,
                "sweep: failed to create MetaSpecDrift notification"
            );
        } else {
            // Channel fan-out per user-management.md §Delivery Channels.
            crate::notification_dispatcher::dispatch_to_channels(state, &notif).await;
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
// §9 — Merge gate drift check
// ---------------------------------------------------------------------------

/// Synthetic gate id for the meta-spec drift warning recorded as a gate
/// result on an MR (§9: the drift appears in the MR's gate results as
/// `[WARN] meta-spec-drift: ...`). The gate result is informational — it
/// carries `GateStatus::Passed` so it never blocks a merge by itself;
/// blocking is decided by the workspace merge policy
/// (`require_current_meta_spec_set`).
pub const META_SPEC_DRIFT_GATE_ID: &str = "meta-spec-drift";

/// A detected meta-spec set drift for one MR (§9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetaSpecDrift {
    /// Meta-spec set SHA recorded in the MR's provenance attestation —
    /// the set under which the code was produced.
    pub produced_set_sha: String,
    /// The workspace's current meta-spec set SHA.
    pub current_set_sha: String,
}

/// Format the drift warning exactly as §9 specifies:
/// `[WARN] meta-spec-drift: code produced under meta-spec set {old_sha},
/// current is {new_sha}. Reconciliation may be needed.`
pub fn format_meta_spec_drift_warning(drift: &MetaSpecDrift) -> String {
    format!(
        "[WARN] meta-spec-drift: code produced under meta-spec set {}, current is {}. \
         Reconciliation may be needed.",
        drift.produced_set_sha, drift.current_set_sha
    )
}

/// Parse a drift warning produced by `format_meta_spec_drift_warning`.
///
/// The format and its parser live together so the merge-queue graph can
/// surface a recorded drift warning gate result without re-deriving it from
/// provenance. Returns `None` for any other output.
pub fn parse_meta_spec_drift_warning(output: &str) -> Option<MetaSpecDrift> {
    let rest = output.strip_prefix("[WARN] meta-spec-drift: code produced under meta-spec set ")?;
    let (produced_set_sha, rest) = rest.split_once(", current is ")?;
    let current_set_sha = rest.strip_suffix(". Reconciliation may be needed.")?;
    if produced_set_sha.is_empty() || current_set_sha.is_empty() {
        return None;
    }
    Some(MetaSpecDrift {
        produced_set_sha: produced_set_sha.to_string(),
        current_set_sha: current_set_sha.to_string(),
    })
}

/// Extract a non-empty `meta_spec_set_sha` from an attestation's signed input.
fn signed_meta_spec_set_sha(att: &gyre_common::Attestation) -> Option<&str> {
    match &att.input {
        gyre_common::AttestationInput::Signed(si) => {
            Some(si.content.meta_spec_set_sha.as_str()).filter(|s| !s.is_empty())
        }
        gyre_common::AttestationInput::Derived(_) => None,
    }
}

/// Resolve the meta-spec set SHA under which an MR's code was produced.
///
/// Looks up the MR's authorization provenance the same way the merge-time
/// attestation note does: by the source branch head commit first, then by the
/// author agent's current task. The recorded SHA comes from the chain's root
/// `SignedInput` (the work authorization). Returns `None` when the MR has no
/// attestation or the attestation records no set SHA — §9 skips the check
/// gracefully in that case.
pub async fn mr_attestation_meta_spec_set_sha(
    state: &AppState,
    mr: &gyre_domain::MergeRequest,
) -> Option<String> {
    let attestation = find_mr_attestation(state, mr).await?;
    attestation_recorded_set_sha(state, &attestation).await
}

/// Find the provenance attestation for an MR (commit first, task fallback).
async fn find_mr_attestation(
    state: &AppState,
    mr: &gyre_domain::MergeRequest,
) -> Option<gyre_common::Attestation> {
    // By source branch head commit — the commit the agent produced.
    if let Ok(Some(repo)) = state.repos.find_by_id(&mr.repository_id).await {
        if let Some(source_sha) =
            crate::git_refs::resolve_ref(&repo.path, &format!("refs/heads/{}", mr.source_branch))
                .await
        {
            if let Ok(Some(att)) = state.chain_attestations.find_by_commit(&source_sha).await {
                return Some(att);
            }
        }
    }

    // Fallback: the author agent's current task.
    if let Some(ref author_id) = mr.author_agent_id {
        if let Ok(Some(agent)) = state.agents.find_by_id(author_id).await {
            if let Some(ref task_id) = agent.current_task_id {
                if let Ok(atts) = state
                    .chain_attestations
                    .find_by_task(task_id.as_str())
                    .await
                {
                    if let Some(leaf) = atts.iter().max_by_key(|a| a.metadata.chain_depth) {
                        return Some(leaf.clone());
                    }
                }
            }
        }
    }

    None
}

/// Read the recorded meta-spec set SHA from an attestation, walking to the
/// chain's root `SignedInput` when the found node is derived.
async fn attestation_recorded_set_sha(
    state: &AppState,
    att: &gyre_common::Attestation,
) -> Option<String> {
    if let Some(sha) = signed_meta_spec_set_sha(att) {
        return Some(sha.to_string());
    }
    // load_chain returns root → leaf; the root carries the work authorization.
    let chain = state.chain_attestations.load_chain(&att.id).await.ok()?;
    chain
        .iter()
        .find_map(signed_meta_spec_set_sha)
        .map(|sha| sha.to_string())
}

/// §9 merge-gate drift evaluation for one MR.
///
/// Compares the meta-spec set SHA recorded in the MR's provenance attestation
/// against the workspace's current set SHA. Returns `None` (no drift) when:
/// - the workspace has no meta-spec set bound (nothing to conform to),
/// - the MR has no attestation, or the attestation records no set SHA
///   (produced before any set was bound — §9 skips the check gracefully),
/// - the recorded SHA matches the current set (pass).
pub async fn mr_meta_spec_drift(
    state: &AppState,
    mr: &gyre_domain::MergeRequest,
) -> Option<MetaSpecDrift> {
    let current_set_sha = crate::compute_meta_spec_set_sha(state, &mr.workspace_id).await;
    if current_set_sha.is_empty() {
        return None;
    }
    let produced_set_sha = mr_attestation_meta_spec_set_sha(state, mr).await?;
    if produced_set_sha == current_set_sha {
        return None;
    }
    Some(MetaSpecDrift {
        produced_set_sha,
        current_set_sha,
    })
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

    #[test]
    fn meta_spec_label_round_trips() {
        assert_eq!(
            meta_spec_label("backend-developer", 4),
            "meta-spec:backend-developer@v4"
        );
        assert_eq!(
            parse_meta_spec_label("meta-spec:backend-developer@v4"),
            Some(("backend-developer", 4))
        );
        // The wave label shares no prefix with these and must not parse.
        assert_eq!(parse_meta_spec_label(RECONCILIATION_LABEL), None);
        assert_eq!(parse_meta_spec_label("auto-created"), None);
        assert_eq!(parse_meta_spec_label("meta-spec:"), None);
        assert_eq!(parse_meta_spec_label("meta-spec:name@vnotanumber"), None);
        assert_eq!(parse_meta_spec_label("meta-spec:@v4"), None);
        // A name containing '@v' still round-trips (version split is rightmost).
        assert_eq!(
            parse_meta_spec_label(&meta_spec_label("a@v1-b", 2)),
            Some(("a@v1-b", 2))
        );
    }

    #[test]
    fn recon_wave_label_round_trips() {
        assert_eq!(recon_wave_label(1_700_000_000), "recon-wave:1700000000");
        assert_eq!(
            parse_recon_wave_label("recon-wave:1700000000"),
            Some(1_700_000_000)
        );
        // The spec label and non-labels must not parse as wave ids.
        assert_eq!(parse_recon_wave_label(RECONCILIATION_LABEL), None);
        assert_eq!(parse_recon_wave_label("auto-created"), None);
        assert_eq!(parse_recon_wave_label(&meta_spec_label("a", 1)), None);
        assert_eq!(parse_recon_wave_label("recon-wave:notanumber"), None);
        assert_eq!(parse_recon_wave_label("recon-wave:"), None);
        // Wave id of a task: from its label, None without one.
        let mut t = gyre_domain::Task::new(Id::new("t1"), "title", 0);
        assert_eq!(task_wave_id(&t), None);
        t.labels = vec![recon_wave_label(42), RECONCILIATION_LABEL.to_string()];
        assert_eq!(task_wave_id(&t), Some(42));
    }

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
        assert!(
            t.labels
                .iter()
                .any(|l| l == "meta-spec:backend-developer@v4"),
            "wave task carries the changed meta-spec provenance label"
        );
        assert!(
            t.labels.iter().any(|l| l.starts_with("recon-wave:")),
            "wave task carries the wave-identity label"
        );
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
        let active_sha = crate::compute_meta_spec_set_sha(&state, &ws.id).await;
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
                None,
                &[],
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
    async fn sweep_drift_notification_fans_out_to_configured_channels() {
        // Emitter-path guard: the sweep's MetaSpecDrift creation must go
        // through the channel dispatcher — a configured email channel
        // queues an outbox entry. Fails if the sweep bypasses
        // dispatch_to_channels (review finding: fan-out wiring is easy to
        // drop on new notification-creation paths).
        let state = test_state();
        let ws = make_workspace(&state, "ws-sweep-chan").await;
        let repo = make_repo(&state, "repo-chan", "ws-sweep-chan").await;

        let set = make_set("ws-sweep-chan", vec![pin("backend-developer", "a1")]);
        put_set(&state, "ws-sweep-chan", &set).await;

        // Provenance under an OLD set SHA → drift.
        let old_att = sample_attestation(repo.id.as_str(), "ws-sweep-chan", "deadbeef", now());
        state.chain_attestations.save(&old_att).await.unwrap();

        // Admin member with an email channel configured (Immediate, all
        // bands) — the dispatcher must fan the drift notification out to it.
        let membership = WorkspaceMembership {
            id: Id::new(uuid::Uuid::new_v4().to_string()),
            user_id: Id::new("user-chan"),
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
        state
            .user_channel_prefs
            .upsert(
                &Id::new("user-chan"),
                &gyre_domain::NotificationChannels {
                    in_app: true,
                    email: gyre_domain::EmailConfig {
                        enabled: true,
                        digest: gyre_domain::DigestFrequency::Immediate,
                        min_priority: gyre_domain::NotificationPriority::Low,
                    },
                    webhook: None,
                    slack: None,
                },
            )
            .await
            .unwrap();

        run_conformance_sweep(&state).await.unwrap();

        // In-app record exists …
        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user-chan"),
                Some(&ws.id),
                None,
                None,
                None,
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
            "expected in-app MetaSpecDrift notification"
        );
        // … and the configured email channel fired.
        let outbox = state
            .kv_store
            .kv_list(crate::notification_dispatcher::EMAIL_OUTBOX_NS)
            .await
            .unwrap();
        assert!(
            outbox.iter().any(|(k, _)| k.starts_with("user-chan:")),
            "sweep drift notification must reach the configured email channel"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn sweep_drift_notification_routes_to_meta_spec_owners() {
        // user-management.md §Who Gets Notified: "Spec drift detected" →
        // Spec owner. The conformance drift is against the workspace's
        // effective meta-spec set, so the pinned specs' ledger owners are
        // recipients — here a plain Viewer (no accountability role), who
        // must receive MetaSpecDrift purely through owner routing.
        let state = test_state();
        let ws = make_workspace(&state, "ws-sweep-own").await;
        let repo = make_repo(&state, "repo-sown", "ws-sweep-own").await;

        let set = make_set("ws-sweep-own", vec![pin("backend-developer", "a1")]);
        put_set(&state, "ws-sweep-own", &set).await;

        // Provenance under an OLD set SHA → drift.
        let old_att = sample_attestation(repo.id.as_str(), "ws-sweep-own", "deadbeef", now());
        state.chain_attestations.save(&old_att).await.unwrap();

        // The pinned spec's owner: a Viewer member (not
        // Admin/Developer/Owner — only owner routing can reach them).
        let mut owner = gyre_domain::User::new(
            Id::new("owner-viewer"),
            "ext-owner-viewer",
            "owner-viewer",
            1_000_000,
        );
        owner.username = "owner-viewer".to_string();
        state.users.create(&owner).await.unwrap();
        let membership = WorkspaceMembership {
            id: Id::new(uuid::Uuid::new_v4().to_string()),
            user_id: Id::new("owner-viewer"),
            workspace_id: ws.id.clone(),
            role: WorkspaceRole::Viewer,
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

        // Ledger entry for the pinned spec naming that owner.
        let entry = gyre_domain::SpecLedgerEntry {
            path: "backend-developer".to_string(),
            title: "Backend Developer persona".to_string(),
            owner: "user:owner-viewer".to_string(),
            kind: Some("meta:persona".to_string()),
            current_sha: "a1".to_string(),
            approval_mode: "human_only".to_string(),
            approval_status: gyre_domain::ApprovalStatus::Approved,
            linked_tasks: vec![],
            linked_mrs: vec![],
            drift_status: "clean".to_string(),
            created_at: 1_000_000,
            updated_at: 1_000_000,
            repo_id: None,
            workspace_id: Some("ws-sweep-own".to_string()),
        };
        state.spec_ledger.save(&entry).await.unwrap();

        run_conformance_sweep(&state).await.unwrap();

        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("owner-viewer"),
                Some(&ws.id),
                None,
                None,
                None,
                None,
                10,
                0,
            )
            .await
            .unwrap();
        assert_eq!(
            notifs.len(),
            1,
            "the drifted meta-spec's owner must receive the MetaSpecDrift notification"
        );
        assert_eq!(notifs[0].notification_type, NotificationType::MetaSpecDrift);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn sweep_clean_when_provenance_matches_current_set() {
        let state = test_state();
        let ws = make_workspace(&state, "ws-sweep-2").await;
        let repo = make_repo(&state, "repo-s2", "ws-sweep-2").await;

        let set = make_set("ws-sweep-2", vec![pin("backend-developer", "a1")]);
        put_set(&state, "ws-sweep-2", &set).await;
        let active_sha = crate::compute_meta_spec_set_sha(&state, &ws.id).await;

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
        let sha1 = crate::compute_meta_spec_set_sha(&state, &ws_id).await;
        assert!(!sha1.is_empty());

        // Identical re-PUT: same entries (deliberately supplied in the same
        // JSON object order; key order in the REQUEST must not matter
        // either, since deserialization into BTreeMap canonicalizes).
        let r2 = put(app.clone(), ws_id.clone()).await;
        assert_eq!(r2.status(), StatusCode::OK);
        let sha2 = crate::compute_meta_spec_set_sha(&state, &ws_id).await;
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
        let sha3 = crate::compute_meta_spec_set_sha(&state, &ws_id).await;
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
        let sha4 = crate::compute_meta_spec_set_sha(&state, &ws_id).await;
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

        // The payload names what the wave changed: the changed meta-spec
        // (name + version) materialized as a provenance label on each wave
        // task at creation. This is the data the Architectural Timeline's
        // reconciliation key-moment narrative renders (system-explorer.md §6).
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
        let payload = msgs[0].payload.as_ref().expect("completion payload");
        assert_eq!(payload["task_count"].as_u64(), Some(2));
        let specs = payload["specs"].as_array().expect("specs array");
        assert_eq!(specs.len(), 1, "one changed meta-spec across both repos");
        assert_eq!(specs[0]["name"], "backend-developer");
        assert_eq!(specs[0]["version"].as_u64(), Some(4));

        // MetaSpecDrift notification for the admin member (via the shared
        // emit_reconciliation_completed helper).
        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user-wave"),
                Some(&ws.id),
                Some(6),
                Some(6),
                None,
                None,
                &[],
                10,
                0,
            )
            .await
            .unwrap();
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
        // the identity must still distinguish the waves. The wave-id label
        // is millisecond-resolution; the sleep guarantees wave 2's id is
        // strictly greater than wave 1's.
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
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

        // Wave scoping: each event's payload names only ITS wave's changed
        // meta-spec version. Wave 1's event (emitted first, in message
        // order) carries v4; wave 2's carries v5 — the v4 task is still
        // terminal in the workspace and must not bleed into wave 2's
        // payload (system-explorer.md §6 renders this as the reconciliation
        // key-moment narrative).
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
        assert_eq!(msgs.len(), 2);
        let wave1_specs = msgs[1].payload.as_ref().unwrap()["specs"].clone();
        let wave2_specs = msgs[0].payload.as_ref().unwrap()["specs"].clone();
        assert_eq!(
            wave1_specs,
            serde_json::json!([{ "name": "backend-developer", "version": 4 }]),
            "wave 1 event names the version its wave aligned"
        );
        assert_eq!(
            wave2_specs,
            serde_json::json!([{ "name": "backend-developer", "version": 5 }]),
            "wave 2 event must not include wave 1's version"
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

    // -- §9: merge-gate drift evaluation -------------------------------------

    /// Create an MR whose author agent's current task carries the given
    /// provenance attestation (the task-based lookup path — the commit-based
    /// path needs a real git repo on disk).
    async fn make_mr_with_task_attestation(
        state: &Arc<AppState>,
        mr_id: &str,
        ws_id: &str,
        repo: &Repository,
        att: gyre_common::Attestation,
    ) -> gyre_domain::MergeRequest {
        let mut agent = gyre_domain::Agent::new(Id::new(format!("agent-{mr_id}")), "author", now());
        agent.workspace_id = Id::new(ws_id);
        agent.current_task_id = Some(Id::new(att.metadata.task_id.clone()));
        state.agents.create(&agent).await.unwrap();
        state.chain_attestations.save(&att).await.unwrap();

        let mut mr = gyre_domain::MergeRequest::new(
            Id::new(mr_id),
            repo.id.clone(),
            format!("MR {mr_id}"),
            "feat/x",
            "main",
            now(),
        );
        mr.workspace_id = Id::new(ws_id);
        mr.author_agent_id = Some(agent.id.clone());
        state.merge_requests.create(&mr).await.unwrap();
        mr
    }

    fn drift_test_set(ws_id: &str) -> crate::api::meta_specs::MetaSpecSet {
        make_set(ws_id, vec![pin("backend-developer", "a1")])
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn drift_check_matching_set_sha_passes() {
        let state = test_state();
        let ws = make_workspace(&state, "ws-drift-1").await;
        let repo = make_repo(&state, "repo-drift-1", "ws-drift-1").await;
        put_set(&state, "ws-drift-1", &drift_test_set("ws-drift-1")).await;
        let current = crate::compute_meta_spec_set_sha(&state, &ws.id).await;
        assert!(!current.is_empty());

        // Provenance under the CURRENT set SHA → no drift (§9 pass case).
        let att = sample_attestation(repo.id.as_str(), "ws-drift-1", &current, now());
        let mr =
            make_mr_with_task_attestation(&state, "mr-drift-1", "ws-drift-1", &repo, att).await;

        assert_eq!(mr_meta_spec_drift(&state, &mr).await, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn drift_check_mismatched_set_sha_reports_drift() {
        let state = test_state();
        let ws = make_workspace(&state, "ws-drift-2").await;
        let repo = make_repo(&state, "repo-drift-2", "ws-drift-2").await;
        put_set(&state, "ws-drift-2", &drift_test_set("ws-drift-2")).await;
        let current = crate::compute_meta_spec_set_sha(&state, &ws.id).await;

        // Provenance under an OLD set SHA → drift.
        let att = sample_attestation(repo.id.as_str(), "ws-drift-2", "deadbeef", now());
        let mr =
            make_mr_with_task_attestation(&state, "mr-drift-2", "ws-drift-2", &repo, att).await;

        let drift = mr_meta_spec_drift(&state, &mr)
            .await
            .expect("drift expected");
        assert_eq!(drift.produced_set_sha, "deadbeef");
        assert_eq!(drift.current_set_sha, current);

        // The warning matches the §9 format exactly and round-trips.
        let warning = format_meta_spec_drift_warning(&drift);
        assert!(
            warning.starts_with(
                "[WARN] meta-spec-drift: code produced under meta-spec set deadbeef, current is "
            ),
            "warning must follow the §9 format: {warning}"
        );
        assert!(warning.ends_with(". Reconciliation may be needed."));
        assert_eq!(
            parse_meta_spec_drift_warning(&warning).as_ref(),
            Some(&drift)
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn drift_check_mr_without_attestation_skips_gracefully() {
        let state = test_state();
        let _ws = make_workspace(&state, "ws-drift-3").await;
        let repo = make_repo(&state, "repo-drift-3", "ws-drift-3").await;
        put_set(&state, "ws-drift-3", &drift_test_set("ws-drift-3")).await;

        // MR with no author agent and no attestation — the check must skip
        // without crashing (§9: "MRs without attestation skip the check").
        let mut mr = gyre_domain::MergeRequest::new(
            Id::new("mr-drift-3"),
            repo.id.clone(),
            "MR no attestation",
            "feat/y",
            "main",
            now(),
        );
        mr.workspace_id = Id::new("ws-drift-3");
        state.merge_requests.create(&mr).await.unwrap();

        assert_eq!(mr_meta_spec_drift(&state, &mr).await, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn drift_check_workspace_without_set_skips() {
        let state = test_state();
        let _ws = make_workspace(&state, "ws-drift-4").await;
        let repo = make_repo(&state, "repo-drift-4", "ws-drift-4").await;
        // No set bound — nothing to conform to.

        let att = sample_attestation(repo.id.as_str(), "ws-drift-4", "deadbeef", now());
        let mr =
            make_mr_with_task_attestation(&state, "mr-drift-4", "ws-drift-4", &repo, att).await;

        assert_eq!(mr_meta_spec_drift(&state, &mr).await, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn drift_check_empty_recorded_sha_skips() {
        let state = test_state();
        let _ws = make_workspace(&state, "ws-drift-5").await;
        let repo = make_repo(&state, "repo-drift-5", "ws-drift-5").await;
        put_set(&state, "ws-drift-5", &drift_test_set("ws-drift-5")).await;

        // Provenance recorded before any set was bound (empty SHA) — cannot
        // assert drift against an unknown baseline, same rule as the sweep.
        let att = sample_attestation(repo.id.as_str(), "ws-drift-5", "", now());
        let mr =
            make_mr_with_task_attestation(&state, "mr-drift-5", "ws-drift-5", &repo, att).await;

        assert_eq!(mr_meta_spec_drift(&state, &mr).await, None);
    }

    #[test]
    fn drift_warning_parse_rejects_foreign_output() {
        assert_eq!(parse_meta_spec_drift_warning("[PASS] cargo test"), None);
        assert_eq!(parse_meta_spec_drift_warning(""), None);
        assert_eq!(
            parse_meta_spec_drift_warning(
                "[WARN] meta-spec-drift: code produced under meta-spec set , current is x. Reconciliation may be needed."
            ),
            None,
            "empty produced SHA must not parse"
        );
    }
    // -- §8: tenant-scope meta-spec changes -----------------------------------

    fn make_tenant_set(
        tenant_id: &str,
        entries: Vec<MetaSpecPinnedEntry>,
    ) -> crate::api::meta_specs::TenantMetaSpecSet {
        let mut personas = std::collections::BTreeMap::new();
        for (i, e) in entries.into_iter().enumerate() {
            // Distinct key namespace from the workspace-set helper's
            // `role-{i}`: a tenant role and a workspace role that happen to
            // share a key name are a genuine rebinding (workspace wins), not
            // something the test data should trip by accident.
            personas.insert(format!("tenant-role-{i}"), e);
        }
        crate::api::meta_specs::TenantMetaSpecSet {
            tenant_id: tenant_id.to_string(),
            personas,
            principles: vec![],
            standards: vec![],
            process: vec![],
        }
    }

    async fn put_tenant_set(
        state: &Arc<AppState>,
        tenant_id: &str,
        set: &crate::api::meta_specs::TenantMetaSpecSet,
    ) {
        let json = serde_json::to_string(set).unwrap();
        state
            .tenant_meta_spec_sets
            .upsert(&Id::new(tenant_id), &json)
            .await
            .unwrap();
    }

    async fn make_workspace_in_tenant(
        state: &Arc<AppState>,
        id: &str,
        tenant_id: &str,
    ) -> gyre_domain::Workspace {
        let ws = Workspace {
            id: Id::new(id),
            tenant_id: Id::new(tenant_id),
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

    async fn reconciliation_tasks_for_workspace(
        state: &Arc<AppState>,
        workspace_id: &str,
    ) -> Vec<gyre_domain::Task> {
        open_tasks_with_label(state, RECONCILIATION_LABEL)
            .await
            .into_iter()
            .filter(|t| t.workspace_id.as_str() == workspace_id)
            .collect()
    }

    /// §8: a tenant-level meta-spec change propagates to every workspace in
    /// the tenant that does not override it, creating per-repo reconciliation
    /// tasks in each (per-workspace reconciliation plans).
    #[tokio::test(flavor = "multi_thread")]
    async fn tenant_reconciliation_creates_tasks_for_inheriting_workspaces() {
        let state = test_state();
        let ms = make_meta_spec(&state, "backend-developer", 4).await;
        put_tenant_set(
            &state,
            "default",
            &make_tenant_set("default", vec![pin("backend-developer", "a1")]),
        )
        .await;

        let _ws1 = make_workspace_in_tenant(&state, "ws-ten-1", "default").await;
        let _repo1 = make_repo(&state, "repo-ten-1", "ws-ten-1").await;
        // A workspace whose own set pins a DIFFERENT spec: it does not
        // override the changed path, so it inherits and reconciles too.
        let _ws2 = make_workspace_in_tenant(&state, "ws-ten-2", "default").await;
        let _repo2 = make_repo(&state, "repo-ten-2", "ws-ten-2").await;
        put_set(
            &state,
            "ws-ten-2",
            &make_set("ws-ten-2", vec![pin("other-spec", "zz")]),
        )
        .await;
        // A workspace in ANOTHER tenant is never in the blast radius.
        let _ws3 = make_workspace_in_tenant(&state, "ws-ten-other", "other-tenant").await;
        let _repo3 = make_repo(&state, "repo-ten-3", "ws-ten-other").await;

        let summary = run_tenant_reconciliation(
            &state,
            &Id::new("default"),
            std::slice::from_ref(&ms.name),
            false,
        )
        .await;

        assert_eq!(summary.workspaces_scanned, 2);
        assert_eq!(summary.workspaces_skipped_override, 0);
        assert_eq!(
            summary.tasks_created, 2,
            "one task per repo in each inheriting workspace"
        );

        let tasks1 = reconciliation_tasks_for_workspace(&state, "ws-ten-1").await;
        assert_eq!(tasks1.len(), 1);
        assert_eq!(
            tasks1[0].title,
            "Align code with updated meta:persona backend-developer v4"
        );
        let tasks2 = reconciliation_tasks_for_workspace(&state, "ws-ten-2").await;
        assert_eq!(tasks2.len(), 1);
        // Cross-tenant workspace untouched.
        let tasks3 = reconciliation_tasks_for_workspace(&state, "ws-ten-other").await;
        assert!(
            tasks3.is_empty(),
            "workspace in another tenant must not reconcile"
        );
    }

    /// §8 step 3: a workspace that overrides the changed meta-spec with its
    /// own pin is skipped — its own rollout policy governs.
    #[tokio::test(flavor = "multi_thread")]
    async fn tenant_reconciliation_skips_overriding_workspace() {
        let state = test_state();
        let ms = make_meta_spec(&state, "backend-developer", 4).await;
        put_tenant_set(
            &state,
            "default",
            &make_tenant_set("default", vec![pin("backend-developer", "a1")]),
        )
        .await;

        let _ws1 = make_workspace_in_tenant(&state, "ws-ovr-1", "default").await;
        let _repo1 = make_repo(&state, "repo-ovr-1", "ws-ovr-1").await;
        // This workspace pins the changed path itself → override.
        put_set(
            &state,
            "ws-ovr-1",
            &make_set("ws-ovr-1", vec![pin("backend-developer", "old-sha")]),
        )
        .await;
        // ...and a sibling that inherits.
        let _ws2 = make_workspace_in_tenant(&state, "ws-ovr-2", "default").await;
        let _repo2 = make_repo(&state, "repo-ovr-2", "ws-ovr-2").await;

        let summary = run_tenant_reconciliation(
            &state,
            &Id::new("default"),
            std::slice::from_ref(&ms.name),
            false,
        )
        .await;

        assert_eq!(summary.workspaces_scanned, 2);
        assert_eq!(summary.workspaces_skipped_override, 1);
        assert_eq!(
            summary.tasks_created, 1,
            "only the non-overriding workspace reconciles"
        );

        let tasks1 = reconciliation_tasks_for_workspace(&state, "ws-ovr-1").await;
        assert!(
            tasks1.is_empty(),
            "overriding workspace must be skipped by tenant propagation"
        );
        let tasks2 = reconciliation_tasks_for_workspace(&state, "ws-ovr-2").await;
        assert_eq!(tasks2.len(), 1);
    }

    /// §8 step 4: the tenant-admin force flag bypasses the workspace-level
    /// rollout policy — urgent compliance-driven changes reach overriding
    /// workspaces too.
    #[tokio::test(flavor = "multi_thread")]
    async fn tenant_reconciliation_force_bypasses_workspace_override() {
        let state = test_state();
        let ms = make_meta_spec(&state, "backend-developer", 4).await;
        put_tenant_set(
            &state,
            "default",
            &make_tenant_set("default", vec![pin("backend-developer", "a1")]),
        )
        .await;

        let _ws1 = make_workspace_in_tenant(&state, "ws-force-1", "default").await;
        let _repo1 = make_repo(&state, "repo-force-1", "ws-force-1").await;
        put_set(
            &state,
            "ws-force-1",
            &make_set("ws-force-1", vec![pin("backend-developer", "old-sha")]),
        )
        .await;

        let summary = run_tenant_reconciliation(
            &state,
            &Id::new("default"),
            std::slice::from_ref(&ms.name),
            true,
        )
        .await;

        assert_eq!(summary.workspaces_scanned, 1);
        assert_eq!(
            summary.workspaces_skipped_override, 0,
            "force must not count the override as a skip"
        );
        assert_eq!(
            summary.tasks_created, 1,
            "force propagates past the override"
        );
        let tasks1 = reconciliation_tasks_for_workspace(&state, "ws-force-1").await;
        assert_eq!(tasks1.len(), 1);
        assert_eq!(
            tasks1[0].title,
            "Align code with updated meta:persona backend-developer v4"
        );
    }

    /// §8 step 3 fail-closed: a workspace whose own set cannot be read
    /// (storage error / corrupt JSON) is NOT treated as "no overrides" —
    /// an empty-set fallback would drop its pins and let tenant
    /// propagation (including force) stomp a workspace that chose its own
    /// version. The workspace is skipped; its own §6 rollout governs.
    #[tokio::test(flavor = "multi_thread")]
    async fn tenant_reconciliation_fails_closed_on_unreadable_workspace_set() {
        let state = test_state();
        let ms = make_meta_spec(&state, "backend-developer", 4).await;
        put_tenant_set(
            &state,
            "default",
            &make_tenant_set("default", vec![pin("backend-developer", "a1")]),
        )
        .await;

        let _ws1 = make_workspace_in_tenant(&state, "ws-err-1", "default").await;
        let _repo1 = make_repo(&state, "repo-err-1", "ws-err-1").await;
        // Corrupt stored set: the override gate must not decode-fail-open
        // into "this workspace overrides nothing".
        state
            .meta_spec_sets
            .upsert(&Id::new("ws-err-1"), "{not valid json")
            .await
            .unwrap();

        // force=true is the strongest case: an urgent compliance change must
        // still not reach a workspace whose override state is unknown.
        let summary = run_tenant_reconciliation(
            &state,
            &Id::new("default"),
            std::slice::from_ref(&ms.name),
            true,
        )
        .await;

        assert_eq!(summary.workspaces_scanned, 1);
        assert_eq!(
            summary.workspaces_skipped_error, 1,
            "unreadable workspace set must be counted as skipped, not decoded as empty"
        );
        assert_eq!(
            summary.tasks_created, 0,
            "override gate must fail closed — no propagation to a workspace with unknown overrides, even with force"
        );
        assert!(reconciliation_tasks_for_workspace(&state, "ws-err-1")
            .await
            .is_empty());
    }

    /// §8 step 1 (blast radius): only paths the tenant set pins propagate —
    /// a tenant-scoped change nothing binds has an empty blast radius.
    #[tokio::test(flavor = "multi_thread")]
    async fn tenant_reconciliation_ignores_unbound_paths() {
        let state = test_state();
        let ms = make_meta_spec(&state, "unbound-spec", 2).await;
        put_tenant_set(
            &state,
            "default",
            &make_tenant_set("default", vec![pin("backend-developer", "a1")]),
        )
        .await;
        let _ws1 = make_workspace_in_tenant(&state, "ws-unb-1", "default").await;
        let _repo1 = make_repo(&state, "repo-unb-1", "ws-unb-1").await;

        let summary = run_tenant_reconciliation(
            &state,
            &Id::new("default"),
            std::slice::from_ref(&ms.name),
            false,
        )
        .await;
        assert_eq!(summary.workspaces_scanned, 0);
        assert_eq!(summary.tasks_created, 0);
        assert!(reconciliation_tasks_for_workspace(&state, "ws-unb-1")
            .await
            .is_empty());
    }

    /// §8 step 1: the blast-radius endpoint reports every workspace whose
    /// EFFECTIVE set binds the spec — inheriting (non-overriding) workspaces
    /// via tenant inheritance, overriding ones via their own binding, and
    /// never a workspace outside the tenant.
    #[tokio::test(flavor = "multi_thread")]
    async fn blast_radius_includes_all_non_overriding_workspaces() {
        use axum::{body::Body, Router};
        use tower::ServiceExt;

        let state = test_state();
        put_tenant_set(
            &state,
            "default",
            &make_tenant_set("default", vec![pin("backend-developer", "a1")]),
        )
        .await;

        // No own set: inherits the tenant default.
        let _ws_a = make_workspace_in_tenant(&state, "ws-br-a", "default").await;
        let _repo_a = make_repo(&state, "repo-br-a", "ws-br-a").await;
        // Overrides the path with its own pin.
        let _ws_b = make_workspace_in_tenant(&state, "ws-br-b", "default").await;
        let _repo_b = make_repo(&state, "repo-br-b", "ws-br-b").await;
        put_set(
            &state,
            "ws-br-b",
            &make_set("ws-br-b", vec![pin("backend-developer", "own-sha")]),
        )
        .await;
        // Own set pinning a different spec: still inherits the changed path.
        let _ws_c = make_workspace_in_tenant(&state, "ws-br-c", "default").await;
        let _repo_c = make_repo(&state, "repo-br-c", "ws-br-c").await;
        put_set(
            &state,
            "ws-br-c",
            &make_set("ws-br-c", vec![pin("other-spec", "zz")]),
        )
        .await;
        // Different tenant, no sets: never affected.
        let _ws_d = make_workspace_in_tenant(&state, "ws-br-d", "other-tenant").await;
        let _repo_d = make_repo(&state, "repo-br-d", "ws-br-d").await;

        let app: Router = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                http::Request::builder()
                    .uri("/api/v1/meta-specs/backend-developer/blast-radius")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        let affected: Vec<String> = json["affected_workspaces"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| w["id"].as_str().unwrap().to_string())
            .collect();
        assert!(
            affected.contains(&"ws-br-a".to_string()),
            "inheriting workspace (no own set) must be in the blast radius"
        );
        assert!(
            affected.contains(&"ws-br-b".to_string()),
            "overriding workspace binds the spec via its own pin"
        );
        assert!(
            affected.contains(&"ws-br-c".to_string()),
            "non-overriding workspace with its own set must be in the blast radius"
        );
        assert!(
            !affected.contains(&"ws-br-d".to_string()),
            "workspace in another tenant must not be affected"
        );

        let repos = json["affected_repos"].as_array().unwrap();
        let reason_for = |rid: &str| {
            repos
                .iter()
                .find(|r| r["id"].as_str() == Some(rid))
                .and_then(|r| r["reason"].as_str())
                .map(|s| s.to_string())
        };
        assert_eq!(
            reason_for("repo-br-a").as_deref(),
            Some("tenant_inheritance"),
            "workspace without its own pin is affected via tenant inheritance"
        );
        assert_eq!(
            reason_for("repo-br-c").as_deref(),
            Some("tenant_inheritance"),
            "workspace not overriding the path is affected via tenant inheritance"
        );
        assert_eq!(
            reason_for("repo-br-b").as_deref(),
            Some("workspace_binding"),
            "overriding workspace is affected via its own binding"
        );
        assert!(reason_for("repo-br-d").is_none());
    }

    /// §8 inheritance merge semantics: tenant defaults with workspace
    /// overrides — workspace pin wins on conflict, non-conflicting tenant
    /// defaults survive.
    #[tokio::test(flavor = "multi_thread")]
    async fn effective_set_merges_tenant_defaults_with_overrides() {
        let state = test_state();
        let tenant = make_tenant_set(
            "default",
            vec![pin("backend-developer", "t1"), pin("reviewer", "t2")],
        );
        put_tenant_set(&state, "default", &tenant).await;
        let ws = make_workspace_in_tenant(&state, "ws-merge-1", "default").await;
        put_set(
            &state,
            "ws-merge-1",
            &make_set(
                "ws-merge-1",
                vec![pin("backend-developer", "w9"), pin("custom-spec", "w1")],
            ),
        )
        .await;
        let effective = resolve_effective_meta_spec_set(&state, &ws).await.unwrap();

        // Workspace override wins for the same path...
        assert_eq!(
            effective
                .personas
                .values()
                .find(|e| e.path == "backend-developer"),
            Some(&pin("backend-developer", "w9"))
        );
        // ...tenant defaults the workspace does not pin survive...
        assert_eq!(
            effective.personas.values().find(|e| e.path == "reviewer"),
            Some(&pin("reviewer", "t2"))
        );
        // ...and the workspace's own extra bindings are kept.
        assert_eq!(
            effective
                .personas
                .values()
                .find(|e| e.path == "custom-spec"),
            Some(&pin("custom-spec", "w1"))
        );

        // A workspace with NO set of its own inherits the tenant defaults.
        let ws2 = make_workspace_in_tenant(&state, "ws-merge-2", "default").await;
        let effective2 = resolve_effective_meta_spec_set(&state, &ws2).await.unwrap();
        assert_eq!(
            effective2
                .personas
                .values()
                .find(|e| e.path == "backend-developer"),
            Some(&pin("backend-developer", "t1"))
        );
        assert_eq!(effective2.workspace_id, "ws-merge-2");

        // Override predicate matches the merge: the overridden path is
        // governed by the workspace, the inherited one by the tenant.
        let ws1_set = serde_json::from_str::<crate::api::meta_specs::MetaSpecSet>(
            &state.meta_spec_sets.get(&ws.id).await.unwrap().unwrap(),
        )
        .unwrap();
        assert!(workspace_overrides_path(
            &tenant,
            &ws1_set,
            "backend-developer"
        ));
        assert!(!workspace_overrides_path(&tenant, &ws1_set, "reviewer"));
    }

    /// §8: the effective-set SHA (§9 constraint / §10 sweep input) reflects
    /// inheritance — a workspace with no own set hashes the merged tenant
    /// defaults, a workspace override changes the SHA, and a workspace with
    /// neither set stays unbound (empty SHA, skip).
    #[tokio::test(flavor = "multi_thread")]
    async fn effective_set_sha_reflects_inheritance() {
        let state = test_state();
        let ws1 = make_workspace_in_tenant(&state, "ws-sha-1", "default").await;
        let ws2 = make_workspace_in_tenant(&state, "ws-sha-2", "other-tenant").await;

        // Neither workspace has any set: both unbound.
        assert!(crate::compute_meta_spec_set_sha(&state, &ws1.id)
            .await
            .is_empty());
        assert!(crate::compute_meta_spec_set_sha(&state, &ws2.id)
            .await
            .is_empty());

        // A tenant set binds ws1 (same tenant) but not ws2 (other tenant).
        put_tenant_set(
            &state,
            "default",
            &make_tenant_set("default", vec![pin("backend-developer", "a1")]),
        )
        .await;
        let sha1 = crate::compute_meta_spec_set_sha(&state, &ws1.id).await;
        assert!(
            !sha1.is_empty(),
            "inherited tenant defaults must bind the workspace"
        );
        assert!(
            crate::compute_meta_spec_set_sha(&state, &ws2.id)
                .await
                .is_empty(),
            "a tenant set must not leak across tenants"
        );

        // A workspace override changes the effective SHA.
        put_set(
            &state,
            "ws-sha-1",
            &make_set("ws-sha-1", vec![pin("backend-developer", "b2")]),
        )
        .await;
        let sha1_overridden = crate::compute_meta_spec_set_sha(&state, &ws1.id).await;
        assert_ne!(
            sha1, sha1_overridden,
            "an override must change the effective SHA"
        );
    }

    /// §8: GET /workspaces/{id}/meta-spec-set returns the EFFECTIVE set —
    /// tenant defaults merged with workspace overrides.
    #[tokio::test(flavor = "multi_thread")]
    async fn workspace_get_meta_spec_set_returns_effective_set() {
        use axum::{body::Body, Router};
        use tower::ServiceExt;

        let state = test_state();
        put_tenant_set(
            &state,
            "default",
            &make_tenant_set("default", vec![pin("reviewer", "t2")]),
        )
        .await;
        let _ws = make_workspace_in_tenant(&state, "ws-get-1", "default").await;

        let app: Router = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                http::Request::builder()
                    .uri("/api/v1/workspaces/ws-get-1/meta-spec-set")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let paths: Vec<&str> = json["personas"]
            .as_object()
            .unwrap()
            .values()
            .map(|e| e["path"].as_str().unwrap())
            .collect();
        assert!(
            paths.contains(&"reviewer"),
            "workspace with no own set must see the inherited tenant default"
        );
    }

    /// §8: approving a Tenant-scoped meta-spec triggers per-workspace
    /// reconciliation through the tenant controller — inheriting workspaces
    /// reconcile, overriding ones are skipped, and `force` reaches them.
    #[tokio::test(flavor = "multi_thread")]
    async fn tenant_spec_approval_triggers_per_workspace_reconciliation() {
        use axum::{body::Body, Router};
        use tower::ServiceExt;

        let state = test_state();
        crate::bootstrap_default_tenant(&state).await;

        let app: Router = crate::api::api_router().with_state(state.clone());

        // Create a workspace with a repo (via API: tenant of the caller,
        // "default") and one overriding workspace.
        async fn create_ws_with_repo(
            app: &Router,
            state: &Arc<AppState>,
            slug: &str,
            repo_id: &str,
            override_pin: bool,
        ) -> String {
            use tower::ServiceExt;
            let resp = app
                .clone()
                .oneshot(
                    http::Request::builder()
                        .method("POST")
                        .uri("/api/v1/workspaces")
                        .header("authorization", "Bearer test-token")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({"name": slug, "slug": slug}).to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), http::StatusCode::CREATED);
            let ws_json: serde_json::Value = {
                let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap();
                serde_json::from_slice(&bytes).unwrap()
            };
            let ws_id = ws_json["id"].as_str().unwrap().to_string();
            let _repo = make_repo(state, repo_id, &ws_id).await;
            if override_pin {
                put_set(
                    state,
                    &ws_id,
                    &make_set(&ws_id, vec![pin("secure-worker", "own-sha")]),
                )
                .await;
            }
            ws_id
        }

        let ws_appr_1 = create_ws_with_repo(&app, &state, "ws-appr-1", "repo-appr-1", false).await;
        let ws_appr_2 = create_ws_with_repo(&app, &state, "ws-appr-2", "repo-appr-2", true).await;

        // Create a Tenant-scoped meta-spec (Pending).
        let resp = app
            .clone()
            .oneshot(
                http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs-registry")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "kind": "meta:persona",
                            "name": "secure-worker",
                            "scope": "Tenant",
                            "scope_id": "default",
                            "prompt": "Be secure."
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::CREATED);
        let ms_json: serde_json::Value = {
            let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap();
            serde_json::from_slice(&bytes).unwrap()
        };
        let ms_id = ms_json["id"].as_str().unwrap().to_string();
        assert_eq!(ms_json["scope"], "Tenant");

        // Bind it in the tenant set. The set PUT propagates, but the spec is
        // still Pending — the approval transition owns the wave (§6).
        let resp = app
            .clone()
            .oneshot(
                http::Request::builder()
                    .method("PUT")
                    .uri("/api/v1/tenants/default/meta-specs/set")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "personas": { "worker": {"path": "secure-worker", "sha": "s1"} },
                            "principles": [], "standards": [], "process": []
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        assert!(
            open_tasks_with_label(&state, RECONCILIATION_LABEL)
                .await
                .is_empty(),
            "a Pending bind is not a rollout; the approval owns the wave"
        );

        // Approve → per-workspace reconciliation through the tenant
        // controller: the inheriting workspace reconciles, the overriding
        // one is skipped.
        let resp = app
            .clone()
            .oneshot(
                http::Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/meta-specs-registry/{ms_id}"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"approval_status": "Approved"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);

        let tasks = open_tasks_with_label(&state, RECONCILIATION_LABEL).await;
        assert_eq!(
            tasks.len(),
            1,
            "only the non-overriding workspace reconciles"
        );
        assert_eq!(tasks[0].workspace_id.as_str(), ws_appr_1);
        assert_eq!(
            tasks[0].title,
            "Align code with updated meta:persona secure-worker v1"
        );

        // Approve new content WITH force: the overriding workspace is
        // reached too (tenant-admin override, §8 step 4).
        let resp = app
            .oneshot(
                http::Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/meta-specs-registry/{ms_id}"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "prompt": "Be very secure.",
                            "approval_status": "Approved",
                            "force": true
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);

        let tasks = open_tasks_with_label(&state, RECONCILIATION_LABEL).await;
        let overriding: Vec<&gyre_domain::Task> = tasks
            .iter()
            .filter(|t| t.workspace_id.as_str() == ws_appr_2)
            .collect();
        assert_eq!(
            overriding.len(),
            1,
            "force must propagate the urgent change past the workspace override"
        );
        assert_eq!(
            overriding[0].title,
            "Align code with updated meta:persona secure-worker v2"
        );
    }

    /// §8 trigger (set-diff path): PUTting a tenant set that pins an
    /// APPROVED meta-spec propagates through the tenant controller — the
    /// inheriting workspace gets a reconciliation task, the overriding one
    /// is skipped, and a removal-only re-PUT creates nothing (removals are
    /// not propagation triggers). This is the killing test for the
    /// `run_tenant_reconciliation` call in `put_tenant_meta_spec_set`:
    /// delete that call and the first assertion fails.
    #[tokio::test(flavor = "multi_thread")]
    async fn tenant_set_put_propagates_approved_pins_to_inheriting_workspaces() {
        use axum::{body::Body, Router};
        use tower::ServiceExt;

        let state = test_state();
        crate::bootstrap_default_tenant(&state).await;
        let app: Router = crate::api::api_router().with_state(state.clone());

        // APPROVED meta-spec: pinning it in the tenant set is a rollout
        // (unlike a Pending bind, whose wave the approval transition owns).
        let ms = make_meta_spec(&state, "backend-developer", 4).await;

        // Inheriting workspace (no set of its own) with a repo.
        let _ws1 = make_workspace_in_tenant(&state, "ws-put-1", "default").await;
        let _repo1 = make_repo(&state, "repo-put-1", "ws-put-1").await;
        // Overriding workspace: its own rollout policy governs the path.
        let _ws2 = make_workspace_in_tenant(&state, "ws-put-2", "default").await;
        let _repo2 = make_repo(&state, "repo-put-2", "ws-put-2").await;
        put_set(
            &state,
            "ws-put-2",
            &make_set("ws-put-2", vec![pin("backend-developer", "own-sha")]),
        )
        .await;

        // PUT the tenant set pinning the APPROVED spec — the set-diff
        // trigger must propagate without any approval transition.
        let resp = app
            .clone()
            .oneshot(
                http::Request::builder()
                    .method("PUT")
                    .uri("/api/v1/tenants/default/meta-specs/set")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "personas": { "worker": {"path": "backend-developer", "sha": "a1"} },
                            "principles": [], "standards": [], "process": []
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);

        let tasks1 = reconciliation_tasks_for_workspace(&state, "ws-put-1").await;
        assert_eq!(
            tasks1.len(),
            1,
            "tenant-set PUT pinning an APPROVED spec must create a reconciliation task in the inheriting workspace"
        );
        assert_eq!(
            tasks1[0].title,
            format!(
                "Align code with updated meta:persona {} v{}",
                ms.name, ms.version
            )
        );
        assert!(
            reconciliation_tasks_for_workspace(&state, "ws-put-2")
                .await
                .is_empty(),
            "the PUT trigger must skip the overriding workspace like the approval trigger does"
        );

        // A fresh inheriting workspace added after the PUT: it has no task
        // yet, so dedup cannot mask a spurious trigger below.
        let _ws3 = make_workspace_in_tenant(&state, "ws-put-3", "default").await;
        let _repo3 = make_repo(&state, "repo-put-3", "ws-put-3").await;

        // Removal-only re-PUT (pin deleted, nothing added): removals are not
        // propagation triggers — no new tasks anywhere.
        let resp = app
            .oneshot(
                http::Request::builder()
                    .method("PUT")
                    .uri("/api/v1/tenants/default/meta-specs/set")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "personas": {}, "principles": [], "standards": [], "process": []
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        assert!(
            reconciliation_tasks_for_workspace(&state, "ws-put-3")
                .await
                .is_empty(),
            "a removal-only re-PUT must not propagate"
        );
        assert_eq!(
            reconciliation_tasks_for_workspace(&state, "ws-put-1").await.len(),
            1,
            "the removal-only re-PUT must not fire a second wave for the already-reconciled workspace"
        );
    }
}
