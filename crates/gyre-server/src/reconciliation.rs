//! Meta-spec reconciliation controller and conformance sweep
//! (meta-spec-reconciliation.md §6, §10, §11).
//!
//! Two entry points:
//!
//! - `run_reconciliation` — the slow rollout (§6). Triggered when a
//!   workspace's meta-spec set binding moves to a new version of a
//!   meta-spec (via `PUT /api/v1/workspaces/{id}/meta-spec-set`) or when a
//!   registry meta-spec is approved on new content. Computes the blast
//!   radius (repos in the workspace bound to the changed meta-spec) and
//!   creates a deduplicated reconciliation task per affected repo.
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

use std::collections::HashMap;
use std::sync::Arc;

use gyre_common::{Id, Notification, NotificationType};
use gyre_domain::{TaskPriority, TaskStatus, TaskType};
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
/// (the workspace binding governs all repos in the workspace — §2), and a
/// task titled `Align code with updated {kind} {name} v{version}` is
/// created unless an open task for the same title already exists.
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
    let mut changed_specs: Vec<gyre_domain::MetaSpec> = Vec::new();
    for path in changed_paths {
        match find_meta_spec_by_name(state, path).await {
            Ok(Some(ms)) => changed_specs.push(ms),
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

    for ms in &changed_specs {
        // §6 flow step 4 / task spec: title "Align code with updated {kind} {name} v{version}".
        let title = format!(
            "Align code with updated {} {} v{}",
            ms.kind, ms.name, ms.version
        );

        // Dedup: skip if a non-terminal task with the same title exists.
        let exists = existing_tasks.iter().any(|t| {
            t.title == title && !matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled)
        });

        if exists {
            summary.tasks_skipped += 1;
            info!(title, "reconciliation: task already exists, skipping");
            continue;
        }

        let task_id = Id::new(uuid::Uuid::new_v4().to_string());
        let mut task = gyre_domain::Task::new(task_id.clone(), &title, now);
        task.priority = TaskPriority::Medium;
        task.labels = vec![
            RECONCILIATION_LABEL.to_string(),
            "auto-created".to_string(),
        ];
        task.description = Some(format!(
            "Auto-created by the meta-spec reconciliation controller \
             (meta-spec-reconciliation.md §6).\nMeta-spec: {} ({}) v{}\n\
             Content hash: {}\nRe-run affected work under the updated rules; \
             no-op outcome (code already conforms) is valid.",
            ms.name, ms.kind, ms.version, ms.content_hash
        ));
        task.workspace_id = ws_id.clone();
        task.repo_id = repos[0].id.clone();
        task.spec_path = Some(ms.name.clone());
        // §6 flow step 5: the repo orchestrator spawns the reconciliation
        // agent — delegation is the signal-chain discriminator for that.
        task.task_type = Some(TaskType::Delegation);

        match state.tasks.create(&task).await {
            Err(e) => warn!(title, error = %e, "reconciliation: failed to create task"),
            Ok(()) => {
                summary.tasks_created += 1;
                info!(title, "reconciliation: created reconciliation task");
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
            }
        }
    }

    if summary.tasks_created > 0 {
        // §11 ReconciliationStarted: controller created tasks for a workspace.
        state
            .emit_event(
                Some(ws_id.clone()),
                gyre_common::message::Destination::Workspace(ws_id.clone()),
                gyre_common::message::MessageKind::Custom(
                    "reconciliation_started".to_string(),
                ),
                Some(serde_json::json!({
                    "workspace_id": ws_id.to_string(),
                    "tasks_created": summary.tasks_created,
                    "tasks_skipped": summary.tasks_skipped,
                })),
            )
            .await;
        // ReconciliationCompleted → Event-tier message + MetaSpecDrift
        // notifications for Admin/Developer/Owner members (§11 / HSI §4).
        crate::emit_reconciliation_completed(
            state,
            ws_id.clone(),
            Some(serde_json::json!({
                "workspace_id": ws_id.to_string(),
                "tasks_created": summary.tasks_created,
                "tasks_skipped": summary.tasks_skipped,
            })),
        )
        .await;
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
        "reconciliation: controller finished"
    );

    summary
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

    fn entries(set: &MetaSpecSet) -> HashMap<String, String> {
        let mut m: HashMap<String, String> = set
            .personas
            .iter()
            .map(|(k, e)| (e.path.clone(), e.sha.clone()))
            .collect();
        // Two personas may pin the same spec path — keep the first pin.
        for e in set.principles.iter().chain(&set.standards).chain(&set.process) {
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
        // Dedup on the deterministic title for this workspace+set pair so
        // repeated sweeps do not pile up tasks.
        let title = format!(
            "Review meta-spec drift in workspace {} (active set {})",
            ws.slug, current_sha
        );
        let exists = existing_tasks.iter().any(|t| {
            t.title == title && !matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled)
        });

        if exists {
            summary.tasks_skipped += 1;
            info!(workspace_id = %ws.id, "sweep: drift-review task already exists");
        } else {
            let task_id = Id::new(uuid::Uuid::new_v4().to_string());
            let mut task = gyre_domain::Task::new(task_id, &title, now);
            task.priority = TaskPriority::Medium;
            task.labels = vec![
                DRIFT_REVIEW_LABEL.to_string(),
                "auto-created".to_string(),
            ];
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
        notif.entity_ref = Some(ws.id.to_string());
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
            .inc_by(created);
    }
    if skipped > 0 {
        state
            .metrics
            .reconciliation_tasks_total
            .with_label_values(&[workspace_id, "skipped"])
            .inc_by(skipped);
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

    fn make_set(ws_id: &str, entries: Vec<MetaSpecPinnedEntry>) -> crate::api::meta_specs::MetaSpecSet {
        crate::api::meta_specs::MetaSpecSet {
            workspace_id: ws_id.to_string(),
            personas: entries
                .into_iter()
                .map(|e| ("backend".to_string(), e))
                .collect(),
            principles: vec![],
            standards: vec![],
            process: vec![],
        }
    }

    async fn put_set(state: &Arc<AppState>, ws_id: &str, set: &crate::api::meta_specs::MetaSpecSet) {
        let json = serde_json::to_string(set).unwrap();
        state.meta_spec_sets.upsert(&Id::new(ws_id), &json).await.unwrap();
    }

    fn open_tasks_with_label(
        state: &Arc<AppState>,
        label: &str,
    ) -> impl Future<Output = Vec<gyre_domain::Task>> + '_ {
        async move {
            state
                .tasks
                .list()
                .await
                .unwrap()
                .into_iter()
                .filter(|t| t.labels.iter().any(|l| l == label))
                .collect()
        }
    }

    // -- §6: reconciliation creates tasks for affected repos ----------------

    #[tokio::test(flavor = "multi_thread")]
    async fn reconciliation_creates_task_for_bound_repo() {
        let state = test_state();
        let ws = make_workspace(&state, "ws-recon-1").await;
        let _repo = make_repo(&state, "repo-1", "ws-recon-1").await;
        let ms = make_meta_spec(&state, "backend-developer", 4).await;

        let summary = run_reconciliation(&state, &ws.id, &[ms.name.clone()]).await;

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

        let summary = run_reconciliation(&state, &ws.id, &[ms.name.clone()]).await;
        assert_eq!(summary.tasks_created, 0);
        assert_eq!(open_tasks_with_label(&state, RECONCILIATION_LABEL).await.len(), 0);
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

        let first = run_reconciliation(&state, &ws.id, &[ms.name.clone()]).await;
        assert_eq!(first.tasks_created, 1);

        // Re-run with the same version → no new task.
        let second = run_reconciliation(&state, &ws.id, &[ms.name.clone()]).await;
        assert_eq!(second.tasks_created, 0);
        assert_eq!(second.tasks_skipped, 1);
        assert_eq!(open_tasks_with_label(&state, RECONCILIATION_LABEL).await.len(), 1);

        // New version → new task (different title).
        state.meta_specs.update(&MetaSpec {
            version: 5,
            ..ms.clone()
        }).await.unwrap();
        let third = run_reconciliation(&state, &ws.id, &[ms.name.clone()]).await;
        assert_eq!(third.tasks_created, 1);
        assert_eq!(open_tasks_with_label(&state, RECONCILIATION_LABEL).await.len(), 2);
    }

    // -- §6: set diffing (trigger condition) ---------------------------------

    #[tokio::test(flavor = "multi_thread")]
    async fn set_diff_detects_version_changes() {
        let old = make_set("ws", vec![pin("backend-developer", "a1")]);
        let new = make_set("ws", vec![pin("backend-developer", "a1")]);
        assert!(diff_meta_spec_set(&old, &new).is_empty(), "same set → no changes");

        let new = make_set("ws", vec![pin("backend-developer", "b2")]);
        assert_eq!(diff_meta_spec_set(&old, &new), vec!["backend-developer".to_string()]);

        // Newly bound entry counts as changed.
        let new = make_set("ws", vec![pin("backend-developer", "a1"), pin("security", "c3")]);
        let mut diff = diff_meta_spec_set(&old, &new);
        diff.sort();
        assert_eq!(diff, vec!["backend-developer".to_string(), "security".to_string()]);

        // Removal is not a trigger.
        let new = make_set("ws", vec![]);
        assert!(diff_meta_spec_set(&old, &new).is_empty());
    }

    // -- §10: conformance sweep detects drift --------------------------------

    fn sample_attestation(repo_id: &str, ws_id: &str, meta_sha: &str, created_at: u64) -> gyre_common::Attestation {
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
            key_binding: KeyBinding {
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
        let active_sha = crate::compute_meta_spec_set_sha(state.meta_spec_sets.as_ref(), &ws.id).await;
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
        state.workspace_memberships.create(&membership).await.unwrap();

        let summary = run_conformance_sweep(&state).await.unwrap();
        assert_eq!(summary.drift_detected, 1);
        assert_eq!(summary.tasks_created, 1);

        let tasks = open_tasks_with_label(&state, DRIFT_REVIEW_LABEL).await;
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].workspace_id.as_str(), "ws-sweep-1");

        // MetaSpecDrift notification created for the admin member.
        let notifs = state
            .notifications
            .list_for_user(&Id::new("user-sweep"), Some(&ws.id), Some(6), Some(6), None, 10, 0)
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
            .list_by_workspace(&ws.id, Some("meta_spec_drift_detected"), None, None, None, Some(50))
            .await
            .unwrap();
        assert!(
            msgs.iter().any(|m| m.kind == MessageKind::Custom(
                "meta_spec_drift_detected".to_string()
            )),
            "expected meta_spec_drift_detected event message"
        );

        // Second sweep: same drift → task deduplicated, but drift still
        // counted and notification refreshed.
        let second = run_conformance_sweep(&state).await.unwrap();
        assert_eq!(second.drift_detected, 1);
        assert_eq!(second.tasks_created, 0);
        assert_eq!(second.tasks_skipped, 1);
        assert_eq!(open_tasks_with_label(&state, DRIFT_REVIEW_LABEL).await.len(), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn sweep_clean_when_provenance_matches_current_set() {
        let state = test_state();
        let ws = make_workspace(&state, "ws-sweep-2").await;
        let repo = make_repo(&state, "repo-s2", "ws-sweep-2").await;

        let set = make_set("ws-sweep-2", vec![pin("backend-developer", "a1")]);
        put_set(&state, "ws-sweep-2", &set).await;
        let active_sha = crate::compute_meta_spec_set_sha(state.meta_spec_sets.as_ref(), &ws.id).await;

        // Provenance under the CURRENT set SHA → no drift.
        let att = sample_attestation(repo.id.as_str(), "ws-sweep-2", &active_sha, now());
        state.chain_attestations.save(&att).await.unwrap();

        let summary = run_conformance_sweep(&state).await.unwrap();
        assert_eq!(summary.drift_detected, 0);
        assert_eq!(summary.tasks_created, 0);
        assert_eq!(open_tasks_with_label(&state, DRIFT_REVIEW_LABEL).await.len(), 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn sweep_skips_workspaces_without_set() {
        let state = test_state();
        let ws = make_workspace(&state, "ws-sweep-3").await;
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
        let ws = make_workspace(&state, "ws-sweep-4").await;
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
        let ws_json: serde_json::Value =
            serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap())
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
            open_tasks_with_label(&state, RECONCILIATION_LABEL).await.len(),
            1,
            "identical re-PUT must not create a second task"
        );
    }
}
