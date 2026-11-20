//! PR hygiene background job (specs/development/agent-workflow.md §PRs as
//! the Collaboration Primitive).
//!
//! "A PR hygiene agent (or background job) continuously monitors for stale
//! PRs — unreviewed, unmerged, conflicted, or abandoned — and takes action:
//! nudge reviewers, rebase, escalate, or close with explanation."
//!
//! Registered as `pr_hygiene_check` in jobs.rs on a minutes-scale interval
//! (default 5 minutes, `GYRE_PR_HYGIENE_INTERVAL_SECS`) — continuously, not
//! the daily cadence of `abandoned_branch_check` (HSI §8, specced
//! separately and left intact: it covers `spec-edit/*` branches only and
//! only notifies).
//!
//! Every evaluation walks each workspace's open (unmerged) MRs against the
//! stale classes using persisted state — `MergeRequest.updated_at`,
//! `reviewers`, `has_conflicts` plus a real merge-base conflict check
//! (`GitOpsPort::can_merge`, an in-memory `git merge` of the two tips), and
//! review records — and takes exactly one recorded action per MR, most
//! severe class first:
//!
//! - **abandoned** (`updated_at` older than the hard-close threshold):
//!   close the MR — the status transition is persisted, and the
//!   explanation is delivered to the author as an MR comment plus an
//!   `MrAutoClosed` notification / agent message. Never a silent close.
//! - **conflicted** (`has_conflicts` or the merge-base check reports
//!   conflicts): trigger the existing jj rebase machinery when the author
//!   is an active agent with a live worktree in the repo (same safeguards
//!   and shared backoff window as the merge processor's §4 rebase);
//!   otherwise raise a `MrNeedsRebase` escalation to workspace
//!   Developers/Admins. Either way a real action is recorded (jj rebase
//!   audit event or notification rows), never silence.
//! - **long-open** (`updated_at` older than the escalation threshold): a
//!   higher-priority `MrStaleEscalation` notification to workspace
//!   Admins/Owners.
//! - **unreviewed** (zero review records, older than the nudge threshold):
//!   a `ReviewerNudge` notification to the assigned reviewers; when the MR
//!   has no reviewers at all, an `MrUnreviewed` escalation to workspace
//!   Developers/Admins asking for one to be assigned.
//!
//! Thresholds are configurable via environment variables with sane
//! defaults (see [`HygieneThresholds`]). All notification actions dedupe
//! through the notification store's exact (workspace, user, type,
//! entity_ref) active-notification lookup — the same mechanism as
//! `abandoned_branch_check` — and agent-reviewer bus emissions dedupe
//! through a KV backoff keyed (MR, agent), so consecutive runs are
//! idempotent, and every action is observable (notification rows, audit
//! events, persisted MR state).

use std::sync::Arc;

use gyre_common::{Id, Notification, NotificationType};
use gyre_domain::{AuditEventType, AuditOutcome, MrStatus, WorkspaceRole};
use tracing::{debug, info, warn};

use crate::AppState;

/// Job name in the registry (jobs.rs).
pub const JOB_NAME: &str = "pr_hygiene_check";

/// KV namespace shared with the merge processor's automatic rebase
/// (merge_processor.rs): key = agent id, value = epoch seconds of the last
/// completed rebase. Reusing the namespace means the hygiene loop and the
/// merge processor honor each other's rate-limit window instead of
/// double-rebasing the same workspace inside 60s.
const JJ_REBASE_BACKOFF_NS: &str = "jj_rebase_backoff";
const JJ_REBASE_WINDOW_SECS: u64 = 60;

/// KV namespace deduplicating agent-reviewer nudges: key =
/// `{mr_id}:{agent_id}`, value = epoch seconds of the last nudge emitted
/// to that agent for that MR. Agent reviewers have no notification-inbox
/// row to dedupe against (the user path dedupes through
/// `has_active_notification`), so the message-bus emission is rate-limited
/// here instead — without it, the job's short cadence would re-emit a
/// duplicate `review_nudge` on every evaluation. The window is the nudge
/// threshold: after it elapses since the last nudge the agent may be
/// nudged again, mirroring the user path's re-notify-after-dismissal
/// semantics.
const NUDGE_BACKOFF_NS: &str = "pr_hygiene_nudge_backoff";

/// Stale-class thresholds. Defaults: nudge after 2 days without a review,
/// escalate to Admins after 7 days of inactivity, auto-close after 30
/// days. Configurable per deployment via environment variables (server
/// settings with sane defaults):
///
/// - `GYRE_PR_HYGIENE_NUDGE_SECS`
/// - `GYRE_PR_HYGIENE_ESCALATE_SECS`
/// - `GYRE_PR_HYGIENE_CLOSE_SECS`
#[derive(Debug, Clone, Copy)]
pub struct HygieneThresholds {
    /// An open MR with zero reviews older than this gets a reviewer nudge.
    pub nudge_after_secs: u64,
    /// An open MR inactive longer than this is escalated to Admins.
    pub escalate_after_secs: u64,
    /// An open MR inactive longer than this is closed with an explanation.
    pub close_after_secs: u64,
}

impl Default for HygieneThresholds {
    fn default() -> Self {
        Self {
            nudge_after_secs: 2 * 24 * 3600,
            escalate_after_secs: 7 * 24 * 3600,
            close_after_secs: 30 * 24 * 3600,
        }
    }
}

impl HygieneThresholds {
    /// Read the deployment's configured thresholds. Unparseable or missing
    /// values fall back to the defaults — never to zero, which would make
    /// every fresh MR stale on its first evaluation.
    pub fn from_env() -> Self {
        let read = |name: &str, default: u64| -> u64 {
            std::env::var(name)
                .ok()
                .and_then(|v| v.trim().parse::<u64>().ok())
                .filter(|v| *v > 0)
                .unwrap_or(default)
        };
        let d = Self::default();
        Self {
            nudge_after_secs: read("GYRE_PR_HYGIENE_NUDGE_SECS", d.nudge_after_secs),
            escalate_after_secs: read("GYRE_PR_HYGIENE_ESCALATE_SECS", d.escalate_after_secs),
            close_after_secs: read("GYRE_PR_HYGIENE_CLOSE_SECS", d.close_after_secs),
        }
    }
}

