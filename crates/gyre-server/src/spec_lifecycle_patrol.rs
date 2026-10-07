//! Spec-lifecycle accountability patrol — task-204.
//!
//! spec-lifecycle.md §Accountability Integration: "The Accountability agent's
//! patrol checks for:
//! - `spec-drift-review` tasks that have been open longer than one Ralph loop cycle
//! - `spec-implementation` tasks that have been in Backlog for more than N days
//! - Specs that were modified but have no corresponding task (should never happen
//!   if the hook works, but defense in depth)
//!
//! If any of these are found, the Accountability agent escalates to the workspace
//! orchestrator."
//!
//! The three checks run against real storage — `TaskRepository` for the task-age
//! checks and `SpecLedgerRepository` (`state.spec_ledger`, the same ledger the
//! post-receive hook writes) for the modified-spec check — and every finding
//! produces a real `MessageKind::Escalation` on the message bus, addressed to the
//! workspace orchestrator's event stream
//! (`GET /api/v1/workspaces/:id/messages`).
//!
//! This is deliberately a *separate* patrol from `spec_patrol.rs` (spec-links.md
//! §Accountability Agent Integration), which inspects the spec *graph*. Different
//! contract, different storage, different findings.
//!
//! Thresholds are never read from the check bodies: they are function parameters
//! (populated from the request, defaults in [`DEFAULT_DRIFT_REVIEW_MAX_AGE_SECS`]
//! / [`DEFAULT_IMPLEMENTATION_BACKLOG_MAX_AGE_SECS`]) so that task-109 can feed
//! per-repo `spec_lifecycle` configuration values without a rewrite.

use gyre_common::message::{Destination, MessageKind};
use gyre_common::Id;
use gyre_domain::{Task, TaskStatus};
use serde::Deserialize;
use tracing::{info, warn};

use crate::git_http::SPEC_WATCHED_PATHS;
use crate::spec_patrol::{PatrolFinding, PatrolResponse};
use crate::AppState;

// ---------------------------------------------------------------------------
// Threshold defaults
// ---------------------------------------------------------------------------

/// Default `spec-drift-review` age limit: 24 h.
///
/// spec-lifecycle.md bounds this by "one Ralph loop cycle"; there is no
/// in-code loop-cycle constant (a cycle ends when an agent completes or
/// fails, not on a clock), so the default is the longest plausible single
/// agent-loop turn — one day. Callers override per request; task-109 will
/// source the value from per-repo spec-lifecycle config.
pub const DEFAULT_DRIFT_REVIEW_MAX_AGE_SECS: u64 = 24 * 60 * 60;

/// Default `spec-implementation` Backlog age limit: 7 days (spec-lifecycle.md's
/// "more than N days"). Overridable per request, same as above.
pub const DEFAULT_IMPLEMENTATION_BACKLOG_MAX_AGE_SECS: u64 = 7 * 24 * 60 * 60;

/// Labels the post-receive hook applies (`git_http::classify_spec_change`).
/// Reconciling against anything else would check a contract the forge never
/// writes.
const LABEL_SPEC_DRIFT_REVIEW: &str = "spec-drift-review";
const LABEL_SPEC_IMPLEMENTATION: &str = "spec-implementation";

/// `source` marker in the escalation payload so the orchestrator can tell a
/// patrol escalation apart from a peer-agent escalation.
const ESCALATION_SOURCE: &str = "spec_lifecycle_patrol";

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

/// Optional body for `POST /api/v1/patrol/spec-lifecycle`.
#[derive(Debug, Deserialize, Default)]
pub struct SpecLifecyclePatrolRequest {
    /// Max age of an open `spec-drift-review` task before it is flagged
    /// (default: [`DEFAULT_DRIFT_REVIEW_MAX_AGE_SECS`]).
    pub drift_review_max_age_secs: Option<u64>,
    /// Max time a `spec-implementation` task may sit in Backlog before it is
    /// flagged (default: [`DEFAULT_IMPLEMENTATION_BACKLOG_MAX_AGE_SECS`]).
    pub implementation_backlog_max_age_secs: Option<u64>,
}

