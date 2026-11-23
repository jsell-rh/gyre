//! Spec-lifecycle accountability patrol — task-204, extended by task-339.
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
//! task-339 (spec-registry.md §Accountability Agent) adds the four
//! manifest-aware checks, keyed on `specs/manifest.yaml` at each repo's
//! HEAD rather than the repo's `SpecLifecycleConfig` watched prefixes:
//! - orphan spec files — `specs/*.md` files at HEAD not registered in the
//!   manifest (the same file-scan class as the push-time coverage check,
//!   `spec_registry::find_unregistered_specs`)
//! - stale manifest entries — manifest entries whose file does not exist
//!   at HEAD (previously only a sync-time warning log)
//! - specs with effective `requires_approval: true` pending for more than
//!   one Ralph loop cycle, evaluated against the durable pending-since
//!   signal (`spec_pending_since`, recorded by the sync/approve/reject
//!   paths — never `updated_at`, which churns on unrelated writes)
//! - specs with effective `auto_create_tasks: true` that were modified but
//!   have no corresponding task (the former config-prefix-keyed check,
//!   rekeyed to the manifest — mirroring `should_create_lifecycle_task`;
//!   manifest-less repos keep the config watched-prefix fallback the hook
//!   itself uses)
//!
//! The checks run against real storage — `TaskRepository` for the task-age
//! checks, `SpecLedgerRepository` (`state.spec_ledger`, the same ledger the
//! post-receive hook writes) for the modified-spec and pending-age checks,
//! and each repo's on-disk git storage (HEAD manifest + file tree) for the
//! coverage checks — and every finding with a resolvable workspace produces
//! a real `MessageKind::Escalation` on the message bus, addressed to the
//! workspace orchestrator's event stream
//! (`GET /api/v1/workspaces/:id/messages`). Findings with no resolvable
//! workspace are skipped and logged (never emitted under a fabricated
//! scope), and still reach the caller through the patrol response.
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
use gyre_domain::{SpecLifecycleConfig, Task, TaskStatus};
use serde::Deserialize;
use std::collections::HashMap;
use tracing::{info, warn};

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

/// Run the spec-lifecycle accountability checks against real storage.
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
    // Checks 1/2 share one task scan (label + age filters). Checks 3-6
    // resolve each candidate spec's coverage through
    // `TaskRepository::list_by_spec_path` — the contract's named mechanism —
    // so coverage is answered by the repository, not by a client-side replay
    // of its rows.
    let tasks = state.tasks.list().await?;
    let ledger = state.spec_ledger.list_all().await?;

    // Per-repo manifest context at HEAD (task-339): the manifest is the key
    // for the four manifest-aware checks. Repos whose storage cannot be
    // read, or that carry no manifest, contribute no context — the
    // ledger-keyed checks fall back to the config rules the lifecycle hook
    // itself uses for manifest-less repos, never to a fabricated manifest.
    let repo_ctxs = load_repo_manifest_contexts(state).await?;
    let ctx_by_repo: HashMap<String, &RepoManifestContext> = repo_ctxs
        .iter()
        .map(|ctx| (ctx.repo_id.clone(), ctx))
        .collect();

    let mut findings = Vec::new();
    check_stale_drift_review_tasks(&tasks, now_secs, drift_review_max_age_secs, &mut findings);
    check_stale_implementation_backlog_tasks(
        &tasks,
        now_secs,
        implementation_backlog_max_age_secs,
        &mut findings,
    );
    check_orphan_spec_files(&repo_ctxs, &mut findings).await;
    check_stale_manifest_entries(&repo_ctxs, &mut findings).await;
    check_pending_beyond_cycle(
        state,
        &ledger,
        &ctx_by_repo,
        now_secs,
        drift_review_max_age_secs,
        &mut findings,
    )
    .await?;
    check_modified_specs_without_tasks(
        state,
        &ledger,
        &ctx_by_repo,
        now_secs,
        drift_review_max_age_secs,
        &mut findings,
    )
    .await?;

    info!(
        findings = findings.len(),
        drift_review_max_age_secs,
        implementation_backlog_max_age_secs,
        "spec_lifecycle_patrol: patrol complete"
    );
    Ok(findings)
}

// ---------------------------------------------------------------------------
// Per-repo manifest context (task-339)
// ---------------------------------------------------------------------------

/// A repo's manifest at HEAD, plus the resolution context needed to turn
/// manifest-keyed findings into escalations (the repo's workspace scope).
///
/// Built once per patrol run from real storage: the repo row (workspace
/// scope), the repo's on-disk git storage (HEAD SHA), and the manifest
/// parsed from that HEAD — the same manifest the push-time hook and ledger
/// sync read.
struct RepoManifestContext {
    repo_id: String,
    workspace_id: String,
    repo_path: String,
    head_sha: String,
    manifest: crate::spec_registry::SpecManifest,
}