/// Scheduler interval for the job (seconds). Default 5 minutes —
/// minutes-scale "continuous" monitoring, unlike the daily
/// `abandoned_branch_check`. Configurable via
/// `GYRE_PR_HYGIENE_INTERVAL_SECS`.
pub fn interval_secs() -> u64 {
    std::env::var("GYRE_PR_HYGIENE_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(300)
}

/// Run one pass of the PR hygiene check across all workspaces with the
/// deployment's configured thresholds (the scheduler entry point).
pub async fn run_once(state: &Arc<AppState>) -> anyhow::Result<()> {
    let thresholds = HygieneThresholds::from_env();
    run_with(state, &thresholds).await
}

/// Run one pass with explicit thresholds (the testable core).
pub async fn run_with(state: &Arc<AppState>, thresholds: &HygieneThresholds) -> anyhow::Result<()> {
    let workspaces = state.workspaces.list().await?;
    debug!(
        count = workspaces.len(),
        "pr_hygiene_check: evaluating workspaces"
    );

    for ws in &workspaces {
        if let Err(e) = evaluate_workspace(state, ws, thresholds).await {
            warn!(workspace_id = %ws.id, error = %e, "pr_hygiene_check: workspace evaluation failed");
        }
    }
    Ok(())
}

/// Evaluate a single workspace's open MRs against the stale classes.
async fn evaluate_workspace(
    state: &Arc<AppState>,
    ws: &gyre_domain::Workspace,
    thresholds: &HygieneThresholds,
) -> anyhow::Result<()> {
    let now = crate::jobs::now_secs();

    let mrs = state.merge_requests.list_by_workspace(&ws.id).await?;
    // "Open" = unmerged and not terminal: Open and Approved MRs are both
    // awaiting merge and can go stale.
    let open: Vec<_> = mrs
        .iter()
        .filter(|mr| matches!(mr.status, MrStatus::Open | MrStatus::Approved))
        .cloned()
        .collect();

    if open.is_empty() {
        return Ok(());
    }
    debug!(workspace = %ws.slug, open = open.len(), "pr_hygiene_check: evaluating open MRs");

    for mr in open {
        let age = now.saturating_sub(mr.updated_at);

        // Most severe class first; one action per MR per evaluation keeps
        // the job's effect on a workspace legible.
        if age >= thresholds.close_after_secs {
            close_abandoned(state, ws, &mr, thresholds, now).await;
            continue;
        }
        if is_conflicted(state, &mr).await {
            act_on_conflict(state, ws, &mr, now).await;
            continue;
        }
        if age >= thresholds.escalate_after_secs {
            escalate_stale(state, ws, &mr, age, now).await;
            continue;
        }
        if age >= thresholds.nudge_after_secs {
            let review_count = state
                .reviews
                .list_reviews(&mr.id)
                .await
                .map(|rs| rs.len())
                .unwrap_or(0);
            if review_count == 0 {
                nudge_unreviewed(state, ws, &mr, thresholds, now).await;
            }
        }
    }
    Ok(())
}

/// Conflicted = the MR's persisted conflict flag, or a real merge-base
/// conflict check against the repo's git store. A `can_merge`
/// infrastructure error (repo missing, ref unresolvable) is NOT a conflict
/// — it is unknown state, and treating it as conflicted would spam
/// rebase-required escalations for every repo blip.
async fn is_conflicted(state: &Arc<AppState>, mr: &gyre_domain::MergeRequest) -> bool {
    if mr.has_conflicts == Some(true) {
        return true;
    }
    let Ok(Some(repo)) = state.repos.find_by_id(&mr.repository_id).await else {
        return false;
    };
    matches!(
        state
            .git_ops
            .can_merge(&repo.path, &mr.source_branch, &mr.target_branch)
            .await,
        Ok(false)
    )
}

// ── abandoned → close with explanation ───────────────────────────────────────

/// Close an abandoned MR: persist the status transition, deliver the
/// explanation to the author (MR comment + notification / agent message),
/// and mirror the close endpoint's trace lifecycle (close-without-merge
/// deletes the gate trace). Never a silent close.
async fn close_abandoned(
    state: &Arc<AppState>,
    ws: &gyre_domain::Workspace,
    mr: &gyre_domain::MergeRequest,
    thresholds: &HygieneThresholds,
    now: u64,
) {
    let days = thresholds.close_after_secs / 86_400;
    let explanation = format!(
        "PR hygiene: merge request '{}' has had no activity for more than {} days. \
         It has been closed automatically. If this work is still relevant, \
         reopen it or push an updated branch and open a new merge request.",
        mr.title, days
    );

    let mut mr = mr.clone();
    if let Err(e) = mr.transition_status(MrStatus::Closed) {
        warn!(mr_id = %mr.id, error = %e, "pr_hygiene_check: invalid close transition, skipping");
        return;
    }
    mr.updated_at = now;
    if let Err(e) = state.merge_requests.update(&mr).await {
        warn!(mr_id = %mr.id, error = %e, "pr_hygiene_check: failed to persist MR close");
        return;
    }

    // Close-without-merge deletes the gate trace (same lifecycle as the
    // PUT /status close endpoint).
    if let Err(e) = state.traces.delete_by_mr(&mr.id).await {
        warn!(mr_id = %mr.id, error = %e, "pr_hygiene_check: failed to delete gate trace on close");
    }

    // Explanatory comment on the MR — the durable record of why it closed.
    let comment = gyre_domain::ReviewComment::new(
        Id::new(uuid::Uuid::new_v4().to_string()),
        mr.id.clone(),
        "system",
        explanation.clone(),
        now,
    );
    if let Err(e) = state.reviews.add_comment(&comment).await {
        warn!(mr_id = %mr.id, error = %e, "pr_hygiene_check: failed to record close explanation comment");
    }

    // Status-change event on the workspace bus.
    state
        .emit_event(
            ws.id.clone(),
            gyre_common::message::Destination::Workspace(ws.id.clone()),
            gyre_common::message::MessageKind::MrStatusChanged,
            Some(serde_json::json!({
                "mr_id": mr.id.to_string(),
                "status": "closed",
                "reason": "pr_hygiene_abandoned",
            })),
        )
        .await;

    // Deliver the explanation to the author: an agent author gets a
    // message in its inbox; a user author gets an MrAutoClosed
    // notification. Workspace Admins/Owners observe every auto-close.
    let notice = MrNotice::new(
        NotificationType::MrAutoClosed,
        "Merge request auto-closed",
        &explanation,
    );
    deliver_author_message_or_notification(state, ws, &mr, &notice, now).await;
    notify_members(
        state,
        ws,
        &mr,
        &notice,
        &[WorkspaceRole::Admin, WorkspaceRole::Owner],
        now,
    )
    .await;

    record_audit(
        state,
        ws,
        &mr,
        "close_abandoned",
        serde_json::json!({
            "closed_at": now,
            "inactive_days": days,
            "explanation": explanation,
        }),
        AuditOutcome::Success,
    )
    .await;
    info!(mr_id = %mr.id, workspace = %ws.slug, "pr_hygiene_check: closed abandoned MR");
}

// ── conflicted → rebase attempt or rebase-required escalation ────────────────

/// A conflicted MR's rebase target: the author agent when it is Active and
/// has a live worktree in the MR's repo (same safeguards as the merge
/// processor's `find_rebase_targets`: dead agents and cleaned-up worktrees
/// have no in-progress work to rebase, and a worktree in a different repo
/// must never be rebased for this repo's MR).
struct RebaseTarget {
    agent_id: Id,
    worktree_path: String,
}

async fn find_rebase_target(
    state: &Arc<AppState>,
    mr: &gyre_domain::MergeRequest,
) -> Option<RebaseTarget> {
    let author_id = mr.author_agent_id.as_ref()?;
    let Ok(Some(agent)) = state.agents.find_by_id(author_id).await else {
        return None;
    };
    if agent.status != gyre_domain::AgentStatus::Active {
        return None;
    }
    let Ok(worktrees) = state.worktrees.find_by_agent(&agent.id).await else {
        return None;
    };
    let wt = worktrees
        .iter()
        .find(|wt| wt.repository_id == mr.repository_id && wt.branch == mr.source_branch)
        .or_else(|| {
            worktrees
                .iter()
                .find(|wt| wt.repository_id == mr.repository_id)
        })?;
    if !std::path::Path::new(&wt.path).exists() {
        return None;
    }
    Some(RebaseTarget {
        agent_id: agent.id.clone(),
        worktree_path: wt.path.clone(),
    })
}

/// True while the shared rebase backoff window is active for the agent.
async fn in_rebase_backoff(state: &Arc<AppState>, agent_id: &Id) -> bool {
    match state
        .kv_store
        .kv_get(JJ_REBASE_BACKOFF_NS, agent_id.as_str())
        .await
    {
        Ok(Some(v)) => match v.parse::<u64>() {
            Ok(last) => crate::jobs::now_secs().saturating_sub(last) < JJ_REBASE_WINDOW_SECS,
            Err(_) => false,
        },
        Ok(None) => false,
        Err(e) => {
            warn!(agent_id = %agent_id, error = %e, "pr_hygiene_check: cannot read rebase backoff, proceeding");
            false
        }
    }
}

async fn persist_rebase_backoff(state: &Arc<AppState>, agent_id: &Id) {
    let now = crate::jobs::now_secs();
    if let Err(e) = state
        .kv_store
        .kv_set(JJ_REBASE_BACKOFF_NS, agent_id.as_str(), now.to_string())
        .await
    {
        warn!(agent_id = %agent_id, error = %e, "pr_hygiene_check: failed to persist rebase backoff");
    }
}

/// KV key scoping one agent's review nudge for one MR.
fn nudge_backoff_key(mr: &gyre_domain::MergeRequest, agent_id: &Id) -> String {
    format!("{}:{}", mr.id.as_str(), agent_id.as_str())
}

/// True while the nudge backoff window is active for the (MR, agent)
/// pair — at most one `review_nudge` emission per window per pair. A KV
/// read failure fails open (the nudge proceeds): the backoff is a
/// deduplication rate limit, not an enforcement gate, and dropping the
/// nudge entirely on an infra blip would leave a stale MR silent — the
/// exact failure class this job exists to prevent.
async fn in_nudge_backoff(state: &Arc<AppState>, key: &str, window_secs: u64, now: u64) -> bool {
    match state.kv_store.kv_get(NUDGE_BACKOFF_NS, key).await {
        Ok(Some(v)) => match v.parse::<u64>() {
            Ok(last) => now.saturating_sub(last) < window_secs,
            Err(_) => false,
        },
        Ok(None) => false,
        Err(e) => {
            warn!(key = %key, error = %e, "pr_hygiene_check: cannot read nudge backoff, proceeding");
            false
        }
    }
}

async fn persist_nudge_backoff(state: &Arc<AppState>, key: &str, now: u64) {
    if let Err(e) = state
        .kv_store
        .kv_set(NUDGE_BACKOFF_NS, key, now.to_string())
        .await
    {
        warn!(key = %key, error = %e, "pr_hygiene_check: failed to persist nudge backoff");
    }
}

/// Act on a conflicted MR: attempt the rebase via the existing jj
/// machinery when applicable (agent-authored branch with a live worktree),
/// otherwise (or when the rebase itself conflicts) raise a rebase-required
/// escalation. Every path records something — a jj rebase audit event, a
/// notification row — never silence.
async fn act_on_conflict(
    state: &Arc<AppState>,
    ws: &gyre_domain::Workspace,
    mr: &gyre_domain::MergeRequest,
    now: u64,
) {
    if let Some(target) = find_rebase_target(state, mr).await {
        if !in_rebase_backoff(state, &target.agent_id).await {
            // The existing rebase machinery (source-control.md §4): rebase
            // the whole in-flight stack onto the MR's target branch.
            match state
                .jj_ops
                .jj_rebase(&target.worktree_path, "@", &mr.target_branch)
                .await
            {
                Ok(gyre_ports::JjRebaseOutcome::Success { rebased_count }) => {
                    persist_rebase_backoff(state, &target.agent_id).await;
                    record_rebase_audit(state, ws, mr, &target, "success", rebased_count, &[])
                        .await;
                    // A clean rebase clears the persisted conflict flag
                    // (same state repair as the merge processor).
                    if mr.has_conflicts == Some(true) {
                        let mut m = mr.clone();
                        m.has_conflicts = Some(false);
                        m.updated_at = now;
                        let _ = state.merge_requests.update(&m).await;
                    }
                    state
                        .emit_event(
                            ws.id.clone(),
                            gyre_common::message::Destination::Agent(target.agent_id.clone()),
                            gyre_common::message::MessageKind::Custom("baseline_moved".to_string()),
                            Some(serde_json::json!({
                                "repo_id": mr.repository_id.to_string(),
                                "branch": mr.source_branch,
                                "target_branch": mr.target_branch,
                                "rebased_count": rebased_count,
                                "source": "pr_hygiene",
                            })),
                        )
                        .await;
                    info!(mr_id = %mr.id, "pr_hygiene_check: rebased conflicted MR branch");
                    return;
                }
                Ok(gyre_ports::JjRebaseOutcome::Conflict {
                    rebased_count,
                    files,
                }) => {
                    persist_rebase_backoff(state, &target.agent_id).await;
                    record_rebase_audit(state, ws, mr, &target, "conflict", rebased_count, &files)
                        .await;
                    // Persist the conflict flag so the state survives
                    // restarts and feeds the next evaluation.
                    let mut m = mr.clone();
                    m.has_conflicts = Some(true);
                    let _ = state.merge_requests.update(&m).await;
                    // The rebase could not complete: escalate for human
                    // resolution and fall through to the notification below.
                }
                Err(e) => {
                    warn!(mr_id = %mr.id, error = %e, "pr_hygiene_check: rebase attempt failed");
                    record_rebase_audit(state, ws, mr, &target, "error", 0, &[]).await;
                }
            }
        } else {
            debug!(mr_id = %mr.id, "pr_hygiene_check: rebase deferred (backoff window)");
            return;
        }
    }

    // Rebase not applicable (or still conflicted after the attempt):
    // rebase-required escalation to workspace Developers/Admins.
    let body = format!(
        "PR hygiene: merge request '{}' ({}) conflicts with its target branch '{}' \
         and could not be rebased automatically. A manual rebase onto the target \
         branch is required before it can merge.",
        mr.title, mr.source_branch, mr.target_branch
    );
    let notice = MrNotice::new(
        NotificationType::MrNeedsRebase,
        "Merge request needs a rebase",
        &body,
    );
    notify_members(
        state,
        ws,
        mr,
        &notice,
        &[WorkspaceRole::Developer, WorkspaceRole::Admin],
        now,
    )
    .await;
    record_audit(
        state,
        ws,
        mr,
        "rebase_required",
        serde_json::json!({
            "source_branch": mr.source_branch,
            "target_branch": mr.target_branch,
        }),
        AuditOutcome::Failure,
    )
    .await;
}

#[allow(clippy::too_many_arguments)]
async fn record_rebase_audit(
    state: &Arc<AppState>,
    ws: &gyre_domain::Workspace,
    mr: &gyre_domain::MergeRequest,
    target: &RebaseTarget,
    outcome: &str,
    rebased_count: usize,
    files: &[String],
) {
    let event = gyre_domain::AuditEvent::new(
        Id::new(uuid::Uuid::new_v4().to_string()),
        AuditEventType::Custom("jj_rebase".to_string()),
        Some(target.agent_id.clone()),
        None,
        None,
        Some(ws.id.clone()),
        Some(mr.repository_id.clone()),
        "merge_queue".to_string(),
        Some(mr.source_branch.clone()),
        if outcome == "success" {
            AuditOutcome::Success
        } else {
            AuditOutcome::Failure
        },
        serde_json::json!({
            "outcome": outcome,
            "rebased_count": rebased_count,
            "conflicted_files": files,
            "target_branch": mr.target_branch,
            "worktree": target.worktree_path,
            "source": "pr_hygiene",
        }),
        None,
        None,
        crate::jobs::now_secs(),
    );
    if let Err(e) = state.audit.record(&event).await {
        warn!(mr_id = %mr.id, error = %e, "pr_hygiene_check: failed to record rebase audit event");
    }
}

// ── long-open → escalation to Admins ─────────────────────────────────────────

async fn escalate_stale(
    state: &Arc<AppState>,
    ws: &gyre_domain::Workspace,
    mr: &gyre_domain::MergeRequest,
    age_secs: u64,
    now: u64,
) {
    let days = age_secs / 86_400;
    let body = format!(
        "PR hygiene: merge request '{}' has been open and inactive for {} days. \
         Long-open PRs are a signal something is wrong — review, rebase, or close it.",
        mr.title, days
    );
    let notice = MrNotice::new(
        NotificationType::MrStaleEscalation,
        "Merge request open too long",
        &body,
    );
    notify_members(
        state,
        ws,
        mr,
        &notice,
        &[WorkspaceRole::Admin, WorkspaceRole::Owner],
        now,
    )
    .await;
    record_audit(
        state,
        ws,
        mr,
        "escalate_stale",
        serde_json::json!({ "inactive_days": days }),
        AuditOutcome::Success,
    )
    .await;
}

// ── unreviewed → reviewer nudge / needs-reviewers escalation ─────────────────

async fn nudge_unreviewed(
    state: &Arc<AppState>,
    ws: &gyre_domain::Workspace,
    mr: &gyre_domain::MergeRequest,
    thresholds: &HygieneThresholds,
    now: u64,
) {
    if mr.reviewers.is_empty() {
        // No reviewers assigned: escalate to workspace Developers/Admins to
        // assign one.
        let body = format!(
            "PR hygiene: merge request '{}' has been waiting for a review with no \
             reviewers assigned. Assign a reviewer so it can proceed.",
            mr.title
        );
        let notice = MrNotice::new(
            NotificationType::MrUnreviewed,
            "Merge request needs reviewers",
            &body,
        );
        notify_members(
            state,
            ws,
            mr,
            &notice,
            &[WorkspaceRole::Developer, WorkspaceRole::Admin],
            now,
        )
        .await;
        record_audit(
            state,
            ws,
            mr,
            "escalate_unreviewed",
            serde_json::json!({ "reviewers": 0 }),
            AuditOutcome::Success,
        )
        .await;
        return;
    }

    let body = format!(
        "PR hygiene: merge request '{}' is waiting for your review.",
        mr.title
    );
    let mut nudged = 0;
    for reviewer in &mr.reviewers {
        // A reviewer may be a user (notification inbox) or an agent
        // (message bus) — both are real deliveries, recorded below via
        // create_notification / emit_event.
        if let Ok(Some(_)) = state.users.find_by_id(reviewer).await {
            let notice = MrNotice::new(NotificationType::ReviewerNudge, "Review requested", &body);
            if create_notification(state, ws, reviewer, mr, &notice, now).await {
                nudged += 1;
            }
        } else if state
            .agents
            .find_by_id(reviewer)
            .await
            .ok()
            .flatten()
            .is_some()
        {
            // Agent reviewer: no notification inbox row, so the bus
            // emission is deduplicated via the KV nudge backoff — the
            // same idempotence guarantee the user path gets from
            // `has_active_notification`. Without it, every evaluation
            // would emit a duplicate `review_nudge` to the agent.
            let backoff_key = nudge_backoff_key(mr, reviewer);
            if in_nudge_backoff(state, &backoff_key, thresholds.nudge_after_secs, now).await {
                debug!(
                    mr = %mr.id,
                    agent = %reviewer,
                    "pr_hygiene_check: suppressed agent review_nudge (nudge backoff active)"
                );
                continue;
            }
            state
                .emit_event(
                    ws.id.clone(),
                    gyre_common::message::Destination::Agent(reviewer.clone()),
                    gyre_common::message::MessageKind::Custom("review_nudge".to_string()),
                    Some(serde_json::json!({
                        "mr_id": mr.id.to_string(),
                        "repo_id": mr.repository_id.to_string(),
                        "title": mr.title,
                    })),
                )
                .await;
            persist_nudge_backoff(state, &backoff_key, now).await;
            nudged += 1;
        } else {
            warn!(mr_id = %mr.id, reviewer = %reviewer, "pr_hygiene_check: reviewer resolves to neither user nor agent");
        }
    }
    if nudged > 0 {
        record_audit(
            state,
            ws,
            mr,
            "nudge_reviewers",
            serde_json::json!({ "nudged": nudged }),
            AuditOutcome::Success,
        )
        .await;
    }
}

// ── shared helpers ───────────────────────────────────────────────────────────

/// One hygiene delivery payload: the (notification type, title, body)
/// triple shared by every delivery path. Bundling it keeps the delivery
/// helpers within clippy's argument budget without suppressing the lint.
struct MrNotice {
    notification_type: NotificationType,
    title: String,
    body: String,
}

impl MrNotice {
    fn new(notification_type: NotificationType, title: &str, body: &str) -> Self {
        Self {
            notification_type,
            title: title.to_string(),
            body: body.to_string(),
        }
    }
}

/// Create a deduplicated notification for one user about one MR. Returns
/// false when suppressed (an active notification of this type for this
/// (user, MR) pair already exists, or the user dismissed one recently).
async fn create_notification(
    state: &Arc<AppState>,
    ws: &gyre_domain::Workspace,
    user_id: &Id,
    mr: &gyre_domain::MergeRequest,
    notice: &MrNotice,
    now: u64,
) -> bool {
    let mr_id_str = mr.id.to_string();
    if state
        .notifications
        .has_active_notification(
            &ws.id,
            user_id,
            notice.notification_type.as_str(),
            &mr_id_str,
            0,
        )
        .await
        .unwrap_or(false)
    {
        debug!(mr = %mr.id, user = %user_id, "pr_hygiene_check: suppressed (active notification exists)");
        return false;
    }

    let notif = Notification::new(
        Id::new(uuid::Uuid::new_v4().to_string()),
        ws.id.clone(),
        user_id.clone(),
        notice.notification_type.clone(),
        notice.title.clone(),
        ws.tenant_id.to_string(),
        now as i64,
    );
    let mut notif = notif;
    notif.body = Some(notice.body.clone());
    notif.source.entity_type = "mr".to_string();
    notif.source.entity_id = mr_id_str;
    notif.source.repo_id = Some(mr.repository_id.clone());

    if let Err(e) = state.create_notification(&notif).await {
        warn!(mr = %mr.id, user = %user_id, error = %e, "pr_hygiene_check: failed to create notification");
        return false;
    }
    // Channel fan-out per user-management.md §Delivery Channels.
    crate::notification_dispatcher::dispatch_to_channels(state, &notif).await;
    true
}

/// Notify every member of the workspace holding one of `roles` about the
/// MR (deduplicated per user).
async fn notify_members(
    state: &Arc<AppState>,
    ws: &gyre_domain::Workspace,
    mr: &gyre_domain::MergeRequest,
    notice: &MrNotice,
    roles: &[WorkspaceRole],
    now: u64,
) {
    let Ok(members) = state.workspace_memberships.list_by_workspace(&ws.id).await else {
        warn!(workspace = %ws.slug, "pr_hygiene_check: cannot list memberships");
        return;
    };
    let targets: Vec<_> = members.iter().filter(|m| roles.contains(&m.role)).collect();
    if targets.is_empty() {
        debug!(workspace = %ws.slug, "pr_hygiene_check: no members with role {:?} to notify", roles);
    }
    for member in targets {
        create_notification(state, ws, &member.user_id, mr, notice, now).await;
    }
}

/// Deliver a message to an agent author, or a notification to a user
/// author. MRs whose author resolves to neither still get the Admin
/// notification from the caller.
async fn deliver_author_message_or_notification(
    state: &Arc<AppState>,
    ws: &gyre_domain::Workspace,
    mr: &gyre_domain::MergeRequest,
    notice: &MrNotice,
    now: u64,
) {
    let Some(author_id) = mr.author_agent_id.clone() else {
        return;
    };
    if state
        .agents
        .find_by_id(&author_id)
        .await
        .ok()
        .flatten()
        .is_some()
    {
        state
            .emit_event(
                ws.id.clone(),
                gyre_common::message::Destination::Agent(author_id.clone()),
                gyre_common::message::MessageKind::Custom("mr_auto_closed".to_string()),
                Some(serde_json::json!({
                    "mr_id": mr.id.to_string(),
                    "repo_id": mr.repository_id.to_string(),
                    "title": mr.title,
                    "explanation": notice.body,
                })),
            )
            .await;
    } else if state
        .users
        .find_by_id(&author_id)
        .await
        .ok()
        .flatten()
        .is_some()
    {
        create_notification(state, ws, &author_id, mr, notice, now).await;
    }
}

/// Record one `pr_hygiene` audit event per action (observability.md: every
/// hygiene action is queryable in the audit trail).
async fn record_audit(
    state: &Arc<AppState>,
    ws: &gyre_domain::Workspace,
    mr: &gyre_domain::MergeRequest,
    action: &str,
    detail: serde_json::Value,
    outcome: AuditOutcome,
) {
    let event = gyre_domain::AuditEvent::new(
        Id::new(uuid::Uuid::new_v4().to_string()),
        AuditEventType::Custom("pr_hygiene".to_string()),
        None,
        None,
        None,
        Some(ws.id.clone()),
        Some(mr.repository_id.clone()),
        "merge_request".to_string(),
        Some(mr.id.to_string()),
        outcome,
        serde_json::json!({
            "action": action,
            "mr_id": mr.id.to_string(),
            "source_branch": mr.source_branch,
            "target_branch": mr.target_branch,
            "detail": detail,
        }),
        None,
        None,
        crate::jobs::now_secs(),
    );
    if let Err(e) = state.audit.record(&event).await {
        warn!(mr_id = %mr.id, error = %e, "pr_hygiene_check: failed to record audit event");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::{test_state, test_state_with_git_ops, ConfigurableGitOps, ConfigurableJjOps};
    use gyre_domain::{
        Agent, AgentWorktree, MergeRequest, MrStatus, Review, ReviewDecision, User, Workspace,
        WorkspaceMembership, WorkspaceRole,
    };

    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    fn thresholds() -> HygieneThresholds {
        HygieneThresholds {
            nudge_after_secs: 1000,
            escalate_after_secs: 5000,
            close_after_secs: 10_000,
        }
    }

    fn make_workspace(id: &str) -> Workspace {
        Workspace {
            id: Id::new(id),
            tenant_id: Id::new("tenant-1"),
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

    fn make_membership(user_id: &str, ws_id: &str, role: WorkspaceRole) -> WorkspaceMembership {
        WorkspaceMembership {
            id: Id::new(uuid::Uuid::new_v4().to_string()),
            user_id: Id::new(user_id),
            workspace_id: Id::new(ws_id),
            role,
            invited_by: Id::new("admin"),
            accepted: true,
            accepted_at: Some(1_000_000),
            created_at: 1_000_000,
        }
    }

    fn make_mr(id: &str, ws_id: &str, repo_id: &Id, updated_at: u64) -> MergeRequest {
        MergeRequest {
            id: Id::new(id),
            repository_id: repo_id.clone(),
            title: format!("MR {id}"),
            source_branch: format!("feat/{id}"),
            target_branch: "main".to_string(),
            status: MrStatus::Open,
            author_agent_id: None,
            reviewers: vec![],
            diff_stats: None,
            has_conflicts: None,
            spec_ref: None,
            depends_on: vec![],
            atomic_group: None,
            workspace_id: Id::new(ws_id),
            created_at: updated_at,
            updated_at,
            reverted_at: None,
            revert_commit_sha: None,
            merge_commit_sha: None,
        }
    }

    async fn make_user(state: &Arc<AppState>, id: &str) {
        let mut user = User::new(Id::new(id), format!("ext-{id}"), id, 0);
        user.tenant_id = Some(Id::new("tenant-1"));
        state.users.create(&user).await.unwrap();
    }

    /// Build a state with configurable git AND jj doubles: the git double
    /// decides `can_merge` (merge-base conflicts), the jj double records
    /// rebase calls and replays a programmed outcome.
    fn state_with_doubles(git: ConfigurableGitOps, jj: Arc<ConfigurableJjOps>) -> Arc<AppState> {
        let mut state = test_state_with_git_ops(Arc::new(git));
        let s = Arc::get_mut(&mut state).expect("test state is uniquely owned");
        s.jj_ops = jj;
        state
    }

    async fn notifications_for(
        state: &Arc<AppState>,
        user_id: &str,
        ws_id: &str,
        type_str: &str,
    ) -> Vec<Notification> {
        state
            .notifications
            .list_for_user(
                &Id::new(user_id),
                Some(&Id::new(ws_id)),
                None,
                None,
                Some(&[type_str]),
                None,
                &[],
                100,
                0,
            )
            .await
            .unwrap()
            .into_iter()
            .filter(|n| n.notification_type.as_str() == type_str)
            .collect()
    }

    // ── Abandoned close ─────────────────────────────────────────────────

    #[tokio::test]
    async fn abandoned_mr_is_closed_with_explanation_delivered_to_author() {
        let state = test_state();
        let now = now_secs();
        let ws = make_workspace("ws-close");
        state.workspaces.create(&ws).await.unwrap();
        let repo = gyre_domain::Repository::new(
            Id::new("repo-close"),
            ws.id.clone(),
            "close-repo",
            "/tmp/close-repo.git",
            0,
        );
        state.repos.create(&repo).await.unwrap();

        // Author agent + a Developer member observing the workspace.
        let mut author = Agent::new(Id::new("agent-author"), "author", 1000);
        author.workspace_id = ws.id.clone();
        state.agents.create(&author).await.unwrap();
        state
            .workspace_memberships
            .create(&make_membership(
                "user-admin",
                "ws-close",
                WorkspaceRole::Admin,
            ))
            .await
            .unwrap();

        let mut mr = make_mr("mr-close", "ws-close", &repo.id, now - 20_000);
        mr.author_agent_id = Some(Id::new("agent-author"));
        state.merge_requests.create(&mr).await.unwrap();

        let mut rx = state.message_broadcast_tx.subscribe();
        run_with(&state, &thresholds()).await.unwrap();

        // Status transition persisted.
        let closed = state
            .merge_requests
            .find_by_id(&Id::new("mr-close"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            closed.status,
            MrStatus::Closed,
            "abandoned MR must be closed"
        );

        // Explanatory comment on the MR (never a silent close).
        let comments = state
            .reviews
            .list_comments(&Id::new("mr-close"))
            .await
            .unwrap();
        assert_eq!(
            comments.len(),
            1,
            "one explanation comment, got {comments:?}"
        );
        assert!(comments[0].body.contains("closed automatically"));
        assert_eq!(comments[0].author_agent_id, "system");

        // The author (an agent) got the explanation as a message.
        let mut saw_author_message = false;
        while let Ok(msg) = rx.try_recv() {
            if let gyre_common::message::Destination::Agent(a) = &msg.to {
                if a.as_str() == "agent-author" && msg.kind.as_str() == "mr_auto_closed" {
                    saw_author_message = true;
                    assert!(msg
                        .payload
                        .as_ref()
                        .unwrap()
                        .get("explanation")
                        .and_then(|e| e.as_str())
                        .unwrap()
                        .contains("closed automatically"));
                }
            }
        }
        assert!(
            saw_author_message,
            "author must receive the close explanation"
        );

        // Workspace Admins observe the auto-close.
        let admin_notifs =
            notifications_for(&state, "user-admin", "ws-close", "MrAutoClosed").await;
        assert_eq!(admin_notifs.len(), 1);

        // The close is recorded in the audit trail.
        let events = gyre_ports::AuditRepository::query(
            &*state.audit,
            &gyre_ports::AuditQueryFilter {
                event_type: Some("pr_hygiene".to_string()),
                limit: 100,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(
            events
                .iter()
                .any(|e| e.detail["action"] == "close_abandoned"),
            "close action must be audited, got {:?}",
            events.iter().map(|e| e.detail.clone()).collect::<Vec<_>>()
        );
    }

    // ── Unreviewed nudge ────────────────────────────────────────────────

    #[tokio::test]
    async fn unreviewed_mr_nudges_assigned_reviewers() {
        let state = test_state();
        let now = now_secs();
        let ws = make_workspace("ws-nudge");
        state.workspaces.create(&ws).await.unwrap();
        let repo = gyre_domain::Repository::new(
            Id::new("repo-nudge"),
            ws.id.clone(),
            "nudge-repo",
            "/tmp/nudge-repo.git",
            0,
        );
        state.repos.create(&repo).await.unwrap();

        make_user(&state, "user-reviewer").await;
        let mut mr = make_mr("mr-nudge", "ws-nudge", &repo.id, now - 2000);
        mr.reviewers = vec![Id::new("user-reviewer")];
        state.merge_requests.create(&mr).await.unwrap();

        run_with(&state, &thresholds()).await.unwrap();

        let notifs = notifications_for(&state, "user-reviewer", "ws-nudge", "ReviewerNudge").await;
        assert_eq!(
            notifs.len(),
            1,
            "assigned reviewer must be nudged exactly once, got {notifs:?}"
        );
        assert_eq!(notifs[0].source.entity_id, "mr-nudge");
    }

    #[tokio::test]
    async fn unreviewed_mr_without_reviewers_escalates_to_developers_and_admins() {
        let state = test_state();
        let now = now_secs();
        let ws = make_workspace("ws-norev");
        state.workspaces.create(&ws).await.unwrap();
        let repo = gyre_domain::Repository::new(
            Id::new("repo-norev"),
            ws.id.clone(),
            "norev-repo",
            "/tmp/norev-repo.git",
            0,
        );
        state.repos.create(&repo).await.unwrap();

        state
            .workspace_memberships
            .create(&make_membership(
                "user-dev",
                "ws-norev",
                WorkspaceRole::Developer,
            ))
            .await
            .unwrap();
        state
            .workspace_memberships
            .create(&make_membership(
                "user-admin",
                "ws-norev",
                WorkspaceRole::Admin,
            ))
            .await
            .unwrap();
        state
            .workspace_memberships
            .create(&make_membership(
                "user-viewer",
                "ws-norev",
                WorkspaceRole::Viewer,
            ))
            .await
            .unwrap();

        let mr = make_mr("mr-norev", "ws-norev", &repo.id, now - 2000);
        state.merge_requests.create(&mr).await.unwrap();

        run_with(&state, &thresholds()).await.unwrap();

        assert_eq!(
            notifications_for(&state, "user-dev", "ws-norev", "MrUnreviewed")
                .await
                .len(),
            1
        );
        assert_eq!(
            notifications_for(&state, "user-admin", "ws-norev", "MrUnreviewed")
                .await
                .len(),
            1
        );
        assert_eq!(
            notifications_for(&state, "user-viewer", "ws-norev", "MrUnreviewed")
                .await
                .len(),
            0,
            "Viewers are not escalation targets"
        );
    }

    #[tokio::test]
    async fn fresh_mr_under_nudge_threshold_gets_no_action() {
        let state = test_state();
        let now = now_secs();
        let ws = make_workspace("ws-fresh");
        state.workspaces.create(&ws).await.unwrap();
        let repo = gyre_domain::Repository::new(
            Id::new("repo-fresh"),
            ws.id.clone(),
            "fresh-repo",
            "/tmp/fresh-repo.git",
            0,
        );
        state.repos.create(&repo).await.unwrap();

        make_user(&state, "user-reviewer").await;
        state
            .workspace_memberships
            .create(&make_membership(
                "user-admin",
                "ws-fresh",
                WorkspaceRole::Admin,
            ))
            .await
            .unwrap();

        let mut mr = make_mr("mr-fresh", "ws-fresh", &repo.id, now - 10);
        mr.reviewers = vec![Id::new("user-reviewer")];
        state.merge_requests.create(&mr).await.unwrap();

        run_with(&state, &thresholds()).await.unwrap();

        assert_eq!(
            notifications_for(&state, "user-reviewer", "ws-fresh", "ReviewerNudge")
                .await
                .len(),
            0,
            "fresh MR must not be nudged"
        );
        assert_eq!(
            notifications_for(&state, "user-admin", "ws-fresh", "MrUnreviewed")
                .await
                .len(),
            0
        );
        let mr = state
            .merge_requests
            .find_by_id(&Id::new("mr-fresh"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(mr.status, MrStatus::Open, "fresh MR must stay open");
    }

    #[tokio::test]
    async fn reviewed_mr_is_not_unreviewed() {
        let state = test_state();
        let now = now_secs();
        let ws = make_workspace("ws-reviewed");
        state.workspaces.create(&ws).await.unwrap();
        let repo = gyre_domain::Repository::new(
            Id::new("repo-reviewed"),
            ws.id.clone(),
            "reviewed-repo",
            "/tmp/reviewed-repo.git",
            0,
        );
        state.repos.create(&repo).await.unwrap();

        make_user(&state, "user-reviewer").await;
        let mut mr = make_mr("mr-reviewed", "ws-reviewed", &repo.id, now - 2000);
        mr.reviewers = vec![Id::new("user-reviewer")];
        state.merge_requests.create(&mr).await.unwrap();

        // A submitted review record: the MR is reviewed, not unreviewed.
        state
            .reviews
            .submit_review(&Review::new(
                Id::new("rev-1"),
                Id::new("mr-reviewed"),
                "user-reviewer",
                ReviewDecision::Approved,
                now,
            ))
            .await
            .unwrap();

        run_with(&state, &thresholds()).await.unwrap();

        assert_eq!(
            notifications_for(&state, "user-reviewer", "ws-reviewed", "ReviewerNudge")
                .await
                .len(),
            0,
            "a reviewed MR must not be nudged as unreviewed"
        );
    }

    // ── Conflicted action ───────────────────────────────────────────────

    #[tokio::test]
    async fn conflicted_mr_with_active_agent_worktree_is_rebased() {
        let git = ConfigurableGitOps::with_conflicts(vec!["feat/mr-conflict"]);
        let jj = Arc::new(ConfigurableJjOps::default());
        let state = state_with_doubles(git, jj.clone());
        let now = now_secs();
        let ws = make_workspace("ws-conflict");
        state.workspaces.create(&ws).await.unwrap();
        let repo = gyre_domain::Repository::new(
            Id::new("repo-conflict"),
            ws.id.clone(),
            "conflict-repo",
            "/tmp/conflict-repo.git",
            0,
        );
        state.repos.create(&repo).await.unwrap();

        let mut author = Agent::new(Id::new("agent-conflict"), "conflict", 1000);
        author.workspace_id = ws.id.clone();
        author.status = gyre_domain::AgentStatus::Active;
        state.agents.create(&author).await.unwrap();

        let wt_dir = tempfile::tempdir().unwrap();
        let wt = AgentWorktree::new(
            Id::new("wt-conflict"),
            Id::new("agent-conflict"),
            repo.id.clone(),
            None,
            "feat/mr-conflict",
            wt_dir.path().to_str().unwrap(),
            1000,
        );
        state.worktrees.create(&wt).await.unwrap();
        std::mem::forget(wt_dir);

        // has_conflicts unset: the real merge-base check (git double says
        // the source branch conflicts with the target) must classify it.
        let mut mr = make_mr("mr-conflict", "ws-conflict", &repo.id, now - 10);
        mr.author_agent_id = Some(Id::new("agent-conflict"));
        state.merge_requests.create(&mr).await.unwrap();

        run_with(&state, &thresholds()).await.unwrap();

        // The existing jj rebase machinery ran in the author's worktree.
        let calls = jj.rebase_calls.lock().clone();
        assert_eq!(
            calls.len(),
            1,
            "conflicted agent-authored MR must be rebased, got {calls:?}"
        );
        assert_eq!(calls[0].1, "@");
        assert_eq!(calls[0].2, "main");

        // The rebase is recorded in the audit trail (jj_rebase event).
        let events = gyre_ports::AuditRepository::query(
            &*state.audit,
            &gyre_ports::AuditQueryFilter {
                event_type: Some("jj_rebase".to_string()),
                limit: 100,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(
            events
                .iter()
                .any(|e| e.detail["source"] == "pr_hygiene" && e.detail["outcome"] == "success"),
            "the hygiene-triggered rebase must be audited"
        );

        // No rebase-required escalation: the rebase resolved it.
        state
            .workspace_memberships
            .create(&make_membership(
                "user-admin",
                "ws-conflict",
                WorkspaceRole::Admin,
            ))
            .await
            .unwrap();
        assert_eq!(
            notifications_for(&state, "user-admin", "ws-conflict", "MrNeedsRebase")
                .await
                .len(),
            0,
            "a successful rebase must not raise a rebase-required escalation"
        );
    }

    #[tokio::test]
    async fn conflicted_mr_without_agent_raises_rebase_required_escalation() {
        let state = test_state();
        let now = now_secs();
        let ws = make_workspace("ws-rebareq");
        state.workspaces.create(&ws).await.unwrap();
        let repo = gyre_domain::Repository::new(
            Id::new("repo-rebareq"),
            ws.id.clone(),
            "rebareq-repo",
            "/tmp/rebareq-repo.git",
            0,
        );
        state.repos.create(&repo).await.unwrap();

        state
            .workspace_memberships
            .create(&make_membership(
                "user-dev",
                "ws-rebareq",
                WorkspaceRole::Developer,
            ))
            .await
            .unwrap();

        // Persisted conflict flag, no author agent: the rebase machinery is
        // not applicable — a rebase-required escalation is the real action.
        let mut mr = make_mr("mr-rebareq", "ws-rebareq", &repo.id, now - 10);
        mr.has_conflicts = Some(true);
        state.merge_requests.create(&mr).await.unwrap();

        run_with(&state, &thresholds()).await.unwrap();

        let notifs = notifications_for(&state, "user-dev", "ws-rebareq", "MrNeedsRebase").await;
        assert_eq!(
            notifs.len(),
            1,
            "conflicted MR with no applicable rebase must escalate, got {notifs:?}"
        );

        // The escalation is recorded in the audit trail.
        let events = gyre_ports::AuditRepository::query(
            &*state.audit,
            &gyre_ports::AuditQueryFilter {
                event_type: Some("pr_hygiene".to_string()),
                limit: 100,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(
            events
                .iter()
                .any(|e| e.detail["action"] == "rebase_required"),
            "rebase_required action must be audited"
        );
    }

    // ── Long-open escalation ────────────────────────────────────────────

    #[tokio::test]
    async fn long_open_mr_escalates_to_admins_at_higher_priority() {
        let state = test_state();
        let now = now_secs();
        let ws = make_workspace("ws-stale");
        state.workspaces.create(&ws).await.unwrap();
        let repo = gyre_domain::Repository::new(
            Id::new("repo-stale"),
            ws.id.clone(),
            "stale-repo",
            "/tmp/stale-repo.git",
            0,
        );
        state.repos.create(&repo).await.unwrap();

        state
            .workspace_memberships
            .create(&make_membership(
                "user-admin",
                "ws-stale",
                WorkspaceRole::Admin,
            ))
            .await
            .unwrap();

        // Older than the escalation threshold but younger than close.
        let mr = make_mr("mr-stale", "ws-stale", &repo.id, now - 6000);
        state.merge_requests.create(&mr).await.unwrap();

        run_with(&state, &thresholds()).await.unwrap();

        let notifs = notifications_for(&state, "user-admin", "ws-stale", "MrStaleEscalation").await;
        assert_eq!(notifs.len(), 1, "long-open MR must escalate to Admins");
        // Higher priority than a nudge (rank 3 vs 7).
        assert_eq!(notifs[0].priority_rank, 3);
        // The MR stays open — escalation, not closure.
        let mr = state
            .merge_requests
            .find_by_id(&Id::new("mr-stale"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(mr.status, MrStatus::Open);
    }

    // ── Idempotence / continuous cadence ────────────────────────────────

    #[tokio::test]
    async fn consecutive_evaluations_do_not_duplicate_notifications() {
        let state = test_state();
        let now = now_secs();
        let ws = make_workspace("ws-idem");
        state.workspaces.create(&ws).await.unwrap();
        let repo = gyre_domain::Repository::new(
            Id::new("repo-idem"),
            ws.id.clone(),
            "idem-repo",
            "/tmp/idem-repo.git",
            0,
        );
        state.repos.create(&repo).await.unwrap();

        make_user(&state, "user-reviewer").await;
        state
            .workspace_memberships
            .create(&make_membership(
                "user-admin",
                "ws-idem",
                WorkspaceRole::Admin,
            ))
            .await
            .unwrap();

        let mut mr = make_mr("mr-idem", "ws-idem", &repo.id, now - 2000);
        mr.reviewers = vec![Id::new("user-reviewer")];
        state.merge_requests.create(&mr).await.unwrap();

        // Two evaluations inside the test window ("continuously monitors").
        run_with(&state, &thresholds()).await.unwrap();
        run_with(&state, &thresholds()).await.unwrap();

        assert_eq!(
            notifications_for(&state, "user-reviewer", "ws-idem", "ReviewerNudge")
                .await
                .len(),
            1,
            "second evaluation must not duplicate the nudge"
        );

        // Escalation path dedupes too.
        let mut stale = make_mr("mr-idem-stale", "ws-idem", &repo.id, now - 6000);
        stale.reviewers = vec![];
        state.merge_requests.create(&stale).await.unwrap();
        run_with(&state, &thresholds()).await.unwrap();
        run_with(&state, &thresholds()).await.unwrap();
        assert_eq!(
            notifications_for(&state, "user-admin", "ws-idem", "MrStaleEscalation")
                .await
                .len(),
            1,
            "second evaluation must not duplicate the escalation"
        );
    }

    #[tokio::test]
    async fn consecutive_evaluations_do_not_duplicate_agent_reviewer_nudges() {
        // Review repair (task-270): an AGENT reviewer has no notification
        // inbox row to dedupe against — the nudge is delivered on the
        // message bus (`review_nudge`). The KV nudge backoff must keep
        // that emission idempotent across consecutive evaluations, the
        // same guarantee the user path gets from `has_active_notification`.
        let state = test_state();
        let now = now_secs();
        let ws = make_workspace("ws-agent-idem");
        state.workspaces.create(&ws).await.unwrap();
        let repo = gyre_domain::Repository::new(
            Id::new("repo-agent-idem"),
            ws.id.clone(),
            "agent-idem-repo",
            "/tmp/agent-idem-repo.git",
            0,
        );
        state.repos.create(&repo).await.unwrap();

        let reviewer = Agent::new(Id::new("agent-reviewer"), "reviewer", 1000);
        state.agents.create(&reviewer).await.unwrap();

        let mut mr = make_mr("mr-agent-idem", "ws-agent-idem", &repo.id, now - 2000);
        mr.reviewers = vec![Id::new("agent-reviewer")];
        state.merge_requests.create(&mr).await.unwrap();

        let mut rx = state.message_broadcast_tx.subscribe();
        // Two evaluations inside the test window.
        run_with(&state, &thresholds()).await.unwrap();
        run_with(&state, &thresholds()).await.unwrap();

        let nudges = {
            let mut n = 0;
            while let Ok(msg) = rx.try_recv() {
                if let gyre_common::message::Destination::Agent(a) = &msg.to {
                    if a.as_str() == "agent-reviewer" && msg.kind.as_str() == "review_nudge" {
                        n += 1;
                    }
                }
            }
            n
        };
        assert_eq!(
            nudges, 1,
            "second evaluation must not duplicate the agent review nudge"
        );

        // The dedup is persisted in the KV store — the backoff key is
        // (MR, agent), so a different MR's nudge to the same agent is
        // still delivered on the first evaluation.
        let key = nudge_backoff_key(&mr, &Id::new("agent-reviewer"));
        let persisted = state.kv_store.kv_get(NUDGE_BACKOFF_NS, &key).await.unwrap();
        assert!(
            persisted.is_some(),
            "nudge backoff must be persisted so restarts stay idempotent"
        );
    }
}