// ---------------------------------------------------------------------------
// Patrol execution
// ---------------------------------------------------------------------------

/// Run the three spec-lifecycle accountability checks against real storage.
///
/// Returns `Err` rather than an empty finding list when a repository query
/// fails: a patrol that cannot read its inputs must not report "nothing
/// wrong".
pub async fn run_spec_lifecycle_patrol(
    state: &AppState,
    now_secs: u64,
    drift_review_max_age_secs: u64,
    implementation_backlog_max_age_secs: u64,
) -> anyhow::Result<Vec<PatrolFinding>> {
    // One task scan feeds all three checks (checks 1/2 by label, check 3 by
    // `spec_path` coverage) — no per-spec queries.
    let tasks = state.tasks.list().await?;
    let ledger = state.spec_ledger.list_all().await?;

    let mut findings = Vec::new();
    check_stale_drift_review_tasks(&tasks, now_secs, drift_review_max_age_secs, &mut findings);
    check_stale_implementation_backlog_tasks(
        &tasks,
        now_secs,
        implementation_backlog_max_age_secs,
        &mut findings,
    );
    check_modified_specs_without_tasks(
        &ledger,
        &tasks,
        now_secs,
        drift_review_max_age_secs,
        &mut findings,
    );

    info!(
        findings = findings.len(),
        drift_review_max_age_secs,
        implementation_backlog_max_age_secs,
        "spec_lifecycle_patrol: patrol complete"
    );
    Ok(findings)
}

/// Check 1 — `spec-drift-review` tasks open longer than one Ralph loop cycle.
///
/// Terminal (`Done`/`Cancelled`) tasks are never flagged: a cancelled drift
/// review is a decision, not an accountability gap.
fn check_stale_drift_review_tasks(
    tasks: &[Task],
    now_secs: u64,
    max_age_secs: u64,
    findings: &mut Vec<PatrolFinding>,
) {
    for task in tasks {
        if matches!(task.status, TaskStatus::Done | TaskStatus::Cancelled) {
            continue;
        }
        if !has_label(task, LABEL_SPEC_DRIFT_REVIEW) {
            continue;
        }
        let age_secs = now_secs.saturating_sub(task.created_at);
        if age_secs <= max_age_secs {
            continue;
        }
        findings.push(PatrolFinding {
            finding_type: "stale_drift_review_task".to_string(),
            severity: "warning".to_string(),
            spec_path: task.spec_path.clone().unwrap_or_default(),
            detail: format!(
                "{LABEL_SPEC_DRIFT_REVIEW} task '{}' ({}) has been open for {}s, \
                 exceeding the {}s limit",
                task.title, task.id, age_secs, max_age_secs
            ),
            suggested_action: format!(
                "Re-review the spec change or re-decompose task {} — a drift review \
                 older than one loop cycle means the spec change was never reconciled \
                 with the implementation",
                task.id
            ),
            task_id: Some(task.id.to_string()),
            workspace_id: Some(task.workspace_id.to_string()),
        });
    }
}

/// Check 2 — `spec-implementation` tasks parked in Backlog for more than N days.
///
/// Requires `status == Backlog` explicitly, which also excludes terminal
/// statuses: an implementation task picked up into InProgress/Review is being
/// worked on, and a Done/Cancelled one is closed out.
fn check_stale_implementation_backlog_tasks(
    tasks: &[Task],
    now_secs: u64,
    max_age_secs: u64,
    findings: &mut Vec<PatrolFinding>,
) {
    for task in tasks {
        if task.status != TaskStatus::Backlog {
            continue;
        }
        if !has_label(task, LABEL_SPEC_IMPLEMENTATION) {
            continue;
        }
        let age_secs = now_secs.saturating_sub(task.created_at);
        if age_secs <= max_age_secs {
            continue;
        }
        findings.push(PatrolFinding {
            finding_type: "stale_implementation_backlog".to_string(),
            severity: "warning".to_string(),
            spec_path: task.spec_path.clone().unwrap_or_default(),
            detail: format!(
                "{LABEL_SPEC_IMPLEMENTATION} task '{}' ({}) has sat in Backlog for {}s, \
                 exceeding the {}s limit",
                task.title, task.id, age_secs, max_age_secs
            ),
            suggested_action: format!(
                "Decompose or schedule task {} — the orchestrator owns task \
                 decomposition, and an approved spec with an unplanned implementation \
                 task is an unstarted delivery",
                task.id
            ),
            task_id: Some(task.id.to_string()),
            workspace_id: Some(task.workspace_id.to_string()),
        });
    }
}