/// Load the manifest context for every repo whose storage and manifest can
/// be resolved at HEAD.
///
/// Fails (rather than returning an empty list) when the repo repository
/// itself cannot be read: a patrol that cannot read its inputs must not
/// report "nothing wrong". Individual repos whose git storage is missing,
/// has no commits, or carries no `specs/manifest.yaml` are skipped with a
/// log line — they simply have no manifest-aware checks to run (the
/// ledger-keyed checks fall back to the config rules for such repos).
async fn load_repo_manifest_contexts(state: &AppState) -> anyhow::Result<Vec<RepoManifestContext>> {
    let repos = state.repos.list().await?;
    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());

    let mut ctxs = Vec::new();
    for repo in repos {
        if repo.path.is_empty() || !std::path::Path::new(&repo.path).exists() {
            tracing::debug!(
                repo_id = %repo.id,
                "spec_lifecycle_patrol: repo storage not on disk — skipping manifest checks"
            );
            continue;
        }
        let Some(head_sha) = resolve_head_sha(&git_bin, &repo.path).await else {
            warn!(
                repo_id = %repo.id,
                "spec_lifecycle_patrol: cannot resolve repo HEAD — skipping manifest checks"
            );
            continue;
        };
        let Some(manifest) = crate::spec_registry::read_manifest(&repo.path, &head_sha).await
        else {
            tracing::debug!(
                repo_id = %repo.id,
                "spec_lifecycle_patrol: no specs/manifest.yaml at HEAD — manifest-keyed \
                 checks fall back to config rules"
            );
            continue;
        };
        ctxs.push(RepoManifestContext {
            repo_id: repo.id.to_string(),
            workspace_id: repo.workspace_id.to_string(),
            repo_path: repo.path.clone(),
            head_sha,
            manifest,
        });
    }
    Ok(ctxs)
}

/// Resolve a repo's current HEAD commit SHA (`git rev-parse HEAD`).
///
/// `None` when the repo has no commits (unborn HEAD) or git fails — the
/// caller skips the repo's manifest checks rather than guessing a revision.
async fn resolve_head_sha(git_bin: &str, repo_path: &str) -> Option<String> {
    let out = tokio::process::Command::new(git_bin)
        .arg("-C")
        .arg(repo_path)
        .args(["rev-parse", "HEAD"])
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!sha.is_empty()).then_some(sha)
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

/// Check 3a — orphan spec files (task-339, spec-registry.md §Accountability
/// Agent: "Spec files that exist but aren't in the manifest").
///
/// Reuses the push-time coverage check's file-scan class
/// (`spec_registry::find_unregistered_specs`, `git ls-tree` at HEAD), so the
/// patrol flags exactly the files a push would be warned/rejected about —
/// including its exclusions (`index.md`, `prior-art/`, `milestones/`, and
/// non-`.md` files such as `manifest.yaml` itself, which is therefore never
/// flagged however it is registered).
async fn check_orphan_spec_files(
    repo_ctxs: &[RepoManifestContext],
    findings: &mut Vec<PatrolFinding>,
) {
    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());
    for ctx in repo_ctxs {
        let manifest_paths: std::collections::HashSet<String> =
            ctx.manifest.specs.iter().map(|e| e.path.clone()).collect();
        for rel in crate::spec_registry::find_unregistered_specs(
            &git_bin,
            &ctx.repo_path,
            &ctx.head_sha,
            &manifest_paths,
        )
        .await
        {
            findings.push(PatrolFinding {
                finding_type: "orphan_spec_file".to_string(),
                severity: "warning".to_string(),
                spec_path: normalize_spec_path(&rel),
                detail: format!(
                    "Spec file 'specs/{rel}' exists at HEAD of repo {} but is not \
                     registered in specs/manifest.yaml",
                    ctx.repo_id
                ),
                suggested_action: format!(
                    "Register 'specs/{rel}' in specs/manifest.yaml (or delete the file) \
                     so the spec registry's lifecycle tracking covers it"
                ),
                task_id: None,
                workspace_id: Some(ctx.workspace_id.clone()),
            });
        }
    }
}

/// Check 3b — stale manifest entries (task-339, spec-registry.md
/// §Accountability Agent: "Manifest entries whose files don't exist").
///
/// `sync_spec_ledger` already logs a warning for these at push time; this
/// check surfaces them as patrol findings escalated to the workspace
/// orchestrator, resolved against the same storage the sync reads (the
/// blob SHA at HEAD via `git rev-parse`).
async fn check_stale_manifest_entries(
    repo_ctxs: &[RepoManifestContext],
    findings: &mut Vec<PatrolFinding>,
) {
    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());
    for ctx in repo_ctxs {
        for entry in &ctx.manifest.specs {
            let rooted = normalize_spec_path(&entry.path);
            let blob = crate::spec_registry::get_blob_sha(
                &git_bin,
                &ctx.repo_path,
                &ctx.head_sha,
                &rooted,
            )
            .await;
            if blob.is_some() {
                continue;
            }
            findings.push(PatrolFinding {
                finding_type: "stale_manifest_entry".to_string(),
                severity: "warning".to_string(),
                spec_path: rooted.clone(),
                detail: format!(
                    "Manifest entry '{rooted}' in repo {} has no corresponding file \
                     at HEAD",
                    ctx.repo_id
                ),
                suggested_action: format!(
                    "Remove the stale '{rooted}' entry from specs/manifest.yaml (or \
                     restore the file) — the ledger cannot track a spec with no content"
                ),
                task_id: None,
                workspace_id: Some(ctx.workspace_id.clone()),
            });
        }
    }
}