/// Check 3 — watched specs with no corresponding task (defense in depth).
///
/// spec-lifecycle.md: "Specs that were modified but have no corresponding task
/// (should never happen if the hook works, but defense in depth)."
///
/// "Has a corresponding task" = at least one non-`Cancelled` task whose
/// `spec_path` names this spec. Both path spellings are accepted because the
/// ledger stores manifest-relative paths (`system/x.md`, from
/// `spec_registry::sync_spec_ledger`) while the post-receive hook records the
/// repo-root git path (`specs/system/x.md`, `git_http::process_spec_lifecycle`).
/// When both sides know the repo, the task must belong to the ledger entry's
/// repo — otherwise an identically-named spec in another repo would mask the gap.
///
/// Deliberate exclusions (each is "not a modification awaiting a task"):
/// - paths outside [`SPEC_WATCHED_PATHS`] — only `specs/system/` and
///   `specs/development/` trigger lifecycle tasks (spec-lifecycle.md §What This
///   Does NOT Do), so anything else can never have a task;
/// - `Deprecated` entries — removal is handled by the hook's `spec-deprecated`
///   task at deprecation time, and that task closing would otherwise leave a
///   permanently un-actionable finding;
/// - entries with no `current_sha` — a manifest entry with no file at HEAD is
///   not a modified spec;
/// - entries updated within the drift-review window — the hook runs in the same
///   tick as the push, so a just-written ledger row must not be flagged before
///   its task exists (same grace the check-1 threshold expresses).
fn check_modified_specs_without_tasks(
    ledger: &[gyre_domain::SpecLedgerEntry],
    tasks: &[Task],
    now_secs: u64,
    grace_secs: u64,
    findings: &mut Vec<PatrolFinding>,
) {
    // (repo_id, spec_path) pairs covered by a non-Cancelled task. Empty repo_id
    // on a task means "scope unknown" and is treated as a wildcard, since those
    // tasks are exactly the ones the ledger may or may not be able to match.
    let covered: Vec<(String, String)> = tasks
        .iter()
        .filter(|t| t.status != TaskStatus::Cancelled)
        .filter_map(|t| {
            t.spec_path
                .as_deref()
                .map(|p| (t.repo_id.to_string(), normalize_spec_path(p)))
        })
        .collect();

    for entry in ledger {
        let path = normalize_spec_path(&entry.path);
        if !is_watched_spec_path(&path) {
            continue;
        }
        if entry.approval_status == gyre_domain::ApprovalStatus::Deprecated {
            continue;
        }
        if entry.current_sha.is_empty() {
            continue;
        }
        if now_secs.saturating_sub(entry.updated_at) <= grace_secs {
            continue;
        }

        let has_task = covered.iter().any(|(task_repo, task_path)| {
            task_path == &path
                && (entry.repo_id.is_none()
                    || task_repo.is_empty()
                    || task_repo.as_str() == entry.repo_id.as_deref().unwrap_or(""))
        });
        if has_task {
            continue;
        }

        findings.push(PatrolFinding {
            finding_type: "spec_without_task".to_string(),
            severity: "error".to_string(),
            spec_path: path.clone(),
            detail: format!(
                "Spec '{}' (sha {}) was modified {}s ago (after the {}s hook grace \
                 window) but no non-cancelled task references it",
                path,
                entry.current_sha,
                now_secs.saturating_sub(entry.updated_at),
                grace_secs
            ),
            suggested_action: format!(
                "Create the spec-lifecycle task for '{}' manually and investigate why \
                 the post-receive hook did not (dedup suppressed it, task creation \
                 failed, or the push bypassed the hook)",
                path
            ),
            task_id: None,
            workspace_id: entry.workspace_id.clone(),
        });
    }
}