/// Check 3c — specs with effective `requires_approval: true` pending for
/// more than one Ralph loop cycle (task-339, spec-registry.md §Accountability
/// Agent).
///
/// The age is evaluated against the durable pending-since signal maintained
/// by the sync/approve/reject paths (`spec_pending_since`) — never against
/// `updated_at`, which churns on unrelated writes, and never unconditionally:
/// a spec pending less than the bound produces no finding, and a pending
/// spec with no recorded signal (a row older than the signal itself) is
/// skipped and logged rather than guessed at. The bound is the patrol's
/// one-cycle threshold (`drift_review_max_age_secs`, default
/// [`DEFAULT_DRIFT_REVIEW_MAX_AGE_SECS`], overridable per request).
///
/// `requires_approval` is effective manifest policy: the manifest at the
/// entry's repo HEAD decides (per-entry override or `defaults:`), and an
/// entry whose manifest cannot be resolved is skipped — there is no
/// config-prefix fallback because the config has no `requires_approval`
/// notion to fall back to.
async fn check_pending_beyond_cycle(
    state: &AppState,
    ledger: &[gyre_domain::SpecLedgerEntry],
    ctx_by_repo: &HashMap<String, &RepoManifestContext>,
    now_secs: u64,
    cycle_bound_secs: u64,
    findings: &mut Vec<PatrolFinding>,
) -> anyhow::Result<()> {
    for entry in ledger {
        if entry.approval_status != gyre_domain::ApprovalStatus::Pending {
            continue;
        }
        let Some(ctx) = entry.repo_id.as_deref().and_then(|r| ctx_by_repo.get(r)) else {
            continue;
        };
        let path = normalize_spec_path(&entry.path);
        let Some(manifest_entry) =
            crate::spec_registry::manifest_entry_for_path(&ctx.manifest, &path)
        else {
            continue;
        };
        if !manifest_entry.effective_requires_approval(&ctx.manifest.defaults) {
            continue;
        }
        let Some(pending_since) =
            crate::spec_pending_since::pending_since(&state.kv_store, &entry.path).await?
        else {
            tracing::debug!(
                spec_path = %entry.path,
                "spec_lifecycle_patrol: pending entry has no recorded pending-since \
                 signal — skipping pending-age check (cannot evaluate age)"
            );
            continue;
        };
        let age_secs = now_secs.saturating_sub(pending_since);
        if age_secs <= cycle_bound_secs {
            continue;
        }
        findings.push(PatrolFinding {
            finding_type: "spec_pending_beyond_cycle".to_string(),
            severity: "warning".to_string(),
            spec_path: path.clone(),
            detail: format!(
                "Spec '{}' (effective requires_approval: true) has been pending for \
                 {}s, exceeding the {}s one-cycle bound",
                path, age_secs, cycle_bound_secs
            ),
            suggested_action: format!(
                "Review the pending spec '{}' — approve or reject it; a spec pending \
                 longer than one Ralph loop cycle means no judgment is being exercised \
                 on it",
                path
            ),
            task_id: None,
            workspace_id: entry
                .workspace_id
                .clone()
                .filter(|ws| !ws.trim().is_empty())
                .or_else(|| Some(ctx.workspace_id.clone())),
        });
    }
    Ok(())
}