/// Escalate every finding to the workspace orchestrator on the message bus.
///
/// Uses `MessageKind::Escalation` — the kind already reserved for
/// "a human/orchestrator must intervene" (see `stale_agents.rs`'s repo
/// orchestrator-death escalation), so no new variant is needed. Destination is
/// the workspace event stream (the pattern `api/specs.rs` uses for
/// orchestrator-directed spec signals): unlike a single agent inbox, it stays
/// readable if the orchestrator is restarted, and is queryable at
/// `GET /api/v1/workspaces/:id/messages?kind=escalation`.
///
/// Findings with no resolvable workspace go to `Destination::Broadcast` —
/// a fabricated `"default"` scope would mis-target the escalation.
pub async fn escalate_findings(state: &AppState, findings: &[PatrolFinding]) -> usize {
    let mut escalated = 0usize;

    for finding in findings {
        let (workspace_id, to) = escalation_route(finding.workspace_id.as_deref());

        state
            .emit_event(
                workspace_id,
                to,
                MessageKind::Escalation,
                Some(serde_json::json!({
                    "source": ESCALATION_SOURCE,
                    "finding_type": finding.finding_type,
                    "severity": finding.severity,
                    "spec_path": finding.spec_path,
                    "task_id": finding.task_id,
                    "message": finding.detail,
                    "suggested_action": finding.suggested_action,
                })),
            )
            .await;
        escalated += 1;
    }

    if escalated > 0 {
        info!(
            escalated,
            "spec_lifecycle_patrol: escalated findings to the workspace orchestrator"
        );
    } else {
        // Clean patrols are silent by design, but a patrol that keeps finding
        // nothing while checks are live is worth one debug line during triage.
        tracing::debug!("spec_lifecycle_patrol: nothing to escalate");
    }

    escalated
}

/// Where an escalation for a finding goes.
///
/// A workspace id is only usable when it is present and non-blank. Missing
/// scope means `Broadcast`, never a guessed workspace: `emit_event` would
/// happily persist an escalation under a scope no operator created, and a
/// literal `"default"` workspace would then receive escalations that belong to
/// someone else.
fn escalation_route(workspace_id: Option<&str>) -> (Option<Id>, Destination) {
    match workspace_id.map(str::trim).filter(|ws| !ws.is_empty()) {
        Some(ws) => {
            let id = Id::new(ws);
            (Some(id.clone()), Destination::Workspace(id))
        }
        None => (None, Destination::Broadcast),
    }
}

/// Does this task carry `label`?
fn has_label(task: &Task, label: &str) -> bool {
    task.labels.iter().any(|l| l == label)
}