/// Check 3 — modified specs with no corresponding task (defense in depth).
///
/// spec-registry.md §Accountability Agent (task-339): "Specs with
/// `auto_create_tasks: true` that were modified but have no corresponding
/// task." spec-lifecycle.md: "Specs that were modified but have no
/// corresponding task (should never happen if the hook works, but defense
/// in depth)."
///
/// Task-eligibility is keyed on the MANIFEST at the spec's repo HEAD — the
/// same key `should_create_lifecycle_task` uses — not on the repo
/// `SpecLifecycleConfig` watched prefixes: a manifest entry with
/// `auto_create_tasks: true` must be examined even when the repo's config
/// watches no prefixes, and a manifest entry with `auto_create_tasks: false`
/// must never be. Repos whose manifest cannot be resolved (no repo row, no
/// storage, no manifest at HEAD) fall back to the config's watched-prefix
/// rules — exactly the fallback the lifecycle hook itself applies to
/// manifest-less repos, so the patrol and the hook can never disagree
/// about which specs are supposed to have tasks.
///
/// "Has a corresponding task" = at least one non-`Cancelled` task whose
/// `spec_path` names this spec, resolved per spec via
/// [`TaskRepository::list_by_spec_path`](gyre_ports::TaskRepository) — the
/// contract's named mechanism. Both path spellings are queried because the
/// ledger stores manifest-relative paths (`system/x.md`, from
/// `spec_registry::sync_spec_ledger`) while the post-receive hook records the
/// repo-root git path (`specs/system/x.md`, `git_http::process_spec_lifecycle`).
/// When both sides know the repo, the task must belong to the ledger entry's
/// repo — otherwise an identically-named spec in another repo would mask the gap.
///
/// Deliberate exclusions (each is "not a modification awaiting a task"):
/// - repos with the lifecycle hook disabled (`enabled = false`) — skipped
///   entirely, mirroring `process_spec_lifecycle`'s early return;
/// - `Deprecated` entries — removal is handled by the hook's `spec-deprecated`
///   task at deprecation time, and that task closing would otherwise leave a
///   permanently un-actionable finding;
/// - entries with no `current_sha` — a manifest entry with no file at HEAD is
///   not a modified spec (the stale-entry check owns that finding);
/// - entries updated within the drift-review window — the hook runs in the same
///   tick as the push, so a just-written ledger row must not be flagged before
///   its task exists (same grace the check-1 threshold expresses).
async fn check_modified_specs_without_tasks(
    state: &AppState,
    ledger: &[gyre_domain::SpecLedgerEntry],
    ctx_by_repo: &HashMap<String, &RepoManifestContext>,
    now_secs: u64,
    grace_secs: u64,
    findings: &mut Vec<PatrolFinding>,
) -> anyhow::Result<()> {
    // Per-repo watched-path configuration, cached per patrol run — only the
    // fallback for repos whose manifest cannot be resolved (the same
    // fallback `process_spec_lifecycle` applies to manifest-less repos).
    let mut config_cache: HashMap<String, SpecLifecycleConfig> = HashMap::new();

    for entry in ledger {
        let path = normalize_spec_path(&entry.path);
        let config = repo_config(state, entry.repo_id.as_deref(), &mut config_cache).await?;
        if !config.enabled {
            // The lifecycle hook is configured off for this repo: missing
            // tasks are configured behavior, not an accountability gap.
            continue;
        }
        // Manifest-keyed eligibility (task-339), mirroring
        // `should_create_lifecycle_task`: the manifest at HEAD decides via
        // `effective_auto_create_tasks`; only manifest-less repos fall back
        // to the config's watched prefixes. A ledger path the current
        // manifest does not declare is never task-eligible (the hook would
        // not have created a task for it either).
        let task_eligible = match entry.repo_id.as_deref().and_then(|r| ctx_by_repo.get(r)) {
            Some(ctx) => crate::spec_registry::manifest_entry_for_path(&ctx.manifest, &path)
                .map(|e| e.effective_auto_create_tasks(&ctx.manifest.defaults))
                .unwrap_or(false),
            None => config.is_watched(&path),
        };
        if !task_eligible {
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

        // Contract mechanism: ask the task repository whether this spec has a
        // task. Both spellings are queried (see doc comment); `Cancelled`
        // counts as no coverage — cancellation means nothing is delivering it.
        let entry_repo = entry.repo_id.as_deref();
        let covered = spec_has_live_task(state, &path, entry_repo).await?;
        if covered {
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
    Ok(())
}

/// Does any non-`Cancelled` task name this spec (either path spelling)?
///
/// The repository answers with exact-`spec_path` rows; both the ledger's
/// manifest-relative spelling and the hook's git-rooted spelling are queried
/// so a task recorded either way covers the spec. A task with an empty
/// `repo_id` ("scope unknown") is a wildcard: those are exactly the tasks the
/// ledger's repo-scoped rows may fail to match by id alone.
async fn spec_has_live_task(
    state: &AppState,
    rooted_path: &str,
    entry_repo: Option<&str>,
) -> anyhow::Result<bool> {
    let alt = strip_specs_prefix(rooted_path);
    for query in [rooted_path, alt.as_str()] {
        for task in state.tasks.list_by_spec_path(query).await? {
            if task.status == TaskStatus::Cancelled {
                continue;
            }
            let task_repo = task.repo_id.as_str();
            if task_repo.is_empty() || entry_repo.is_none() || task_repo == entry_repo.unwrap_or("")
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Ledger spelling (`system/x.md`) of a rooted spec path
/// (`specs/system/x.md`), for querying tasks recorded the hook's way.
fn strip_specs_prefix(rooted_path: &str) -> String {
    rooted_path
        .strip_prefix("specs/")
        .map(|p| p.to_string())
        .unwrap_or_else(|| rooted_path.to_string())
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
/// Findings with no resolvable workspace are skipped and logged — never
/// broadcast. Under the scoped emit contract (`AppState::emit_event`) every
/// server-originated message resolves its tenant from the workspace record;
/// there is no scope-less emission path, and a fabricated `"default"`
/// workspace/tenant would re-target the escalation to whatever scope the
/// literal happens to collide with (the same skip-and-log contract as
/// `api/specs.rs`'s SpecApproved/deprecation emissions). The finding still
/// reaches the caller through the patrol response; only the bus emission is
/// skipped.
pub async fn escalate_findings(state: &AppState, findings: &[PatrolFinding]) -> usize {
    let mut escalated = 0usize;

    for finding in findings {
        let Some(workspace_id) = escalation_workspace(finding.workspace_id.as_deref()) else {
            warn!(
                finding_type = %finding.finding_type,
                spec_path = %finding.spec_path,
                "spec_lifecycle_patrol: finding has no resolvable workspace \
                 scope; skipping escalation (cannot resolve tenant scope)"
            );
            continue;
        };

        state
            .emit_event(
                workspace_id.clone(),
                Destination::Workspace(workspace_id),
                MessageKind::Escalation,
                Some(serde_json::json!({
                    // message-bus.md §Payload Schemas: `escalation` requires
                    // `reason` (string), `context` optional. The identifying
                    // fields are extension fields, which the schema lets pass
                    // through.
                    "reason": finding.detail,
                    "context": format!(
                        "spec-lifecycle accountability patrol finding \
                         (source: {ESCALATION_SOURCE}, severity: {})",
                        finding.severity
                    ),
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

/// The workspace an escalation for a finding is routed to.
///
/// A workspace id is only usable when it is present and non-blank. A missing
/// scope yields `None` — the caller skips the emission and logs: `emit_event`
/// resolves the tenant from the workspace record, and there is no valid
/// scope-less fallback (a literal `"default"` workspace would receive
/// escalations that belong to someone else).
fn escalation_workspace(workspace_id: Option<&str>) -> Option<Id> {
    workspace_id
        .map(str::trim)
        .filter(|ws| !ws.is_empty())
        .map(Id::new)
}

/// Does this task carry `label`?
fn has_label(task: &Task, label: &str) -> bool {
    task.labels.iter().any(|l| l == label)
}

/// The repo's spec-lifecycle config, or defaults when the ledger entry has no
/// repo scope.
///
/// Fails (rather than defaulting) when the config store itself errors: a
/// patrol that cannot read its inputs must not report "nothing wrong".
async fn repo_config(
    state: &AppState,
    repo_id: Option<&str>,
    cache: &mut HashMap<String, SpecLifecycleConfig>,
) -> anyhow::Result<SpecLifecycleConfig> {
    match repo_id {
        Some(id) if !id.is_empty() => match cache.get(id) {
            Some(c) => Ok(c.clone()),
            None => {
                let c = state.spec_lifecycle_configs.get_for_repo(id).await?;
                cache.insert(id.to_string(), c.clone());
                Ok(c)
            }
        },
        _ => Ok(SpecLifecycleConfig::default()),
    }
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

    const NOW: u64 = 1_000_000_000;
    const DRIFT_MAX: u64 = 86_400; // 24h
    const BACKLOG_MAX: u64 = 604_800; // 7d

    fn make_task(id: &str, labels: &[&str], status: TaskStatus, created_at: u64) -> Task {
        let mut task = Task::new(Id::new(id), format!("Task {id}"), created_at);
        task.labels = labels.iter().map(|l| l.to_string()).collect();
        task.status = status;
        task.priority = TaskPriority::High;
        task.workspace_id = Id::new("ws1");
        task.repo_id = Id::new("repo1");
        task.spec_path = Some("specs/system/agent-runtime.md".to_string());
        task
    }

    /// The escalation path resolves the tenant scope from the workspace
    /// record (`AppState::message_tenant_id`) and skips persisting when the
    /// workspace does not exist, so every test that asserts persisted
    /// escalation messages must seed the workspace it attributes findings
    /// to (mirrors `spec_patrol.rs` tests).
    fn make_workspace(id: &str) -> gyre_domain::Workspace {
        gyre_domain::Workspace {
            id: Id::new(id),
            tenant_id: Id::new("default"),
            name: format!("ws-{id}"),
            slug: format!("ws-{id}"),
            description: None,
            budget: None,
            max_repos: None,
            max_agents_per_repo: None,
            trust_level: gyre_domain::TrustLevel::Guided,
            llm_model: None,
            created_at: 1_000_000,
            compute_target_id: None,
        }
    }

    fn make_ledger_entry(
        path: &str,
        workspace_id: Option<&str>,
        updated_at: u64,
        status: ApprovalStatus,
    ) -> SpecLedgerEntry {
        SpecLedgerEntry {
            bindings_sha: String::new(),
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
        // Covered via the ledger spelling: the task's spec_path is the
        // manifest-relative form (`system/...`) rather than the hook's
        // git-rooted form. Coverage must be answered for either spelling —
        // a query that only tried `specs/system/...` would flag this spec.
        state
            .spec_ledger
            .save(&make_ledger_entry(
                "system/covered-ledger.md",
                Some("ws1"),
                old,
                ApprovalStatus::Approved,
            ))
            .await
            .unwrap();
        let mut ledger_spelled_task = make_task(
            "impl-covered-ledger",
            &["spec-implementation"],
            TaskStatus::Backlog,
            NOW - 10,
        );
        ledger_spelled_task.spec_path = Some("system/covered-ledger.md".to_string());
        state.tasks.create(&ledger_spelled_task).await.unwrap();
        // A task in a DIFFERENT repo naming this spec must not mask the gap:
        // repo1's spec stays uncovered even though repo2 has a task with the
        // same path.
        state
            .spec_ledger
            .save(&make_ledger_entry(
                "system/other-repo.md",
                Some("ws1"),
                old,
                ApprovalStatus::Approved,
            ))
            .await
            .unwrap();
        let mut other_repo_task = make_task(
            "impl-other-repo",
            &["spec-implementation"],
            TaskStatus::InProgress,
            NOW - 10,
        );
        other_repo_task.spec_path = Some("specs/system/other-repo.md".to_string());
        other_repo_task.repo_id = gyre_common::Id::new("repo2");
        state.tasks.create(&other_repo_task).await.unwrap();
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
        let mut orphaned: Vec<&str> = findings
            .iter()
            .filter(|f| f.finding_type == "spec_without_task")
            .map(|f| f.spec_path.as_str())
            .collect();
        orphaned.sort_unstable();

        assert_eq!(
            orphaned,
            vec!["specs/system/orphaned.md", "specs/system/other-repo.md"],
            "only the truly uncovered specs (repo-mismatched task does not mask): {findings:?}"
        );
    }

    /// All three checks at once, end to end: exactly the stale/orphaned items
    /// are flagged, each produces a persisted `escalation` message on the
    /// workspace stream, and nothing else does.
    #[tokio::test]
    async fn flags_and_escalates_only_the_accountability_gaps() {
        let state = test_state();
        // The escalation is persisted only when the workspace (and thereby
        // its tenant scope) can be resolved; seed it so the assert below
        // proves real emission rather than silent skip-on-unresolvable.
        state
            .workspaces
            .create(&make_workspace("ws1"))
            .await
            .unwrap();

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

        // The persisted payload must satisfy the message-bus contract for the
        // `escalation` kind (message-bus.md §Payload Schemas: `reason`
        // required) — the same validator the receipt paths run. A patrol that
        // emits schema-invalid escalations hands consumers a payload their
        // tooling rejects on send.
        for msg in &messages {
            MessageKind::Escalation
                .validate_payload(msg.payload.as_ref())
                .expect("escalation payload must satisfy the message-bus escalation schema");
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

    /// A finding with no resolvable workspace skips the bus escalation (and
    /// logs) instead of fabricating a scope: the scoped `emit_event` contract
    /// resolves the tenant from the workspace record, and there is no
    /// scope-less broadcast path (a fabricated `"default"` tenant would
    /// re-target the escalation). The finding itself still reaches the caller
    /// through the patrol response.
    #[tokio::test]
    async fn workspaceless_finding_skips_instead_of_fabricating_scope() {
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

        // Subscribed before the escalation: any emission — broadcast or
        // scoped — passes through this channel, so a quiet channel proves
        // nothing was emitted at all.
        let mut rx = state.message_broadcast_tx.subscribe();
        escalate_findings(&state, &findings).await;

        let leaked = tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv()).await;
        assert!(
            leaked.is_err(),
            "a workspaceless finding must not emit any message (fabricated-scope \
             broadcast included): {:?}",
            leaked.map(|m| m.map(|m| (m.kind, m.to)))
        );

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
        let ws = escalation_workspace(Some("ws7"));
        assert_eq!(ws.map(|i| i.to_string()), Some("ws7".to_string()));

        for blank in [None, Some(""), Some("   ")] {
            let ws = escalation_workspace(blank);
            assert!(
                ws.is_none(),
                "{blank:?} has no workspace — the escalation must skip, not route"
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
    }

    /// Check 3's watched-path filter is the repo's spec-lifecycle config
    /// (spec-lifecycle.md §Configuration), not a hardcoded list: with repo1
    /// configured to watch only `specs/milestones/`, the orphaned milestone
    /// spec is flagged while the default-watched `system/` orphan is not
    /// (under that config the hook would never create a task for it) — and a
    /// repo with the lifecycle hook disabled produces no findings at all
    /// (mirrors `process_spec_lifecycle`'s early return).
    #[tokio::test]
    async fn check3_watched_paths_follow_per_repo_config() {
        let state = test_state();
        let old = NOW - DRIFT_MAX - 1;

        // repo1 watches only specs/milestones/ (ignored_paths cleared — the
        // defaults ignore milestones, which would mask the watched-path
        // assertion).
        let cfg = SpecLifecycleConfig {
            watched_paths: vec!["specs/milestones/".to_string()],
            ignored_paths: vec![],
            ..SpecLifecycleConfig::default()
        };
        state
            .spec_lifecycle_configs
            .set_for_repo("repo1", cfg)
            .await
            .unwrap();
        // Orphaned milestone spec: watched under this repo's config.
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
        // Orphaned system spec: default-watched, but NOT watched by repo1's
        // config — flagging it would mean the filter is hardcoded.
        state
            .spec_ledger
            .save(&make_ledger_entry(
                "system/agent-runtime.md",
                Some("ws1"),
                old,
                ApprovalStatus::Approved,
            ))
            .await
            .unwrap();
        // repo2 has the lifecycle hook disabled: an orphaned, default-watched
        // spec there is configured behavior, not an accountability gap.
        state
            .spec_lifecycle_configs
            .set_for_repo(
                "repo2",
                SpecLifecycleConfig {
                    enabled: false,
                    ..SpecLifecycleConfig::default()
                },
            )
            .await
            .unwrap();
        let mut disabled_repo_entry = make_ledger_entry(
            "system/disabled-repo.md",
            Some("ws1"),
            old,
            ApprovalStatus::Approved,
        );
        disabled_repo_entry.repo_id = Some("repo2".to_string());
        state.spec_ledger.save(&disabled_repo_entry).await.unwrap();

        let findings = run(&state).await;
        let orphaned: Vec<&str> = findings
            .iter()
            .filter(|f| f.finding_type == "spec_without_task")
            .map(|f| f.spec_path.as_str())
            .collect();
        assert_eq!(
            orphaned,
            vec!["specs/milestones/m36.md"],
            "watched paths must come from the repo's config, not a hardcoded list: {findings:?}"
        );
    }

    // -----------------------------------------------------------------------
    // task-339: manifest-aware checks (orphans, stale entries, pending age,
    // manifest-keyed modified-without-task)
    // -----------------------------------------------------------------------

    /// Create a real git repo with the given files committed on `main`, and
    /// return the directory plus the HEAD SHA (the same fixture shape
    /// `spec_registry`'s tests use — the patrol reads real git storage).
    async fn make_git_repo(files: &[(&str, &str)]) -> (tempfile::TempDir, String) {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().to_str().unwrap();
        let run = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(path)
                .output()
                .expect("git command failed");
            assert!(
                out.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };

        run(&["init", "-b", "main"]);
        run(&["config", "user.email", "test@test.com"]);
        run(&["config", "user.name", "Test"]);
        for (file_path, content) in files {
            let full = dir.path().join(file_path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(&full, content).unwrap();
            run(&["add", file_path]);
        }
        run(&["commit", "-m", "initial"]);
        let sha = run(&["rev-parse", "HEAD"]);
        (dir, sha)
    }

    /// Register a repo row pointing at real on-disk git storage, so the
    /// patrol's manifest resolution reads the actual HEAD.
    async fn register_repo(state: &AppState, id: &str, ws_id: &str, path: &str) {
        let repo = gyre_domain::Repository::new(
            Id::new(id),
            Id::new(ws_id),
            format!("repo-{id}"),
            path.to_string(),
            1_000_000,
        );
        state.repos.create(&repo).await.unwrap();
    }

    /// AC 1 (task-339): an unregistered `specs/orphan.md` on disk and a
    /// manifest entry pointing at a missing `specs/ghost.md` — one patrol
    /// run returns BOTH findings and emits real escalations to the owning
    /// workspace orchestrator's event stream. `manifest.yaml` and
    /// `index.md` must never be flagged as orphans (the push-time scan's
    /// exclusions), and the registered spec with its file present produces
    /// nothing.
    #[tokio::test]
    async fn flags_orphan_files_and_stale_manifest_entries_with_escalations() {
        let state = test_state();
        state
            .workspaces
            .create(&make_workspace("ws1"))
            .await
            .unwrap();
        let (dir, _sha) = make_git_repo(&[
            (
                "specs/manifest.yaml",
                "version: 1\nspecs:\n  - path: system/real.md\n    title: Real\n    owner: user:test\n  - path: system/ghost.md\n    title: Ghost\n    owner: user:test\n",
            ),
            ("specs/system/real.md", "# Real\n"),
            ("specs/orphan.md", "# Orphan\n"),
            ("specs/index.md", "# Index\n"),
        ])
        .await;
        register_repo(&state, "repo1", "ws1", dir.path().to_str().unwrap()).await;

        let response = run_and_escalate(&state, NOW, DRIFT_MAX, BACKLOG_MAX)
            .await
            .expect("patrol should succeed");

        let mut flagged: Vec<String> = response
            .findings
            .iter()
            .map(|f| format!("{}:{}", f.finding_type, f.spec_path))
            .collect();
        flagged.sort();
        assert_eq!(
            flagged,
            vec![
                "orphan_spec_file:specs/orphan.md",
                "stale_manifest_entry:specs/system/ghost.md",
            ],
            "exactly the orphan file and the stale manifest entry: {findings:?}",
            findings = response.findings
        );

        // Real escalation rows on the orchestrator's stream, one per
        // finding, payload identifying each.
        let messages = state
            .messages
            .list_by_workspace(&Id::new("ws1"), Some("escalation"), None, None, None, None)
            .await
            .unwrap();
        assert_eq!(
            messages.len(),
            2,
            "both findings must escalate, not merely log: {messages:?}"
        );
        let payloads: Vec<String> = messages
            .iter()
            .map(|m| m.payload.as_ref().expect("escalation payload").to_string())
            .collect();
        for needle in [
            "orphan_spec_file",
            "specs/orphan.md",
            "stale_manifest_entry",
            "specs/system/ghost.md",
            ESCALATION_SOURCE,
        ] {
            assert!(
                payloads.iter().any(|p| p.contains(needle)),
                "escalation payload must identify {needle}: {payloads:?}"
            );
        }
        for msg in &messages {
            MessageKind::Escalation
                .validate_payload(msg.payload.as_ref())
                .expect("escalation payload must satisfy the message-bus escalation schema");
        }
    }

    /// AC 2 (task-339): a `requires_approval: true` spec pending longer than
    /// the cycle bound produces a pending-age finding; the SAME spec pending
    /// less than the bound does not. The age comes from the durable
    /// pending-since signal recorded by the ledger sync (a real push-path
    /// write), not from `updated_at` and not unconditionally — a
    /// `requires_approval: false` pending spec and a pending entry with no
    /// recorded signal are never flagged.
    #[tokio::test]
    async fn flags_pending_beyond_cycle_only_when_the_recorded_age_exceeds_it() {
        let state = test_state();
        state
            .workspaces
            .create(&make_workspace("ws1"))
            .await
            .unwrap();
        let (dir, sha) = make_git_repo(&[
            (
                "specs/manifest.yaml",
                "version: 1\nspecs:\n  - path: system/slow.md\n    title: Slow\n    owner: user:test\n    requires_approval: true\n    auto_create_tasks: false\n  - path: system/fast.md\n    title: Fast\n    owner: user:test\n    requires_approval: false\n    auto_create_tasks: false\n  - path: system/legacy.md\n    title: Legacy\n    owner: user:test\n    requires_approval: true\n    auto_create_tasks: false\n",
            ),
            ("specs/system/slow.md", "# Slow\n"),
            ("specs/system/fast.md", "# Fast\n"),
            ("specs/system/legacy.md", "# Legacy\n"),
        ])
        .await;
        register_repo(&state, "repo1", "ws1", dir.path().to_str().unwrap()).await;

        // Real ledger sync (the push path): registers all three specs as
        // Pending and records their pending-since at T0 in the durable KV
        // store.
        const T0: u64 = 1_700_000_000;
        crate::spec_registry::sync_spec_ledger(
            &state.spec_ledger,
            &state.spec_links_store,
            &state.spec_link_repo,
            dir.path().to_str().unwrap(),
            &sha,
            T0,
            Some("repo1"),
            Some("ws1"),
            None,
            None,
            None,
            None,
            None,
            Some(&state.kv_store),
            Some(&state.tasks),
        )
        .await;

        // A pending row older than the pending-since signal itself (written
        // before task-339): no recorded signal, so the age cannot be
        // evaluated — the entry must be skipped, not flagged via updated_at.
        crate::spec_pending_since::clear_pending(&state.kv_store, "system/legacy.md").await;

        // Fresh pending (minutes old, well under the bound): NO findings at
        // all — this is the probe's non-target state (c).
        let fresh = run_spec_lifecycle_patrol(&state, T0 + 300, DRIFT_MAX, BACKLOG_MAX)
            .await
            .unwrap();
        assert!(
            fresh.is_empty(),
            "a spec pending 300s must not be flagged (and nothing else may fire): {fresh:?}"
        );

        // Pending beyond the cycle bound: exactly slow.md (requires_approval
        // true, recorded pending-since past the bound). fast.md is pending
        // too but requires_approval: false; legacy.md has no recorded signal.
        let response = run_and_escalate(&state, T0 + DRIFT_MAX + 1, DRIFT_MAX, BACKLOG_MAX)
            .await
            .unwrap();
        let pending: Vec<&str> = response
            .findings
            .iter()
            .map(|f| f.finding_type.as_str())
            .collect();
        assert_eq!(
            pending,
            vec!["spec_pending_beyond_cycle"],
            "only the over-bound requires_approval spec: {findings:?}",
            findings = response.findings
        );
        assert_eq!(response.findings[0].spec_path, "specs/system/slow.md");

        // The finding escalates to the owning workspace orchestrator.
        let messages = state
            .messages
            .list_by_workspace(&Id::new("ws1"), Some("escalation"), None, None, None, None)
            .await
            .unwrap();
        assert_eq!(messages.len(), 1, "exactly one escalation: {messages:?}");
        let payload = messages[0]
            .payload
            .as_ref()
            .expect("escalation payload")
            .to_string();
        assert!(
            payload.contains("spec_pending_beyond_cycle")
                && payload.contains("specs/system/slow.md"),
            "escalation must identify the pending spec: {payload}"
        );
    }

    /// AC 3 (task-339): a spec with `auto_create_tasks: true` whose SHA
    /// changed but whose lifecycle task was deleted produces the
    /// modified-without-task finding EVEN when the repo's `[spec_lifecycle]`
    /// config watches no prefixes — the check is manifest-keyed, not
    /// config-keyed. A manifest entry with `auto_create_tasks: false` is
    /// never flagged.
    #[tokio::test]
    async fn modified_without_task_is_keyed_on_the_manifest_not_the_config() {
        let state = test_state();
        state
            .workspaces
            .create(&make_workspace("ws1"))
            .await
            .unwrap();
        let (dir, _sha) = make_git_repo(&[
            (
                "specs/manifest.yaml",
                "version: 1\nspecs:\n  - path: system/manifest-watched.md\n    title: A\n    owner: user:test\n  - path: system/opted-out.md\n    title: B\n    owner: user:test\n    auto_create_tasks: false\n",
            ),
            ("specs/system/manifest-watched.md", "# A\n"),
            ("specs/system/opted-out.md", "# B\n"),
        ])
        .await;
        register_repo(&state, "repo1", "ws1", dir.path().to_str().unwrap()).await;

        // The repo's [spec_lifecycle] config watches NO prefixes: under the
        // pre-task-339 config-prefix keying neither spec is ever examined.
        // (ignored_paths cleared so nothing masks the manifest key.)
        state
            .spec_lifecycle_configs
            .set_for_repo(
                "repo1",
                SpecLifecycleConfig {
                    watched_paths: vec![],
                    ignored_paths: vec![],
                    ..SpecLifecycleConfig::default()
                },
            )
            .await
            .unwrap();

        // Ledger rows for specs modified past the grace window whose
        // lifecycle tasks were deleted (no task references either path).
        let old = NOW - DRIFT_MAX - 1;
        state
            .spec_ledger
            .save(&make_ledger_entry(
                "system/manifest-watched.md",
                Some("ws1"),
                old,
                ApprovalStatus::Approved,
            ))
            .await
            .unwrap();
        state
            .spec_ledger
            .save(&make_ledger_entry(
                "system/opted-out.md",
                Some("ws1"),
                old,
                ApprovalStatus::Approved,
            ))
            .await
            .unwrap();

        let response = run_and_escalate(&state, NOW, DRIFT_MAX, BACKLOG_MAX)
            .await
            .unwrap();
        let flagged: Vec<&str> = response
            .findings
            .iter()
            .filter(|f| f.finding_type == "spec_without_task")
            .map(|f| f.spec_path.as_str())
            .collect();
        assert_eq!(
            flagged,
            vec!["specs/system/manifest-watched.md"],
            "manifest auto_create_tasks must decide, not the config's watched \
             prefixes: {findings:?}",
            findings = response.findings
        );

        // The manifest-keyed finding escalates like every other.
        let messages = state
            .messages
            .list_by_workspace(&Id::new("ws1"), Some("escalation"), None, None, None, None)
            .await
            .unwrap();
        assert_eq!(messages.len(), 1, "exactly one escalation: {messages:?}");
    }
}