/// True when a spec path is under a watched prefix
/// (`specs/system/`, `specs/development/`) — the same list the post-receive
/// hook filters on, so the patrol and the hook can never disagree about which
/// specs are supposed to have tasks.
fn is_watched_spec_path(path: &str) -> bool {
    SPEC_WATCHED_PATHS
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

/// Canonical `specs/`-rooted form of a spec path.
///
/// The spec ledger stores manifest-relative paths (`system/x.md`); the
/// lifecycle hook stores git paths (`specs/system/x.md`). Both mean the same
/// spec, so comparisons are made in the rooted form.
fn normalize_spec_path(path: &str) -> String {
    if path.starts_with("specs/") {
        path.to_string()
    } else {
        format!("specs/{path}")
    }
}

/// Run the patrol, escalate every finding, and return the response.
///
/// Shared by the HTTP handler and the endpoint tests so the escalation step
/// can't be skipped by a caller that reaches for `run_spec_lifecycle_patrol`
/// directly.
pub async fn run_and_escalate(
    state: &AppState,
    now_secs: u64,
    drift_review_max_age_secs: u64,
    implementation_backlog_max_age_secs: u64,
) -> anyhow::Result<PatrolResponse> {
    let findings = run_spec_lifecycle_patrol(
        state,
        now_secs,
        drift_review_max_age_secs,
        implementation_backlog_max_age_secs,
    )
    .await?;

    let escalated = escalate_findings(state, &findings).await;
    if escalated > 0 {
        warn!(
            escalated,
            "spec_lifecycle_patrol: accountability gaps escalated to workspace orchestrators"
        );
    }

    Ok(PatrolResponse { findings })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;
    use gyre_domain::spec_ledger::{ApprovalStatus, SpecLedgerEntry};
    use gyre_domain::TaskPriority;

    const NOW: u64 = 2_000_000;
    const DRIFT_MAX: u64 = 86_400; // 24h
    const BACKLOG_MAX: u64 = 604_800; // 7d

    fn make_task(id: &str, labels: &[&str], status: TaskStatus, created_at: u64) -> Task {
        let mut task = Task::new(Id::new(id), &format!("Task {id}"), created_at);
        task.labels = labels.iter().map(|l| l.to_string()).collect();
        task.status = status;
        task.priority = TaskPriority::High;
        task.workspace_id = Id::new("ws1");
        task.repo_id = Id::new("repo1");
        task.spec_path = Some("specs/system/agent-runtime.md".to_string());
        task
    }

    fn make_ledger_entry(
        path: &str,
        workspace_id: Option<&str>,
        updated_at: u64,
        status: ApprovalStatus,
    ) -> SpecLedgerEntry {
        SpecLedgerEntry {
            path: path.to_string(),
            title: format!("Spec {path}"),
            owner: "user:test".to_string(),
            kind: None,
            current_sha: "sha-current".to_string(),
            approval_mode: "human_only".to_string(),
            approval_status: status,
            linked_tasks: vec![],
            linked_mrs: vec![],
            drift_status: "clean".to_string(),
            created_at: updated_at,
            updated_at,
            repo_id: Some("repo1".to_string()),
            workspace_id: workspace_id.map(|w| w.to_string()),
        }
    }

    async fn run(state: &AppState) -> Vec<PatrolFinding> {
        run_spec_lifecycle_patrol(state, NOW, DRIFT_MAX, BACKLOG_MAX)
            .await
            .expect("patrol query should succeed")
    }

    fn types(findings: &[PatrolFinding]) -> Vec<&str> {
        findings.iter().map(|f| f.finding_type.as_str()).collect()
    }

    /// Check 1: stale flagged, fresh not, `Done`/`Cancelled` never.
    #[tokio::test]
    async fn flags_stale_drift_review_tasks_only() {
        let state = test_state();
        state
            .tasks
            .create(&make_task(
                "drift-stale",
                &["spec-drift-review", "auto-created"],
                TaskStatus::Backlog,
                NOW - DRIFT_MAX - 1,
            ))
            .await
            .unwrap();
        state
            .tasks
            .create(&make_task(
                "drift-fresh",
                &["spec-drift-review", "auto-created"],
                TaskStatus::InProgress,
                NOW - 60,
            ))
            .await
            .unwrap();
        state
            .tasks
            .create(&make_task(
                "drift-done",
                &["spec-drift-review", "auto-created"],
                TaskStatus::Done,
                NOW - 40 * DRIFT_MAX,
            ))
            .await
            .unwrap();
        state
            .tasks
            .create(&make_task(
                "drift-cancelled",
                &["spec-drift-review", "auto-created"],
                TaskStatus::Cancelled,
                NOW - 40 * DRIFT_MAX,
            ))
            .await
            .unwrap();

        let findings = run(&state).await;
        let stale: Vec<&str> = findings
            .iter()
            .filter(|f| f.finding_type == "stale_drift_review_task")
            .map(|f| f.task_id.as_deref().unwrap_or_default())
            .collect();

        assert_eq!(
            stale,
            vec!["drift-stale"],
            "only the non-terminal task past the threshold may be flagged: {findings:?}"
        );
        // The stale task's spec is covered, so check 3 must stay quiet about it.
        assert!(!types(&findings).contains(&"spec_without_task"));
    }

    /// Check 2: Backlog past the limit flagged; InProgress and fresh not.
    #[tokio::test]
    async fn flags_stale_implementation_backlog_only() {
        let state = test_state();
        state
            .tasks
            .create(&make_task(
                "impl-stale",
                &["spec-implementation", "auto-created"],
                TaskStatus::Backlog,
                NOW - BACKLOG_MAX - 1,
            ))
            .await
            .unwrap();
        state
            .tasks
            .create(&make_task(
                "impl-inprogress",
                &["spec-implementation", "auto-created"],
                TaskStatus::InProgress,
                NOW - BACKLOG_MAX - 1,
            ))
            .await
            .unwrap();
        state
            .tasks
            .create(&make_task(
                "impl-fresh",
                &["spec-implementation", "auto-created"],
                TaskStatus::Backlog,
                NOW - 60,
            ))
            .await
            .unwrap();

        let findings = run(&state).await;
        let flagged: Vec<&str> = findings
            .iter()
            .filter(|f| f.finding_type == "stale_implementation_backlog")
            .map(|f| f.task_id.as_deref().unwrap_or_default())
            .collect();

        assert_eq!(
            flagged,
            vec!["impl-stale"],
            "only Backlog past N days: {findings:?}"
        );
    }

    /// Check 3: watched spec with no task flagged; covered spec and
    /// unwatched/deprecated specs not.
    #[tokio::test]
    async fn flags_watched_specs_without_tasks() {
        let state = test_state();
        // Old enough to be past the hook grace window.
        let old = NOW - DRIFT_MAX - 1;

        // Orphaned: no task references it.
        state
            .spec_ledger
            .save(&make_ledger_entry(
                "system/orphaned.md",
                Some("ws1"),
                old,
                ApprovalStatus::Approved,
            ))
            .await
            .unwrap();
        // Covered: a Done task (non-Cancelled) references it in git-path form.
        state
            .spec_ledger
            .save(&make_ledger_entry(
                "system/covered.md",
                Some("ws1"),
                old,
                ApprovalStatus::Approved,
            ))
            .await
            .unwrap();
        let mut cover_task = make_task(
            "impl-covered",
            &["spec-implementation"],
            TaskStatus::Done,
            NOW - 10,
        );
        cover_task.spec_path = Some("specs/system/covered.md".to_string());
        state.tasks.create(&cover_task).await.unwrap();
        // Unwatched path: the hook never creates tasks for milestones.
        state
            .spec_ledger
            .save(&make_ledger_entry(
                "milestones/m36.md",
                Some("ws1"),
                old,
                ApprovalStatus::Approved,
            ))
            .await
            .unwrap();
        // Deprecated: removal handled by the hook's spec-deprecated task.
        state
            .spec_ledger
            .save(&make_ledger_entry(
                "system/gone.md",
                Some("ws1"),
                old,
                ApprovalStatus::Deprecated,
            ))
            .await
            .unwrap();
        // Modified within the grace window: the hook runs in this same tick.
        state
            .spec_ledger
            .save(&make_ledger_entry(
                "system/just-pushed.md",
                Some("ws1"),
                NOW - 10,
                ApprovalStatus::Pending,
            ))
            .await
            .unwrap();

        let findings = run(&state).await;
        let orphaned: Vec<&str> = findings
            .iter()
            .filter(|f| f.finding_type == "spec_without_task")
            .map(|f| f.spec_path.as_str())
            .collect();

        assert_eq!(
            orphaned,
            vec!["specs/system/orphaned.md"],
            "got: {findings:?}"
        );
    }

    /// All three checks at once, end to end: exactly the stale/orphaned items
    /// are flagged, each produces a persisted `escalation` message on the
    /// workspace stream, and nothing else does.
    #[tokio::test]
    async fn flags_and_escalates_only_the_accountability_gaps() {
        let state = test_state();

        state
            .tasks
            .create(&make_task(
                "drift-stale",
                &["spec-drift-review"],
                TaskStatus::Blocked,
                NOW - DRIFT_MAX - 1,
            ))
            .await
            .unwrap();
        state
            .tasks
            .create(&make_task(
                "drift-fresh",
                &["spec-drift-review"],
                TaskStatus::Review,
                NOW - 60,
            ))
            .await
            .unwrap();
        state
            .tasks
            .create(&make_task(
                "impl-stale",
                &["spec-implementation"],
                TaskStatus::Backlog,
                NOW - BACKLOG_MAX - 1,
            ))
            .await
            .unwrap();
        state
            .spec_ledger
            .save(&make_ledger_entry(
                "system/orphaned.md",
                Some("ws1"),
                NOW - DRIFT_MAX - 1,
                ApprovalStatus::Approved,
            ))
            .await
            .unwrap();
        // Spec referenced by a *cancelled* task: the cancellation means nothing
        // is delivering it, so it is still a gap (and must not be silenced by
        // the cancelled task's spec_path).
        state
            .spec_ledger
            .save(&make_ledger_entry(
                "system/only-cancelled.md",
                Some("ws1"),
                NOW - DRIFT_MAX - 1,
                ApprovalStatus::Approved,
            ))
            .await
            .unwrap();
        let mut cancelled = make_task(
            "cancelled-task",
            &["spec-implementation"],
            TaskStatus::Cancelled,
            NOW - BACKLOG_MAX - 1,
        );
        cancelled.spec_path = Some("specs/system/only-cancelled.md".to_string());
        state.tasks.create(&cancelled).await.unwrap();

        let response = run_and_escalate(&state, NOW, DRIFT_MAX, BACKLOG_MAX)
            .await
            .expect("patrol should succeed");

        let mut flagged: Vec<String> = response
            .findings
            .iter()
            .map(|f| {
                format!(
                    "{}:{}",
                    f.finding_type,
                    f.task_id.clone().unwrap_or(f.spec_path.clone())
                )
            })
            .collect();
        flagged.sort();
        assert_eq!(
            flagged,
            vec![
                "spec_without_task:specs/system/only-cancelled.md",
                "spec_without_task:specs/system/orphaned.md",
                "stale_drift_review_task:drift-stale",
                "stale_implementation_backlog:impl-stale",
            ],
            "exactly the accountability gaps must be flagged"
        );

        // Real escalation: one persisted Escalation per finding on the
        // workspace event stream, payload identifying the finding.
        let messages = state
            .messages
            .list_by_workspace(&Id::new("ws1"), Some("escalation"), None, None, None, None)
            .await
            .unwrap();
        assert_eq!(
            messages.len(),
            4,
            "every finding must be escalated, not merely logged: {messages:?}"
        );
        let payloads: Vec<String> = messages
            .iter()
            .map(|m| m.payload.as_ref().expect("escalation payload").to_string())
            .collect();
        for needle in [
            "stale_drift_review_task",
            "drift-stale",
            "stale_implementation_backlog",
            "impl-stale",
            "specs/system/orphaned.md",
            "specs/system/only-cancelled.md",
            ESCALATION_SOURCE,
        ] {
            assert!(
                payloads.iter().any(|p| p.contains(needle)),
                "escalation payload must identify {needle}: {payloads:?}"
            );
        }
    }

    /// A clean board produces no findings and no escalation messages.
    #[tokio::test]
    async fn clean_patrol_escalates_nothing() {
        let state = test_state();
        state
            .tasks
            .create(&make_task(
                "drift-fresh",
                &["spec-drift-review"],
                TaskStatus::Backlog,
                NOW - 5,
            ))
            .await
            .unwrap();

        let response = run_and_escalate(&state, NOW, DRIFT_MAX, BACKLOG_MAX)
            .await
            .unwrap();
        assert!(response.findings.is_empty(), "got: {:?}", response.findings);

        let messages = state
            .messages
            .list_by_workspace(&Id::new("ws1"), Some("escalation"), None, None, None, None)
            .await
            .unwrap();
        assert!(
            messages.is_empty(),
            "clean patrol must not escalate: {messages:?}"
        );
    }

    /// Thresholds come from the caller: tightening them must change what is
    /// flagged (proves the checks are not no-ops and are not inverted).
    #[tokio::test]
    async fn thresholds_are_not_hardcoded_in_the_checks() {
        let state = test_state();
        state
            .tasks
            .create(&make_task(
                "drift-2h",
                &["spec-drift-review"],
                TaskStatus::Backlog,
                NOW - 2 * 3600,
            ))
            .await
            .unwrap();

        let loose = run_spec_lifecycle_patrol(&state, NOW, DRIFT_MAX, BACKLOG_MAX)
            .await
            .unwrap();
        assert!(loose.is_empty(), "2h < 24h: {loose:?}");

        let tight = run_spec_lifecycle_patrol(&state, NOW, 3600, 3600)
            .await
            .unwrap();
        assert_eq!(
            types(&tight),
            vec!["stale_drift_review_task"],
            "2h > 1h: {tight:?}"
        );
    }

    /// A finding with no workspace still escalates (broadcast) instead of
    /// fabricating a scope.
    #[tokio::test]
    async fn workspaceless_finding_broadcasts() {
        let state = test_state();
        let mut task = make_task(
            "drift-no-ws",
            &["spec-drift-review"],
            TaskStatus::Backlog,
            NOW - DRIFT_MAX - 1,
        );
        task.workspace_id = Id::new("");
        state.tasks.create(&task).await.unwrap();

        let findings = run(&state).await;
        assert_eq!(types(&findings), vec!["stale_drift_review_task"]);
        escalate_findings(&state, &findings).await;

        // Broadcast messages are not persisted (see AppState::emit_event), so
        // delivery is asserted on the dispatch channel itself — subscribed
        // before the escalation, since broadcast receivers only see what is
        // published after they subscribe.
        let mut rx = state.message_broadcast_tx.subscribe();
        escalate_findings(&state, &findings).await;

        let msg = rx
            .recv()
            .await
            .expect("workspaceless finding must still dispatch an escalation");
        assert_eq!(msg.kind, MessageKind::Escalation);
        assert!(matches!(msg.to, Destination::Broadcast));
        assert!(msg.workspace_id.is_none(), "no workspace to attribute");

        // And nothing may be persisted under a scope no operator created.
        for scope in ["", "default"] {
            let stored = state
                .messages
                .list_by_workspace(&Id::new(scope), Some("escalation"), None, None, None, None)
                .await
                .unwrap();
            assert!(
                stored.is_empty(),
                "escalation must not be persisted under fabricated scope {scope:?}: {stored:?}"
            );
        }
    }

    #[test]
    fn escalation_routing_never_fabricates_a_scope() {
        let (ws, to) = escalation_route(Some("ws7"));
        assert!(matches!(&to, Destination::Workspace(id) if id.to_string() == "ws7"));
        assert_eq!(ws.map(|i| i.to_string()), Some("ws7".to_string()));

        for blank in [None, Some(""), Some("   ")] {
            let (ws, to) = escalation_route(blank);
            assert!(ws.is_none(), "{blank:?} has no workspace");
            assert!(
                matches!(to, Destination::Broadcast),
                "{blank:?} must broadcast"
            );
        }
    }

    #[test]
    fn normalize_accepts_both_path_spellings() {
        assert_eq!(normalize_spec_path("system/x.md"), "specs/system/x.md");
        assert_eq!(
            normalize_spec_path("specs/system/x.md"),
            "specs/system/x.md"
        );
        assert!(is_watched_spec_path("specs/system/x.md"));
        assert!(is_watched_spec_path("specs/development/y.md"));
        assert!(!is_watched_spec_path("specs/milestones/z.md"));
        assert!(!is_watched_spec_path("specs/prior-art/z.md"));
        assert!(!is_watched_spec_path("src/system/x.md"));
    }
}
