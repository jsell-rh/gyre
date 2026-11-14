use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use tracing::{error, info, warn};

use gyre_common::Id;
use gyre_domain::{
    AnalyticsEvent, GateResult, GateStatus, MergeQueueEntry, MergeQueueEntryStatus, MergeRequest,
    MergeResult, MrStatus, TaskPriority,
};
use uuid::Uuid;

use crate::AppState;

pub fn spawn_merge_processor(state: Arc<AppState>) {
    tokio::spawn(async move {
        // Record every cycle in the job registry so /healthz and /readyz can
        // detect a dead processor loop (business-continuity.md §2).
        state.job_registry.mark_scheduled("merge_processor").await;
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            let started_at = crate::jobs::now_secs();
            let result = process_next(&state).await;
            state
                .job_registry
                .record_cycle("merge_processor", started_at, &result)
                .await;
            if let Err(e) = result {
                error!("merge processor error: {e:#}");
            }
        }
    });
}

/// Run one merge-processor cycle. Used by the job framework for manual triggering.
pub async fn run_once(state: &AppState) -> anyhow::Result<()> {
    process_next(state).await
}

// ── Automatic jj rebase on target branch movement (source-control.md §4) ──

use gyre_domain::{AgentStatus, AuditEventType, AuditOutcome};
use gyre_ports::JjRebaseOutcome;

/// KV namespace for the per-agent rebase backoff window. Key = agent id,
/// value = epoch seconds of the last *completed* rebase (Success or
/// Conflict). A failure must NOT consume the window (R2 F4): a broken
/// workspace would otherwise suppress every retry for 60s.
const JJ_REBASE_BACKOFF_NS: &str = "jj_rebase_backoff";
/// KV namespace for the per-agent pending-rebase marker. Key = agent id,
/// value = JSON `{repo_id, target_branch, new_base_sha}` of the movement
/// that was deferred by the backoff window and must be replayed once it
/// expires (batch semantics: one deferred movement per agent — a later
/// movement overwrites an earlier pending one, which is exactly the
/// "batch into one rebase" behavior the plan requires).
const JJ_REBASE_PENDING_NS: &str = "jj_rebase_pending";
/// Rebase rate-limit window (plan item 5). Multiple baseline movements
/// inside the window collapse into the pending marker above; the replay
/// runs once the window expires.
const JJ_REBASE_WINDOW_SECS: u64 = 60;

/// An in-flight agent identified for rebasing after a merge landed.
struct RebaseTarget {
    agent_id: Id,
    worktree_path: String,
    branch: String,
}

/// One pending (deferred) rebase recorded in KV.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PendingRebase {
    repo_id: String,
    target_branch: String,
    new_base_sha: String,
}

/// Identify the in-flight agents that must be rebased after a merge landed
/// on `target_branch`: authors of other Open/Approved MRs targeting the
/// same branch in the same repo, whose agent is still Active, and whose
/// worktree directory exists on disk.
///
/// The merged MR's own author is excluded — their work just landed.
async fn find_rebase_targets(
    state: &AppState,
    repo: &gyre_domain::Repository,
    target_branch: &str,
    merged_author_agent_ids: &[Id],
) -> Vec<RebaseTarget> {
    let mut targets = Vec::new();
    let mrs = match state.merge_requests.list_by_repo(&repo.id).await {
        Ok(m) => m,
        Err(e) => {
            warn!(repo_id = %repo.id, error = %e, "cannot list MRs for rebase targets");
            return targets;
        }
    };
    for mr in mrs {
        if mr.target_branch != target_branch {
            continue;
        }
        if !matches!(mr.status, MrStatus::Open | MrStatus::Approved) {
            continue;
        }
        let Some(author_id) = &mr.author_agent_id else {
            continue;
        };
        if merged_author_agent_ids.contains(author_id) {
            continue;
        }
        // Safeguard (plan item 5): only Active agents — Dead/Completed/Idle
        // agents have no in-progress work to rebase.
        let Ok(Some(agent)) = state.agents.find_by_id(author_id).await else {
            continue;
        };
        if agent.status != AgentStatus::Active {
            continue;
        }
        // Safeguard (plan item 5): skip agents whose worktree is gone
        // (already cleaned up, or never provisioned). The fallback for a
        // missing (repo, source_branch) record stays INSIDE the moved repo
        // (R12 F1): the worktree branch record can drift from the MR's
        // source branch, but a worktree in a DIFFERENT repo must never be
        // rebased for this repo's movement — the agent's other-repo
        // in-flight work would be spliced onto this repo's baseline.
        let Ok(worktrees) = state.worktrees.find_by_agent(&agent.id).await else {
            continue;
        };
        let Some(wt) = worktrees
            .iter()
            .find(|wt| wt.repository_id == repo.id && wt.branch == mr.source_branch)
            .or_else(|| worktrees.iter().find(|wt| wt.repository_id == repo.id))
        else {
            continue;
        };
        if !std::path::Path::new(&wt.path).exists() {
            continue;
        }
        targets.push(RebaseTarget {
            agent_id: agent.id.clone(),
            worktree_path: wt.path.clone(),
            branch: mr.source_branch.clone(),
        });
    }
    targets
}

/// Record a rebase audit event (plan item 5: log all rebase operations).
/// The audit trail is the system of record for which agent was rebased
/// onto which base, when, and with what outcome.
#[allow(clippy::too_many_arguments)]
async fn record_rebase_audit(
    state: &AppState,
    repo: &gyre_domain::Repository,
    agent: &RebaseTarget,
    target_branch: &str,
    outcome: &str,
    rebased_count: usize,
    files: &[String],
    new_base_sha: &str,
) {
    let now = crate::jobs::now_secs();
    let event = gyre_domain::AuditEvent::new(
        Id::new(Uuid::new_v4().to_string()),
        AuditEventType::Custom("jj_rebase".to_string()),
        Some(agent.agent_id.clone()),
        None,
        None,
        Some(repo.workspace_id.clone()),
        Some(repo.id.clone()),
        "merge_queue".to_string(),
        Some(agent.branch.clone()),
        AuditOutcome::Success,
        serde_json::json!({
            "outcome": outcome,
            "rebased_count": rebased_count,
            "conflicted_files": files,
            "new_base_sha": new_base_sha,
            "target_branch": target_branch,
            "worktree": agent.worktree_path,
        }),
        None,
        None,
        now,
    );
    if let Err(e) = state.audit.record(&event).await {
        warn!(agent_id = %agent.agent_id, error = %e, "failed to record jj rebase audit event");
    }
}

/// Attempt the rebase of one agent's in-flight work onto the new base.
///
/// Returns `true` when the deferral (pending marker) may be consumed —
/// i.e. the rebase actually ran and reached a terminal outcome (Success or
/// Conflict) — and `false` when it must stay deferred: backoff window hit,
/// or infrastructure failure (an Err must not consume the deferral; the
/// F4 guarantee holds on BOTH the trigger and replay paths, R2 F8).
async fn rebase_one_agent(
    state: &AppState,
    repo: &gyre_domain::Repository,
    agent: &RebaseTarget,
    target_branch: &str,
    new_base_sha: &str,
) -> bool {
    // Rate limit (plan item 5): inside the window, defer instead of skip.
    // The pending marker records the newest movement; the replay once the
    // window expires batches every intermediate movement into one rebase.
    let in_window = match state
        .kv_store
        .kv_get(JJ_REBASE_BACKOFF_NS, agent.agent_id.as_str())
        .await
    {
        Ok(Some(v)) => match v.parse::<u64>() {
            Ok(last) => {
                let now = crate::jobs::now_secs();
                now.saturating_sub(last) < JJ_REBASE_WINDOW_SECS
            }
            Err(_) => false,
        },
        Ok(None) => false,
        Err(e) => {
            warn!(agent_id = %agent.agent_id, error = %e, "cannot read rebase backoff, proceeding");
            false
        }
    };
    if in_window {
        let pending = PendingRebase {
            repo_id: repo.id.to_string(),
            target_branch: target_branch.to_string(),
            new_base_sha: new_base_sha.to_string(),
        };
        match serde_json::to_string(&pending) {
            Ok(v) => {
                if let Err(e) = state
                    .kv_store
                    .kv_set(JJ_REBASE_PENDING_NS, agent.agent_id.as_str(), v)
                    .await
                {
                    warn!(agent_id = %agent.agent_id, error = %e, "failed to record pending rebase");
                }
            }
            Err(e) => {
                warn!(agent_id = %agent.agent_id, error = %e, "cannot serialize pending rebase")
            }
        }
        info!(agent_id = %agent.agent_id, branch = %agent.branch, "rebase deferred: inside backoff window, pending marker set");
        return false;
    }

    run_rebase(state, repo, agent, target_branch, new_base_sha).await
}

/// Execute the jj rebase and apply all side effects (audit, notifications,
/// conflict state). Returns `true` on a terminal outcome (Success or
/// Conflict — including no-op success), `false` on infrastructure error
/// (caller keeps the deferral alive).
async fn run_rebase(
    state: &AppState,
    repo: &gyre_domain::Repository,
    agent: &RebaseTarget,
    target_branch: &str,
    new_base_sha: &str,
) -> bool {
    // -b @ rebases the branch containing the working copy — the whole
    // in-flight stack ("the agent's in-progress work", source-control.md
    // §4). Destination is the branch that just MOVED (`target_branch`),
    // not the repo's default branch: MRs can target non-default branches
    // (create_mr performs no default-branch validation), and rebasing an
    // agent onto a branch their MR does not target splices their work
    // into an unrelated line of history. Conflicts are state, not errors
    // (jj's conflict-as-state model).
    let outcome = state
        .jj_ops
        .jj_rebase(&agent.worktree_path, "@", target_branch)
        .await;

    match outcome {
        Ok(JjRebaseOutcome::Success { rebased_count }) => {
            // Persist the backoff only AFTER a completed rebase (R2 F4:
            // guard state after the guarded action).
            let now = crate::jobs::now_secs();
            if let Err(e) = state
                .kv_store
                .kv_set(
                    JJ_REBASE_BACKOFF_NS,
                    agent.agent_id.as_str(),
                    now.to_string(),
                )
                .await
            {
                warn!(agent_id = %agent.agent_id, error = %e, "failed to persist rebase backoff");
            }

            record_rebase_audit(
                state,
                repo,
                agent,
                target_branch,
                "success",
                rebased_count,
                &[],
                new_base_sha,
            )
            .await;

            // A clean rebase clears the conflict state an earlier
            // conflicted rebase surfaced on the author's MR (R19 F3):
            // the flag must not outlive the conflict itself — the agent
            // resolved it in their workspace and the stack now sits
            // cleanly on the moved target. Only a flagged MR is touched
            // (no write on the ordinary never-conflicted path).
            if let Ok(Some(mut mr)) =
                find_mr_by_author_branch(state, &repo.id, &agent.agent_id, &agent.branch).await
            {
                if mr.has_conflicts == Some(true) {
                    mr.has_conflicts = Some(false);
                    let _ = state.merge_requests.update(&mr).await;
                }
            }

            // Notify the agent: baseline moved, here is the new base.
            state
                .emit_event(
                    Some(repo.workspace_id.clone()),
                    gyre_common::message::Destination::Agent(agent.agent_id.clone()),
                    gyre_common::message::MessageKind::Custom("baseline_moved".to_string()),
                    Some(serde_json::json!({
                        "repo_id": repo.id.to_string(),
                        "branch": agent.branch,
                        "target_branch": target_branch,
                        "new_base_sha": new_base_sha,
                        "rebased_count": rebased_count,
                    })),
                )
                .await;
            true
        }
        Ok(JjRebaseOutcome::Conflict {
            rebased_count,
            files,
        }) => {
            let now = crate::jobs::now_secs();
            if let Err(e) = state
                .kv_store
                .kv_set(
                    JJ_REBASE_BACKOFF_NS,
                    agent.agent_id.as_str(),
                    now.to_string(),
                )
                .await
            {
                warn!(agent_id = %agent.agent_id, error = %e, "failed to persist rebase backoff");
            }

            record_rebase_audit(
                state,
                repo,
                agent,
                target_branch,
                "conflict",
                rebased_count,
                &files,
                new_base_sha,
            )
            .await;

            // Conflict-as-state (plan item 3): surface the conflict on the
            // MR and via SpeculativeConflict + Escalation events. The agent
            // keeps working on non-conflicting files.
            if let Ok(Some(mut mr)) =
                find_mr_by_author_branch(state, &repo.id, &agent.agent_id, &agent.branch).await
            {
                mr.has_conflicts = Some(true);
                let _ = state.merge_requests.update(&mr).await;
            }

            state
                .emit_event(
                    Some(repo.workspace_id.clone()),
                    gyre_common::message::Destination::Workspace(repo.workspace_id.clone()),
                    gyre_common::message::MessageKind::SpeculativeConflict,
                    Some(serde_json::json!({
                        "repo_id": repo.id.to_string(),
                        "branch": agent.branch,
                        "target_branch": target_branch,
                        "new_base_sha": new_base_sha,
                        "conflicting_files": files,
                    })),
                )
                .await;
            state
                .emit_event(
                    Some(repo.workspace_id.clone()),
                    gyre_common::message::Destination::Agent(agent.agent_id.clone()),
                    gyre_common::message::MessageKind::Escalation,
                    Some(serde_json::json!({
                        "repo_id": repo.id.to_string(),
                        "branch": agent.branch,
                        "target_branch": target_branch,
                        "new_base_sha": new_base_sha,
                        "reason": "rebase conflict",
                        "conflicting_files": files,
                    })),
                )
                .await;
            true
        }
        Err(e) => {
            // Infrastructure failure: do NOT consume a pending deferral
            // (R2 F8) and do NOT write the backoff (R2 F4 — a failed
            // rebase must not consume the window).
            record_rebase_failure(
                state,
                repo,
                agent,
                target_branch,
                &e.to_string(),
                new_base_sha,
            )
            .await;
            false
        }
    }
}

/// Find the (non-Closed) MR authored by `agent_id` on `branch` in `repo`.
/// The repo filter is load-bearing (R3 F10): an agent can hold Open MRs on
/// the same-named source branch in two repos, and a rebase conflict in one
/// repo must not flag the other repo's MR.
async fn find_mr_by_author_branch(
    state: &AppState,
    repo_id: &Id,
    agent_id: &Id,
    branch: &str,
) -> anyhow::Result<Option<MergeRequest>> {
    let all = state.merge_requests.list().await?;
    Ok(all.into_iter().find(|m| {
        m.repository_id == *repo_id
            && m.author_agent_id.as_ref() == Some(agent_id)
            && m.source_branch == branch
            && m.status != MrStatus::Closed
    }))
}

/// Record an audit event for a failed rebase attempt.
async fn record_rebase_failure(
    state: &AppState,
    repo: &gyre_domain::Repository,
    agent: &RebaseTarget,
    target_branch: &str,
    error: &str,
    new_base_sha: &str,
) {
    let now = crate::jobs::now_secs();
    let event = gyre_domain::AuditEvent::new(
        Id::new(Uuid::new_v4().to_string()),
        AuditEventType::Custom("jj_rebase".to_string()),
        Some(agent.agent_id.clone()),
        None,
        None,
        Some(repo.workspace_id.clone()),
        Some(repo.id.clone()),
        "merge_queue".to_string(),
        Some(agent.branch.clone()),
        AuditOutcome::Failure,
        serde_json::json!({
            "outcome": "error",
            "error": error,
            "new_base_sha": new_base_sha,
            "target_branch": target_branch,
            "worktree": agent.worktree_path,
        }),
        None,
        None,
        now,
    );
    if let Err(e) = state.audit.record(&event).await {
        warn!(agent_id = %agent.agent_id, error = %e, "failed to record jj rebase audit event");
    }
    warn!(
        agent_id = %agent.agent_id,
        branch = %agent.branch,
        error = error,
        "automatic jj rebase failed"
    );
}

/// Rebase all in-flight agents after a merge landed on `target_branch`
/// (plan item 1). Called as the LAST step of a successful merge, after
/// post-merge gates pass (R2 F3: never rebase onto a base that recovery
/// might revert).
async fn rebase_inflight_agents(
    state: &AppState,
    repo: &gyre_domain::Repository,
    target_branch: &str,
    new_base_sha: &str,
    merged_author_agent_ids: &[Id],
) {
    let targets = find_rebase_targets(state, repo, target_branch, merged_author_agent_ids).await;
    for target in targets {
        rebase_one_agent(state, repo, &target, target_branch, new_base_sha).await;
    }
}

/// Replay deferred rebases whose backoff window has expired (plan item 5
/// batch semantics). Runs on every processor cycle. A replay attempt that
/// fails with an infrastructure error keeps the pending marker (R2 F8).
async fn replay_pending_rebases(state: &AppState) {
    let pendings = match state.kv_store.kv_list(JJ_REBASE_PENDING_NS).await {
        Ok(list) => list,
        Err(e) => {
            warn!(error = %e, "cannot list pending rebases");
            return;
        }
    };
    for (agent_id, raw) in pendings {
        let Ok(pending) = serde_json::from_str::<PendingRebase>(&raw) else {
            // Corrupt marker — drop it; a future movement re-records it.
            let _ = state
                .kv_store
                .kv_remove(JJ_REBASE_PENDING_NS, &agent_id)
                .await;
            continue;
        };
        let in_window = match state.kv_store.kv_get(JJ_REBASE_BACKOFF_NS, &agent_id).await {
            Ok(Some(v)) => match v.parse::<u64>() {
                Ok(last) => crate::jobs::now_secs().saturating_sub(last) < JJ_REBASE_WINDOW_SECS,
                Err(_) => false,
            },
            _ => false,
        };
        if in_window {
            continue;
        }

        // Re-derive the current rebase target for this agent — the agent
        // may have completed or the worktree removed since deferral. When
        // the deferral is no longer actionable, drop the marker.
        let actionable = async {
            let repo = state
                .repos
                .find_by_id(&Id::new(&pending.repo_id))
                .await
                .ok()
                .flatten()?;
            let agent = state
                .agents
                .find_by_id(&Id::new(&agent_id))
                .await
                .ok()
                .flatten()?;
            if agent.status != AgentStatus::Active {
                return None;
            }
            let worktrees = state.worktrees.find_by_agent(&agent.id).await.ok()?;
            // Repo-scoped only (R12 F1): the deferral names this repo; a
            // worktree in any other repo must never receive it. When the
            // agent no longer has a worktree in the pending repo, the
            // deferral is not actionable and the marker is dropped below.
            let wt = worktrees
                .iter()
                .find(|wt| wt.repository_id.as_str() == pending.repo_id)?;
            if !std::path::Path::new(&wt.path).exists() {
                return None;
            }
            Some((
                repo,
                RebaseTarget {
                    agent_id: agent.id.clone(),
                    worktree_path: wt.path.clone(),
                    branch: wt.branch.clone(),
                },
            ))
        }
        .await;

        let Some((repo, target)) = actionable else {
            let _ = state
                .kv_store
                .kv_remove(JJ_REBASE_PENDING_NS, &agent_id)
                .await;
            continue;
        };

        let consumed = run_rebase(
            state,
            &repo,
            &target,
            &pending.target_branch,
            &pending.new_base_sha,
        )
        .await;
        if consumed {
            let _ = state
                .kv_store
                .kv_remove(JJ_REBASE_PENDING_NS, &agent_id)
                .await;
        }
        // else: keep the marker — an infra failure must not drop the
        // deferral (R2 F8); the next cycle retries.
    }
}

// ── Circuit Breaker (platform-model.md §6) ──

const REVERT_COUNTS_NS: &str = "revert_counts";

/// Increment and return the MR's revert count (circuit breaker, §6).
///
/// The counter is keyed on the resubmission-stable identity (see
/// [`revert_breaker_key`], task-095 R3-F2): the remediation flow instructs
/// the author to re-do the work on a fresh branch and open a NEW merge
/// request, so a per-MR-id key could never accumulate across the
/// fix-and-resubmit cycle the protocol itself prescribes.
pub(crate) async fn increment_revert_count(
    state: &AppState,
    mr: &MergeRequest,
) -> anyhow::Result<u64> {
    let key = revert_breaker_key(mr);
    let current = state
        .kv_store
        .kv_get(REVERT_COUNTS_NS, &key)
        .await?
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let next = current + 1;
    state
        .kv_store
        .kv_set(REVERT_COUNTS_NS, &key, next.to_string())
        .await?;
    Ok(next)
}

/// Circuit-breaker counter key for an MR (task-095 R3-F2).
///
/// Keyed on `spec_ref` when the MR carries one — a resubmission of the
/// same spec re-enters with the same `spec_ref`, so "the same MR reverted
/// 3 times" accumulates across the fresh-branch/fresh-MR resubmission
/// cycle the remediation task prescribes, and fires on the spec that
/// keeps failing post-merge validation (spec Circuit Breaker item 3: "the
/// spec it references may need revisiting"). Falls back to the MR id for
/// MRs with no spec binding (manual reverts of unbound work).
pub(crate) fn revert_breaker_key(mr: &MergeRequest) -> String {
    match mr.spec_ref.as_deref() {
        Some(spec_ref) => format!("spec:{}", spec_ref),
        None => format!("mr:{}", mr.id),
    }
}

/// Circuit breaker tripped: remove the reverted work from the merge queue
/// permanently and escalate to a human (platform-model.md §6 Circuit
/// Breaker).
///
/// Queue removal covers EVERY MR sharing the breaker key (task-095 R3-F2):
/// the counter accumulates across the fix-and-resubmit cycle the remediation
/// flow prescribes, so when it fires, a resubmitted MR for the same spec is
/// already over threshold — cancelling only the triggering MR's entries
/// would let its resubmission ride the queue with a stale pre-trip entry.
pub(crate) async fn trip_circuit_breaker(
    state: &AppState,
    repo_id: &str,
    mr: &MergeRequest,
) -> anyhow::Result<()> {
    let mr_id = mr.id.as_str();
    let key = revert_breaker_key(mr);
    warn!(mr_id, key = %key, "circuit breaker tripped: work reverted 3 times");

    // Cancel queue entries of every MR sharing the breaker key.
    let all_mrs = state.merge_requests.list().await?;
    let tripped_mr_ids: std::collections::HashSet<String> = all_mrs
        .iter()
        .filter(|m| revert_breaker_key(m) == key)
        .map(|m| m.id.to_string())
        .collect();
    let queue = state.merge_queue.list_queue().await?;
    for entry in queue
        .iter()
        .filter(|e| tripped_mr_ids.contains(e.merge_request_id.as_str()))
    {
        let _ = state
            .merge_queue
            .update_status(
                &entry.id,
                MergeQueueEntryStatus::Cancelled,
                Some("circuit breaker: failed post-merge validation 3 times".to_string()),
            )
            .await;
    }

    // Spec Circuit Breaker item 3: name the spec the human must revisit.
    let spec_note = mr
        .spec_ref
        .as_deref()
        .map(|s| format!(" (spec {s} may need revisiting)"))
        .unwrap_or_default();
    let escalation_title =
        format!("MR {mr_id} has failed post-merge validation 3 times{spec_note}");

    // Escalate to humans. Tenant resolved from the MR's workspace — never
    // a fabricated "default" scope identity (see
    // scripts/check-fabricated-scope-defaults.sh; review F4 repair). The
    // recipient set is every workspace member — a superset of the §Who Gets
    // Notified escalation chain that misses no specced recipient (the
    // breaker is a workspace-level event with no single agent to resolve
    // `spawned_by` from).
    let tenant_id = match state.workspaces.find_by_id(&mr.workspace_id).await {
        Ok(Some(ws)) => ws.tenant_id.to_string(),
        _ => {
            warn!(
                mr_id,
                workspace_id = %mr.workspace_id,
                "circuit breaker: cannot resolve tenant for escalation notifications — skipping notification fan-out"
            );
            String::new()
        }
    };
    if !tenant_id.is_empty() {
        let members = state
            .workspace_memberships
            .list_by_workspace(&mr.workspace_id)
            .await
            .unwrap_or_default();
        for member in &members {
            crate::notifications::notify_rich(
                state,
                member.workspace_id.clone(),
                member.user_id.clone(),
                gyre_common::NotificationType::AgentEscalation,
                escalation_title.clone(),
                &tenant_id,
                Some(
                    serde_json::json!({
                        "repo_id": repo_id,
                        "mr_id": mr_id,
                        "spec_ref": mr.spec_ref,
                        "circuit_breaker": true,
                    })
                    .to_string(),
                ),
                Some(mr_id.to_string()),
                Some(repo_id.to_string()),
                "mr",
                None,
            )
            .await;
        }
    }

    // Escalation task: the referenced spec may need revisiting.
    let now = crate::api::now_secs();
    let task_id = Id::new(Uuid::new_v4().to_string());
    let mut task = gyre_domain::Task::new(task_id, escalation_title, now);
    task.priority = TaskPriority::Critical;
    task.labels = vec!["circuit-breaker".to_string(), "auto-created".to_string()];
    task.description = Some(format!(
        "MR {mr_id} in repo {repo_id} was reverted 3 times by the post-merge \
         recovery protocol (breaker key {key}). All queue entries for this \
         key have been cancelled. {}",
        mr.spec_ref
            .as_deref()
            .map(|s| format!("The spec it references ({s}) may need revisiting."))
            .unwrap_or_else(|| "No spec is bound to this MR.".to_string())
    ));
    task.workspace_id = mr.workspace_id.clone();
    task.repo_id = Id::new(repo_id.to_string());
    if let Err(e) = state.tasks.create(&task).await {
        warn!(mr_id, error = %e, "failed to create circuit-breaker task");
    }

    Ok(())
}

/// Record the §9 meta-spec drift warning as a gate result on the MR
/// (meta-spec-reconciliation.md §9).
///
/// The gate result carries `GateStatus::Passed` — the warning is
/// informational and never blocks a merge by itself (§9: "warn, don't
/// block"); blocking is decided by the workspace merge policy in the
/// caller. Idempotent across merge-processor retry cycles: an existing
/// drift gate result for the MR is updated in place instead of duplicated.
async fn record_meta_spec_drift_warning(
    state: &AppState,
    mr: &MergeRequest,
    drift: &crate::reconciliation::MetaSpecDrift,
) {
    let output = crate::reconciliation::format_meta_spec_drift_warning(drift);
    let now = crate::api::now_secs();

    let existing = state
        .gate_results
        .list_by_mr_id(mr.id.as_str())
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|r| r.gate_id.as_str() == crate::reconciliation::META_SPEC_DRIFT_GATE_ID);

    let saved = match existing {
        Some(r) => {
            state
                .gate_results
                .update_status(
                    &r.id.to_string(),
                    GateStatus::Passed,
                    Some(now),
                    Some(now),
                    Some(output.clone()),
                )
                .await
        }
        None => {
            let result = GateResult {
                id: Id::new(Uuid::new_v4().to_string()),
                gate_id: Id::new(crate::reconciliation::META_SPEC_DRIFT_GATE_ID.to_string()),
                mr_id: mr.id.clone(),
                status: GateStatus::Passed,
                output: Some(output.clone()),
                started_at: Some(now),
                finished_at: Some(now),
            };
            state.gate_results.save(&result).await
        }
    };
    if let Err(e) = saved {
        warn!(
            mr_id = %mr.id,
            error = %e,
            "failed to record meta-spec drift gate result"
        );
    }

    warn!(
        mr_id = %mr.id,
        produced_set = %drift.produced_set_sha,
        current_set = %drift.current_set_sha,
        "meta-spec drift detected (warn only, merge proceeds)"
    );
}

/// Check if all `depends_on` for the given MR have status `Merged`.
/// Returns `Ok(true)` if all dependencies are satisfied, `Ok(false)` if any are pending.
async fn dependencies_satisfied(state: &AppState, mr_id: &Id) -> anyhow::Result<bool> {
    let mr = match state.merge_requests.find_by_id(mr_id).await? {
        Some(m) => m,
        None => return Ok(true), // MR not found — let the processor handle the error
    };
    for dep in &mr.depends_on {
        match state.merge_requests.find_by_id(&dep.target_mr_id).await? {
            Some(dep_mr) if dep_mr.status == MrStatus::Merged => continue,
            Some(_) => return Ok(false), // dependency not yet merged
            None => return Ok(false),    // missing dep — block until resolved
        }
    }
    Ok(true)
}

/// Check if all members of an atomic group are ready (all gates passed, deps satisfied).
/// Returns `Ok(true)` if no group or all members are ready.
async fn atomic_group_ready(state: &AppState, group: &str, mr_id: &Id) -> anyhow::Result<bool> {
    let all_mrs = state.merge_requests.list().await?;
    let members: Vec<_> = all_mrs
        .iter()
        .filter(|m| m.atomic_group.as_deref() == Some(group))
        .collect();

    for member in &members {
        if member.id == *mr_id {
            continue; // the current MR is handled by the caller
        }
        // Check deps satisfied for each group member
        if !dependencies_satisfied(state, &member.id).await? {
            return Ok(false);
        }
        // Check if the member is in the queue and gates are ready
        match crate::gate_executor::check_gates_for_mr(state, &member.id).await {
            Ok(true) => {}
            _ => return Ok(false),
        }
    }
    Ok(true)
}

/// Check dependencies for health issues and handle them (P5):
///
/// - Closed deps: creates a remediation task and fails the queue entry.
/// - Deps with 3+ gate failures: logs an escalation warning.
///
/// Returns `true` if a blocking issue was found (caller should skip further processing).
async fn handle_dep_health_issues(
    state: &AppState,
    entry: &MergeQueueEntry,
    mr: &MergeRequest,
) -> anyhow::Result<bool> {
    for dep in &mr.depends_on {
        let dep_id = &dep.target_mr_id;
        let dep_mr = match state.merge_requests.find_by_id(dep_id).await? {
            Some(d) => d,
            None => continue,
        };

        // Case 1: Dependency MR was closed before merging.
        if dep_mr.status == MrStatus::Closed {
            warn!(
                mr_id = %mr.id,
                dep_id = %dep_id,
                "dependency MR was closed; failing queue entry and creating reassessment task"
            );

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let task_id = Id::new(Uuid::new_v4().to_string());
            let mut task = gyre_domain::Task::new(
                task_id,
                format!("Dependency MR-{dep_id} was closed, reassess MR-{}", mr.id),
                now,
            );
            task.priority = TaskPriority::High;
            task.labels = vec!["dep-failure".to_string(), "auto-created".to_string()];
            task.description = Some(format!(
                "Dependency merge request {dep_id} was closed before merging. \
                 Review whether MR {} can still proceed or needs to be updated.",
                mr.id
            ));
            if let Err(e) = state.tasks.create(&task).await {
                warn!(mr_id = %mr.id, "failed to create dep-failure task: {e}");
            }

            state
                .merge_queue
                .update_status(
                    &entry.id,
                    MergeQueueEntryStatus::Failed,
                    Some(format!("dependency MR-{dep_id} was closed")),
                )
                .await?;

            return Ok(true);
        }

        // Case 2: Dependency has 3+ gate failures — log escalation.
        let gate_fail_count = state
            .gate_results
            .list_by_mr_id(dep_id.as_str())
            .await
            .unwrap_or_default()
            .iter()
            .filter(|r| matches!(r.status, GateStatus::Failed))
            .count();

        if gate_fail_count >= 3 {
            warn!(
                mr_id = %mr.id,
                dep_id = %dep_id,
                gate_fail_count,
                "dependency MR has {} gate failures; escalating — manual intervention may be needed",
                gate_fail_count,
            );
        }
    }

    Ok(false)
}

/// Build a dependency graph from queued entries and their MRs.
///
/// Returns an adjacency list (mr_id → set of mr_ids it depends on) restricted to
/// entries currently in the queue. Dependencies on MRs outside the queue (already
/// merged or not yet queued) are not included — they are handled by
/// `dependencies_satisfied()` at merge time.
async fn build_queue_dependency_graph(
    state: &AppState,
    queued_entries: &[MergeQueueEntry],
) -> anyhow::Result<HashMap<String, HashSet<String>>> {
    let queued_mr_ids: HashSet<String> = queued_entries
        .iter()
        .map(|e| e.merge_request_id.to_string())
        .collect();

    let mut graph: HashMap<String, HashSet<String>> = HashMap::new();

    // Initialize every queued MR with an empty dependency set.
    for mr_id in &queued_mr_ids {
        graph.entry(mr_id.clone()).or_default();
    }

    // Populate edges from depends_on.
    for entry in queued_entries {
        let mr_id = entry.merge_request_id.to_string();
        if let Ok(Some(mr)) = state
            .merge_requests
            .find_by_id(&entry.merge_request_id)
            .await
        {
            for dep in &mr.depends_on {
                let dep_str = dep.target_mr_id.to_string();
                if queued_mr_ids.contains(&dep_str) {
                    graph.entry(mr_id.clone()).or_default().insert(dep_str);
                }
            }

            // Atomic group members imply ordering: within the same group, members
            // form a dependency chain in their enqueue order. This ensures the
            // topological sort respects atomic group sequencing.
            if let Some(ref group) = mr.atomic_group {
                let group_members: Vec<_> = queued_entries
                    .iter()
                    .filter(|e| {
                        e.merge_request_id != entry.merge_request_id
                            && queued_mr_ids.contains(&e.merge_request_id.to_string())
                    })
                    .collect();

                for other in &group_members {
                    if let Ok(Some(other_mr)) = state
                        .merge_requests
                        .find_by_id(&other.merge_request_id)
                        .await
                    {
                        if other_mr.atomic_group.as_deref() == Some(group) {
                            // Earlier-enqueued member is a dependency of later-enqueued member.
                            if other.enqueued_at < entry.enqueued_at {
                                graph
                                    .entry(mr_id.clone())
                                    .or_default()
                                    .insert(other.merge_request_id.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(graph)
}

/// Topological sort using Kahn's algorithm with priority-aware tier ordering.
///
/// Within each tier of independent MRs (nodes whose in-queue dependencies are all
/// resolved), higher-priority entries are processed first. This implements the spec's
/// "Priority Within Dependency Tiers" requirement: dependencies define ordering
/// constraints, but within independent MRs, priority still applies.
///
/// Returns entries in processing order.
fn topological_sort_with_priority(
    entries: &[MergeQueueEntry],
    graph: &HashMap<String, HashSet<String>>,
) -> Vec<MergeQueueEntry> {
    // Build a priority lookup: mr_id → (priority, enqueued_at) for tie-breaking.
    let entry_map: HashMap<String, &MergeQueueEntry> = entries
        .iter()
        .map(|e| (e.merge_request_id.to_string(), e))
        .collect();

    // Compute in-degree for each node.
    // in_deg[node] = number of in-graph dependencies node has (edges INTO node).
    let mut in_deg: HashMap<String, usize> = graph.keys().map(|k| (k.clone(), 0)).collect();
    for (node, deps) in graph {
        for dep in deps {
            if graph.contains_key(dep) {
                // `node` depends on `dep`, so `node` has an incoming edge from `dep`.
                // But in-degree counts edges INTO a node. `dep → node` means in_deg[node] += 1.
                // Wait — the graph is mr_id → {its dependencies}. So graph[A] = {B} means
                // A depends on B, i.e., edge B → A (B must come before A).
                // in_degree[A] should count how many dependencies A has in the graph.
                *in_deg.entry(node.clone()).or_insert(0) += 1;
            }
        }
    }

    let mut result = Vec::new();
    let mut available: Vec<String> = in_deg
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(id, _)| id.clone())
        .collect();

    while !available.is_empty() {
        // Sort available nodes by priority descending, then enqueued_at ascending for stability.
        available.sort_by(|a, b| {
            let ea = entry_map.get(a);
            let eb = entry_map.get(b);
            let pa = ea.map(|e| e.priority).unwrap_or(0);
            let pb = eb.map(|e| e.priority).unwrap_or(0);
            let ta = ea.map(|e| e.enqueued_at).unwrap_or(u64::MAX);
            let tb = eb.map(|e| e.enqueued_at).unwrap_or(u64::MAX);
            pb.cmp(&pa).then(ta.cmp(&tb))
        });

        // Take the highest-priority available node.
        let node = available.remove(0);

        if let Some(entry) = entry_map.get(&node) {
            result.push((*entry).clone());
        }

        // Remove this node's outgoing edges (other nodes that depend on it).
        for (other, deps) in graph.iter() {
            if deps.contains(&node) {
                if let Some(deg) = in_deg.get_mut(other) {
                    *deg = deg.saturating_sub(1);
                    if *deg == 0
                        && !result
                            .iter()
                            .any(|e| e.merge_request_id.to_string() == *other)
                    {
                        available.push(other.clone());
                    }
                }
            }
        }
    }

    result
}

/// Compute the maximum dependency chain depth in the graph using topological sort + DP.
///
/// Uses topological ordering to compute the longest path (chain depth) from any root
/// to any leaf. BFS finds shortest paths and would underestimate depth in DAGs with
/// diamond patterns; this algorithm computes the correct longest path in a DAG.
///
/// Returns the maximum chain depth (0 = no dependencies, 1 = one level, etc.).
fn compute_max_chain_depth(graph: &HashMap<String, HashSet<String>>) -> usize {
    if graph.is_empty() {
        return 0;
    }

    // Compute in-degree for topological ordering.
    let mut in_deg: HashMap<String, usize> = graph.keys().map(|k| (k.clone(), 0)).collect();
    for (node, deps) in graph {
        for dep in deps {
            if graph.contains_key(dep) {
                *in_deg.entry(node.clone()).or_insert(0) += 1;
            }
        }
    }

    // Kahn's algorithm to get topological order.
    let mut queue: VecDeque<String> = in_deg
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(id, _)| id.clone())
        .collect();

    // depth[node] = longest path ending at node.
    let mut depth: HashMap<String, usize> = graph.keys().map(|k| (k.clone(), 0)).collect();

    let mut topo_order = Vec::new();
    while let Some(node) = queue.pop_front() {
        topo_order.push(node.clone());
        // For every other node that depends on `node`, reduce in-degree.
        for (other, deps) in graph.iter() {
            if deps.contains(&node) {
                let new_depth = depth.get(&node).copied().unwrap_or(0) + 1;
                let entry = depth.entry(other.clone()).or_insert(0);
                if new_depth > *entry {
                    *entry = new_depth;
                }
                if let Some(deg) = in_deg.get_mut(other) {
                    *deg = deg.saturating_sub(1);
                    if *deg == 0 {
                        queue.push_back(other.clone());
                    }
                }
            }
        }
    }

    depth.values().copied().max().unwrap_or(0)
}

/// Emit a warning notification when the dependency chain depth exceeds 10 levels.
///
/// Per merge-dependencies.md §Failure Handling: "Dependency chain too deep (>10 levels):
/// Warning to repo orchestrator. Not rejected, but flagged as a decomposition smell."
async fn warn_deep_chain(state: &AppState, depth: usize, workspace_id: &Id) {
    // caller-scope:ok — single workspace notification, not iterating cross-workspace entities
    warn!(
        chain_depth = depth,
        workspace_id = workspace_id.as_str(),
        "MR dependency chain is {} levels deep (>10) — decomposition smell",
        depth
    );

    let members = state
        .workspace_memberships
        .list_by_workspace(workspace_id)
        .await
        .unwrap_or_default();

    let body_json = serde_json::json!({
        "chain_depth": depth,
        "threshold": 10,
        "workspace_id": workspace_id.as_str(),
    })
    .to_string();

    for member in &members {
        crate::notifications::notify_rich(
            state,
            workspace_id.clone(),
            member.user_id.clone(),
            gyre_common::NotificationType::DependencyChainTooDeep,
            format!("MR dependency chain is {depth} levels deep — consider decomposing further"),
            "default",
            Some(body_json.clone()),
            None,
            None,
            "workspace",
            None,
        )
        .await;
    }
}

/// Merge all members of an atomic group as a single transactional unit.
///
/// Per merge-dependencies.md §Atomic Group Rules:
/// 1. All members must have all gates passed before any member merges.
/// 2. The merge processor locks the queue and merges all members in dependency
///    order with no interleaving.
/// 3. If any member fails to merge (conflict), the entire group is rolled back
///    and requeued.
/// 4. Atomic groups imply dependency ordering among members.
///
/// Returns `Ok(())` on success or failure — errors are handled internally
/// (rollback, requeue, notify). Only propagates infrastructure errors.
async fn merge_atomic_group(
    state: &AppState,
    group_name: &str,
    first_mr: &MergeRequest,
    sorted: &[MergeQueueEntry],
    queued_entries: &[MergeQueueEntry],
) -> anyhow::Result<()> {
    // Collect all group members from the queued entries, sorted by topological order.
    let group_member_ids: HashSet<String> = {
        let all_mrs = state.merge_requests.list().await?;
        all_mrs
            .iter()
            .filter(|m| m.atomic_group.as_deref() == Some(group_name))
            .map(|m| m.id.to_string())
            .collect()
    };

    // Filter sorted entries to only those in this group, preserving topological order.
    let group_entries: Vec<MergeQueueEntry> = sorted
        .iter()
        .filter(|e| group_member_ids.contains(&e.merge_request_id.to_string()))
        .filter(|e| {
            queued_entries
                .iter()
                .any(|q| q.id == e.id && q.status == MergeQueueEntryStatus::Queued)
        })
        .cloned()
        .collect();

    if group_entries.is_empty() {
        return Ok(());
    }

    info!(
        group = %group_name,
        member_count = group_entries.len(),
        "starting atomic group transactional merge"
    );

    // Look up the repository (all group members share the same repo).
    let repo = match state.repos.find_by_id(&first_mr.repository_id).await? {
        Some(r) => r,
        None => {
            warn!(repo_id = %first_mr.repository_id, "repository not found for atomic group");
            for ge in &group_entries {
                state
                    .merge_queue
                    .update_status(
                        &ge.id,
                        MergeQueueEntryStatus::Failed,
                        Some("repository not found".to_string()),
                    )
                    .await?;
            }
            return Ok(());
        }
    };

    // Post-merge recovery (TASK-095): if the queue is paused for this repo
    // (a previous merge failed post-merge validation and main is red), do
    // not merge. Group entries are still Queued — leave them for retry once
    // the queue resumes.
    if merge_queue_paused(state, &repo.id).await {
        info!(
            group = %group_name,
            repo_id = %repo.id,
            "merge queue paused for repo, leaving atomic group queued"
        );
        return Ok(());
    }

    // meta-spec-reconciliation.md §9 — the drift gate for every group
    // member is evaluated during candidate selection (step 4c'' in
    // `process_next`), which expands atomic-group candidates to all group
    // members: a strictly-blocked drifted member holds the whole group
    // (each member entry is requeued with the drift reason) while later
    // entries in the queue still merge in the same cycle, and warn-only
    // drift records each member's §9 warning gate result before the
    // transactional merge starts. Nothing to re-check here.

    // Record the target branch HEAD before starting, for rollback.
    let target_branch = &first_mr.target_branch;
    let pre_group_sha =
        crate::git_refs::resolve_ref(&repo.path, &format!("refs/heads/{target_branch}")).await;

    // Mark all group members as Processing.
    for ge in &group_entries {
        state
            .merge_queue
            .update_status(&ge.id, MergeQueueEntryStatus::Processing, None)
            .await?;
    }

    // Track successfully merged entries for potential rollback.
    let mut merged_entries: Vec<(MergeQueueEntry, MergeRequest)> = Vec::new();
    // Merge commits produced in this group run, for post-merge validation
    // (TASK-095). Preserves member order; used to identify which member's
    // merge failed the post-merge gate.
    let mut group_merge_shas: Vec<String> = Vec::new();

    // Sequentially merge each member in dependency order.
    for ge in &group_entries {
        let mr = match state
            .merge_requests
            .find_by_id(&ge.merge_request_id)
            .await?
        {
            Some(m) => m,
            None => {
                warn!(mr_id = %ge.merge_request_id, "MR not found during atomic group merge");
                // Rollback and requeue.
                rollback_atomic_group(
                    state,
                    group_name,
                    &repo,
                    target_branch,
                    pre_group_sha.as_deref(),
                    &merged_entries,
                    &group_entries,
                    &format!("MR {} not found", ge.merge_request_id),
                    &ge.merge_request_id,
                )
                .await?;
                return Ok(());
            }
        };

        // R4: re-validate member MR status at fetch time. The selection
        // loop's 4b'' check (task-095 R3-F2) validates only the first
        // candidate it dispatches on; a later-enqueued group member that
        // was closed or reverted while its entry sat Queued must not be
        // silently merged by the group path — same rule as the single
        // path. Closed/Reverted are terminal MR states, so unlike the
        // conflict path (which requeues and retries), the stale member's
        // entry is failed permanently and the partial group merge is
        // rolled back; the healthy members proceed as a reduced group on
        // the next cycle. `rollback_atomic_group` requeues every entry,
        // so the Failed status must be set AFTER it to stay terminal.
        if !matches!(mr.status, MrStatus::Open | MrStatus::Approved) {
            warn!(
                group = %group_name,
                entry_id = %ge.id,
                mr_id = %mr.id,
                status = ?mr.status,
                "atomic group member MR is not mergeable, failing entry and rolling back group"
            );
            rollback_atomic_group(
                state,
                group_name,
                &repo,
                target_branch,
                pre_group_sha.as_deref(),
                &merged_entries,
                &group_entries,
                &format!(
                    "member MR {} is not mergeable (status {:?})",
                    mr.id, mr.status
                ),
                &mr.id,
            )
            .await?;
            state
                .merge_queue
                .update_status(
                    &ge.id,
                    MergeQueueEntryStatus::Failed,
                    Some(format!(
                        "merge request status {:?} cannot be merged",
                        mr.status
                    )),
                )
                .await?;
            return Ok(());
        }

        // P5: Check dependency health for this member.
        if handle_dep_health_issues(state, ge, &mr).await? {
            // Dep health issue found — rollback the group.
            rollback_atomic_group(
                state,
                group_name,
                &repo,
                target_branch,
                pre_group_sha.as_deref(),
                &merged_entries,
                &group_entries,
                &format!("dependency health issue for MR {}", mr.id),
                &mr.id,
            )
            .await?;
            return Ok(());
        }

        // TASK-077 / HSI §2 (F5): merge-time ABAC gate for the merge
        // processor's internal service identity — same gate as the
        // single-entry path; without it the atomic group path would be a
        // Supervised bypass. Human-approval escape: an MR already Approved
        // (by a human, via the status endpoint) proceeds.
        // The selection loop (Step 4c') already skips groups with a held
        // member, so this is defense in depth against TOCTOU — a member
        // becoming held between selection and group processing still rolls
        // the group back, without starving the queue.
        if let Some(reason) = supervised_trust_hold(state, &mr, &repo).await {
            warn!(
                group = %group_name,
                mr_id = %mr.id,
                workspace_id = %mr.workspace_id,
                "supervised trust: holding atomic group for human MR approval"
            );
            rollback_atomic_group(
                state,
                group_name,
                &repo,
                target_branch,
                pre_group_sha.as_deref(),
                &merged_entries,
                &group_entries,
                &reason,
                &mr.id,
            )
            .await?;
            return Ok(());
        }

        // Attempt the merge for this member.
        let result = state
            .git_ops
            .merge_branches(&repo.path, &mr.source_branch, &mr.target_branch)
            .await;

        match result {
            Ok(MergeResult::Success { merge_commit_sha }) => {
                info!(
                    group = %group_name,
                    mr_id = %mr.id,
                    sha = %merge_commit_sha,
                    "atomic group member merged successfully"
                );

                // Update MR status to Merged.
                let mut updated_mr = mr.clone();
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                // R3-F1: record the member's own merge commit SHA.
                updated_mr.merge_commit_sha = Some(merge_commit_sha.clone());
                updated_mr.updated_at = now;
                if updated_mr.status == MrStatus::Open {
                    let _ = updated_mr.transition_status(MrStatus::Approved);
                }
                if let Err(e) = updated_mr.transition_status(MrStatus::Merged) {
                    warn!("could not transition MR to Merged: {e}");
                }
                let _ = state.merge_requests.update(&updated_mr).await;
                // HSI §3a: promote the MR's gate trace to attestation on merge.
                if let Err(e) = state.traces.promote_to_attestation(&updated_mr.id).await {
                    warn!(mr_id = %updated_mr.id, error = %e, "failed to promote gate trace to attestation");
                }

                state
                    .merge_queue
                    .update_status(&ge.id, MergeQueueEntryStatus::Merged, None)
                    .await?;

                merged_entries.push((ge.clone(), updated_mr));
                // Track the merge commit for post-merge validation (TASK-095).
                group_merge_shas.push(merge_commit_sha.clone());
            }
            Ok(MergeResult::Conflict { message }) => {
                warn!(
                    group = %group_name,
                    mr_id = %mr.id,
                    reason = %message,
                    "atomic group member merge conflict — rolling back entire group"
                );
                rollback_atomic_group(
                    state,
                    group_name,
                    &repo,
                    target_branch,
                    pre_group_sha.as_deref(),
                    &merged_entries,
                    &group_entries,
                    &format!("conflict in MR {}: {}", mr.id, message),
                    &mr.id,
                )
                .await?;
                return Ok(());
            }
            Err(e) => {
                error!(
                    group = %group_name,
                    mr_id = %mr.id,
                    error = %e,
                    "atomic group member git error — rolling back entire group"
                );
                rollback_atomic_group(
                    state,
                    group_name,
                    &repo,
                    target_branch,
                    pre_group_sha.as_deref(),
                    &merged_entries,
                    &group_entries,
                    &format!("git error in MR {}: {}", mr.id, e),
                    &mr.id,
                )
                .await?;
                return Ok(());
            }
        }
    }

    // All group members merged successfully — now emit deferred side effects
    // (notifications and analytics). These are deferred to avoid irretractable
    // emissions inside the rollback-capable merge loop.
    for (ge, updated_mr) in &merged_entries {
        let now = updated_mr.updated_at;
        let ev = AnalyticsEvent::new(
            Id::new(Uuid::new_v4().to_string()),
            "merge_queue.processed",
            updated_mr.author_agent_id.as_ref().map(|id| id.to_string()),
            serde_json::json!({
                "entry_id": ge.id.to_string(),
                "mr_id": updated_mr.id.to_string(),
                "result": "merged",
                "atomic_group": group_name,
                // Scope facets (search.md §Searchable Entities: Activity
                // Event is filterable by workspace/repo).
                "workspace_id": updated_mr.workspace_id.to_string(),
                "repo_id": updated_mr.repository_id.to_string(),
            }),
            now,
        );
        let _ = state.analytics.record(&ev).await;
        // search.md §Searchable Entities: activity events are indexed on write.
        crate::search_indexer::index(state, crate::search_indexer::activity_event_document(&ev))
            .await;

        if let Some(ref author_id) = updated_mr.author_agent_id {
            crate::notifications::notify_mr_merged(
                state,
                author_id,
                &updated_mr.workspace_id,
                &updated_mr.id.to_string(),
                "default",
            )
            .await;
        }
    }

    info!(
        group = %group_name,
        merged_count = merged_entries.len(),
        "atomic group merged successfully"
    );

    // TASK-095: post-merge validation for the atomic group. The post-merge
    // gate runs against the new HEAD of the default branch after the group
    // landed — same rule as single-entry merges. On failure, run the
    // recovery protocol for the group: pause queue, revert the group's
    // merges, mark members Reverted, notify, remediation tasks, invalidate
    // gate results. See platform-model.md §6.
    if let Some(last_sha) = group_merge_shas.last() {
        if let Err(failure_reason) =
            crate::gate_executor::run_post_merge_gates(state, &repo, last_sha).await
        {
            recover_atomic_group_from_post_merge_failure(
                state,
                &repo,
                &merged_entries,
                &group_merge_shas,
                &failure_reason,
            )
            .await;
        } else {
            // source-control.md §4: the group landing moved the baseline —
            // rebase the other in-flight agents onto the new HEAD, after
            // the group's post-merge gates pass (R2 F3: never rebase onto
            // a base a group-recovery revert could strand).
            let merged_authors: Vec<Id> = merged_entries
                .iter()
                .filter_map(|(_, m)| m.author_agent_id.clone())
                .collect();
            rebase_inflight_agents(state, &repo, target_branch, last_sha, &merged_authors).await;
        }
    }

    Ok(())
}

/// Roll back an atomic group merge: reset the target branch, requeue all members,
/// and notify all distinct authors.
async fn rollback_atomic_group(
    state: &AppState,
    group_name: &str,
    repo: &gyre_domain::Repository,
    target_branch: &str,
    pre_group_sha: Option<&str>,
    merged_entries: &[(MergeQueueEntry, MergeRequest)],
    all_group_entries: &[MergeQueueEntry],
    failure_reason: &str,
    failing_mr_id: &Id,
) -> anyhow::Result<()> {
    // Step 1: Reset the target branch to the pre-group SHA (undo already-merged members).
    if !merged_entries.is_empty() {
        if let Some(sha) = pre_group_sha {
            info!(
                group = %group_name,
                target_branch = %target_branch,
                reset_to = %sha,
                merged_count = merged_entries.len(),
                "rolling back atomic group: resetting target branch"
            );
            if let Err(e) = state
                .git_ops
                .reset_branch(&repo.path, target_branch, sha)
                .await
            {
                error!(
                    group = %group_name,
                    error = %e,
                    "failed to reset branch during atomic group rollback"
                );
            }
        } else {
            warn!(
                group = %group_name,
                "no pre-group SHA recorded; cannot reset branch"
            );
        }

        // Revert MR statuses for already-merged members.
        for (_, mr) in merged_entries {
            let mut reverted_mr = mr.clone();
            reverted_mr.status = MrStatus::Open;
            reverted_mr.updated_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let _ = state.merge_requests.update(&reverted_mr).await;
        }
    }

    // Step 2: Requeue all group members (set status back to Queued with failure reason).
    for ge in all_group_entries {
        state
            .merge_queue
            .update_status(
                &ge.id,
                MergeQueueEntryStatus::Queued,
                Some(format!("atomic group rollback: {failure_reason}")),
            )
            .await?;
    }

    // Step 3: Collect distinct author agent IDs from all group members.
    let mut author_ids: HashSet<String> = HashSet::new();
    let mut workspace_id: Option<Id> = None;
    for ge in all_group_entries {
        if let Ok(Some(mr)) = state.merge_requests.find_by_id(&ge.merge_request_id).await {
            if let Some(ref author) = mr.author_agent_id {
                author_ids.insert(author.to_string());
            }
            if workspace_id.is_none() {
                workspace_id = Some(mr.workspace_id.clone());
            }
        }
    }

    let Some(ws_id) = workspace_id else {
        warn!(
            group = %group_name,
            "atomic group rolled back, but workspace lookup failed; skipping scoped notifications"
        );
        return Ok(());
    };

    // Step 4: Notify all distinct authors (spec says "notify all authors" — no broader audience).
    let body_json = serde_json::json!({
        "group": group_name,
        "failing_mr_id": failing_mr_id.to_string(),
        "failure_reason": failure_reason,
        "member_count": all_group_entries.len(),
        "rolled_back_count": merged_entries.len(),
    })
    .to_string();

    for author_id in &author_ids {
        // The author is an AGENT; notifications live in HUMAN inboxes. Resolve
        // the agent's spawning user (falling back to the agent id itself when
        // no human is recorded) — same rule as notify_gate_failure /
        // notify_mr_merged. Addressing the raw agent id delivers to a user
        // that does not exist, so no human ever sees the failure notice.
        let author = Id::new(author_id);
        let user_id = match state.agents.find_by_id(&author).await {
            Ok(Some(agent)) => match &agent.spawned_by {
                Some(sb) => Id::new(sb.clone()),
                None => author.clone(),
            },
            _ => author.clone(),
        };
        crate::notifications::notify_rich(
            state,
            ws_id.clone(),
            user_id,
            gyre_common::NotificationType::AtomicGroupFailure,
            format!(
                "Atomic group '{}' failed: {} — all members rolled back and requeued",
                group_name, failure_reason
            ),
            "default",
            Some(body_json.clone()),
            Some(failing_mr_id.to_string()),
            Some(repo.id.to_string()),
            "mr",
            None,
        )
        .await;
    }

    // Step 5: Emit AtomicGroupFailed event.
    state
        .emit_event(
            Some(ws_id.clone()),
            gyre_common::message::Destination::Workspace(ws_id),
            gyre_common::MessageKind::AtomicGroupFailed,
            Some(serde_json::json!({
                "group": group_name,
                "failing_mr_id": failing_mr_id.to_string(),
                "failure_reason": failure_reason,
                "member_count": all_group_entries.len(),
                "rolled_back_count": merged_entries.len(),
                "member_mr_ids": all_group_entries.iter().map(|e| e.merge_request_id.to_string()).collect::<Vec<_>>(),
            })),
        )
        .await;

    warn!(
        group = %group_name,
        failing_mr = %failing_mr_id,
        "atomic group rollback complete: {} members requeued, {} authors notified",
        all_group_entries.len(),
        author_ids.len(),
    );

    Ok(())
}

async fn process_next(state: &AppState) -> anyhow::Result<()> {
    // Replay deferred rebases whose backoff window has expired on EVERY
    // cycle, not only idle ones (R3 F12): under sustained merge-queue load —
    // source-control.md §4's "20+ agents pushing concurrently" scenario —
    // the queue never fully drains, and idle-only replay would starve
    // expired deferrals until it does. The per-agent backoff window checked
    // inside the replay is the rate limit: at most one rebase attempt per
    // agent per window, and the processor's 5s cycle bounds the scan cost.
    replay_pending_rebases(state).await;

    // Step 1: Get all queued entries.
    let all_queued = state.merge_queue.list_queue().await?;
    let queued_entries: Vec<_> = all_queued
        .into_iter()
        .filter(|e| e.status == MergeQueueEntryStatus::Queued)
        .collect();

    if queued_entries.is_empty() {
        return Ok(());
    }

    // Step 2: Build dependency graph from depends_on + atomic_group.
    let graph = build_queue_dependency_graph(state, &queued_entries).await?;

    // Step 2b: Check chain depth and warn if >10.
    let max_depth = compute_max_chain_depth(&graph);
    if max_depth > 10 {
        // Find a workspace_id from any queued entry for the notification target.
        if let Some(first) = queued_entries.first() {
            if let Ok(Some(mr)) = state
                .merge_requests
                .find_by_id(&first.merge_request_id)
                .await
            {
                warn_deep_chain(state, max_depth, &mr.workspace_id).await;
            }
        }
    }

    // Step 3: Topological sort (respecting priority within each tier).
    let sorted = topological_sort_with_priority(&queued_entries, &graph);

    // Step 4: For each entry in topological order, check deps (4a), gates (4b),
    // and atomic group readiness (4c). Skip entries that aren't ready; select
    // the first entry that passes all checks.
    let (entry, mr) = {
        let mut found = None;
        for candidate in &sorted {
            // Step 4a: Are all dependencies Merged?
            if !dependencies_satisfied(state, &candidate.merge_request_id).await? {
                continue;
            }

            // Step 4b: Are all gates passed?
            match crate::gate_executor::check_gates_for_mr(state, &candidate.merge_request_id).await
            {
                Ok(true) => {}         // all gates passed (or no gates)
                Ok(false) => continue, // gates still running — skip, try next candidate
                Err(reason) => {
                    // Gate failed — fail this entry and try next candidate.
                    warn!(
                        entry_id = %candidate.id,
                        reason = %reason,
                        "quality gate failed during candidate selection, failing entry"
                    );
                    state
                        .merge_queue
                        .update_status(
                            &candidate.id,
                            MergeQueueEntryStatus::Failed,
                            Some(format!("quality gate failed: {reason}")),
                        )
                        .await?;
                    continue;
                }
            }

            // Look up the MR (needed for atomic group check and later processing).
            let candidate_mr = match state
                .merge_requests
                .find_by_id(&candidate.merge_request_id)
                .await?
            {
                Some(mr) => mr,
                None => {
                    warn!(mr_id = %candidate.merge_request_id, "MR not found for queue entry, skipping");
                    state
                        .merge_queue
                        .update_status(
                            &candidate.id,
                            MergeQueueEntryStatus::Failed,
                            Some("merge request not found".to_string()),
                        )
                        .await?;
                    continue;
                }
            };
            // Step 4b'': platform-model.md §6 — only a mergeable MR may
            // merge. A queue entry that references an MR reverted, closed,
            // or already merged after enqueue (e.g. the recovery protocol
            // reverted it while the entry sat queued) must not silently
            // re-merge it (task-095 R3-F2). Fail the entry explicitly.
            if !matches!(candidate_mr.status, MrStatus::Open | MrStatus::Approved) {
                warn!(
                    entry_id = %candidate.id,
                    mr_id = %candidate_mr.id,
                    status = ?candidate_mr.status,
                    "queue entry references non-mergeable MR, failing entry"
                );
                state
                    .merge_queue
                    .update_status(
                        &candidate.id,
                        MergeQueueEntryStatus::Failed,
                        Some(format!(
                            "merge request status {:?} cannot be merged",
                            candidate_mr.status
                        )),
                    )
                    .await?;
                continue;
            }

            // Step 4b': Recovery protocol — skip entries in paused repos
            // (broken main; queue stays paused until main is green).
            if merge_queue_paused(state, &candidate_mr.repository_id).await {
                info!(
                    entry_id = %candidate.id,
                    repo_id = %candidate_mr.repository_id,
                    "merge queue paused for repo, skipping candidate"
                );
                continue;
            }

            // Step 4c: Is this part of an atomic group? If so, are all members ready?
            if let Some(ref group) = candidate_mr.atomic_group {
                match atomic_group_ready(state, group, &candidate_mr.id).await {
                    Ok(true) => {}
                    Ok(false) => {
                        info!(
                            entry_id = %candidate.id,
                            group = %group,
                            "atomic group not ready, skipping to next candidate"
                        );
                        continue;
                    }
                    Err(e) => {
                        warn!(entry_id = %candidate.id, error = %e, "error checking atomic group, skipping");
                        continue;
                    }
                }
            }

            // Step 4c': TASK-077 / HSI §2 (F5) supervised-trust hold,
            // evaluated DURING selection (review repair 6425cc46). A
            // trust-held candidate is skipped exactly like the other
            // not-ready conditions above so the processor tries the next
            // candidate in the SAME cycle. Evaluating the hold only after
            // selection and returning early starves the shared queue:
            // `update_status` does not rotate `enqueued_at`, so a held
            // head-of-queue entry is re-selected and re-held every cycle,
            // monopolizing the processor and blocking later entries —
            // cross-workspace Guided/Autonomous merges and same-workspace
            // human-approved MRs (the spec's own review-escape) alike.
            // The entry stays Queued with a hold reason so the
            // human-approval path stays live; once a human approves the MR
            // via the status endpoint, the gate passes and it merges.
            if let Some(reason) = selection_trust_hold(state, &candidate_mr).await? {
                info!(
                    entry_id = %candidate.id,
                    mr_id = %candidate.merge_request_id,
                    %reason,
                    "supervised trust: holding merge for human MR approval, trying next candidate"
                );
                state
                    .merge_queue
                    .update_status(&candidate.id, MergeQueueEntryStatus::Queued, Some(reason))
                    .await?;
                continue;
            }

            // Step 4c'': meta-spec-reconciliation.md §9 — strict-policy
            // meta-spec drift hold, evaluated DURING selection for the
            // same starvation reason as the trust hold above (task-159
            // review repair): a strictly-blocked drifted entry is requeued
            // with the drift reason and skipped, so the processor tries
            // the next candidate in the SAME cycle. Evaluating the drift
            // hold only after selection and returning early starves the
            // shared queue — `update_status` does not rotate
            // `enqueued_at`, so a drifted head-of-queue entry under
            // `require_current_meta_spec_set` would be re-selected and
            // re-blocked every cycle, denying merge service to later
            // entries in the same workspace and to entries in other
            // workspaces. Warn-only drift (the default) records the §9
            // warning gate result in the same helper and never blocks.
            if let Some(reason) = selection_meta_spec_drift_hold(state, &candidate_mr).await? {
                info!(
                    entry_id = %candidate.id,
                    mr_id = %candidate.merge_request_id,
                    %reason,
                    "meta-spec drift: holding merge under strict workspace policy, trying next candidate"
                );
                state
                    .merge_queue
                    .update_status(&candidate.id, MergeQueueEntryStatus::Queued, Some(reason))
                    .await?;
                continue;
            }

            found = Some((candidate.clone(), candidate_mr));
            break;
        }
        match found {
            Some(pair) => pair,
            None => return Ok(()),
        }
    };

    info!(entry_id = %entry.id, mr_id = %entry.merge_request_id, "processing merge queue entry");

    // If this entry is part of an atomic group, merge all group members
    // as a transactional unit — no interleaving allowed.
    if let Some(ref group_name) = mr.atomic_group {
        return merge_atomic_group(state, group_name, &mr, &sorted, &queued_entries).await;
    }

    // ── Single-entry merge path (non-atomic-group) ─────────────────────

    // Mark as Processing
    state
        .merge_queue
        .update_status(&entry.id, MergeQueueEntryStatus::Processing, None)
        .await?;

    // P5: Check dependency health (closed deps, gate failure escalation).
    if handle_dep_health_issues(state, &entry, &mr).await? {
        return Ok(());
    }

    // Look up the repository
    let repo = match state.repos.find_by_id(&mr.repository_id).await? {
        Some(r) => r,
        None => {
            warn!(repo_id = %mr.repository_id, "repository not found for MR");
            state
                .merge_queue
                .update_status(
                    &entry.id,
                    MergeQueueEntryStatus::Failed,
                    Some("repository not found".to_string()),
                )
                .await?;
            return Ok(());
        }
    };

    // Post-merge recovery: if the queue is paused for this repo (a previous
    // merge failed post-merge validation and main is red), do not merge.
    // Leave the entry queued so it is retried once the queue resumes.
    if merge_queue_paused(state, &repo.id).await {
        info!(
            entry_id = %entry.id,
            repo_id = %repo.id,
            "merge queue paused for repo, leaving entry queued"
        );
        // The entry was claimed as Processing above; put it back so it is
        // retried once the queue resumes.
        state
            .merge_queue
            .update_status(&entry.id, MergeQueueEntryStatus::Queued, None)
            .await?;
        return Ok(());
    }

    // meta-spec-reconciliation.md §9 — the drift gate was already
    // evaluated during candidate selection (step 4c'' above): warn-only
    // drift recorded its `[WARN] meta-spec-drift` gate result there, and
    // strict-policy drift requeued the entry with the drift reason before
    // it could be selected. Nothing to re-check on the claimed entry.

    // Note: Atomic group readiness (step 4c) is already verified in the
    // selection loop above — no need to re-check here.

    // Check spec enforcement policy before merging (agent-gates.md
    // §Forge Enforcement Policies — five per-repo policies).
    {
        let policy = state
            .spec_policies
            .get_for_repo(&repo.id.to_string())
            .await
            .unwrap_or_default();

        // Blocking policies. `require_signed_approval` also implies a
        // spec_ref is present: an MR with no spec_ref has no referenced
        // approval, signed or otherwise.
        let needs_spec_ref = policy.require_spec_ref
            || policy.require_approved_spec
            || policy.require_signed_approval;
        let mut violation: Option<String> = if needs_spec_ref && mr.spec_ref.is_none() {
            Some("spec policy requires a spec_ref but MR has none".to_string())
        } else {
            None
        };

        if violation.is_none() {
            if let Some(spec_ref) = mr.spec_ref.as_deref() {
                if policy.require_approved_spec {
                    if let Err(reason) = crate::api::gates::verify_spec_ref(state, spec_ref).await {
                        violation = Some(format!("spec approval required: {reason}"));
                    }
                }
                if violation.is_none() && policy.require_signed_approval {
                    if let Err(reason) =
                        crate::api::gates::verify_signed_spec_ref(state, spec_ref).await
                    {
                        violation = Some(format!("signed spec approval required: {reason}"));
                    }
                }
            }
        }

        if let Some(reason) = violation {
            warn!(entry_id = %entry.id, %reason, "spec policy blocked merge");
            state
                .merge_queue
                .update_status(
                    &entry.id,
                    MergeQueueEntryStatus::Failed,
                    Some(reason.clone()),
                )
                .await?;
            report_spec_policy_violation(state, &mr, &repo.id, &reason).await;
            return Ok(());
        }

        // Check if the spec_ref SHA is current (warn_stale_spec / require_current_spec).
        if policy.warn_stale_spec || policy.require_current_spec {
            if let Some(spec_ref) = mr.spec_ref.as_deref() {
                // Parse "path@sha" — same format used by verify_spec_ref.
                if let Some((path, sha)) = spec_ref.rsplit_once('@') {
                    let current = crate::git_refs::resolve_spec_blob_sha(&repo.path, path).await;
                    let is_stale = match &current {
                        // If the file can't be resolved (new/empty repo), treat as non-stale.
                        Some(cur) => cur != sha,
                        None => false,
                    };
                    if is_stale {
                        let current_sha = current.unwrap_or_default();
                        if policy.require_current_spec {
                            let reason = format!(
                                "spec policy requires current spec: '{path}' HEAD is {current_sha} but MR references {sha}"
                            );
                            warn!(entry_id = %entry.id, %reason, "stale spec blocked merge");
                            state
                                .merge_queue
                                .update_status(
                                    &entry.id,
                                    MergeQueueEntryStatus::Failed,
                                    Some(reason.clone()),
                                )
                                .await?;
                            report_spec_policy_violation(state, &mr, &repo.id, &reason).await;
                            return Ok(());
                        } else {
                            // warn_stale_spec only — advisory comment and domain
                            // event, don't block.
                            warn!(
                                entry_id = %entry.id,
                                mr_id = %mr.id,
                                spec_path = %path,
                                spec_sha = %sha,
                                %current_sha,
                                "stale spec_ref detected (warn only)"
                            );
                            record_stale_spec_advisory(state, &mr, path, sha, &current_sha).await;
                            state
                                .emit_event(
                                    Some(mr.workspace_id.clone()),
                                    gyre_common::message::Destination::Workspace(
                                        mr.workspace_id.clone(),
                                    ),
                                    gyre_common::message::MessageKind::StaleSpecWarning,
                                    Some(serde_json::json!({
                                        "mr_id": mr.id.to_string(),
                                        "repo_id": repo.id.to_string(),
                                        "spec_path": path,
                                        "spec_sha": sha,
                                        "current_sha": current_sha,
                                    })),
                                )
                                .await;
                        }
                    }
                }
            }
        }
    }

    // TASK-020: Check breaking change enforcement policy.
    // If the MR's repo has unacknowledged breaking changes and the workspace policy
    // is `block`, reject the merge until all dependent repos acknowledge.
    {
        let breaking_changes = state
            .breaking_changes
            .list_by_source_repo(&mr.repository_id)
            .await
            .unwrap_or_default();

        let unacknowledged: Vec<_> = breaking_changes
            .iter()
            .filter(|bc| !bc.acknowledged)
            .collect();

        if !unacknowledged.is_empty() {
            let dep_policy = state
                .dependency_policies
                .get_for_workspace(&mr.workspace_id)
                .await
                .unwrap_or_default();

            match dep_policy.breaking_change_behavior {
                gyre_domain::BreakingChangeBehavior::Block => {
                    let count = unacknowledged.len();
                    let reason = format!(
                        "dependency policy requires acknowledgment: {count} unacknowledged \
                         breaking change(s) from this repo"
                    );
                    warn!(entry_id = %entry.id, %reason, "breaking change policy blocked merge");
                    state
                        .merge_queue
                        .update_status(&entry.id, MergeQueueEntryStatus::Failed, Some(reason))
                        .await?;
                    return Ok(());
                }
                gyre_domain::BreakingChangeBehavior::Warn => {
                    let count = unacknowledged.len();
                    warn!(
                        entry_id = %entry.id,
                        mr_id = %mr.id,
                        unacknowledged_count = count,
                        "breaking change detected (warn only, proceeding with merge)"
                    );
                }
                gyre_domain::BreakingChangeBehavior::Notify => {
                    // Notify only — no merge blocking or warnings.
                }
            }
        }
    }

    // TASK-019: Check spec link merge gates.
    // spec-links.md §Merge Gates: when an MR references a spec, the forge checks
    // that spec's links for supersession, conflicts, and unimplemented dependencies.
    if let Some(spec_ref) = mr.spec_ref.as_deref() {
        // Parse "path@sha" to get the spec path.
        let spec_path = spec_ref
            .rsplit_once('@')
            .map(|(p, _)| p)
            .unwrap_or(spec_ref);

        // Collect links for the referenced spec without holding lock across awaits.
        let relevant_links: Vec<crate::spec_registry::SpecLinkEntry> = {
            let links = state.spec_links_store.lock().await;
            links
                .iter()
                .filter(|l| l.source_path == spec_path || l.target_path == spec_path)
                .cloned()
                .collect()
        };

        // Check 1: If the spec is superseded (has an inbound `supersedes` link
        // from another spec), reject the merge.
        for link in &relevant_links {
            if link.link_type == crate::spec_registry::SpecLinkType::Supersedes
                && link.target_path == spec_path
            {
                let reason = format!(
                    "spec '{}' has been superseded by '{}'. Update your spec_ref.",
                    spec_path, link.source_path
                );
                warn!(entry_id = %entry.id, %reason, "spec link merge gate blocked merge");
                state
                    .merge_queue
                    .update_status(&entry.id, MergeQueueEntryStatus::Failed, Some(reason))
                    .await?;
                return Ok(());
            }
        }

        // Check 2: If the spec has a `conflicts_with` link to an approved spec,
        // block the merge. conflicts_with is bidirectional (spec-links.md §Cycle Detection),
        // so check both source_path and target_path matching the referenced spec.
        for link in &relevant_links {
            if link.link_type == crate::spec_registry::SpecLinkType::ConflictsWith
                && (link.source_path == spec_path || link.target_path == spec_path)
            {
                let other_spec_path = if link.source_path == spec_path {
                    &link.target_path
                } else {
                    &link.source_path
                };
                if let Ok(Some(conflicting)) = state.spec_ledger.find_by_path(other_spec_path).await
                {
                    if conflicting.approval_status == crate::spec_registry::ApprovalStatus::Approved
                    {
                        let reason = format!(
                            "spec '{}' conflicts with approved spec '{}' — resolve the conflict first",
                            spec_path, other_spec_path
                        );
                        warn!(entry_id = %entry.id, %reason, "spec link merge gate blocked merge");
                        state
                            .merge_queue
                            .update_status(&entry.id, MergeQueueEntryStatus::Failed, Some(reason))
                            .await?;
                        return Ok(());
                    }
                }
            }
        }

        // Check 3: If the spec has `depends_on` links where the target spec's
        // implementation is incomplete, add a warning (not a block).
        for link in &relevant_links {
            if link.link_type == crate::spec_registry::SpecLinkType::DependsOn
                && link.source_path == spec_path
            {
                if let Ok(Some(target)) = state.spec_ledger.find_by_path(&link.target_path).await {
                    if target.approval_status != crate::spec_registry::ApprovalStatus::Approved {
                        warn!(
                            entry_id = %entry.id,
                            mr_id = %mr.id,
                            spec_path = %spec_path,
                            dependency = %link.target_path,
                            dep_status = %target.approval_status,
                            "spec dependency not yet implemented (warning only, not blocking merge)"
                        );
                        state
                            .emit_event(
                                Some(mr.workspace_id.clone()),
                                gyre_common::message::Destination::Workspace(
                                    mr.workspace_id.clone(),
                                ),
                                gyre_common::message::MessageKind::StaleSpecWarning,
                                Some(serde_json::json!({
                                    "mr_id": mr.id.to_string(),
                                    "spec_path": spec_path,
                                    "dependency": link.target_path,
                                    "dependency_status": target.approval_status.to_string(),
                                    "warning": "dependency spec implementation not yet complete",
                                })),
                            )
                            .await;
                    }
                }
            }
        }
    }

    // Note: Quality gates (step 4b) are already verified in the selection
    // loop above — no need to re-check here.

    // Phase 3 (TASK-008): Constraint enforcement — block merge if attestation
    // chain is invalid or any constraint fails. Evaluated before the merge so we
    // can fail the queue entry without leaving a dangling merge commit.
    //
    // We use the source branch HEAD as the "merge commit" for diff computation
    // since the actual merge commit doesn't exist yet.
    if let Some(source_sha) =
        crate::git_refs::resolve_ref(&repo.path, &format!("refs/heads/{}", mr.source_branch)).await
    {
        if let Err(reason) = crate::constraint_check::enforce_merge_constraints(
            state,
            &mr.id.to_string(),
            &repo.id.to_string(),
            &repo.path,
            &source_sha,
            &mr.workspace_id,
            &mr.source_branch,
            &mr.target_branch,
            &repo.default_branch,
        )
        .await
        {
            warn!(
                entry_id = %entry.id,
                mr_id = %mr.id,
                %reason,
                "constraint enforcement blocked merge"
            );
            state
                .merge_queue
                .update_status(&entry.id, MergeQueueEntryStatus::Failed, Some(reason))
                .await?;
            return Ok(());
        }
    }

    // TASK-077 / HSI §2 (F5): merge-time ABAC enforcement for the merge
    // processor's internal service identity. In a Supervised workspace the
    // `trust:require-human-mr-review` Deny matches (subject.type "system",
    // subject.id "merge-processor") and the merge is HELD — not failed —
    // until a human approves the MR via the status endpoint. The processor's
    // own Open → Approved transition happens only after this gate, so it
    // cannot self-satisfy the escape.
    // The selection loop (Step 4c') already skips trust-held candidates, so
    // this is defense in depth against TOCTOU — a policy created or an MR
    // un-approved between selection and processing still holds here, for
    // one cycle, without starving the queue (selection re-skips next cycle).
    if let Some(reason) = supervised_trust_hold(state, &mr, &repo).await {
        warn!(
            entry_id = %entry.id,
            mr_id = %mr.id,
            workspace_id = %mr.workspace_id,
            "supervised trust: holding merge for human MR approval"
        );
        // Requeue (not Failed): Failed is terminal and would permanently
        // block the human-approval path. Queued entries are retried on the
        // next cycle; once a human sets the MR to Approved, the gate
        // passes and the merge proceeds.
        state
            .merge_queue
            .update_status(&entry.id, MergeQueueEntryStatus::Queued, Some(reason))
            .await?;
        return Ok(());
    }

    // TASK-061 (§7.2): Populate attestation chain ABAC subject attributes
    // and evaluate policies with action=merge, resource_type=attestation.
    // The decision gates the merge: a Deny fails the queue entry with the
    // matched policy, mirroring the constraint-enforcement block above
    // (task-077 F5 flaw class — an evaluation whose result is discarded
    // gates nothing). Look up attestation chain via the source branch head
    // commit or agent task.
    {
        let mut chain_found = false;

        // Try by source branch head commit first.
        if let Some(source_sha) =
            crate::git_refs::resolve_ref(&repo.path, &format!("refs/heads/{}", mr.source_branch))
                .await
        {
            if let Ok(Some(chain_att)) = state.chain_attestations.find_by_commit(&source_sha).await
            {
                let chain = state
                    .chain_attestations
                    .load_chain(&chain_att.id)
                    .await
                    .unwrap_or_default();
                let decision = evaluate_attestation_abac(
                    state, &chain, &chain_att, &mr, &entry, &repo, "merge",
                )
                .await;
                chain_found = true;
                if decision.effect == gyre_domain::policy::PolicyEffect::Deny {
                    let reason = format!(
                        "attestation ABAC policy denied merge (matched policy: {:?})",
                        decision.matched_policy
                    );
                    warn!(
                        entry_id = %entry.id,
                        mr_id = %mr.id,
                        matched_policy = ?decision.matched_policy,
                        "attestation ABAC denied merge — failing queue entry"
                    );
                    state
                        .merge_queue
                        .update_status(&entry.id, MergeQueueEntryStatus::Failed, Some(reason))
                        .await?;
                    return Ok(());
                }
            }
        }

        // Fallback: look up via agent's current task.
        if !chain_found {
            if let Some(author_id) = &mr.author_agent_id {
                if let Ok(Some(agent)) = state.agents.find_by_id(author_id).await {
                    if let Some(task_id) = &agent.current_task_id {
                        if let Ok(atts) = state
                            .chain_attestations
                            .find_by_task(task_id.as_str())
                            .await
                        {
                            if let Some(leaf) = atts.iter().max_by_key(|a| a.metadata.chain_depth) {
                                let chain = state
                                    .chain_attestations
                                    .load_chain(&leaf.id)
                                    .await
                                    .unwrap_or_default();
                                let decision = evaluate_attestation_abac(
                                    state, &chain, leaf, &mr, &entry, &repo, "merge",
                                )
                                .await;
                                if decision.effect == gyre_domain::policy::PolicyEffect::Deny {
                                    let reason = format!(
                                        "attestation ABAC policy denied merge (matched policy: {:?})",
                                        decision.matched_policy
                                    );
                                    warn!(
                                        entry_id = %entry.id,
                                        mr_id = %mr.id,
                                        matched_policy = ?decision.matched_policy,
                                        "attestation ABAC denied merge — failing queue entry"
                                    );
                                    state
                                        .merge_queue
                                        .update_status(
                                            &entry.id,
                                            MergeQueueEntryStatus::Failed,
                                            Some(reason),
                                        )
                                        .await?;
                                    return Ok(());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Attempt the merge
    let result = state
        .git_ops
        .merge_branches(&repo.path, &mr.source_branch, &mr.target_branch)
        .await;

    match result {
        Ok(MergeResult::Success { merge_commit_sha }) => {
            info!(entry_id = %entry.id, sha = %merge_commit_sha, "merge succeeded");

            // Update MR status to Merged
            let mut updated_mr = mr;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            // R3-F1: record the MR's own merge commit SHA — the manual
            // revert endpoint reverts this, never the current branch HEAD.
            updated_mr.merge_commit_sha = Some(merge_commit_sha.clone());
            updated_mr.updated_at = now;
            // Transition: Approved -> Merged. If already Open, transition to Approved first.
            if updated_mr.status == MrStatus::Open {
                let _ = updated_mr.transition_status(MrStatus::Approved);
            }
            if let Err(e) = updated_mr.transition_status(MrStatus::Merged) {
                warn!("could not transition MR to Merged: {e}");
            }
            let _ = state.merge_requests.update(&updated_mr).await;
            // HSI §3a: promote the MR's gate trace to attestation on merge
            // (permanent — survives per-MR cleanup for provenance).
            if let Err(e) = state.traces.promote_to_attestation(&updated_mr.id).await {
                warn!(mr_id = %updated_mr.id, error = %e, "failed to promote gate trace to attestation");
            }

            state
                .merge_queue
                .update_status(&entry.id, MergeQueueEntryStatus::Merged, None)
                .await?;

            // Auto-track merge_queue.processed analytics event
            let ev = AnalyticsEvent::new(
                Id::new(Uuid::new_v4().to_string()),
                "merge_queue.processed",
                updated_mr.author_agent_id.as_ref().map(|id| id.to_string()),
                serde_json::json!({
                    "entry_id": entry.id.to_string(),
                    "mr_id": updated_mr.id.to_string(),
                    "result": "merged",
                    // Scope facets (search.md §Searchable Entities: Activity
                    // Event is filterable by workspace/repo).
                    "workspace_id": updated_mr.workspace_id.to_string(),
                    "repo_id": updated_mr.repository_id.to_string(),
                }),
                now,
            );
            let _ = state.analytics.record(&ev).await;
            // search.md §Searchable Entities: activity events are indexed on write.
            crate::search_indexer::index(
                state,
                crate::search_indexer::activity_event_document(&ev),
            )
            .await;

            // Build and store a signed merge attestation bundle (G5).
            let gate_results_snapshot = state
                .gate_results
                .list_by_mr_id(updated_mr.id.as_str())
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|r| crate::attestation::AttestationGateResult {
                    gate_id: r.gate_id.to_string(),
                    gate_type: String::new(), // gate type looked up below
                    status: format!("{:?}", r.status),
                    output: r.output.clone(),
                })
                .collect::<Vec<_>>();

            // Enrich gate_type from quality_gates.
            let mut gate_results_enriched = Vec::with_capacity(gate_results_snapshot.len());
            for mut gr in gate_results_snapshot {
                if let Ok(Some(gate)) = state.quality_gates.find_by_id(&gr.gate_id).await {
                    gr.gate_type = format!("{:?}", gate.gate_type);
                }
                gate_results_enriched.push(gr);
            }

            // Check spec approval status.
            let spec_fully_approved = if let Some(spec_ref) = updated_mr.spec_ref.as_deref() {
                crate::api::gates::verify_spec_ref(state, spec_ref)
                    .await
                    .is_ok()
            } else {
                true // no spec bound — treat as approved
            };

            // Look up conversation SHA uploaded by the authoring agent (HSI §5).
            let conversation_sha = if let Some(ref agent_id) = updated_mr.author_agent_id {
                let kv_key = format!("conv_sha:{}", agent_id.as_str());
                state
                    .kv_store
                    .kv_get("agent_provenance", &kv_key)
                    .await
                    .unwrap_or(None)
            } else {
                None
            };

            // Agent-runtime §2: record exactly which meta-specs the authoring
            // agent ran under (loaded from the prompt-set record stored at
            // spawn time). Empty for agents spawned before prompt assembly or
            // with no applicable meta-specs.
            let meta_specs_used = match updated_mr.author_agent_id.as_ref() {
                Some(agent_id) => {
                    crate::prompt_assembly::load_prompt_set_record(state, agent_id.as_str())
                        .await
                        .map(|rec| rec.meta_specs_used)
                        .unwrap_or_default()
                }
                None => vec![],
            };

            let attestation = crate::attestation::MergeAttestation {
                attestation_version: 1,
                mr_id: updated_mr.id.to_string(),
                merge_commit_sha: merge_commit_sha.clone(),
                merged_at: now,
                gate_results: gate_results_enriched,
                spec_ref: updated_mr.spec_ref.clone(),
                spec_fully_approved,
                author_agent_id: updated_mr.author_agent_id.as_ref().map(|id| id.to_string()),
                conversation_sha,
                // completion_summary is populated from the AgentCompleted message payload
                // when the agent calls agent.complete with a summary. At merge time, the
                // processor does not re-fetch the summary — it is stored at agent.complete time.
                completion_summary: None,
                meta_specs_used,
            };

            let bundle =
                crate::attestation::sign_attestation(attestation, &state.agent_signing_key);

            // Persist attestation bundle in the attestation store (always).
            let _ = state
                .attestation_store
                .save(&updated_mr.id.to_string(), &bundle)
                .await;
            info!(mr_id = %updated_mr.id, sha = %merge_commit_sha, "attestation bundle created and stored");

            // §5.3 location 2 — Git note on refs/notes/attestations.
            // During the dual-write period, we write EITHER the chain attestation
            // OR the legacy AttestationBundle — never both — to avoid a race
            // condition between two fire-and-forget tokio::spawn writes to the
            // same ref. The chain attestation takes priority when available.
            let chain_note_written = {
                let mut written = false;
                if let Ok(Some(chain_att)) = state
                    .chain_attestations
                    .find_by_commit(&merge_commit_sha)
                    .await
                {
                    // Load the full chain (root to leaf) from the database.
                    let chain = match state.chain_attestations.load_chain(&chain_att.id).await {
                        Ok(c) if !c.is_empty() => c,
                        _ => vec![chain_att],
                    };
                    crate::attestation::attach_chain_attestation_note(
                        &repo.path,
                        &merge_commit_sha,
                        &chain,
                    )
                    .await;
                    written = true;
                } else if let Some(ref agent_id) = updated_mr.author_agent_id {
                    // Fallback: look up by agent's task if no commit-indexed attestation.
                    if let Ok(Some(agent)) = state.agents.find_by_id(agent_id).await {
                        if let Some(ref task_id) = agent.current_task_id {
                            if let Ok(atts) = state
                                .chain_attestations
                                .find_by_task(task_id.as_str())
                                .await
                            {
                                if let Some(leaf) =
                                    atts.iter().max_by_key(|a| a.metadata.chain_depth)
                                {
                                    // Load the full chain from the leaf.
                                    let chain =
                                        match state.chain_attestations.load_chain(&leaf.id).await {
                                            Ok(c) if !c.is_empty() => c,
                                            _ => vec![leaf.clone()],
                                        };
                                    crate::attestation::attach_chain_attestation_note(
                                        &repo.path,
                                        &merge_commit_sha,
                                        &chain,
                                    )
                                    .await;
                                    written = true;
                                }
                            }
                        }
                    }
                }
                written
            };

            // Legacy fallback: write the AttestationBundle as a git note only
            // when no chain attestation was written, avoiding a race on the
            // same refs/notes/attestations ref.
            if !chain_note_written {
                let bundle_json = serde_json::to_string(&bundle).unwrap_or_default();
                let repo_path = repo.path.clone();
                let sha_for_note = merge_commit_sha.clone();
                tokio::spawn(async move {
                    let out = tokio::process::Command::new("git")
                        .args([
                            "-C",
                            &repo_path,
                            "notes",
                            "--ref=refs/notes/attestations",
                            "add",
                            "-f",
                            "-m",
                            &bundle_json,
                            &sha_for_note,
                        ])
                        .output()
                        .await;
                    match out {
                        Ok(o) if o.status.success() => {
                            tracing::info!(sha = %sha_for_note, "legacy attestation note attached");
                        }
                        Ok(o) => {
                            tracing::warn!(
                                sha = %sha_for_note,
                                stderr = %String::from_utf8_lossy(&o.stderr),
                                "git notes failed — attestation stored in memory only"
                            );
                        }
                        Err(e) => {
                            tracing::warn!(sha = %sha_for_note, error = %e, "git not found — attestation stored in memory only");
                        }
                    }
                });
            }

            // TASK-007 (Phase 2): Merge-time constraint evaluation (audit-only).
            // Collect all constraints (explicit + strategy-implied + gate),
            // evaluate against the merged diff, log results, emit violations.
            // Does NOT block the merge.
            crate::constraint_check::evaluate_merge_constraints(
                state,
                &updated_mr.id.to_string(),
                &repo.id.to_string(),
                &repo.path,
                &merge_commit_sha,
                &updated_mr.workspace_id,
                &updated_mr.source_branch,
                &updated_mr.target_branch,
                &repo.default_branch,
            )
            .await;

            // Notify the MR author that their MR was merged (HSI §2).
            if let Some(ref author_id) = updated_mr.author_agent_id {
                crate::notifications::notify_mr_merged(
                    state,
                    author_id,
                    &updated_mr.workspace_id,
                    &updated_mr.id.to_string(),
                    "default",
                )
                .await;
            }

            // TASK-022: Cascade testing — trigger test tasks in dependent repos.
            trigger_cascade_tests(
                state,
                &repo.id.to_string(),
                &repo.name,
                &merge_commit_sha,
                &updated_mr.workspace_id,
            )
            .await;

            // TASK-095: Post-merge validation — run post_merge-phase gates
            // against the new HEAD of the default branch. On failure, run
            // the recovery protocol (pause queue, revert, notify, task,
            // invalidate gates). See platform-model.md §6.
            if let Err(failure_reason) =
                crate::gate_executor::run_post_merge_gates(state, &repo, &merge_commit_sha).await
            {
                recover_from_post_merge_failure(
                    state,
                    &repo,
                    &updated_mr,
                    &merge_commit_sha,
                    &failure_reason,
                )
                .await;
            } else {
                // source-control.md §4: the baseline just moved — rebase
                // every other in-flight agent on this target branch onto
                // the new HEAD. Runs AFTER post-merge gates pass, so a
                // gate failure + revert never strands agents on an
                // orphaned base (R2 F3).
                let merged_authors: Vec<Id> = updated_mr.author_agent_id.iter().cloned().collect();
                rebase_inflight_agents(
                    state,
                    &repo,
                    &updated_mr.target_branch,
                    &merge_commit_sha,
                    &merged_authors,
                )
                .await;
            }
        }
        Ok(MergeResult::Conflict { message }) => {
            warn!(entry_id = %entry.id, reason = %message, "merge conflict");
            state
                .merge_queue
                .update_status(
                    &entry.id,
                    MergeQueueEntryStatus::Failed,
                    Some(format!("conflict: {}", message)),
                )
                .await?;
        }
        Err(e) => {
            error!(entry_id = %entry.id, error = %e, "git merge error");
            state
                .merge_queue
                .update_status(
                    &entry.id,
                    MergeQueueEntryStatus::Failed,
                    Some(format!("git error: {e}")),
                )
                .await?;
        }
    }

    Ok(())
}

/// Author identity for forge-generated advisory MR comments.
const SPEC_POLICY_COMMENT_AUTHOR: &str = "forge:spec-policy";

/// Report a spec-policy merge violation to the MR author (agent-gates.md
/// §Forge Enforcement Policies): the queue entry already carries the failure
/// reason, but the author must hear it directly — a GateFailure workspace
/// event plus an inbox notification to the author's spawning user (same
/// delivery rule as `notify_gate_failure`).
async fn report_spec_policy_violation(
    state: &AppState,
    mr: &gyre_domain::MergeRequest,
    repo_id: &Id,
    reason: &str,
) {
    state
        .emit_event(
            Some(mr.workspace_id.clone()),
            gyre_common::message::Destination::Workspace(mr.workspace_id.clone()),
            gyre_common::message::MessageKind::GateFailure,
            Some(serde_json::json!({
                "mr_id": mr.id.to_string(),
                "repo_id": repo_id.to_string(),
                "gate_type": "spec_enforcement",
                "status": "Failed",
                "output": reason,
                "spec_ref": mr.spec_ref,
            })),
        )
        .await;

    if let Some(ref author_id) = mr.author_agent_id {
        // The tenant scope comes from the workspace lookup — when it cannot
        // be resolved there is no scoperable tenant, so skip the inbox
        // notification rather than fabricate one
        // (check-fabricated-scope-defaults.sh). The workspace event above
        // already reported the violation.
        match state.workspaces.find_by_id(&mr.workspace_id).await {
            Ok(Some(ws)) => {
                crate::notifications::notify_spec_policy_violation(
                    state,
                    author_id,
                    &mr.workspace_id,
                    mr.id.as_str(),
                    reason,
                    ws.tenant_id.as_str(),
                )
                .await;
            }
            Ok(None) => warn!(
                mr_id = %mr.id,
                "spec-policy violation: workspace not found; skipping author notification"
            ),
            Err(e) => warn!(
                mr_id = %mr.id,
                error = %e,
                "spec-policy violation: workspace lookup failed; skipping author notification"
            ),
        }
    }
}

/// `warn_stale_spec` advisory (agent-gates.md): a stale spec_ref warns but
/// does not block. Record an advisory MR comment so the author sees the
/// warning on the MR itself, not only in the event stream. Deduplicated per
/// (path, referenced SHA, current SHA) so queue retries don't spam the
/// timeline.
async fn record_stale_spec_advisory(
    state: &AppState,
    mr: &gyre_domain::MergeRequest,
    spec_path: &str,
    spec_sha: &str,
    current_sha: &str,
) {
    let body = format!(
        "⚠️ Stale spec reference: '{spec_path}' HEAD has moved to {current_sha} but this MR \
         references {spec_sha}. The code may implement an older version of the spec \
         (warn_stale_spec policy — advisory, merge not blocked)."
    );

    // Dedupe: an identical advisory (same author + body) already on the MR
    // means a previous queue pass already warned about exactly this drift.
    if let Ok(existing) = state.reviews.list_comments(&mr.id).await {
        if existing
            .iter()
            .any(|c| c.author_agent_id == SPEC_POLICY_COMMENT_AUTHOR && c.body == body)
        {
            return;
        }
    }

    let comment = gyre_domain::ReviewComment::new(
        Id::new(uuid::Uuid::new_v4().to_string()),
        mr.id.clone(),
        SPEC_POLICY_COMMENT_AUTHOR,
        body,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    );
    if let Err(e) = state.reviews.add_comment(&comment).await {
        warn!(
            mr_id = %mr.id,
            error = format!("{e:#}"),
            "failed to record stale-spec advisory comment"
        );
    }
}

/// KV namespace for merge queue pause state. Key = repo id, value = JSON
/// `{"paused": bool, "reason": string}`.
const MERGE_QUEUE_PAUSE_NS: &str = "merge_queue_pause";

/// Is the merge queue paused for this repo?
async fn merge_queue_paused(state: &AppState, repo_id: &Id) -> bool {
    match state
        .kv_store
        .kv_get(MERGE_QUEUE_PAUSE_NS, repo_id.as_str())
        .await
    {
        Ok(Some(v)) => serde_json::from_str::<serde_json::Value>(&v)
            .ok()
            .and_then(|j| j.get("paused").and_then(|p| p.as_bool()))
            .unwrap_or(false),
        _ => false,
    }
}

/// Merge-queue ("forge") health for a repo, as consumed by the ABAC
/// environment attribute `env.main_health` (abac-policy-engine.md §Attributes).
/// `"red"` when the queue is paused (merge pipeline halted), `"green"` when
/// merges may proceed.
pub(crate) async fn queue_health(state: &AppState, repo_id: &str) -> &'static str {
    if merge_queue_paused(state, &gyre_common::Id::new(repo_id)).await {
        "red"
    } else {
        "green"
    }
}

/// Pause the merge queue for this repo (no more merges until main is green).
/// Persists the pause state and emits a `MergeQueuePaused` event so the
/// Workspace Orchestrator can reprioritize work (platform-model.md §6).
pub(crate) async fn pause_merge_queue(
    state: &AppState,
    repo: &gyre_domain::Repository,
    reason: &str,
) {
    let payload = serde_json::json!({ "paused": true, "reason": reason }).to_string();
    if let Err(e) = state
        .kv_store
        .kv_set(MERGE_QUEUE_PAUSE_NS, repo.id.as_str(), payload)
        .await
    {
        error!(repo_id = %repo.id, error = %e, "failed to persist merge queue pause");
    }
    state
        .emit_event(
            Some(repo.workspace_id.clone()),
            gyre_common::message::Destination::Workspace(repo.workspace_id.clone()),
            gyre_common::message::MessageKind::MergeQueuePaused,
            Some(serde_json::json!({
                "repo_id": repo.id.to_string(),
                "reason": reason,
            })),
        )
        .await;

    // user-management.md §Who Gets Notified — "Merge queue paused" notifies
    // all workspace Admins/Owners. Event above reaches the orchestrator; this
    // reaches the humans.
    crate::notification_dispatcher::notify_merge_queue_paused(
        state,
        &repo.workspace_id,
        repo,
        reason,
    )
    .await;
}

/// Resume the merge queue for this repo: clear the pause state and emit
/// `MergeQueueResumed` so watchers know main is green again.
pub(crate) async fn resume_merge_queue(state: &AppState, repo: &gyre_domain::Repository) {
    if let Err(e) = state
        .kv_store
        .kv_remove(MERGE_QUEUE_PAUSE_NS, repo.id.as_str())
        .await
    {
        warn!(repo_id = %repo.id, error = %e, "failed to clear merge queue pause");
    }
    state
        .emit_event(
            Some(repo.workspace_id.clone()),
            gyre_common::message::Destination::Workspace(repo.workspace_id.clone()),
            gyre_common::message::MessageKind::MergeQueueResumed,
            Some(serde_json::json!({
                "repo_id": repo.id.to_string(),
            })),
        )
        .await;
}

/// Post-merge validation failure recovery (platform-model.md §6):
///
/// 1. Pause the merge queue for this repo.
/// 2. Create a revert commit undoing the merge and push it to the default branch.
/// 3. Re-run post-merge gates on the reverted HEAD.
///    - PASS: resume the queue (main is green again).
///    - FAIL: escalate to a human via a MergeQueueEscalation notification; stay paused.
/// 4. Mark the MR `Reverted` (both branches — the merge is already undone).
/// 5. Send the author agent a RevertNotification (event + persisted notification).
/// 6. Create a remediation task describing the failure.
/// 7. Invalidate the MR's gate results (they must re-run).
async fn recover_from_post_merge_failure(
    state: &AppState,
    repo: &gyre_domain::Repository,
    mr: &MergeRequest,
    merge_commit_sha: &str,
    failure_reason: &str,
) {
    // Step 1: pause the merge queue.
    pause_merge_queue(state, repo, failure_reason).await;

    // Step 2: forge the revert commit on the default branch (git revert
    // -m 1 semantics — only this merge's inverse patch, later merges
    // preserved, task-095 R4-F1).
    let revert_sha = match state
        .git_ops
        .revert_commit(&repo.path, &repo.default_branch, merge_commit_sha)
        .await
    {
        Ok(gyre_domain::RevertResult::Success {
            revert_commit_sha: sha,
        }) => sha,
        Ok(gyre_domain::RevertResult::Conflict { message }) => {
            // The inverse patch collides with later changes: the branch
            // is untouched. Do NOT fall back to anything destructive —
            // stay paused and escalate to a human.
            error!(
                mr_id = %mr.id,
                merge_sha = %merge_commit_sha,
                reason = %message,
                "revert conflicted with later changes — queue stays paused, escalating"
            );
            notify_escalation(state, repo, mr, &message).await;
            return;
        }
        Err(e) => {
            error!(
                mr_id = %mr.id,
                merge_sha = %merge_commit_sha,
                error = %e,
                "revert commit creation failed — queue stays paused, escalating"
            );
            // Cannot revert: escalate immediately, stay paused.
            notify_escalation(
                state,
                repo,
                mr,
                &format!("revert of merge commit {merge_commit_sha} failed: {e}"),
            )
            .await;
            return;
        }
    };

    // Step 3: re-run post-merge gates on the reverted HEAD.
    let revert_green = crate::gate_executor::run_post_merge_gates(state, repo, &revert_sha)
        .await
        .is_ok();

    if revert_green {
        // Main is green again — resume the queue.
        resume_merge_queue(state, repo).await;
    } else {
        // Something else is wrong — escalate to a human, stay paused.
        notify_escalation(
            state,
            repo,
            mr,
            &format!(
                "post-merge gates still failing on reverted HEAD {revert_sha} after reverting MR {} (merge commit {merge_commit_sha}): {failure_reason}",
                mr.id
            ),
        )
        .await;
    }

    // Circuit breaker (platform-model.md §6): the same MR reverted 3 times
    // is removed from the merge queue permanently and escalated to a human.
    if let Ok(revert_count) = increment_revert_count(state, mr).await {
        if revert_count >= 3 {
            if let Err(e) = trip_circuit_breaker(state, repo.id.as_str(), mr).await {
                error!(mr_id = %mr.id, error = %e, "failed to trip circuit breaker");
            }
        }
    }

    // Steps 4–7: per-MR side effects (mark Reverted, notify, remediation
    // task, invalidate gate results).
    apply_revert_side_effects(
        state,
        repo,
        mr,
        merge_commit_sha,
        &revert_sha,
        failure_reason,
    )
    .await;
}

/// Per-MR recovery side effects (platform-model.md §6 steps 4–7): mark the
/// MR `Reverted`, send the author agent a RevertNotification (event +
/// persisted notification), create a remediation task describing the
/// failure, and invalidate the MR's gate results so they must re-run.
///
/// `revert_sha` is the revert commit that undid the MR's merge (for atomic
/// groups, the final revert commit of the group run).
pub(crate) async fn apply_revert_side_effects(
    state: &AppState,
    repo: &gyre_domain::Repository,
    mr: &MergeRequest,
    merge_commit_sha: &str,
    revert_sha: &str,
    failure_reason: &str,
) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Step 4: mark the MR Reverted. revert_commit_sha records the revert
    // commit that undid this MR (a full revert MR object is not created —
    // the forge pushed the revert directly to the default branch; task-095
    // R2-3: this is a git SHA, not an MR id).
    let mut updated = mr.clone();
    if let Err(e) = updated.revert(revert_sha.to_string(), now) {
        warn!(mr_id = %mr.id, error = %e, "MR could not transition to Reverted (already reverted?)");
    } else {
        if let Err(e) = state.merge_requests.update(&updated).await {
            error!(mr_id = %mr.id, error = %e, "failed to persist Reverted MR status");
        }
    }

    // Step 5: RevertNotification to the author agent via event + notification.
    if let Some(author_agent_id) = &updated.author_agent_id {
        let payload = serde_json::json!({
            "mr_id": updated.id.to_string(),
            "repo_id": repo.id.to_string(),
            "merge_commit_sha": merge_commit_sha,
            "revert_commit_sha": revert_sha,
            "reason": failure_reason,
        });
        state
            .emit_event(
                Some(updated.workspace_id.clone()),
                gyre_common::message::Destination::Agent(author_agent_id.clone()),
                gyre_common::message::MessageKind::MrReverted,
                Some(payload.clone()),
            )
            .await;

        // Persisted notification per user-management.md §Who Gets Notified
        // ("MR reverted" → MR author + workspace Admins). The author's
        // spawning user is the author recipient; falls back to no
        // notification when there is no human behind the agent (the
        // MrReverted event above already reached the agent itself).
        crate::notifications::notify_mr_reverted(
            state,
            author_agent_id,
            &updated.workspace_id,
            updated.id.as_str(),
            failure_reason,
        )
        .await;
    }

    // Step 6: remediation task.
    let task_id = Id::new(Uuid::new_v4().to_string());
    let mut task = gyre_domain::Task::new(
        task_id,
        format!("MR {} reverted: {}", updated.id, failure_reason),
        now,
    );
    task.priority = TaskPriority::High;
    task.labels = vec!["reverted-mr".to_string(), "auto-created".to_string()];
    task.workspace_id = updated.workspace_id.clone();
    task.repo_id = updated.repository_id.clone();
    // The remediation instruction must not silently discard the breaker
    // identity (task-095 R3-F2): the counter keys on spec_ref, so the
    // resubmitted MR has to carry the same spec binding or "reverted 3
    // times" could never accumulate across the resubmission cycle.
    let spec_carry = updated
        .spec_ref
        .as_deref()
        .map(|s| {
            format!(
                " Bind the new merge request to the same spec ({s}) — the \
                 recovery protocol counts repeated post-merge failures of \
                 the same spec toward the circuit breaker."
            )
        })
        .unwrap_or_default();
    task.description = Some(format!(
        "MR '{}' ({}) was merged to {} but failed post-merge validation: {}. \
         The merge was reverted via commit {}. Re-do the work on a fresh branch \
         and re-open a merge request.{spec_carry}",
        updated.title, updated.id, repo.default_branch, failure_reason, revert_sha,
    ));
    if let Err(e) = state.tasks.create(&task).await {
        error!(task_id = %task.id, error = %e, "failed to persist remediation task");
    }

    // Step 7: invalidate the MR's gate results — they must re-run.
    if let Ok(results) = state.gate_results.list_by_mr_id(updated.id.as_str()).await {
        for r in results {
            let _ = state
                .gate_results
                .update_status(
                    r.id.as_str(),
                    GateStatus::Pending,
                    None,
                    None,
                    Some("invalidated: MR reverted, must re-run".to_string()),
                )
                .await;
        }
    }
}

/// Atomic-group post-merge failure recovery (platform-model.md §6): the
/// group merged as one unit, so it reverts as one unit.
///
/// 1. Pause the merge queue for this repo.
/// 2. Revert every merge commit of the group run in reverse order — each
///    revert restores the branch tree one merge back, so the final revert
///    commit leaves the tree at its pre-group state (history is preserved,
///    unlike `rollback_atomic_group`'s branch reset).
/// 3. Re-run post-merge gates on the reverted HEAD; resume on pass,
///    escalate (stay paused) on failure.
/// 4. Steps 4–7 of the protocol for every merged member: mark `Reverted`,
///    RevertNotification, remediation task, gate-result invalidation.
async fn recover_atomic_group_from_post_merge_failure(
    state: &AppState,
    repo: &gyre_domain::Repository,
    merged_entries: &[(MergeQueueEntry, MergeRequest)],
    group_merge_shas: &[String],
    failure_reason: &str,
) {
    // Step 1: pause the merge queue.
    pause_merge_queue(state, repo, failure_reason).await;

    // Step 2: revert every group merge commit, newest first (git revert
    // -m 1 semantics — each revert applies only that merge's inverse
    // patch on the current tip, task-095 R4-F1).
    let mut final_revert_sha: Option<String> = None;
    for sha in group_merge_shas.iter().rev() {
        match state
            .git_ops
            .revert_commit(&repo.path, &repo.default_branch, sha)
            .await
        {
            Ok(gyre_domain::RevertResult::Success { revert_commit_sha }) => {
                final_revert_sha = Some(revert_commit_sha)
            }
            Ok(gyre_domain::RevertResult::Conflict { message }) => {
                // Inverse patch collides with later changes; branch
                // untouched. Stay paused, escalate to a human.
                error!(
                    repo_id = %repo.id,
                    merge_sha = %sha,
                    reason = %message,
                    "group revert conflicted with later changes — queue stays paused, escalating"
                );
                if let Some((_, mr)) = merged_entries.first() {
                    notify_escalation(state, repo, mr, &message).await;
                }
                return;
            }
            Err(e) => {
                error!(
                    repo_id = %repo.id,
                    merge_sha = %sha,
                    error = %e,
                    "group revert commit creation failed — queue stays paused, escalating"
                );
                if let Some((_, mr)) = merged_entries.first() {
                    notify_escalation(
                        state,
                        repo,
                        mr,
                        &format!("revert of atomic-group merge commit {sha} failed: {e}"),
                    )
                    .await;
                }
                return;
            }
        }
    }
    let Some(revert_sha) = final_revert_sha else {
        return; // empty group — nothing merged, nothing to recover
    };

    // Step 3: re-run post-merge gates on the reverted HEAD.
    let revert_green = crate::gate_executor::run_post_merge_gates(state, repo, &revert_sha)
        .await
        .is_ok();

    if revert_green {
        resume_merge_queue(state, repo).await;
    } else {
        if let Some((_, mr)) = merged_entries.last() {
            notify_escalation(
                state,
                repo,
                mr,
                &format!(
                    "post-merge gates still failing on reverted HEAD {revert_sha} after \
                     reverting atomic group ({} members): {failure_reason}",
                    merged_entries.len(),
                ),
            )
            .await;
        }
    }

    // Steps 4–7 for every merged member. Each member is attributed the
    // merge commit that landed it (matched by position in the group run)
    // and the final revert commit that undid the group.
    for (i, (_, mr)) in merged_entries.iter().enumerate() {
        let merge_sha = group_merge_shas.get(i).map(String::as_str).unwrap_or("");
        apply_revert_side_effects(state, repo, mr, merge_sha, &revert_sha, failure_reason).await;
    }

    // Circuit breaker (platform-model.md §6): per member, 3 reverts →
    // permanent queue removal + human escalation.
    for (_, mr) in merged_entries {
        if let Ok(revert_count) = increment_revert_count(state, mr).await {
            if revert_count >= 3 {
                if let Err(e) = trip_circuit_breaker(state, repo.id.as_str(), mr).await {
                    error!(mr_id = %mr.id, error = %e, "failed to trip circuit breaker");
                }
            }
        }
    }
}

/// Escalate a merge-queue problem to a human: a priority-1
/// MergeQueueEscalation notification to the MR author's spawning user.
async fn notify_escalation(
    state: &AppState,
    repo: &gyre_domain::Repository,
    mr: &MergeRequest,
    message: &str,
) {
    let user_id = match &mr.author_agent_id {
        Some(author_id) => {
            let spawned_by = state
                .agents
                .find_by_id(author_id)
                .await
                .ok()
                .flatten()
                .and_then(|a| a.spawned_by);
            match &spawned_by {
                Some(sb) => Id::new(sb.clone()),
                None => author_id.clone(),
            }
        }
        None => mr.workspace_id.clone(),
    };
    crate::notifications::notify_rich(
        state,
        mr.workspace_id.clone(),
        user_id,
        gyre_common::NotificationType::MergeQueueEscalation,
        format!("Merge queue escalation on {}", repo.name),
        "default",
        Some(
            serde_json::json!({
                "repo_id": repo.id.to_string(),
                "mr_id": mr.id.to_string(),
                "message": message,
            })
            .to_string(),
        ),
        Some(mr.id.to_string()),
        Some(repo.id.to_string()),
        "mr",
        None,
    )
    .await;
}

/// Trigger cascade tests for all repos that depend on the merged repo.
///
/// When `require_cascade_tests` is enabled in the workspace dependency policy
/// and a merge completes in `merged_repo_id`, this function:
/// 1. Queries the dependency graph for all repos depending on the merged repo
/// 2. Creates a cascade test task in each dependent repo
/// 3. Emits a `cascade_test_triggered` event to the dependent repo's workspace
/// 4. Notifies workspace members of the cascade test
pub(crate) async fn trigger_cascade_tests(
    state: &AppState,
    merged_repo_id: &str,
    merged_repo_name: &str,
    merge_commit_sha: &str,
    workspace_id: &Id,
) {
    // Check workspace dependency policy.
    let policy = state
        .dependency_policies
        .get_for_workspace(workspace_id)
        .await
        .unwrap_or_default();

    if !policy.require_cascade_tests {
        return;
    }

    // Query dependency graph for all repos depending on the merged repo.
    let dependents = match state
        .dependencies
        .list_dependents(&Id::new(merged_repo_id))
        .await
    {
        Ok(deps) => deps,
        Err(e) => {
            warn!("cascade-test: failed to list dependents for {merged_repo_id}: {e}");
            return;
        }
    };

    if dependents.is_empty() {
        return;
    }

    let now = crate::api::now_secs();
    let sha_short = &merge_commit_sha[..8.min(merge_commit_sha.len())];

    for dep_edge in &dependents {
        // Resolve the dependent repo's workspace — the task belongs to
        // the dependent repo's workspace, not the merged repo's.
        let dep_repo = state
            .repos
            .find_by_id(&dep_edge.source_repo_id)
            .await
            .ok()
            .flatten();

        let dep_workspace_id = dep_repo
            .as_ref()
            .map(|r| r.workspace_id.clone())
            .unwrap_or_else(|| workspace_id.clone());

        let dep_repo_name = dep_repo
            .as_ref()
            .map(|r| r.name.clone())
            .unwrap_or_else(|| dep_edge.source_repo_id.to_string());

        // Check the dependent workspace's policy — respect opt-out (checklist #76).
        let dep_policy = state
            .dependency_policies
            .get_for_workspace(&dep_workspace_id)
            .await
            .unwrap_or_default();

        if !dep_policy.require_cascade_tests {
            info!(
                dependent_repo = dep_edge.source_repo_id.as_str(),
                dependent_workspace = dep_workspace_id.as_str(),
                "skipping cascade test — dependent workspace has cascade testing disabled"
            );
            continue;
        }

        // Create a cascade test task in the dependent repo.
        let task_id = Id::new(Uuid::new_v4().to_string());
        let mut task = gyre_domain::Task::new(
            task_id,
            format!("Cascade test: {dep_repo_name} against {merged_repo_name}@{sha_short}"),
            now,
        );
        task.priority = TaskPriority::High;
        task.labels = vec!["cascade-test".to_string(), "auto-created".to_string()];
        task.description = Some(format!(
            "Repo '{merged_repo_name}' merged commit {merge_commit_sha}. \
             Run tests in '{dep_repo_name}' against the new version to verify compatibility."
        ));
        task.workspace_id = dep_workspace_id.clone();
        task.repo_id = dep_edge.source_repo_id.clone();

        if let Err(e) = state.tasks.create(&task).await {
            warn!("cascade-test: failed to create task for {dep_repo_name}: {e}");
        }

        // Emit a cascade_test_triggered event to the dependent repo's workspace.
        let payload = serde_json::json!({
            "event": "cascade_test_triggered",
            "merged_repo_id": merged_repo_id,
            "merged_repo_name": merged_repo_name,
            "merge_commit_sha": merge_commit_sha,
            "dependent_repo_id": dep_edge.source_repo_id.as_str(),
            "dependent_repo_name": dep_repo_name,
            "task_id": task.id.as_str(),
        });

        state
            .emit_event(
                Some(dep_workspace_id.clone()),
                gyre_common::message::Destination::Workspace(dep_workspace_id.clone()),
                gyre_common::MessageKind::Custom("cascade_test_triggered".to_string()),
                Some(payload),
            )
            .await;

        // Notify workspace members of the dependent repo.
        let members = state
            .workspace_memberships
            .list_by_workspace(&dep_workspace_id)
            .await
            .unwrap_or_default();

        for member in &members {
            let body_json = serde_json::json!({
                "merged_repo": merged_repo_name,
                "merge_commit_sha": merge_commit_sha,
                "dependent_repo": dep_repo_name,
                "task_id": task.id.as_str(),
            })
            .to_string();

            crate::notifications::notify_rich(
                state,
                dep_workspace_id.clone(),
                member.user_id.clone(),
                gyre_common::NotificationType::CascadeTestTriggered,
                format!(
                    "Cascade test triggered: {dep_repo_name} against {merged_repo_name}@{sha_short}"
                ),
                "default",
                Some(body_json),
                Some(dep_edge.source_repo_id.to_string()),
                Some(merged_repo_id.to_string()),
                "repo",
                None,
            )
            .await;
        }

        info!(
            merged_repo = merged_repo_id,
            dependent_repo = dep_edge.source_repo_id.as_str(),
            "cascade test task created for dependent repo"
        );
    }

    info!(
        merged_repo = merged_repo_id,
        dependent_count = dependents.len(),
        "cascade testing triggered for {} dependent repo(s)",
        dependents.len(),
    );
}

/// Report the result of a cascade test and handle failures.
///
/// Called when a cascade test task completes. On failure:
/// 1. Creates a follow-up task in the dependent repo with failure details
/// 2. Emits a `cascade_test_failed` event to the workspace orchestrator
/// 3. Sends CascadeTestFailed notifications to workspace members
///
/// On success, emits a `cascade_test_passed` activity event.
pub(crate) async fn report_cascade_test_result(
    state: &AppState,
    task: &gyre_domain::Task,
    passed: bool,
    failure_details: Option<&str>,
) {
    let now = crate::api::now_secs();

    if passed {
        // Emit cascade_test_passed activity event.
        state
            .emit_event(
                Some(task.workspace_id.clone()),
                gyre_common::message::Destination::Workspace(task.workspace_id.clone()),
                gyre_common::MessageKind::Custom("cascade_test_passed".to_string()),
                Some(serde_json::json!({
                    "event": "cascade_test_passed",
                    "task_id": task.id.as_str(),
                    "repo_id": task.repo_id.as_str(),
                    "title": &task.title,
                })),
            )
            .await;

        info!(
            task_id = task.id.as_str(),
            repo_id = task.repo_id.as_str(),
            "cascade test passed"
        );
    } else {
        let details = failure_details.unwrap_or("Cascade test failed — see task for details");

        // Create a follow-up task in the dependent repo with failure details.
        let follow_up_id = Id::new(Uuid::new_v4().to_string());
        let mut follow_up = gyre_domain::Task::new(
            follow_up_id,
            format!("Cascade test failure: {}", task.title),
            now,
        );
        follow_up.priority = TaskPriority::High;
        follow_up.labels = vec![
            "cascade-test-failure".to_string(),
            "auto-created".to_string(),
        ];
        follow_up.description = Some(format!(
            "Cascade test failed. Original task: {}. Details: {}",
            task.title, details
        ));
        follow_up.workspace_id = task.workspace_id.clone();
        follow_up.repo_id = task.repo_id.clone();

        if let Err(e) = state.tasks.create(&follow_up).await {
            warn!("cascade-test: failed to create follow-up task: {e}");
        }

        // Emit cascade_test_failed event to workspace orchestrator.
        let payload = serde_json::json!({
            "event": "cascade_test_failed",
            "task_id": task.id.as_str(),
            "follow_up_task_id": follow_up.id.as_str(),
            "repo_id": task.repo_id.as_str(),
            "title": &task.title,
            "failure_details": details,
        });

        state
            .emit_event(
                Some(task.workspace_id.clone()),
                gyre_common::message::Destination::Workspace(task.workspace_id.clone()),
                gyre_common::MessageKind::Custom("cascade_test_failed".to_string()),
                Some(payload),
            )
            .await;

        // Notify workspace members of cascade test failure.
        let members = state
            .workspace_memberships
            .list_by_workspace(&task.workspace_id)
            .await
            .unwrap_or_default();

        for member in &members {
            let body_json = serde_json::json!({
                "task_id": task.id.as_str(),
                "follow_up_task_id": follow_up.id.as_str(),
                "repo_id": task.repo_id.as_str(),
                "failure_details": details,
            })
            .to_string();

            crate::notifications::notify_rich(
                state,
                task.workspace_id.clone(),
                member.user_id.clone(),
                gyre_common::NotificationType::CascadeTestFailed,
                format!("Cascade test failed: {}", task.title),
                "default",
                Some(body_json),
                Some(task.repo_id.to_string()),
                None,
                "repo",
                None,
            )
            .await;
        }

        info!(
            task_id = task.id.as_str(),
            repo_id = task.repo_id.as_str(),
            "cascade test failed, follow-up task created"
        );
    }
}

/// TASK-077 / HSI §2 (F5): Evaluate merge-time ABAC for the merge processor's
/// internal service identity.
///
/// The merge processor does NOT use the global `GYRE_AUTH_TOKEN` — ABAC bypass
/// is checked by identity (`subject.id == "gyre-system-token"`), and the
/// processor's subject id is "merge-processor", so it IS subject to the
/// Supervised trust policy (`trust:require-human-mr-review`, a Deny on
/// `subject.type == "system"` for merge/mr). On an EXPLICIT Deny match the
/// merge is held until a human approves the MR (see the callers — the
/// human-approval escape is `mr.status == MrStatus::Approved`, set by a human
/// via the MR status endpoint, never by the processor before the gate).
/// The engine's default-deny result (`matched_policy: None` — no policy
/// governs system merge/mr) is NOT a hold: it is the Guided/Autonomous state
/// per HSI §2 ("The merge processor is NOT blocked — no
/// trust:require-human-mr-review policy exists"). Callers must gate on
/// `matched_policy.is_some()` — which requires the request-pipeline catch-all
/// `default-deny` policy to be EXCLUDED from this evaluation (below): it is
/// seeded as a built-in at startup and matches every action unconditionally,
/// so leaving it in scope would make the Guided/Autonomous state
/// unrepresentable — every autonomous merge would return an explicit
/// `Some(default-deny)` Deny and be held forever. The catch-all exists for
/// the HTTP middleware pipeline (abac-policy-engine.md §Request Pipeline);
/// the merge processor is an internal service whose merge authority is
/// governed by trust-preset and user Denies, not the pipeline catch-all.
///
/// Workspace-scoped policies from OTHER workspaces are filtered out: the
/// engine's `evaluate` does not check scope_id, and one workspace's trust Deny
/// must not hold another workspace's merges.
///
/// Returns `Err` when the policy or workspace store cannot be READ (review
/// F9): a read failure means the trust state cannot be determined, and the
/// caller (`supervised_trust_hold`) fails CLOSED — never degrades the error
/// to "no policies", which would silently drop a Supervised workspace's
/// merge Deny and let the merge proceed.
async fn evaluate_merge_abac(
    state: &AppState,
    mr: &MergeRequest,
    repo: &gyre_domain::Repository,
) -> anyhow::Result<crate::policy_engine::EvalResult> {
    use anyhow::Context as _;

    // Resolve tenant_id from repo → workspace (same pattern as
    // evaluate_attestation_abac; subject.tenant_id is required context parity).
    // A workspace-store read failure PROPAGATES (review F9): the previous
    // `Err(_) => None` silently dropped tenant context, which could make
    // tenant-conditioned Deny policies silently not match — the same
    // fail-open degradation class as the policy-list read below.
    let workspace = state
        .workspaces
        .find_by_id(&repo.workspace_id)
        .await
        .context("supervised trust gate: workspace store read failed")?;
    let tenant_id = workspace.map(|w| w.tenant_id.to_string());

    let mut ctx = crate::policy_engine::AttributeContext::default();
    ctx.set("subject.type", "system");
    ctx.set("subject.id", "merge-processor");
    if let Some(tid) = &tenant_id {
        ctx.set("subject.tenant_id", tid);
    }
    ctx.set("resource.type", "mr");
    ctx.set("resource.repo_id", repo.id.as_str());
    ctx.set("resource.workspace_id", mr.workspace_id.as_str());

    // Review F9: the policy-list read error PROPAGATES. The previous
    // `.unwrap_or_default()` degraded a store failure to an empty policy
    // set, and because the hold below requires an EXPLICIT Deny match, the
    // merge proceeded in a Supervised workspace with the error discarded —
    // fail OPEN. This is the flaw class the repo mechanically bans for
    // refs (check-fail-open-ref-resolution.sh): a default converting
    // "cannot determine state" into "state is fine" on a decision signal.
    let policies = state
        .policies
        .list()
        .await
        .context("supervised trust gate: policy store read failed")?;
    let policies: Vec<_> = policies
        .into_iter()
        .filter(|p| {
            // Pipeline catch-all: unconditional deny for HTTP subjects — not a
            // statement about system merge authority (see doc comment above).
            p.id.as_str() != crate::abac_middleware::DEFAULT_DENY_POLICY_ID
                && (p.scope != gyre_domain::policy::PolicyScope::Workspace
                    || p.scope_id.as_ref() == Some(&mr.workspace_id))
        })
        .collect();
    Ok(crate::policy_engine::evaluate(
        policies, &ctx, "merge", "mr",
    ))
}

/// TASK-077 / HSI §2 (F5): the supervised-trust hold predicate shared by the
/// selection loop and both merge paths. Returns `Some(reason)` when the MR
/// is not yet human-approved (the escape is `mr.status == Approved`, set
/// only via the MR status endpoint, never by the processor before the
/// gate) and an EXPLICIT Deny policy matches the merge processor's service
/// identity; `None` when the merge may proceed. See `evaluate_merge_abac`
/// for why only an explicit Deny match (`matched_policy.is_some()`) holds:
/// the engine's default-deny is the Guided/Autonomous state per HSI §2.
///
/// Fails CLOSED on a policy/workspace store read failure (review F9): a
/// read error means the trust state cannot be determined, and degrading it
/// to "no policies" would let a Supervised workspace's merge Deny silently
/// vanish while the operator believes the workspace is Supervised. A read
/// failure is therefore a HOLD (requeue), never a pass — the same
/// fail-closed direction as the HTTP ABAC middleware, whose identical
/// `unwrap_or_default` yields an empty set that the engine default-denies.
/// The hold is a requeue, not a failure, so the merge retries once the
/// store recovers, and the human-approval escape (`mr.status == Approved`)
/// still applies.
async fn supervised_trust_hold(
    state: &AppState,
    mr: &MergeRequest,
    repo: &gyre_domain::Repository,
) -> Option<String> {
    if mr.status == MrStatus::Approved {
        return None; // human-approval escape (HSI §2/§13)
    }
    let result = match evaluate_merge_abac(state, mr, repo).await {
        Ok(result) => result,
        Err(e) => {
            warn!(
                mr_id = %mr.id,
                workspace_id = %mr.workspace_id,
                error = %e,
                "supervised trust gate: policy evaluation unavailable, failing closed (hold)"
            );
            return Some(format!(
                "supervised trust: policy store unavailable for MR {}, failing closed",
                mr.id
            ));
        }
    };
    if result.effect == gyre_domain::policy::PolicyEffect::Deny && result.matched_policy.is_some() {
        Some(format!(
            "supervised trust: human MR approval required for MR {}",
            mr.id
        ))
    } else {
        None
    }
}

/// Selection-loop trust gate (TASK-077 / HSI §2 F5, review repair
/// 6425cc46): returns the hold reason when the candidate — or, for an
/// atomic-group candidate, ANY member of its group — is held by an
/// explicit trust Deny before human approval. Group members are checked
/// as a unit because `merge_atomic_group` rolls the whole group back on a
/// held member; skipping only the individual candidate would re-select
/// the group next cycle and starve the queue the same way. A missing
/// repository returns `None` (not held): the merge paths own the
/// "repository not found" failure semantics.
async fn selection_trust_hold(
    state: &AppState,
    candidate_mr: &MergeRequest,
) -> anyhow::Result<Option<String>> {
    let members: Vec<MergeRequest> = if let Some(ref group) = candidate_mr.atomic_group {
        let all_mrs = state.merge_requests.list().await?;
        all_mrs
            .into_iter()
            .filter(|m| m.atomic_group.as_deref() == Some(group))
            .collect()
    } else {
        vec![candidate_mr.clone()]
    };

    // All group members share the same repository (merge_atomic_group
    // depends on this); a single candidate uses its own repo.
    let repo = match state.repos.find_by_id(&candidate_mr.repository_id).await? {
        Some(r) => r,
        None => return Ok(None),
    };

    for mr in &members {
        if let Some(reason) = supervised_trust_hold(state, mr, &repo).await {
            return Ok(Some(reason));
        }
    }
    Ok(None)
}

/// meta-spec-reconciliation.md §9 — strict-policy meta-spec drift hold,
/// evaluated DURING candidate selection (task-159 review repair), exactly
/// like `selection_trust_hold` above. Expands atomic-group candidates to
/// all group members (the group merges — or is held — as a unit).
/// Returns `Some(reason)` when any member is drifted under a workspace
/// merge policy with `require_current_meta_spec_set: true`; the caller
/// requeues the entry with the reason and tries the next candidate in the
/// SAME cycle, so a strictly-blocked head-of-queue entry cannot starve the
/// shared merge queue. Warn-only drift (the default, §9 "warn, don't
/// block") records the `[WARN] meta-spec-drift` gate result here and
/// never blocks. MRs without an attestation and workspaces without a
/// bound set skip the check gracefully.
async fn selection_meta_spec_drift_hold(
    state: &AppState,
    candidate_mr: &MergeRequest,
) -> anyhow::Result<Option<String>> {
    let members: Vec<MergeRequest> = if let Some(ref group) = candidate_mr.atomic_group {
        let all_mrs = state.merge_requests.list().await?;
        all_mrs
            .into_iter()
            .filter(|m| m.atomic_group.as_deref() == Some(group))
            .collect()
    } else {
        vec![candidate_mr.clone()]
    };

    for mr in &members {
        if let Some(drift) = crate::reconciliation::mr_meta_spec_drift(state, mr).await {
            let policy =
                crate::merge_policy::merge_policy_or_default(state, &mr.workspace_id).await;
            if policy.require_current_meta_spec_set {
                warn!(
                    mr_id = %mr.id,
                    produced_set = %drift.produced_set_sha,
                    current_set = %drift.current_set_sha,
                    "meta-spec drift blocked merge (strict workspace policy)"
                );
                return Ok(Some(format!(
                    "meta-spec drift: code produced under meta-spec set {}, current is {} \
                     (require_current_meta_spec_set)",
                    drift.produced_set_sha, drift.current_set_sha
                )));
            }
            record_meta_spec_drift_warning(state, mr, &drift).await;
        }
    }
    Ok(None)
}

/// TASK-061 (§7.2): Evaluate ABAC policies with attestation chain subject attributes.
///
/// Builds an `AttributeContext` from the attestation chain (chain_depth,
/// root_signer, constraint_count, tenant_id) and evaluates policies with the
/// given action and resource_type=attestation. Audit-only — result is logged
/// but not enforced. Returns the evaluation result for testability.
async fn evaluate_attestation_abac(
    state: &AppState,
    chain: &[gyre_common::Attestation],
    leaf: &gyre_common::Attestation,
    mr: &MergeRequest,
    entry: &MergeQueueEntry,
    repo: &gyre_domain::Repository,
    action: &str,
) -> crate::policy_engine::EvalResult {
    let chain_depth = leaf.metadata.chain_depth;
    let signer = gyre_domain::root_signer(chain);
    let cc = gyre_domain::constraint_count(chain) as i64;

    // Resolve tenant_id from repo → workspace (checklist item 125: ABAC context parity).
    let tenant_id = match state.workspaces.find_by_id(&repo.workspace_id).await {
        Ok(Some(ws)) => Some(ws.tenant_id.to_string()),
        _ => None,
    };

    let mut ctx = crate::policy_engine::AttributeContext::default();
    ctx.set("subject.type", "agent");
    if let Some(ref author) = mr.author_agent_id {
        ctx.set("subject.id", author.as_str());
    }
    if let Some(ref tid) = tenant_id {
        ctx.set("subject.tenant_id", tid);
    }
    ctx.set_number("subject.chain_depth", chain_depth as i64);
    ctx.set_number("subject.constraint_count", cc);
    if let Some(ref rs) = signer {
        ctx.set("subject.root_signer", rs);
    }
    // An attestation chain was located for this work. This is the attribute
    // `builtin:require-signed-authorization` (§7.2) keys its unless-clause
    // on, and `builtin:attested-authorization` allows on.
    ctx.set_bool("subject.attestation_chain_present", true);
    ctx.set("resource.type", "attestation");
    ctx.set("resource.repo_id", repo.id.as_str());

    let policies = state.policies.list().await.unwrap_or_default();
    let eval_result = crate::policy_engine::evaluate(policies, &ctx, action, "attestation");

    // Record the decision in the policy-decision audit trail, exactly as the
    // ABAC middleware does for HTTP requests, so merge-time authorization
    // decisions are queryable via /api/v1/policies/decisions.
    let decision = crate::policy_engine::build_decision(
        &eval_result,
        mr.author_agent_id
            .as_ref()
            .map(|a| a.as_str())
            .unwrap_or("unknown-agent"),
        "agent",
        action,
        "attestation",
        repo.id.as_str(),
    );
    let _ = state.policies.record_decision(&decision).await;

    tracing::info!(
        entry_id = %entry.id,
        mr_id = %mr.id,
        chain_depth = chain_depth,
        root_signer = ?signer,
        constraint_count = cc,
        decision = ?eval_result.effect,
        matched_policy = ?eval_result.matched_policy,
        "attestation ABAC evaluation for {action}"
    );

    eval_result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;
    use gyre_common::Id;
    use gyre_domain::{
        DependencyEdge, DependencyPolicy, DependencyType, DetectionMethod, Repository,
    };

    async fn create_repo_in_workspace(
        state: &AppState,
        name: &str,
        workspace_id: &str,
    ) -> Repository {
        // Seed the workspace record (idempotent): emissions resolve the
        // tenant scope from it and skip when it does not exist (task-099 F1),
        // so the workspace a fixture references must be real for event and
        // notification assertions to observe anything.
        if state
            .workspaces
            .find_by_id(&Id::new(workspace_id))
            .await
            .unwrap()
            .is_none()
        {
            let ws = gyre_domain::Workspace::new(
                Id::new(workspace_id),
                Id::new("t1"),
                workspace_id,
                workspace_id,
                0,
            );
            state.workspaces.create(&ws).await.unwrap();
        }
        let repo = Repository::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            Id::new(workspace_id),
            name,
            format!("/tmp/{name}.git"),
            0,
        );
        std::fs::create_dir_all(&format!("/tmp/{name}.git")).ok();
        state.repos.create(&repo).await.unwrap();
        repo
    }

    async fn create_dependency(state: &AppState, source_repo_id: &Id, target_repo_id: &Id) {
        let edge = DependencyEdge::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            source_repo_id.clone(),
            target_repo_id.clone(),
            DependencyType::Code,
            "Cargo.toml",
            "target-crate",
            DetectionMethod::CargoToml,
            1000,
        );
        state.dependencies.save(&edge).await.unwrap();
    }

    #[tokio::test]
    async fn trigger_cascade_tests_creates_tasks_for_dependents() {
        let state = test_state();

        let repo_b = create_repo_in_workspace(&state, "repo-b", "ws-1").await;
        let repo_a = create_repo_in_workspace(&state, "repo-a", "ws-1").await;

        // repo-a depends on repo-b
        create_dependency(&state, &repo_a.id, &repo_b.id).await;

        // Enable cascade testing
        let policy = DependencyPolicy {
            require_cascade_tests: true,
            ..Default::default()
        };
        state
            .dependency_policies
            .set_for_workspace(&Id::new("ws-1"), &policy)
            .await
            .unwrap();

        // Subscribe to broadcast channel before calling (F1).
        let mut rx = state.message_broadcast_tx.subscribe();

        trigger_cascade_tests(
            &state,
            repo_b.id.as_str(),
            "repo-b",
            "abc123def456",
            &Id::new("ws-1"),
        )
        .await;

        // Verify a cascade test task was created in repo-a
        let tasks = state.tasks.list_by_repo(&repo_a.id).await.unwrap();
        assert_eq!(tasks.len(), 1);
        assert!(tasks[0].title.contains("repo-a"));
        assert!(tasks[0].title.contains("repo-b"));
        assert!(tasks[0].title.contains("abc123de")); // sha_short
        assert_eq!(tasks[0].priority, TaskPriority::High);
        assert!(tasks[0].labels.contains(&"cascade-test".to_string()));
        assert!(tasks[0].labels.contains(&"auto-created".to_string()));
        assert_eq!(tasks[0].workspace_id.as_str(), "ws-1");
        assert_eq!(tasks[0].repo_id.as_str(), repo_a.id.as_str());

        // Verify cascade_test_triggered event was emitted (F1).
        let msg = rx
            .try_recv()
            .expect("cascade_test_triggered event should be emitted");
        assert_eq!(
            msg.kind,
            gyre_common::MessageKind::Custom("cascade_test_triggered".to_string())
        );
        let payload = msg.payload.as_ref().expect("event should have payload");
        assert_eq!(payload["event"], "cascade_test_triggered");
        assert_eq!(payload["merged_repo_id"], repo_b.id.as_str());
        assert_eq!(payload["dependent_repo_id"], repo_a.id.as_str());
    }

    #[tokio::test]
    async fn trigger_cascade_tests_skips_when_policy_disabled() {
        let state = test_state();

        let repo_b = create_repo_in_workspace(&state, "repo-b", "ws-1").await;
        let repo_a = create_repo_in_workspace(&state, "repo-a", "ws-1").await;

        create_dependency(&state, &repo_a.id, &repo_b.id).await;

        // Disable cascade testing
        let policy = DependencyPolicy {
            require_cascade_tests: false,
            ..Default::default()
        };
        state
            .dependency_policies
            .set_for_workspace(&Id::new("ws-1"), &policy)
            .await
            .unwrap();

        let mut rx = state.message_broadcast_tx.subscribe();

        trigger_cascade_tests(
            &state,
            repo_b.id.as_str(),
            "repo-b",
            "abc123def456",
            &Id::new("ws-1"),
        )
        .await;

        // No tasks should be created
        let tasks = state.tasks.list_by_repo(&repo_a.id).await.unwrap();
        assert!(tasks.is_empty());

        // No events should be emitted when policy is disabled.
        assert!(
            rx.try_recv().is_err(),
            "no events should be emitted when cascade testing is disabled"
        );
    }

    #[tokio::test]
    async fn trigger_cascade_tests_creates_task_per_dependent() {
        let state = test_state();

        let repo_b = create_repo_in_workspace(&state, "repo-b", "ws-1").await;
        let repo_a = create_repo_in_workspace(&state, "repo-a", "ws-1").await;
        let repo_c = create_repo_in_workspace(&state, "repo-c", "ws-1").await;
        let repo_d = create_repo_in_workspace(&state, "repo-d", "ws-1").await;

        // All depend on repo-b
        create_dependency(&state, &repo_a.id, &repo_b.id).await;
        create_dependency(&state, &repo_c.id, &repo_b.id).await;
        create_dependency(&state, &repo_d.id, &repo_b.id).await;

        let policy = DependencyPolicy {
            require_cascade_tests: true,
            ..Default::default()
        };
        state
            .dependency_policies
            .set_for_workspace(&Id::new("ws-1"), &policy)
            .await
            .unwrap();

        let mut rx = state.message_broadcast_tx.subscribe();

        trigger_cascade_tests(
            &state,
            repo_b.id.as_str(),
            "repo-b",
            "sha12345",
            &Id::new("ws-1"),
        )
        .await;

        // Each dependent repo gets its own task
        let tasks_a = state.tasks.list_by_repo(&repo_a.id).await.unwrap();
        let tasks_c = state.tasks.list_by_repo(&repo_c.id).await.unwrap();
        let tasks_d = state.tasks.list_by_repo(&repo_d.id).await.unwrap();

        assert_eq!(tasks_a.len(), 1);
        assert_eq!(tasks_c.len(), 1);
        assert_eq!(tasks_d.len(), 1);

        assert!(tasks_a[0].title.contains("repo-a"));
        assert!(tasks_c[0].title.contains("repo-c"));
        assert!(tasks_d[0].title.contains("repo-d"));

        // Verify one cascade_test_triggered event per dependent.
        let mut event_count = 0;
        while rx.try_recv().is_ok() {
            event_count += 1;
        }
        assert_eq!(event_count, 3, "should emit one event per dependent repo");
    }

    #[tokio::test]
    async fn trigger_cascade_tests_no_dependents_is_noop() {
        let state = test_state();

        let repo_b = create_repo_in_workspace(&state, "repo-b", "ws-1").await;

        let policy = DependencyPolicy {
            require_cascade_tests: true,
            ..Default::default()
        };
        state
            .dependency_policies
            .set_for_workspace(&Id::new("ws-1"), &policy)
            .await
            .unwrap();

        let mut rx = state.message_broadcast_tx.subscribe();

        trigger_cascade_tests(
            &state,
            repo_b.id.as_str(),
            "repo-b",
            "sha12345",
            &Id::new("ws-1"),
        )
        .await;

        // No tasks since no dependents exist
        let all_tasks = state.tasks.list().await.unwrap();
        assert!(all_tasks.is_empty());

        // No events since no dependents exist.
        assert!(
            rx.try_recv().is_err(),
            "no events should be emitted when there are no dependents"
        );
    }

    #[tokio::test]
    async fn trigger_cascade_tests_resolves_dependent_workspace() {
        let state = test_state();

        let repo_b = create_repo_in_workspace(&state, "repo-b", "ws-MERGED").await;
        let repo_a = create_repo_in_workspace(&state, "repo-a", "ws-DEPENDENT").await;

        create_dependency(&state, &repo_a.id, &repo_b.id).await;

        let policy = DependencyPolicy {
            require_cascade_tests: true,
            ..Default::default()
        };
        // Set policy for both workspaces so cascade testing is enabled on both sides.
        state
            .dependency_policies
            .set_for_workspace(&Id::new("ws-MERGED"), &policy)
            .await
            .unwrap();
        state
            .dependency_policies
            .set_for_workspace(&Id::new("ws-DEPENDENT"), &policy)
            .await
            .unwrap();

        // Subscribe to broadcast channel before calling.
        let mut rx = state.message_broadcast_tx.subscribe();

        trigger_cascade_tests(
            &state,
            repo_b.id.as_str(),
            "repo-b",
            "sha12345",
            &Id::new("ws-MERGED"),
        )
        .await;

        // Task should be in the dependent repo's workspace, not the merged repo's
        let tasks = state.tasks.list_by_repo(&repo_a.id).await.unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(
            tasks[0].workspace_id.as_str(),
            "ws-DEPENDENT",
            "task should be in the dependent repo's workspace"
        );

        // Event should target the dependent workspace, not the merged workspace.
        let msg = rx
            .try_recv()
            .expect("cascade_test_triggered event should be emitted");
        assert_eq!(
            msg.kind,
            gyre_common::MessageKind::Custom("cascade_test_triggered".to_string())
        );
        // Event destination should be the dependent workspace.
        assert_eq!(
            msg.to,
            gyre_common::message::Destination::Workspace(Id::new("ws-DEPENDENT")),
            "event should target the dependent workspace"
        );
    }

    #[tokio::test]
    async fn report_cascade_test_result_passed_no_follow_up_task() {
        let state = test_state();

        let repo_a = create_repo_in_workspace(&state, "repo-a", "ws-1").await;

        let mut task = gyre_domain::Task::new(
            Id::new("cascade-task-1"),
            "Cascade test: repo-a against repo-b@abc12345",
            1000,
        );
        task.labels = vec!["cascade-test".to_string(), "auto-created".to_string()];
        task.workspace_id = Id::new("ws-1");
        task.repo_id = repo_a.id.clone();
        state.tasks.create(&task).await.unwrap();

        // Subscribe to broadcast channel before calling (F5).
        let mut rx = state.message_broadcast_tx.subscribe();

        report_cascade_test_result(&state, &task, true, None).await;

        // No follow-up task should be created for a passing test
        let tasks = state.tasks.list_by_repo(&repo_a.id).await.unwrap();
        assert_eq!(
            tasks.len(),
            1,
            "only the original cascade test task should exist"
        );

        // Verify cascade_test_passed event was emitted (F5).
        let msg = rx
            .try_recv()
            .expect("cascade_test_passed event should be emitted");
        assert_eq!(
            msg.kind,
            gyre_common::MessageKind::Custom("cascade_test_passed".to_string())
        );
        let payload = msg.payload.as_ref().expect("event should have payload");
        assert_eq!(payload["event"], "cascade_test_passed");
        assert_eq!(payload["task_id"], "cascade-task-1");
        assert_eq!(payload["repo_id"], repo_a.id.as_str());
    }

    #[tokio::test]
    async fn report_cascade_test_result_failed_creates_follow_up_task() {
        let state = test_state();

        let repo_a = create_repo_in_workspace(&state, "repo-a", "ws-1").await;

        let mut task = gyre_domain::Task::new(
            Id::new("cascade-task-2"),
            "Cascade test: repo-a against repo-b@abc12345",
            1000,
        );
        task.labels = vec!["cascade-test".to_string(), "auto-created".to_string()];
        task.workspace_id = Id::new("ws-1");
        task.repo_id = repo_a.id.clone();
        state.tasks.create(&task).await.unwrap();

        // Subscribe to broadcast channel before calling (F1).
        let mut rx = state.message_broadcast_tx.subscribe();

        report_cascade_test_result(
            &state,
            &task,
            false,
            Some("Tests failed: 3 failures in integration tests"),
        )
        .await;

        // A follow-up task should be created
        let tasks = state.tasks.list_by_repo(&repo_a.id).await.unwrap();
        assert_eq!(tasks.len(), 2, "original + follow-up task");

        let follow_up = tasks
            .iter()
            .find(|t| t.labels.contains(&"cascade-test-failure".to_string()))
            .expect("follow-up task should have cascade-test-failure label");

        assert!(follow_up.title.contains("Cascade test failure"));
        assert_eq!(follow_up.priority, TaskPriority::High);
        assert!(follow_up.labels.contains(&"auto-created".to_string()));
        assert_eq!(follow_up.workspace_id.as_str(), "ws-1");
        assert_eq!(follow_up.repo_id.as_str(), repo_a.id.as_str());
        assert!(follow_up
            .description
            .as_ref()
            .unwrap()
            .contains("Tests failed: 3 failures"));

        // Verify cascade_test_failed event was emitted (F1).
        let msg = rx
            .try_recv()
            .expect("cascade_test_failed event should be emitted");
        assert_eq!(
            msg.kind,
            gyre_common::MessageKind::Custom("cascade_test_failed".to_string())
        );
        let payload = msg.payload.as_ref().expect("event should have payload");
        assert_eq!(payload["event"], "cascade_test_failed");
        assert_eq!(payload["task_id"], "cascade-task-2");
        assert_eq!(payload["repo_id"], repo_a.id.as_str());
        assert!(payload["failure_details"]
            .as_str()
            .unwrap()
            .contains("Tests failed: 3 failures"));
        // follow_up_task_id should be present
        assert!(
            payload["follow_up_task_id"].as_str().is_some(),
            "event should include follow_up_task_id"
        );
    }

    #[tokio::test]
    async fn report_cascade_test_failure_notifies_workspace_members() {
        let state = test_state();

        let repo_a = create_repo_in_workspace(&state, "repo-a", "ws-1").await;

        // Add a workspace member
        let membership = gyre_domain::WorkspaceMembership::new(
            Id::new("member-1"),
            Id::new("user-1"),
            Id::new("ws-1"),
            gyre_domain::WorkspaceRole::Developer,
            Id::new("admin"),
            1000,
        );
        state
            .workspace_memberships
            .create(&membership)
            .await
            .unwrap();

        let mut task = gyre_domain::Task::new(
            Id::new("cascade-task-3"),
            "Cascade test: repo-a against repo-b@abc12345",
            1000,
        );
        task.labels = vec!["cascade-test".to_string(), "auto-created".to_string()];
        task.workspace_id = Id::new("ws-1");
        task.repo_id = repo_a.id.clone();
        state.tasks.create(&task).await.unwrap();

        // Subscribe to broadcast channel before calling (F1).
        let mut rx = state.message_broadcast_tx.subscribe();

        report_cascade_test_result(&state, &task, false, Some("test failure")).await;

        // Verify notification was created for the workspace member
        let notifications = state
            .notifications
            .list_for_user(
                &Id::new("user-1"),
                Some(&Id::new("ws-1")),
                None,
                None,
                None,
                None,
                100,
                0,
            )
            .await
            .unwrap();

        assert!(
            !notifications.is_empty(),
            "workspace member should be notified"
        );
        let notif = &notifications[0];
        assert_eq!(
            notif.notification_type,
            gyre_common::NotificationType::CascadeTestFailed
        );
        assert!(notif.title.contains("Cascade test failed"));

        // Verify notification body JSON contains expected fields (F3).
        let body: serde_json::Value =
            serde_json::from_str(notif.body.as_ref().expect("notification should have body"))
                .expect("notification body should be valid JSON");
        assert_eq!(body["task_id"], "cascade-task-3");
        assert!(
            body["follow_up_task_id"].as_str().is_some(),
            "body should contain follow_up_task_id"
        );
        assert_eq!(body["repo_id"], repo_a.id.as_str());
        assert_eq!(body["failure_details"], "test failure");

        // Verify entity_ref and priority (F3).
        assert_eq!(
            notif.source.entity_id.as_str(),
            repo_a.id.as_str(),
            "source entity should be the repo_id"
        );
        assert_eq!(
            notif.priority_rank,
            gyre_common::NotificationType::CascadeTestFailed.default_priority_rank(),
            "priority rank should match CascadeTestFailed priority"
        );

        // Verify cascade_test_failed event was also emitted (F1).
        let msg = rx
            .try_recv()
            .expect("cascade_test_failed event should be emitted");
        assert_eq!(
            msg.kind,
            gyre_common::MessageKind::Custom("cascade_test_failed".to_string())
        );
    }

    #[tokio::test]
    async fn trigger_cascade_tests_default_policy_enables_cascade() {
        let state = test_state();

        let repo_b = create_repo_in_workspace(&state, "repo-b", "ws-1").await;
        let repo_a = create_repo_in_workspace(&state, "repo-a", "ws-1").await;

        create_dependency(&state, &repo_a.id, &repo_b.id).await;

        // Default policy has require_cascade_tests: true
        // (no explicit policy set — uses default)

        let mut rx = state.message_broadcast_tx.subscribe();

        trigger_cascade_tests(
            &state,
            repo_b.id.as_str(),
            "repo-b",
            "sha12345",
            &Id::new("ws-1"),
        )
        .await;

        // Default policy enables cascade testing, so task should be created
        let tasks = state.tasks.list_by_repo(&repo_a.id).await.unwrap();
        assert_eq!(tasks.len(), 1);

        // Event should be emitted with default policy.
        let msg = rx
            .try_recv()
            .expect("event should be emitted with default policy");
        assert_eq!(
            msg.kind,
            gyre_common::MessageKind::Custom("cascade_test_triggered".to_string())
        );
    }

    /// F2: Verify that CascadeTestTriggered notifications are sent to workspace members
    /// when cascade tests are triggered.
    #[tokio::test]
    async fn trigger_cascade_tests_notifies_workspace_members() {
        let state = test_state();

        let repo_b = create_repo_in_workspace(&state, "repo-b", "ws-1").await;
        let repo_a = create_repo_in_workspace(&state, "repo-a", "ws-1").await;

        create_dependency(&state, &repo_a.id, &repo_b.id).await;

        // Add a workspace member
        let membership = gyre_domain::WorkspaceMembership::new(
            Id::new("member-1"),
            Id::new("user-1"),
            Id::new("ws-1"),
            gyre_domain::WorkspaceRole::Developer,
            Id::new("admin"),
            1000,
        );
        state
            .workspace_memberships
            .create(&membership)
            .await
            .unwrap();

        let policy = DependencyPolicy {
            require_cascade_tests: true,
            ..Default::default()
        };
        state
            .dependency_policies
            .set_for_workspace(&Id::new("ws-1"), &policy)
            .await
            .unwrap();

        let mut rx = state.message_broadcast_tx.subscribe();

        trigger_cascade_tests(
            &state,
            repo_b.id.as_str(),
            "repo-b",
            "abc123def456",
            &Id::new("ws-1"),
        )
        .await;

        // Verify cascade_test_triggered event was emitted.
        let msg = rx
            .try_recv()
            .expect("cascade_test_triggered event should be emitted");
        assert_eq!(
            msg.kind,
            gyre_common::MessageKind::Custom("cascade_test_triggered".to_string())
        );

        // Verify CascadeTestTriggered notification was sent
        let notifications = state
            .notifications
            .list_for_user(
                &Id::new("user-1"),
                Some(&Id::new("ws-1")),
                None,
                None,
                None,
                None,
                100,
                0,
            )
            .await
            .unwrap();

        assert!(
            !notifications.is_empty(),
            "workspace member should receive CascadeTestTriggered notification"
        );
        let notif = &notifications[0];
        assert_eq!(
            notif.notification_type,
            gyre_common::NotificationType::CascadeTestTriggered
        );
        assert!(notif.title.contains("Cascade test triggered"));
        assert!(notif.title.contains("repo-a"));
        assert!(notif.title.contains("repo-b"));

        // Verify notification body JSON fields
        let body: serde_json::Value =
            serde_json::from_str(notif.body.as_ref().expect("notification should have body"))
                .expect("body should be valid JSON");
        assert_eq!(body["merged_repo"], "repo-b");
        assert_eq!(body["merge_commit_sha"], "abc123def456");
        assert_eq!(body["dependent_repo"], "repo-a");
        assert!(
            body["task_id"].as_str().is_some(),
            "body should contain task_id"
        );

        // Verify entity_ref and priority
        assert_eq!(
            notif.source.entity_id.as_str(),
            repo_a.id.as_str(),
            "source entity should be the dependent repo_id"
        );
        assert_eq!(
            notif.priority_rank,
            gyre_common::NotificationType::CascadeTestTriggered.default_priority_rank(),
            "priority rank should match CascadeTestTriggered priority"
        );
    }

    /// F4: Verify that cascade tests are skipped for dependent repos whose workspace
    /// has cascade testing disabled, even if the merged repo's workspace has it enabled.
    #[tokio::test]
    async fn trigger_cascade_tests_respects_dependent_workspace_opt_out() {
        let state = test_state();

        let repo_b = create_repo_in_workspace(&state, "repo-b", "ws-MERGED").await;
        let repo_a = create_repo_in_workspace(&state, "repo-a", "ws-DEPENDENT").await;

        create_dependency(&state, &repo_a.id, &repo_b.id).await;

        // Enable cascade testing in the merged repo's workspace
        let merged_policy = DependencyPolicy {
            require_cascade_tests: true,
            ..Default::default()
        };
        state
            .dependency_policies
            .set_for_workspace(&Id::new("ws-MERGED"), &merged_policy)
            .await
            .unwrap();

        // Disable cascade testing in the dependent repo's workspace
        let dep_policy = DependencyPolicy {
            require_cascade_tests: false,
            ..Default::default()
        };
        state
            .dependency_policies
            .set_for_workspace(&Id::new("ws-DEPENDENT"), &dep_policy)
            .await
            .unwrap();

        let mut rx = state.message_broadcast_tx.subscribe();

        trigger_cascade_tests(
            &state,
            repo_b.id.as_str(),
            "repo-b",
            "sha12345",
            &Id::new("ws-MERGED"),
        )
        .await;

        // No task should be created — dependent workspace opted out
        let tasks = state.tasks.list_by_repo(&repo_a.id).await.unwrap();
        assert!(
            tasks.is_empty(),
            "no cascade test task should be created when dependent workspace has cascade testing disabled"
        );

        // No events should be emitted when dependent workspace opts out.
        assert!(
            rx.try_recv().is_err(),
            "no events should be emitted when dependent workspace has cascade testing disabled"
        );
    }

    // ── Topological sort & priority tests (TASK-026) ─────────────────────────

    /// Create an MR with the given id, workspace, and depends_on list.
    async fn create_mr_with_deps(
        state: &AppState,
        mr_id: &str,
        workspace_id: &str,
        depends_on: Vec<&str>,
    ) -> gyre_domain::MergeRequest {
        let mut mr = gyre_domain::MergeRequest::new(
            Id::new(mr_id),
            Id::new("repo-1"),
            format!("MR {mr_id}"),
            format!("feat/{mr_id}"),
            "main",
            1000,
        );
        mr.workspace_id = Id::new(workspace_id);
        mr.depends_on = depends_on
            .into_iter()
            .map(|id| {
                gyre_domain::MergeRequestDependency::new(
                    Id::new(id),
                    gyre_domain::DependencySource::Explicit,
                )
            })
            .collect();
        state.merge_requests.create(&mr).await.unwrap();
        mr
    }

    /// Enqueue an MR with a given priority and enqueued_at timestamp.
    async fn enqueue_mr(
        state: &AppState,
        mr_id: &str,
        priority: u32,
        enqueued_at: u64,
    ) -> MergeQueueEntry {
        let entry = MergeQueueEntry::new(
            Id::new(format!("entry-{mr_id}")),
            Id::new(mr_id),
            priority,
            enqueued_at,
        );
        state.merge_queue.enqueue(&entry).await.unwrap();
        entry
    }

    #[tokio::test]
    async fn topological_sort_prioritizes_independent_mrs() {
        // Two independent MRs (no deps between them): higher priority processes first.
        let state = test_state();

        create_mr_with_deps(&state, "mr-low", "ws-1", vec![]).await;
        create_mr_with_deps(&state, "mr-high", "ws-1", vec![]).await;

        let e_low = enqueue_mr(&state, "mr-low", 25, 1000).await; // Low priority
        let e_high = enqueue_mr(&state, "mr-high", 100, 1001).await; // Critical priority

        let entries = vec![e_low.clone(), e_high.clone()];
        let graph = build_queue_dependency_graph(&state, &entries)
            .await
            .unwrap();
        let sorted = topological_sort_with_priority(&entries, &graph);

        assert_eq!(sorted.len(), 2);
        assert_eq!(
            sorted[0].merge_request_id.as_str(),
            "mr-high",
            "higher priority MR should be first"
        );
        assert_eq!(
            sorted[1].merge_request_id.as_str(),
            "mr-low",
            "lower priority MR should be second"
        );
    }

    #[tokio::test]
    async fn topological_sort_respects_dependencies_over_priority() {
        // mr-high (priority 100) depends on mr-low (priority 25).
        // mr-low must come first despite lower priority.
        let state = test_state();

        create_mr_with_deps(&state, "mr-low", "ws-1", vec![]).await;
        create_mr_with_deps(&state, "mr-high", "ws-1", vec!["mr-low"]).await;

        let e_low = enqueue_mr(&state, "mr-low", 25, 1000).await;
        let e_high = enqueue_mr(&state, "mr-high", 100, 1001).await;

        let entries = vec![e_high.clone(), e_low.clone()]; // intentional reverse order
        let graph = build_queue_dependency_graph(&state, &entries)
            .await
            .unwrap();
        let sorted = topological_sort_with_priority(&entries, &graph);

        assert_eq!(sorted.len(), 2);
        assert_eq!(
            sorted[0].merge_request_id.as_str(),
            "mr-low",
            "dependency must come first even with lower priority"
        );
        assert_eq!(
            sorted[1].merge_request_id.as_str(),
            "mr-high",
            "dependent MR comes after its dependency"
        );
    }

    #[tokio::test]
    async fn topological_sort_priority_within_same_tier() {
        // Diamond: mr-a depends on mr-root, mr-b depends on mr-root.
        // mr-a has higher priority than mr-b.
        // Expected: mr-root first, then mr-a, then mr-b.
        let state = test_state();

        create_mr_with_deps(&state, "mr-root", "ws-1", vec![]).await;
        create_mr_with_deps(&state, "mr-a", "ws-1", vec!["mr-root"]).await;
        create_mr_with_deps(&state, "mr-b", "ws-1", vec!["mr-root"]).await;

        let e_root = enqueue_mr(&state, "mr-root", 50, 1000).await;
        let e_a = enqueue_mr(&state, "mr-a", 100, 1001).await;
        let e_b = enqueue_mr(&state, "mr-b", 25, 1002).await;

        let entries = vec![e_b.clone(), e_root.clone(), e_a.clone()];
        let graph = build_queue_dependency_graph(&state, &entries)
            .await
            .unwrap();
        let sorted = topological_sort_with_priority(&entries, &graph);

        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[0].merge_request_id.as_str(), "mr-root");
        assert_eq!(
            sorted[1].merge_request_id.as_str(),
            "mr-a",
            "higher priority independent MR in tier 2 should be first"
        );
        assert_eq!(
            sorted[2].merge_request_id.as_str(),
            "mr-b",
            "lower priority independent MR in tier 2 should be second"
        );
    }

    #[tokio::test]
    async fn topological_sort_chain_ordering() {
        // Linear chain: mr-c depends on mr-b, mr-b depends on mr-a.
        // Even if mr-c has highest priority, order must be: a, b, c.
        let state = test_state();

        create_mr_with_deps(&state, "mr-a", "ws-1", vec![]).await;
        create_mr_with_deps(&state, "mr-b", "ws-1", vec!["mr-a"]).await;
        create_mr_with_deps(&state, "mr-c", "ws-1", vec!["mr-b"]).await;

        let e_a = enqueue_mr(&state, "mr-a", 25, 1000).await;
        let e_b = enqueue_mr(&state, "mr-b", 50, 1001).await;
        let e_c = enqueue_mr(&state, "mr-c", 100, 1002).await;

        let entries = vec![e_c.clone(), e_a.clone(), e_b.clone()];
        let graph = build_queue_dependency_graph(&state, &entries)
            .await
            .unwrap();
        let sorted = topological_sort_with_priority(&entries, &graph);

        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[0].merge_request_id.as_str(), "mr-a");
        assert_eq!(sorted[1].merge_request_id.as_str(), "mr-b");
        assert_eq!(sorted[2].merge_request_id.as_str(), "mr-c");
    }

    #[tokio::test]
    async fn topological_sort_atomic_group_ordering() {
        // mr-first and mr-second are in atomic group "bundle".
        // mr-first enqueued before mr-second.
        // Atomic group implies: mr-second depends on mr-first.
        let state = test_state();

        let mut mr_first = gyre_domain::MergeRequest::new(
            Id::new("mr-first"),
            Id::new("repo-1"),
            "First in group",
            "feat/first",
            "main",
            1000,
        );
        mr_first.workspace_id = Id::new("ws-1");
        mr_first.atomic_group = Some("bundle".to_string());
        state.merge_requests.create(&mr_first).await.unwrap();

        let mut mr_second = gyre_domain::MergeRequest::new(
            Id::new("mr-second"),
            Id::new("repo-1"),
            "Second in group",
            "feat/second",
            "main",
            1000,
        );
        mr_second.workspace_id = Id::new("ws-1");
        mr_second.atomic_group = Some("bundle".to_string());
        state.merge_requests.create(&mr_second).await.unwrap();

        // mr-second has higher priority but must come after mr-first (enqueue order)
        let e_first = enqueue_mr(&state, "mr-first", 25, 1000).await;
        let e_second = enqueue_mr(&state, "mr-second", 100, 1001).await;

        let entries = vec![e_second.clone(), e_first.clone()];
        let graph = build_queue_dependency_graph(&state, &entries)
            .await
            .unwrap();
        let sorted = topological_sort_with_priority(&entries, &graph);

        assert_eq!(sorted.len(), 2);
        assert_eq!(
            sorted[0].merge_request_id.as_str(),
            "mr-first",
            "earlier-enqueued atomic group member must come first"
        );
        assert_eq!(
            sorted[1].merge_request_id.as_str(),
            "mr-second",
            "later-enqueued atomic group member must come second"
        );
    }

    #[tokio::test]
    async fn chain_depth_warning_emitted_at_depth_exceeding_10() {
        // Build a chain of 12 MRs: mr-0 <- mr-1 <- ... <- mr-11.
        // Chain depth = 11 levels, which exceeds the >10 threshold.
        let state = test_state();

        // Add a workspace member for notification delivery.
        let membership = gyre_domain::WorkspaceMembership::new(
            Id::new("member-1"),
            Id::new("user-1"),
            Id::new("ws-1"),
            gyre_domain::WorkspaceRole::Developer,
            Id::new("admin"),
            1000,
        );
        state
            .workspace_memberships
            .create(&membership)
            .await
            .unwrap();

        let mut entries = Vec::new();
        for i in 0..12 {
            let mr_id = format!("mr-{i}");
            let deps: Vec<String> = if i == 0 {
                vec![]
            } else {
                vec![format!("mr-{}", i - 1)]
            };
            let mut mr = gyre_domain::MergeRequest::new(
                Id::new(&mr_id),
                Id::new("repo-1"),
                format!("MR {mr_id}"),
                format!("feat/{mr_id}"),
                "main",
                1000,
            );
            mr.workspace_id = Id::new("ws-1");
            mr.depends_on = deps
                .into_iter()
                .map(|id| {
                    gyre_domain::MergeRequestDependency::new(
                        Id::new(id),
                        gyre_domain::DependencySource::Explicit,
                    )
                })
                .collect();
            state.merge_requests.create(&mr).await.unwrap();

            let entry = enqueue_mr(&state, &mr_id, 50, 1000 + i as u64).await;
            entries.push(entry);
        }

        let graph = build_queue_dependency_graph(&state, &entries)
            .await
            .unwrap();
        let max_depth = compute_max_chain_depth(&graph);
        assert_eq!(max_depth, 11, "chain of 12 MRs has depth 11");
        assert!(max_depth > 10, "should exceed the >10 threshold");

        // Actually trigger the warning via process_next — which will call warn_deep_chain.
        // Instead of calling process_next (which would try to merge),
        // call warn_deep_chain directly.
        warn_deep_chain(&state, max_depth, &Id::new("ws-1")).await;

        // Verify notification was created.
        let notifications = state
            .notifications
            .list_for_user(
                &Id::new("user-1"),
                Some(&Id::new("ws-1")),
                None,
                None,
                None,
                None,
                100,
                0,
            )
            .await
            .unwrap();

        assert_eq!(notifications.len(), 1);
        let notif = &notifications[0];
        assert_eq!(
            notif.notification_type,
            gyre_common::NotificationType::DependencyChainTooDeep
        );
        assert_eq!(
            notif.priority_rank,
            gyre_common::NotificationType::DependencyChainTooDeep.default_priority_rank(),
        );
        assert_eq!(
            notif.priority_rank, 7,
            "DependencyChainTooDeep should be priority 7"
        );
        assert!(notif.title.contains("11 levels deep"));

        // Verify body JSON.
        let body: serde_json::Value =
            serde_json::from_str(notif.body.as_ref().expect("should have body"))
                .expect("should be valid JSON");
        assert_eq!(body["chain_depth"], 11);
        assert_eq!(body["threshold"], 10);
        assert_eq!(body["workspace_id"], "ws-1");
    }

    #[tokio::test]
    async fn chain_depth_not_warned_at_10_or_below() {
        // Chain of 11 MRs = depth 10, should NOT trigger warning.
        let state = test_state();

        let mut entries = Vec::new();
        for i in 0..11 {
            let mr_id = format!("mr-{i}");
            let deps: Vec<String> = if i == 0 {
                vec![]
            } else {
                vec![format!("mr-{}", i - 1)]
            };
            let mut mr = gyre_domain::MergeRequest::new(
                Id::new(&mr_id),
                Id::new("repo-1"),
                format!("MR {mr_id}"),
                format!("feat/{mr_id}"),
                "main",
                1000,
            );
            mr.workspace_id = Id::new("ws-1");
            mr.depends_on = deps
                .into_iter()
                .map(|id| {
                    gyre_domain::MergeRequestDependency::new(
                        Id::new(id),
                        gyre_domain::DependencySource::Explicit,
                    )
                })
                .collect();
            state.merge_requests.create(&mr).await.unwrap();

            let entry = enqueue_mr(&state, &mr_id, 50, 1000 + i as u64).await;
            entries.push(entry);
        }

        let graph = build_queue_dependency_graph(&state, &entries)
            .await
            .unwrap();
        let max_depth = compute_max_chain_depth(&graph);
        assert_eq!(max_depth, 10, "chain of 11 MRs has depth 10");
        assert!(max_depth <= 10, "should NOT exceed the >10 threshold");
    }

    #[tokio::test]
    async fn chain_depth_diamond_asymmetric_uses_longest_path() {
        // Asymmetric diamond: mr-a -> mr-g (direct, depth 1)
        //                     mr-a -> mr-b -> mr-c -> mr-g (depth 3)
        // Longest path through g should be 3, not 1.
        // This tests that the algorithm computes longest paths, not shortest (BFS).
        let state = test_state();

        create_mr_with_deps(&state, "mr-a", "ws-1", vec![]).await;
        create_mr_with_deps(&state, "mr-b", "ws-1", vec!["mr-a"]).await;
        create_mr_with_deps(&state, "mr-c", "ws-1", vec!["mr-b"]).await;
        create_mr_with_deps(&state, "mr-g", "ws-1", vec!["mr-a", "mr-c"]).await;

        let e_a = enqueue_mr(&state, "mr-a", 50, 1000).await;
        let e_b = enqueue_mr(&state, "mr-b", 50, 1001).await;
        let e_c = enqueue_mr(&state, "mr-c", 50, 1002).await;
        let e_g = enqueue_mr(&state, "mr-g", 50, 1003).await;

        let entries = vec![e_a, e_b, e_c, e_g];
        let graph = build_queue_dependency_graph(&state, &entries)
            .await
            .unwrap();
        let max_depth = compute_max_chain_depth(&graph);
        assert_eq!(
            max_depth, 3,
            "longest path a->b->c->g has depth 3, not shortest a->g depth 1"
        );
    }

    #[tokio::test]
    async fn topological_sort_same_priority_uses_enqueue_order() {
        // Two independent MRs with same priority: FIFO by enqueued_at.
        let state = test_state();

        create_mr_with_deps(&state, "mr-earlier", "ws-1", vec![]).await;
        create_mr_with_deps(&state, "mr-later", "ws-1", vec![]).await;

        let e_earlier = enqueue_mr(&state, "mr-earlier", 50, 1000).await;
        let e_later = enqueue_mr(&state, "mr-later", 50, 1001).await;

        let entries = vec![e_later.clone(), e_earlier.clone()];
        let graph = build_queue_dependency_graph(&state, &entries)
            .await
            .unwrap();
        let sorted = topological_sort_with_priority(&entries, &graph);

        assert_eq!(sorted.len(), 2);
        assert_eq!(
            sorted[0].merge_request_id.as_str(),
            "mr-earlier",
            "earlier-enqueued MR should be first when priorities are equal"
        );
        assert_eq!(sorted[1].merge_request_id.as_str(), "mr-later");
    }

    #[tokio::test]
    async fn topological_sort_dep_outside_queue_ignored() {
        // mr-b depends on mr-a, but mr-a is not in the queue (already merged).
        // mr-b should still be processable.
        let state = test_state();

        // Create mr-a but DON'T enqueue it.
        create_mr_with_deps(&state, "mr-a", "ws-1", vec![]).await;
        create_mr_with_deps(&state, "mr-b", "ws-1", vec!["mr-a"]).await;

        let e_b = enqueue_mr(&state, "mr-b", 50, 1000).await;

        let entries = vec![e_b.clone()];
        let graph = build_queue_dependency_graph(&state, &entries)
            .await
            .unwrap();
        let sorted = topological_sort_with_priority(&entries, &graph);

        assert_eq!(sorted.len(), 1);
        assert_eq!(
            sorted[0].merge_request_id.as_str(),
            "mr-b",
            "MR with out-of-queue dependency should still appear in sorted order"
        );
        // The dep on mr-a is not in the graph (only in-queue deps are edges).
        // dependencies_satisfied() at merge time will check the actual MR status.
        assert!(
            graph.get("mr-b").unwrap().is_empty(),
            "out-of-queue dependency should not appear in graph edges"
        );
    }

    #[tokio::test]
    async fn empty_queue_returns_empty_sort() {
        let state = test_state();
        let entries: Vec<MergeQueueEntry> = vec![];
        let graph = build_queue_dependency_graph(&state, &entries)
            .await
            .unwrap();
        let sorted = topological_sort_with_priority(&entries, &graph);
        assert!(sorted.is_empty());
        assert_eq!(compute_max_chain_depth(&graph), 0);
    }

    /// F1 scenario: When the highest-priority candidate has pending gates, the
    /// selection loop should skip it and process the next candidate whose gates
    /// have passed. Before the fix, the loop selected the first deps-satisfied
    /// candidate and only checked gates after the loop — if gates were pending,
    /// the entry was requeued and subsequent candidates were never tried.
    #[tokio::test]
    async fn selection_skips_gates_pending_tries_next_candidate() {
        use gyre_domain::{GateResult, GateType, QualityGate};

        let state = test_state();

        // Create a repo for the MRs.
        let repo = create_repo_in_workspace(&state, "test-repo", "ws-1").await;

        // Create two independent MRs (no dependencies between them).
        let mut mr_a = gyre_domain::MergeRequest::new(
            Id::new("mr-a"),
            repo.id.clone(),
            "MR A (gates pending)",
            "feat/a",
            "main",
            1000,
        );
        mr_a.workspace_id = Id::new("ws-1");
        state.merge_requests.create(&mr_a).await.unwrap();

        let mut mr_b = gyre_domain::MergeRequest::new(
            Id::new("mr-b"),
            repo.id.clone(),
            "MR B (gates passed)",
            "feat/b",
            "main",
            1000,
        );
        mr_b.workspace_id = Id::new("ws-1");
        state.merge_requests.create(&mr_b).await.unwrap();

        // Enqueue: mr-a with higher priority, mr-b with lower priority.
        enqueue_mr(&state, "mr-a", 100, 1000).await;
        enqueue_mr(&state, "mr-b", 50, 1001).await;

        // Create a required quality gate.
        let gate = QualityGate {
            id: Id::new("gate-1"),
            repo_id: repo.id.clone(),
            name: "unit-tests".to_string(),
            gate_type: GateType::TestCommand,
            command: Some("cargo test".to_string()),
            required_approvals: None,
            persona: None,
            validation_type: None,
            required: true,
            gate_phase: Default::default(),
            timeout_secs: None,
            position: 0,
            created_at: 1000,
        };
        state.quality_gates.save(&gate).await.unwrap();

        // mr-a has a pending gate result → check_gates_for_mr returns Ok(false).
        let gate_result_a = GateResult {
            id: Id::new("gr-a"),
            gate_id: Id::new("gate-1"),
            mr_id: Id::new("mr-a"),
            status: GateStatus::Pending,
            output: None,
            started_at: None,
            finished_at: None,
        };
        state.gate_results.save(&gate_result_a).await.unwrap();

        // mr-b has NO gate results → check_gates_for_mr returns Ok(true) (all passed).
        // (No gate result means no required gates are pending/failed.)

        // Run a merge-processor cycle.
        // event-emission:ok — this test verifies candidate selection behavior (gates pending → skip),
        // not merge-time event emission which is covered by other tests.
        process_next(&state).await.unwrap();

        // Verify: mr-b was selected and merged (not mr-a).
        let updated_b = state
            .merge_requests
            .find_by_id(&Id::new("mr-b"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            updated_b.status,
            MrStatus::Merged,
            "mr-b should be merged because it was the next candidate after mr-a was skipped"
        );

        // Verify: mr-a was NOT processed — still Open status.
        let updated_a = state
            .merge_requests
            .find_by_id(&Id::new("mr-a"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            updated_a.status,
            MrStatus::Open,
            "mr-a should remain Open because its gates are pending"
        );

        // Verify queue states: mr-b's entry should be Merged, mr-a's should still be Queued.
        let entry_a = state
            .merge_queue
            .find_by_id(&Id::new("entry-mr-a"))
            .await
            .unwrap()
            .expect("mr-a entry should exist");
        assert_eq!(
            entry_a.status,
            MergeQueueEntryStatus::Queued,
            "mr-a queue entry should remain Queued"
        );

        let entry_b = state
            .merge_queue
            .find_by_id(&Id::new("entry-mr-b"))
            .await
            .unwrap()
            .expect("mr-b entry should exist");
        assert_eq!(
            entry_b.status,
            MergeQueueEntryStatus::Merged,
            "mr-b queue entry should be Merged"
        );
    }

    /// Verify that a candidate with failed gates is marked Failed during selection
    /// and the next candidate is tried.
    #[tokio::test]
    async fn selection_fails_gates_failed_entry_tries_next() {
        use gyre_domain::{GateResult, GateType, QualityGate};

        let state = test_state();

        let repo = create_repo_in_workspace(&state, "test-repo", "ws-1").await;

        // Create two independent MRs.
        let mut mr_a = gyre_domain::MergeRequest::new(
            Id::new("mr-a"),
            repo.id.clone(),
            "MR A (gate failed)",
            "feat/a",
            "main",
            1000,
        );
        mr_a.workspace_id = Id::new("ws-1");
        state.merge_requests.create(&mr_a).await.unwrap();

        let mut mr_b = gyre_domain::MergeRequest::new(
            Id::new("mr-b"),
            repo.id.clone(),
            "MR B (no gates)",
            "feat/b",
            "main",
            1000,
        );
        mr_b.workspace_id = Id::new("ws-1");
        state.merge_requests.create(&mr_b).await.unwrap();

        enqueue_mr(&state, "mr-a", 100, 1000).await;
        enqueue_mr(&state, "mr-b", 50, 1001).await;

        // Create a required gate and a failed result for mr-a.
        let gate = QualityGate {
            id: Id::new("gate-1"),
            repo_id: repo.id.clone(),
            name: "unit-tests".to_string(),
            gate_type: GateType::TestCommand,
            command: Some("cargo test".to_string()),
            required_approvals: None,
            persona: None,
            validation_type: None,
            required: true,
            gate_phase: Default::default(),
            timeout_secs: None,
            position: 0,
            created_at: 1000,
        };
        state.quality_gates.save(&gate).await.unwrap();

        let gate_result_a = GateResult {
            id: Id::new("gr-a"),
            gate_id: Id::new("gate-1"),
            mr_id: Id::new("mr-a"),
            status: GateStatus::Failed,
            output: Some("test failure".to_string()),
            started_at: Some(1000),
            finished_at: Some(1001),
        };
        state.gate_results.save(&gate_result_a).await.unwrap();

        // event-emission:ok — this test verifies candidate selection behavior (gate failed → fail entry,
        // try next), not merge-time event emission which is covered by other tests.
        process_next(&state).await.unwrap();

        // mr-a should be Failed (gate failure during selection).
        let entry_a = state
            .merge_queue
            .find_by_id(&Id::new("entry-mr-a"))
            .await
            .unwrap()
            .expect("mr-a entry should exist");
        assert_eq!(
            entry_a.status,
            MergeQueueEntryStatus::Failed,
            "mr-a queue entry should be Failed due to gate failure"
        );

        // mr-b should be merged (selected after mr-a was failed).
        let updated_b = state
            .merge_requests
            .find_by_id(&Id::new("mr-b"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            updated_b.status,
            MrStatus::Merged,
            "mr-b should be merged after mr-a was failed due to gate failure"
        );
    }

    /// Verify that a candidate whose atomic group is not ready is skipped,
    /// and the next topologically-ordered candidate is tried (spec step 4c).
    #[tokio::test]
    async fn selection_skips_atomic_group_not_ready_tries_next_candidate() {
        use gyre_domain::{GateResult, GateType, QualityGate};

        let state = test_state();

        let repo = create_repo_in_workspace(&state, "test-repo", "ws-1").await;

        // MR A: in atomic group "bundle", high priority.
        let mut mr_a = gyre_domain::MergeRequest::new(
            Id::new("mr-a"),
            repo.id.clone(),
            "MR A (in atomic group)",
            "feat/a",
            "main",
            1000,
        );
        mr_a.workspace_id = Id::new("ws-1");
        mr_a.atomic_group = Some("bundle".to_string());
        state.merge_requests.create(&mr_a).await.unwrap();

        // MR C: also in atomic group "bundle" — has a pending gate,
        // making the group not ready for MR A.
        let mut mr_c = gyre_domain::MergeRequest::new(
            Id::new("mr-c"),
            repo.id.clone(),
            "MR C (group member, gates pending)",
            "feat/c",
            "main",
            1000,
        );
        mr_c.workspace_id = Id::new("ws-1");
        mr_c.atomic_group = Some("bundle".to_string());
        state.merge_requests.create(&mr_c).await.unwrap();

        // MR B: no atomic group, low priority, all gates passed.
        let mut mr_b = gyre_domain::MergeRequest::new(
            Id::new("mr-b"),
            repo.id.clone(),
            "MR B (no group)",
            "feat/b",
            "main",
            1000,
        );
        mr_b.workspace_id = Id::new("ws-1");
        state.merge_requests.create(&mr_b).await.unwrap();

        // Enqueue: mr-a (high priority), mr-b (low priority).
        // mr-c is NOT enqueued — it's only a group member affecting readiness.
        enqueue_mr(&state, "mr-a", 100, 1000).await;
        enqueue_mr(&state, "mr-b", 50, 1001).await;

        // Create a required gate and a pending result for mr-c.
        // This makes atomic_group_ready("bundle", "mr-a") return Ok(false)
        // because group member mr-c has a pending required gate.
        let gate = QualityGate {
            id: Id::new("gate-1"),
            repo_id: repo.id.clone(),
            name: "unit-tests".to_string(),
            gate_type: GateType::TestCommand,
            command: Some("cargo test".to_string()),
            required_approvals: None,
            persona: None,
            validation_type: None,
            required: true,
            gate_phase: Default::default(),
            timeout_secs: None,
            position: 0,
            created_at: 1000,
        };
        state.quality_gates.save(&gate).await.unwrap();

        let gate_result_c = GateResult {
            id: Id::new("gr-c"),
            gate_id: Id::new("gate-1"),
            mr_id: Id::new("mr-c"),
            status: GateStatus::Pending,
            output: None,
            started_at: None,
            finished_at: None,
        };
        state.gate_results.save(&gate_result_c).await.unwrap();

        // Run a merge-processor cycle.
        // event-emission:ok — this test verifies candidate selection behavior (atomic group
        // not ready → skip), not merge-time event emission which is covered by other tests.
        process_next(&state).await.unwrap();

        // Verify: mr-b was selected and merged (not mr-a).
        let updated_b = state
            .merge_requests
            .find_by_id(&Id::new("mr-b"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            updated_b.status,
            MrStatus::Merged,
            "mr-b should be merged because it was the next candidate after mr-a was skipped"
        );

        // Verify: mr-a was NOT processed — still Open status.
        let updated_a = state
            .merge_requests
            .find_by_id(&Id::new("mr-a"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            updated_a.status,
            MrStatus::Open,
            "mr-a should remain Open because its atomic group is not ready"
        );

        // Verify queue states: mr-b's entry should be Merged, mr-a's should still be Queued.
        let entry_a = state
            .merge_queue
            .find_by_id(&Id::new("entry-mr-a"))
            .await
            .unwrap()
            .expect("mr-a entry should exist");
        assert_eq!(
            entry_a.status,
            MergeQueueEntryStatus::Queued,
            "mr-a queue entry should remain Queued"
        );

        let entry_b = state
            .merge_queue
            .find_by_id(&Id::new("entry-mr-b"))
            .await
            .unwrap()
            .expect("mr-b entry should exist");
        assert_eq!(
            entry_b.status,
            MergeQueueEntryStatus::Merged,
            "mr-b queue entry should be Merged"
        );
    }

    // ── Atomic group transactional merge tests (TASK-027) ─────────────────

    /// Helper to create an MR in an atomic group.
    async fn create_mr_in_group(
        state: &AppState,
        mr_id: &str,
        repo_id: &Id,
        workspace_id: &str,
        group: &str,
        source_branch: &str,
        author_agent_id: Option<&str>,
    ) -> gyre_domain::MergeRequest {
        let mut mr = gyre_domain::MergeRequest::new(
            Id::new(mr_id),
            repo_id.clone(),
            format!("MR {mr_id}"),
            source_branch,
            "main",
            1000,
        );
        mr.workspace_id = Id::new(workspace_id);
        mr.atomic_group = Some(group.to_string());
        mr.author_agent_id = author_agent_id.map(Id::new);
        state.merge_requests.create(&mr).await.unwrap();
        mr
    }

    /// Test: All members of an atomic group merge in a single processor cycle
    /// in dependency order with no interleaving. Also verifies that deferred
    /// side effects (per-member analytics events and author notifications) are
    /// emitted after the successful group merge.
    #[tokio::test]
    async fn atomic_group_all_members_merge_in_one_cycle() {
        let state = test_state();

        let repo = create_repo_in_workspace(&state, "test-repo", "ws-1").await;

        // Create agent records for author agents so notify_mr_merged can resolve
        // the human user via spawned_by.
        let mut agent_1 = gyre_domain::Agent::new(Id::new("agent-1"), "agent-1", 1000);
        agent_1.spawned_by = Some("user-human-1".to_string());
        agent_1.workspace_id = Id::new("ws-1");
        state.agents.create(&agent_1).await.unwrap();

        let mut agent_2 = gyre_domain::Agent::new(Id::new("agent-2"), "agent-2", 1000);
        agent_2.spawned_by = Some("user-human-2".to_string());
        agent_2.workspace_id = Id::new("ws-1");
        state.agents.create(&agent_2).await.unwrap();

        // Create two MRs in atomic group "bundle".
        create_mr_in_group(
            &state,
            "mr-first",
            &repo.id,
            "ws-1",
            "bundle",
            "feat/first",
            Some("agent-1"),
        )
        .await;
        create_mr_in_group(
            &state,
            "mr-second",
            &repo.id,
            "ws-1",
            "bundle",
            "feat/second",
            Some("agent-2"),
        )
        .await;

        // Enqueue in order: mr-first first, mr-second second.
        enqueue_mr(&state, "mr-first", 50, 1000).await;
        enqueue_mr(&state, "mr-second", 50, 1001).await;

        // Subscribe to broadcast channel before running.
        let mut rx = state.message_broadcast_tx.subscribe();

        // Run a single merge-processor cycle.
        process_next(&state).await.unwrap();

        // Both members should be merged.
        let updated_first = state
            .merge_requests
            .find_by_id(&Id::new("mr-first"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            updated_first.status,
            MrStatus::Merged,
            "mr-first should be merged"
        );

        let updated_second = state
            .merge_requests
            .find_by_id(&Id::new("mr-second"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            updated_second.status,
            MrStatus::Merged,
            "mr-second should be merged"
        );

        // Both queue entries should be Merged.
        let entry_first = state
            .merge_queue
            .find_by_id(&Id::new("entry-mr-first"))
            .await
            .unwrap()
            .expect("mr-first entry should exist");
        assert_eq!(entry_first.status, MergeQueueEntryStatus::Merged);

        let entry_second = state
            .merge_queue
            .find_by_id(&Id::new("entry-mr-second"))
            .await
            .unwrap()
            .expect("mr-second entry should exist");
        assert_eq!(entry_second.status, MergeQueueEntryStatus::Merged);

        // No AtomicGroupFailed event should be emitted (success case).
        let mut found_failed = false;
        while let Ok(msg) = rx.try_recv() {
            if msg.kind == gyre_common::MessageKind::AtomicGroupFailed {
                found_failed = true;
            }
        }
        assert!(
            !found_failed,
            "no AtomicGroupFailed event should be emitted on success"
        );

        // --- Deferred side-effect assertions (F3 coverage) ---

        // Verify analytics events: each merged member should have a
        // "merge_queue.processed" event with "atomic_group" in the payload.
        let analytics_events = state
            .analytics
            .query(Some("merge_queue.processed"), None, 100)
            .await
            .unwrap();
        assert_eq!(
            analytics_events.len(),
            2,
            "two analytics events should be recorded (one per merged member)"
        );
        for ev in &analytics_events {
            assert_eq!(ev.event_name, "merge_queue.processed");
            let payload = &ev.properties;
            assert_eq!(
                payload["result"], "merged",
                "analytics event result should be 'merged'"
            );
            assert_eq!(
                payload["atomic_group"], "bundle",
                "analytics event should reference the atomic group name"
            );
        }

        // Verify author notifications: each author's human user should receive
        // a merge notification via notify_mr_merged (which resolves spawned_by).
        let notifs_user1 = state
            .notifications
            .list_for_user(
                &Id::new("user-human-1"),
                Some(&Id::new("ws-1")),
                None,
                None,
                None,
                None,
                100,
                0,
            )
            .await
            .unwrap();
        assert!(
            !notifs_user1.is_empty(),
            "user-human-1 (agent-1's spawner) should receive a merge notification"
        );
        assert!(
            notifs_user1[0].title.contains("merged"),
            "notification title should mention 'merged'"
        );

        let notifs_user2 = state
            .notifications
            .list_for_user(
                &Id::new("user-human-2"),
                Some(&Id::new("ws-1")),
                None,
                None,
                None,
                None,
                100,
                0,
            )
            .await
            .unwrap();
        assert!(
            !notifs_user2.is_empty(),
            "user-human-2 (agent-2's spawner) should receive a merge notification"
        );
        assert!(
            notifs_user2[0].title.contains("merged"),
            "notification title should mention 'merged'"
        );
    }

    /// Test: No interleaving — a non-group MR does not merge between group members.
    #[tokio::test]
    async fn atomic_group_no_interleaving_with_non_group_mr() {
        let state = test_state();

        let repo = create_repo_in_workspace(&state, "test-repo", "ws-1").await;

        // Create two MRs in atomic group "bundle".
        create_mr_in_group(&state, "mr-g1", &repo.id, "ws-1", "bundle", "feat/g1", None).await;
        create_mr_in_group(&state, "mr-g2", &repo.id, "ws-1", "bundle", "feat/g2", None).await;

        // Create a non-group MR with HIGHER priority than the group members.
        let mut mr_solo = gyre_domain::MergeRequest::new(
            Id::new("mr-solo"),
            repo.id.clone(),
            "Solo MR",
            "feat/solo",
            "main",
            1000,
        );
        mr_solo.workspace_id = Id::new("ws-1");
        state.merge_requests.create(&mr_solo).await.unwrap();

        // Enqueue: group members first, solo with higher priority.
        enqueue_mr(&state, "mr-g1", 50, 1000).await;
        enqueue_mr(&state, "mr-g2", 50, 1001).await;
        enqueue_mr(&state, "mr-solo", 100, 1002).await;

        // Run one cycle — the solo MR has higher priority but the topological sort
        // picks mr-solo first (it's independent). But since mr-g1 is also independent,
        // if mr-solo is selected first (higher priority), it should merge solo.
        // Then on the next cycle, the group should merge together.

        // First cycle: mr-solo merges (highest priority independent).
        // event-emission:ok — this test verifies atomic group no-interleaving behavior,
        // not merge-time event emission which is covered by other tests.
        process_next(&state).await.unwrap();

        let solo_status = state
            .merge_requests
            .find_by_id(&Id::new("mr-solo"))
            .await
            .unwrap()
            .unwrap()
            .status;
        assert_eq!(
            solo_status,
            MrStatus::Merged,
            "solo MR should merge in the first cycle"
        );

        // Second cycle: both group members should merge together.
        process_next(&state).await.unwrap();

        let g1_status = state
            .merge_requests
            .find_by_id(&Id::new("mr-g1"))
            .await
            .unwrap()
            .unwrap()
            .status;
        let g2_status = state
            .merge_requests
            .find_by_id(&Id::new("mr-g2"))
            .await
            .unwrap()
            .unwrap()
            .status;

        assert_eq!(g1_status, MrStatus::Merged, "mr-g1 should be merged");
        assert_eq!(g2_status, MrStatus::Merged, "mr-g2 should be merged");
    }

    /// Test: Rollback on failure — all already-merged members are rolled back
    /// and the entire group is requeued.
    #[tokio::test]
    async fn atomic_group_rollback_requeues_all_members() {
        let state = test_state();

        let repo = create_repo_in_workspace(&state, "test-repo", "ws-1").await;

        // Create two MRs in atomic group "bundle".
        let mr_first = create_mr_in_group(
            &state,
            "mr-first",
            &repo.id,
            "ws-1",
            "bundle",
            "feat/first",
            Some("agent-1"),
        )
        .await;
        let mr_second = create_mr_in_group(
            &state,
            "mr-second",
            &repo.id,
            "ws-1",
            "bundle",
            "feat/second",
            Some("agent-2"),
        )
        .await;

        // Enqueue both.
        let e_first = enqueue_mr(&state, "mr-first", 50, 1000).await;
        let e_second = enqueue_mr(&state, "mr-second", 50, 1001).await;

        // Subscribe to broadcast channel.
        let mut rx = state.message_broadcast_tx.subscribe();

        // Simulate: mr-first was already merged, mr-second failed.
        let merged_entries = vec![(e_first.clone(), mr_first.clone())];
        let all_group_entries = vec![e_first.clone(), e_second.clone()];

        rollback_atomic_group(
            &state,
            "bundle",
            &repo,
            "main",
            Some("pre_group_sha_abc123"),
            &merged_entries,
            &all_group_entries,
            "conflict in MR mr-second: file.rs",
            &mr_second.id,
        )
        .await
        .unwrap();

        // Both entries should be requeued (status back to Queued).
        let entry_first = state
            .merge_queue
            .find_by_id(&e_first.id)
            .await
            .unwrap()
            .expect("mr-first entry should exist");
        assert_eq!(
            entry_first.status,
            MergeQueueEntryStatus::Queued,
            "mr-first should be requeued after rollback"
        );

        let entry_second = state
            .merge_queue
            .find_by_id(&e_second.id)
            .await
            .unwrap()
            .expect("mr-second entry should exist");
        assert_eq!(
            entry_second.status,
            MergeQueueEntryStatus::Queued,
            "mr-second should be requeued after rollback"
        );

        // AtomicGroupFailed event should be emitted.
        let msg = rx
            .try_recv()
            .expect("AtomicGroupFailed event should be emitted");
        assert_eq!(msg.kind, gyre_common::MessageKind::AtomicGroupFailed);
        let payload = msg.payload.as_ref().expect("event should have payload");
        assert_eq!(payload["group"], "bundle");
        assert_eq!(payload["failing_mr_id"], mr_second.id.to_string());
        assert_eq!(payload["member_count"], 2);
        assert_eq!(payload["rolled_back_count"], 1);
    }

    /// Test: All distinct authors receive AtomicGroupFailure notifications
    /// when a group merge fails.
    #[tokio::test]
    async fn atomic_group_failure_notifies_all_distinct_authors() {
        let state = test_state();

        let repo = create_repo_in_workspace(&state, "test-repo", "ws-1").await;

        // Create two MRs with DIFFERENT author_agent_ids.
        let mr_first = create_mr_in_group(
            &state,
            "mr-first",
            &repo.id,
            "ws-1",
            "bundle",
            "feat/first",
            Some("author-A"),
        )
        .await;
        let mr_second = create_mr_in_group(
            &state,
            "mr-second",
            &repo.id,
            "ws-1",
            "bundle",
            "feat/second",
            Some("author-B"),
        )
        .await;

        // Add a workspace member (human user who should also be notified).
        let membership = gyre_domain::WorkspaceMembership::new(
            Id::new("member-1"),
            Id::new("user-1"),
            Id::new("ws-1"),
            gyre_domain::WorkspaceRole::Developer,
            Id::new("admin"),
            1000,
        );
        state
            .workspace_memberships
            .create(&membership)
            .await
            .unwrap();

        let e_first = enqueue_mr(&state, "mr-first", 50, 1000).await;
        let e_second = enqueue_mr(&state, "mr-second", 50, 1001).await;

        // Subscribe to broadcast.
        let mut rx = state.message_broadcast_tx.subscribe();

        // Rollback with failure.
        let merged_entries = vec![(e_first.clone(), mr_first.clone())];
        let all_group_entries = vec![e_first.clone(), e_second.clone()];

        rollback_atomic_group(
            &state,
            "bundle",
            &repo,
            "main",
            Some("pre_group_sha_abc123"),
            &merged_entries,
            &all_group_entries,
            "conflict in MR mr-second",
            &mr_second.id,
        )
        .await
        .unwrap();

        // Check notifications for author-A.
        let notifs_a = state
            .notifications
            .list_for_user(
                &Id::new("author-A"),
                Some(&Id::new("ws-1")),
                None,
                None,
                None,
                None,
                100,
                0,
            )
            .await
            .unwrap();
        assert!(
            !notifs_a.is_empty(),
            "author-A should receive a notification"
        );
        assert_eq!(
            notifs_a[0].notification_type,
            gyre_common::NotificationType::AtomicGroupFailure
        );
        assert_eq!(
            notifs_a[0].priority,
            gyre_common::NotificationType::AtomicGroupFailure.default_priority(),
            "priority should match AtomicGroupFailure default"
        );
        assert!(notifs_a[0].title.contains("bundle"));
        assert!(notifs_a[0].title.contains("rolled back"));

        // Verify notification body.
        let body: serde_json::Value = serde_json::from_str(
            notifs_a[0]
                .body
                .as_ref()
                .expect("notification should have body"),
        )
        .expect("body should be valid JSON");
        assert_eq!(body["group"], "bundle");
        assert_eq!(body["failing_mr_id"], mr_second.id.to_string());
        assert_eq!(body["member_count"], 2);
        assert_eq!(body["rolled_back_count"], 1);

        // Verify entity_ref.
        assert_eq!(
            notifs_a[0].source.entity_id.as_str(),
            mr_second.id.as_str(),
            "source entity should be the failing MR id"
        );

        // Check notifications for author-B.
        let notifs_b = state
            .notifications
            .list_for_user(
                &Id::new("author-B"),
                Some(&Id::new("ws-1")),
                None,
                None,
                None,
                None,
                100,
                0,
            )
            .await
            .unwrap();
        assert!(
            !notifs_b.is_empty(),
            "author-B should receive a notification"
        );
        assert_eq!(
            notifs_b[0].notification_type,
            gyre_common::NotificationType::AtomicGroupFailure
        );

        // Check that workspace member user-1 does NOT receive a notification
        // (spec says "notify all authors" — no broader audience).
        let notifs_user = state
            .notifications
            .list_for_user(
                &Id::new("user-1"),
                Some(&Id::new("ws-1")),
                None,
                None,
                None,
                None,
                100,
                0,
            )
            .await
            .unwrap();
        assert!(
            notifs_user.is_empty(),
            "workspace member (non-author) should NOT receive a notification"
        );

        // Verify event.
        let msg = rx
            .try_recv()
            .expect("AtomicGroupFailed event should be emitted");
        assert_eq!(msg.kind, gyre_common::MessageKind::AtomicGroupFailed);
    }

    /// Test: Atomic group implies dependency ordering among members.
    /// Members enqueued in order automatically have dependency edges.
    #[tokio::test]
    async fn atomic_group_implies_dependency_ordering() {
        let state = test_state();

        let repo = create_repo_in_workspace(&state, "test-repo", "ws-1").await;

        // Create three MRs in the same atomic group.
        create_mr_in_group(&state, "mr-a", &repo.id, "ws-1", "deploy", "feat/a", None).await;
        create_mr_in_group(&state, "mr-b", &repo.id, "ws-1", "deploy", "feat/b", None).await;
        create_mr_in_group(&state, "mr-c", &repo.id, "ws-1", "deploy", "feat/c", None).await;

        // Enqueue in order: a, b, c. Atomic group implies a <- b <- c.
        let e_a = enqueue_mr(&state, "mr-a", 50, 1000).await;
        let e_b = enqueue_mr(&state, "mr-b", 50, 1001).await;
        let e_c = enqueue_mr(&state, "mr-c", 50, 1002).await;

        let entries = vec![e_c.clone(), e_b.clone(), e_a.clone()]; // shuffled
        let graph = build_queue_dependency_graph(&state, &entries)
            .await
            .unwrap();
        let sorted = topological_sort_with_priority(&entries, &graph);

        // Dependency ordering should force a, b, c order.
        assert_eq!(sorted.len(), 3);
        assert_eq!(
            sorted[0].merge_request_id.as_str(),
            "mr-a",
            "first-enqueued group member should be first"
        );
        assert_eq!(
            sorted[1].merge_request_id.as_str(),
            "mr-b",
            "second-enqueued group member should be second"
        );
        assert_eq!(
            sorted[2].merge_request_id.as_str(),
            "mr-c",
            "third-enqueued group member should be third"
        );

        // Run a single merge cycle — all three should merge together.
        // event-emission:ok — this test verifies atomic group ordering behavior,
        // not merge-time event emission which is covered by other tests.
        process_next(&state).await.unwrap();

        let status_a = state
            .merge_requests
            .find_by_id(&Id::new("mr-a"))
            .await
            .unwrap()
            .unwrap()
            .status;
        let status_b = state
            .merge_requests
            .find_by_id(&Id::new("mr-b"))
            .await
            .unwrap()
            .unwrap()
            .status;
        let status_c = state
            .merge_requests
            .find_by_id(&Id::new("mr-c"))
            .await
            .unwrap()
            .unwrap()
            .status;

        assert_eq!(status_a, MrStatus::Merged, "mr-a should be merged");
        assert_eq!(status_b, MrStatus::Merged, "mr-b should be merged");
        assert_eq!(status_c, MrStatus::Merged, "mr-c should be merged");
    }

    /// Test: Rollback with no pre-group SHA (edge case — e.g., target branch
    /// doesn't exist yet). Should still requeue and notify.
    #[tokio::test]
    async fn atomic_group_rollback_without_pre_group_sha() {
        let state = test_state();

        let repo = create_repo_in_workspace(&state, "test-repo", "ws-1").await;

        let mr_first = create_mr_in_group(
            &state,
            "mr-first",
            &repo.id,
            "ws-1",
            "bundle",
            "feat/first",
            Some("agent-1"),
        )
        .await;

        let e_first = enqueue_mr(&state, "mr-first", 50, 1000).await;

        let mut rx = state.message_broadcast_tx.subscribe();

        // Rollback with no pre-group SHA.
        let merged_entries: Vec<(MergeQueueEntry, MergeRequest)> = vec![];
        let all_group_entries = vec![e_first.clone()];

        rollback_atomic_group(
            &state,
            "bundle",
            &repo,
            "main",
            None, // no pre-group SHA
            &merged_entries,
            &all_group_entries,
            "MR not found",
            &mr_first.id,
        )
        .await
        .unwrap();

        // Entry should still be requeued.
        let entry = state
            .merge_queue
            .find_by_id(&e_first.id)
            .await
            .unwrap()
            .expect("entry should exist");
        assert_eq!(
            entry.status,
            MergeQueueEntryStatus::Queued,
            "entry should be requeued even without pre-group SHA"
        );

        // Event should still be emitted.
        let msg = rx
            .try_recv()
            .expect("AtomicGroupFailed event should be emitted");
        assert_eq!(msg.kind, gyre_common::MessageKind::AtomicGroupFailed);
    }

    /// HSI §3a trace lifecycle: when the merge processor merges an MR, the
    /// stored gate trace is promoted to attestation — observable because a
    /// subsequent `delete_by_mr` no longer removes it (permanent).
    #[tokio::test]
    async fn merging_mr_promotes_gate_trace_to_attestation() {
        let state = test_state();
        let repo = create_repo_in_workspace(&state, "promote-repo", "ws-1").await;

        let mut mr = gyre_domain::MergeRequest::new(
            Id::new("mr-promote"),
            repo.id.clone(),
            "MR promote",
            "feat/promote",
            "main",
            1000,
        );
        mr.workspace_id = Id::new("ws-1");
        mr.status = gyre_domain::MrStatus::Approved;
        state.merge_requests.create(&mr).await.unwrap();
        enqueue_mr(&state, "mr-promote", 50, 1000).await;

        // Pre-store a gate trace for the MR (as a TraceCapture gate would).
        let trace = gyre_common::GateTrace {
            id: Id::new("trace-promote"),
            mr_id: mr.id.clone(),
            gate_run_id: Id::new("gr-promote"),
            commit_sha: "0123456789abcdef0123456789abcdef01234567".to_string(),
            spans: vec![],
            captured_at: 1000,
        };
        state.traces.store(&trace).await.unwrap();
        assert!(state.traces.get_by_mr(&mr.id).await.unwrap().is_some());

        // Run a merge-processor cycle: no gates → MR merges.
        // event-emission:ok — this test verifies trace promotion, not events.
        // notification-coverage:ok — MR has no author agents, so no user is
        // notifiable; this test verifies trace promotion, not notifications.
        process_next(&state).await.unwrap();

        let updated = state
            .merge_requests
            .find_by_id(&mr.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, gyre_domain::MrStatus::Merged);

        // Promotion observable: delete_by_mr is now a no-op (permanent trace).
        state.traces.delete_by_mr(&mr.id).await.unwrap();
        assert!(
            state.traces.get_by_mr(&mr.id).await.unwrap().is_some(),
            "merged MR's trace must survive delete_by_mr (promoted to attestation)"
        );
    }

    // --- TASK-061: Integration tests for evaluate_attestation_abac ---

    mod attestation_abac_tests {
        use super::*;
        use crate::mem::test_state;
        use gyre_common::{
            Attestation, AttestationInput, AttestationMetadata, AttestationOutput, DerivedInput,
            GateAttestation, GateConstraint, Id, InputContent, KeyBinding, OutputConstraint,
            PersonaRef, ScopeConstraint, SignedInput,
        };
        use gyre_domain::policy::{
            Condition, ConditionOp, ConditionValue, Policy, PolicyEffect, PolicyScope,
        };
        use gyre_domain::{MergeQueueEntry, MergeRequest, Workspace};

        fn sample_key_binding(identity: &str) -> KeyBinding {
            KeyBinding {
                public_key: vec![1, 2, 3, 4],
                user_identity: identity.to_string(),
                issuer: "https://keycloak.example.com".to_string(),
                trust_anchor_id: "tenant-keycloak".to_string(),
                issued_at: 1_700_000_000,
                expires_at: 1_700_003_600,
                user_signature: vec![10, 20, 30, 40],
                platform_countersign: vec![50, 60, 70, 80],
            }
        }

        fn sample_output_constraint(name: &str) -> OutputConstraint {
            OutputConstraint {
                name: name.to_string(),
                expression: format!("output.check(\"{name}\")"),
            }
        }

        fn root_attestation(identity: &str) -> Attestation {
            Attestation {
                id: "sha256:root-att".to_string(),
                input: AttestationInput::Signed(SignedInput {
                    content: InputContent {
                        spec_path: "specs/system/payments.md".to_string(),
                        spec_sha: "abc123".to_string(),
                        workspace_id: "ws-abac".to_string(),
                        repo_id: "repo-abac".to_string(),
                        persona_constraints: vec![PersonaRef {
                            name: "security".to_string(),
                        }],
                        meta_spec_set_sha: "def456".to_string(),
                        scope: ScopeConstraint {
                            allowed_paths: vec!["src/**".to_string()],
                            forbidden_paths: vec![],
                        },
                    },
                    output_constraints: vec![sample_output_constraint("scope-check")],
                    valid_until: 1_700_000_000,
                    expected_generation: Some(1),
                    signature: vec![10, 20, 30],
                    key_binding: sample_key_binding(identity),
                }),
                output: AttestationOutput {
                    content_hash: vec![1, 2, 3],
                    commit_sha: "aaa111".to_string(),
                    agent_signature: None,
                    gate_results: vec![GateAttestation {
                        gate_id: "gate-1".to_string(),
                        gate_name: "unit-tests".to_string(),
                        gate_type: gyre_common::gate::GateType::TestCommand,
                        status: gyre_common::gate::GateStatus::Passed,
                        output_hash: vec![80, 90],
                        constraint: Some(GateConstraint {
                            gate_id: "gate-1".to_string(),
                            gate_name: "unit-tests".to_string(),
                            constraint: sample_output_constraint("gate-constraint"),
                            signed_by: vec![50, 60, 70],
                        }),
                        signature: vec![11, 22, 33],
                        key_binding: sample_key_binding("gate-agent"),
                    }],
                },
                metadata: AttestationMetadata {
                    created_at: 1_700_000_000,
                    workspace_id: "ws-abac".to_string(),
                    repo_id: "repo-abac".to_string(),
                    task_id: "TASK-ABAC".to_string(),
                    agent_id: "agent:worker-abac".to_string(),
                    chain_depth: 0,
                },
            }
        }

        fn derived_attestation(depth: u32) -> Attestation {
            Attestation {
                id: format!("sha256:derived-{depth}"),
                input: AttestationInput::Derived(DerivedInput {
                    parent_ref: vec![99, 88],
                    preconditions: vec![],
                    update: "narrow_scope".to_string(),
                    output_constraints: vec![
                        sample_output_constraint("derived-c1"),
                        sample_output_constraint("derived-c2"),
                    ],
                    signature: vec![11],
                    key_binding: sample_key_binding("orchestrator"),
                }),
                output: AttestationOutput {
                    content_hash: vec![4, 5, 6],
                    commit_sha: format!("bbb{depth}"),
                    agent_signature: None,
                    gate_results: vec![],
                },
                metadata: AttestationMetadata {
                    created_at: 1_700_000_100,
                    workspace_id: "ws-abac".to_string(),
                    repo_id: "repo-abac".to_string(),
                    task_id: "TASK-ABAC".to_string(),
                    agent_id: "agent:worker-abac".to_string(),
                    chain_depth: depth,
                },
            }
        }

        /// Integration test: evaluate_attestation_abac populates chain_depth,
        /// root_signer, constraint_count, and tenant_id from a real attestation
        /// chain — then a policy conditioned on chain_depth > 5 fires correctly.
        #[tokio::test]
        async fn evaluate_attestation_abac_populates_all_attributes_from_chain() {
            let state = test_state();

            // Create workspace with a specific tenant_id so we can verify tenant propagation.
            let ws = Workspace::new(
                Id::new("ws-abac"),
                Id::new("tenant-abac-test"),
                "ws-abac",
                "ws-abac",
                0,
            );
            state.workspaces.create(&ws).await.unwrap();

            // Create repo in that workspace.
            let repo = gyre_domain::Repository::new(
                Id::new("repo-abac"),
                Id::new("ws-abac"),
                "abac-repo",
                "/tmp/abac-repo.git",
                0,
            );
            state.repos.create(&repo).await.unwrap();

            // Build a chain: root (depth=0) + derived (depth=1) + derived (depth=2).
            // Root has 1 output_constraint + 1 gate_constraint = 2
            // Derived(1) has 2 output_constraints + 0 gate = 2
            // Derived(2) has 2 output_constraints + 0 gate = 2
            // Total constraint_count = 6
            let chain = vec![
                root_attestation("user:test-signer"),
                derived_attestation(1),
                derived_attestation(2),
            ];
            let leaf = &chain[2]; // depth=2

            // Create MR and queue entry (required by function signature).
            let mut mr = MergeRequest::new(
                Id::new("mr-abac"),
                repo.id.clone(),
                "ABAC test MR",
                "feat/abac",
                "main",
                1000,
            );
            mr.author_agent_id = Some(Id::new("agent:worker-abac"));
            mr.workspace_id = Id::new("ws-abac");

            let entry = MergeQueueEntry::new(Id::new("entry-abac"), mr.id.clone(), 50, 1000);

            // Seed a deny policy: chain_depth > 1 → deny push to attestation resources.
            let deny_policy = Policy {
                id: Id::new("deny-deep-chain"),
                name: "deny-deep-chain".to_string(),
                description: "Deny pushes with chain_depth > 1".to_string(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 100,
                effect: PolicyEffect::Deny,
                conditions: vec![Condition {
                    attribute: "subject.chain_depth".to_string(),
                    operator: ConditionOp::GreaterThan,
                    value: ConditionValue::Number(1),
                }],
                actions: vec!["merge".to_string()],
                resource_types: vec!["attestation".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("test"),
                created_at: 0,
                updated_at: 0,
            };
            state.policies.create(&deny_policy).await.unwrap();

            // Also seed a blanket allow so shallow chains would pass.
            let allow_policy = Policy {
                id: Id::new("allow-attestation"),
                name: "allow-attestation".to_string(),
                description: "Allow attestation merges".to_string(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 10,
                effect: PolicyEffect::Allow,
                conditions: vec![],
                actions: vec!["merge".to_string()],
                resource_types: vec!["attestation".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("test"),
                created_at: 0,
                updated_at: 0,
            };
            state.policies.create(&allow_policy).await.unwrap();

            // Call the production function with the real chain data.
            let result =
                evaluate_attestation_abac(&state, &chain, leaf, &mr, &entry, &repo, "merge").await;

            // chain_depth=2 > 1 → deny policy should match.
            assert_eq!(result.effect, PolicyEffect::Deny);
            assert_eq!(result.matched_policy, Some("deny-deep-chain".to_string()),);
        }

        /// Integration test: shallow chain (depth=0) is allowed when deny policy
        /// only fires for chain_depth > 5 — verifies attributes are correct for
        /// the allow path.
        #[tokio::test]
        async fn evaluate_attestation_abac_shallow_chain_allowed() {
            let state = test_state();

            let ws = Workspace::new(
                Id::new("ws-shallow"),
                Id::new("tenant-shallow"),
                "ws-shallow",
                "ws-shallow",
                0,
            );
            state.workspaces.create(&ws).await.unwrap();

            let repo = gyre_domain::Repository::new(
                Id::new("repo-shallow"),
                Id::new("ws-shallow"),
                "shallow-repo",
                "/tmp/shallow-repo.git",
                0,
            );
            state.repos.create(&repo).await.unwrap();

            // Single-node chain: root only (depth=0).
            let chain = vec![root_attestation("user:shallow-signer")];
            let leaf = &chain[0]; // depth=0

            let mut mr = MergeRequest::new(
                Id::new("mr-shallow"),
                repo.id.clone(),
                "Shallow MR",
                "feat/shallow",
                "main",
                1000,
            );
            mr.author_agent_id = Some(Id::new("agent:worker-shallow"));
            mr.workspace_id = Id::new("ws-shallow");

            let entry = MergeQueueEntry::new(Id::new("entry-shallow"), mr.id.clone(), 50, 1000);

            // Deny policy for deep chains only (chain_depth > 5).
            let deny_policy = Policy {
                id: Id::new("deny-deep-5"),
                name: "deny-deep-5".to_string(),
                description: "Deny deep chains".to_string(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 100,
                effect: PolicyEffect::Deny,
                conditions: vec![Condition {
                    attribute: "subject.chain_depth".to_string(),
                    operator: ConditionOp::GreaterThan,
                    value: ConditionValue::Number(5),
                }],
                actions: vec!["merge".to_string()],
                resource_types: vec!["attestation".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("test"),
                created_at: 0,
                updated_at: 0,
            };
            state.policies.create(&deny_policy).await.unwrap();

            let allow_policy = Policy {
                id: Id::new("allow-shallow"),
                name: "allow-shallow".to_string(),
                description: "Allow attestation merges".to_string(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 10,
                effect: PolicyEffect::Allow,
                conditions: vec![],
                actions: vec!["merge".to_string()],
                resource_types: vec!["attestation".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("test"),
                created_at: 0,
                updated_at: 0,
            };
            state.policies.create(&allow_policy).await.unwrap();

            let result =
                evaluate_attestation_abac(&state, &chain, leaf, &mr, &entry, &repo, "merge").await;

            // chain_depth=0 ≤ 5 → deny doesn't fire, allow matches.
            assert_eq!(result.effect, PolicyEffect::Allow);
            assert_eq!(result.matched_policy, Some("allow-shallow".to_string()),);
        }

        /// Integration test: root_signer condition is populated from the attestation
        /// chain and correctly matched by policies.
        #[tokio::test]
        async fn evaluate_attestation_abac_root_signer_policy_match() {
            let state = test_state();

            let ws = Workspace::new(
                Id::new("ws-signer"),
                Id::new("tenant-signer"),
                "ws-signer",
                "ws-signer",
                0,
            );
            state.workspaces.create(&ws).await.unwrap();

            let repo = gyre_domain::Repository::new(
                Id::new("repo-signer"),
                Id::new("ws-signer"),
                "signer-repo",
                "/tmp/signer-repo.git",
                0,
            );
            state.repos.create(&repo).await.unwrap();

            let chain = vec![
                root_attestation("user:trusted-principal"),
                derived_attestation(1),
            ];
            let leaf = &chain[1];

            let mut mr = MergeRequest::new(
                Id::new("mr-signer"),
                repo.id.clone(),
                "Signer MR",
                "feat/signer",
                "main",
                1000,
            );
            mr.author_agent_id = Some(Id::new("agent:signer-worker"));
            mr.workspace_id = Id::new("ws-signer");

            let entry = MergeQueueEntry::new(Id::new("entry-signer"), mr.id.clone(), 50, 1000);

            // Allow only when root_signer == "user:trusted-principal".
            let allow_trusted = Policy {
                id: Id::new("allow-trusted-signer"),
                name: "allow-trusted-signer".to_string(),
                description: "Allow only trusted signer".to_string(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 50,
                effect: PolicyEffect::Allow,
                conditions: vec![Condition {
                    attribute: "subject.root_signer".to_string(),
                    operator: ConditionOp::Equals,
                    value: ConditionValue::String("user:trusted-principal".to_string()),
                }],
                actions: vec!["merge".to_string()],
                resource_types: vec!["attestation".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("test"),
                created_at: 0,
                updated_at: 0,
            };
            state.policies.create(&allow_trusted).await.unwrap();

            let result =
                evaluate_attestation_abac(&state, &chain, leaf, &mr, &entry, &repo, "merge").await;

            // root_signer "user:trusted-principal" matches the policy condition.
            assert_eq!(result.effect, PolicyEffect::Allow);
            assert_eq!(
                result.matched_policy,
                Some("allow-trusted-signer".to_string()),
            );
        }

        /// Integration test: constraint_count is correctly populated and a policy
        /// conditioned on constraint_count < 10 matches for a small chain.
        #[tokio::test]
        async fn evaluate_attestation_abac_constraint_count_match() {
            let state = test_state();

            let ws = Workspace::new(Id::new("ws-cc"), Id::new("tenant-cc"), "ws-cc", "ws-cc", 0);
            state.workspaces.create(&ws).await.unwrap();

            let repo = gyre_domain::Repository::new(
                Id::new("repo-cc"),
                Id::new("ws-cc"),
                "cc-repo",
                "/tmp/cc-repo.git",
                0,
            );
            state.repos.create(&repo).await.unwrap();

            // Chain: root (1 output + 1 gate = 2) + derived (2 output + 0 gate = 2)
            // Total constraint_count = 4
            let chain = vec![root_attestation("user:cc-signer"), derived_attestation(1)];
            let leaf = &chain[1];

            let mut mr = MergeRequest::new(
                Id::new("mr-cc"),
                repo.id.clone(),
                "CC MR",
                "feat/cc",
                "main",
                1000,
            );
            mr.author_agent_id = Some(Id::new("agent:cc-worker"));
            mr.workspace_id = Id::new("ws-cc");

            let entry = MergeQueueEntry::new(Id::new("entry-cc"), mr.id.clone(), 50, 1000);

            // Allow when constraint_count < 10.
            let allow_low_cc = Policy {
                id: Id::new("allow-low-cc"),
                name: "allow-low-cc".to_string(),
                description: "Allow when few constraints".to_string(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 50,
                effect: PolicyEffect::Allow,
                conditions: vec![Condition {
                    attribute: "subject.constraint_count".to_string(),
                    operator: ConditionOp::LessThan,
                    value: ConditionValue::Number(10),
                }],
                actions: vec!["merge".to_string()],
                resource_types: vec!["attestation".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("test"),
                created_at: 0,
                updated_at: 0,
            };
            state.policies.create(&allow_low_cc).await.unwrap();

            let result =
                evaluate_attestation_abac(&state, &chain, leaf, &mr, &entry, &repo, "merge").await;

            // constraint_count=4 < 10 → allow.
            assert_eq!(result.effect, PolicyEffect::Allow);
            assert_eq!(result.matched_policy, Some("allow-low-cc".to_string()),);
        }

        /// Integration test: tenant_id is resolved from workspace and set in the
        /// ABAC context — verified via a policy that conditions on subject.tenant_id.
        #[tokio::test]
        async fn evaluate_attestation_abac_sets_tenant_id_from_workspace() {
            let state = test_state();

            let ws = Workspace::new(
                Id::new("ws-tenant"),
                Id::new("tenant-specific-id"),
                "ws-tenant",
                "ws-tenant",
                0,
            );
            state.workspaces.create(&ws).await.unwrap();

            let repo = gyre_domain::Repository::new(
                Id::new("repo-tenant"),
                Id::new("ws-tenant"),
                "tenant-repo",
                "/tmp/tenant-repo.git",
                0,
            );
            state.repos.create(&repo).await.unwrap();

            let chain = vec![root_attestation("user:tenant-signer")];
            let leaf = &chain[0];

            let mut mr = MergeRequest::new(
                Id::new("mr-tenant"),
                repo.id.clone(),
                "Tenant MR",
                "feat/tenant",
                "main",
                1000,
            );
            mr.author_agent_id = Some(Id::new("agent:tenant-worker"));
            mr.workspace_id = Id::new("ws-tenant");

            let entry = MergeQueueEntry::new(Id::new("entry-tenant"), mr.id.clone(), 50, 1000);

            // Allow only when tenant_id == "tenant-specific-id".
            let allow_tenant = Policy {
                id: Id::new("allow-tenant"),
                name: "allow-tenant".to_string(),
                description: "Allow only specific tenant".to_string(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 50,
                effect: PolicyEffect::Allow,
                conditions: vec![Condition {
                    attribute: "subject.tenant_id".to_string(),
                    operator: ConditionOp::Equals,
                    value: ConditionValue::String("tenant-specific-id".to_string()),
                }],
                actions: vec!["merge".to_string()],
                resource_types: vec!["attestation".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("test"),
                created_at: 0,
                updated_at: 0,
            };
            state.policies.create(&allow_tenant).await.unwrap();

            let result =
                evaluate_attestation_abac(&state, &chain, leaf, &mr, &entry, &repo, "merge").await;

            // tenant_id resolved from workspace → "tenant-specific-id" → matches.
            assert_eq!(result.effect, PolicyEffect::Allow);
            assert_eq!(result.matched_policy, Some("allow-tenant".to_string()),);
        }
    }

    // ── TASK-095: post-merge validation + recovery protocol ─────────────

    use gyre_common::message::MessageKind;
    use gyre_common::NotificationType;
    use gyre_domain::{GatePhase, MrStatus};

    /// Create a post-merge TestCommand gate for a repo.
    async fn create_post_merge_gate(state: &AppState, repo_id: &Id, command: &str, required: bool) {
        let gate = gyre_domain::QualityGate {
            id: Id::new("gate-pm-1"),
            repo_id: repo_id.clone(),
            name: "post-merge-tests".to_string(),
            gate_type: gyre_domain::GateType::TestCommand,
            command: Some(command.to_string()),
            required_approvals: None,
            persona: None,
            validation_type: None,
            required,
            gate_phase: GatePhase::PostMerge,
            timeout_secs: Some(30),
            position: 0,
            created_at: 1000,
        };
        state.quality_gates.save(&gate).await.unwrap();
    }

    /// Shared setup for recovery tests: repo, author agent (spawned by a
    /// human), MR, queue entry, and a passing pre-merge gate result so the
    /// MR is mergeable.
    async fn setup_recovery_mr(state: &AppState) -> (Repository, gyre_domain::MergeRequest) {
        // Seed the workspace record: the MR-reverted notification resolves
        // the tenant from it (never fabricating a "default" tenant).
        let ws = gyre_domain::Workspace::new(
            Id::new("ws-1"),
            Id::new("tenant-recov"),
            "ws-1",
            "ws-1",
            0,
        );
        state.workspaces.create(&ws).await.unwrap();
        let repo = create_repo_in_workspace(state, "recovery-repo", "ws-1").await;
        let mut agent = gyre_domain::Agent::new(Id::new("agent-recov"), "agent-recov", 1000);
        agent.spawned_by = Some("user-recov".to_string());
        agent.workspace_id = Id::new("ws-1");
        state.agents.create(&agent).await.unwrap();

        let mut mr = gyre_domain::MergeRequest::new(
            Id::new("mr-recov"),
            repo.id.clone(),
            "MR recovery test",
            "feat/recov",
            "main",
            1000,
        );
        mr.workspace_id = Id::new("ws-1");
        mr.author_agent_id = Some(Id::new("agent-recov"));
        state.merge_requests.create(&mr).await.unwrap();

        enqueue_mr(state, "mr-recov", 100, 1000).await;

        (repo, mr)
    }

    #[tokio::test]
    async fn post_merge_pass_does_not_pause_queue() {
        let state = test_state();
        let (repo, mr) = setup_recovery_mr(&state).await;

        // `true` always exits 0 → post-merge validation passes.
        create_post_merge_gate(&state, &repo.id, "true", true).await;

        let mut rx = state.message_broadcast_tx.subscribe();
        process_next(&state).await.unwrap();

        // MR merged, no pause recorded, no MergeQueuePaused event.
        let updated = state
            .merge_requests
            .find_by_id(&mr.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, MrStatus::Merged);
        assert!(!merge_queue_paused(&state, &repo.id).await);
        assert!(rx.try_recv().is_err(), "no events expected on pass");

        // Merge-success notification was delivered to the author's spawner.
        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user-recov"),
                Some(&Id::new("ws-1")),
                None,
                None,
                None,
                None,
                100,
                0,
            )
            .await
            .unwrap();
        assert!(
            notifs
                .iter()
                .any(|n| n.notification_type == NotificationType::MrMerged),
            "author spawner should receive the merge notification, got {:?}",
            notifs
                .iter()
                .map(|n| &n.notification_type)
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn post_merge_fail_runs_full_recovery_protocol() {
        let state = test_state();
        let (repo, mr) = setup_recovery_mr(&state).await;

        // Workspace Admin — per user-management.md §Who Gets Notified, "MR
        // reverted" fans out to the author AND the workspace Admins.
        let membership = gyre_domain::WorkspaceMembership::new(
            Id::new("member-recovery-admin"),
            Id::new("admin-recov"),
            Id::new("ws-1"),
            gyre_domain::WorkspaceRole::Admin,
            Id::new("admin"),
            1000,
        );
        state
            .workspace_memberships
            .create(&membership)
            .await
            .unwrap();

        // `false` always exits non-zero → required post-merge gate fails,
        // and re-running on the reverted HEAD fails again → escalation path.
        create_post_merge_gate(&state, &repo.id, "false", true).await;

        // Pre-existing gate result to verify invalidation.
        let gr = gyre_domain::GateResult {
            id: Id::new("gr-recov"),
            gate_id: Id::new("gate-pm-1"),
            mr_id: mr.id.clone(),
            status: GateStatus::Passed,
            output: None,
            started_at: None,
            finished_at: None,
        };
        state.gate_results.save(&gr).await.unwrap();

        let mut rx = state.message_broadcast_tx.subscribe();
        process_next(&state).await.unwrap();

        // 1. Queue is paused with the failure reason.
        let paused = merge_queue_paused(&state, &repo.id).await;
        assert!(
            paused,
            "merge queue should be paused after post-merge failure"
        );
        let raw = state
            .kv_store
            .kv_get("merge_queue_pause", repo.id.as_str())
            .await
            .unwrap()
            .expect("pause state should be persisted");
        let pause_json: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(pause_json["paused"], true);
        assert!(
            pause_json["reason"]
                .as_str()
                .unwrap()
                .contains("post-merge-tests"),
            "pause reason should name the failed gate: {}",
            pause_json["reason"]
        );

        // 2. Events: MergeQueuePaused (workspace) and MrReverted (author agent).
        let mut saw_paused = false;
        let mut saw_reverted = false;
        while let Ok(msg) = rx.try_recv() {
            match msg.kind {
                MessageKind::MergeQueuePaused => {
                    saw_paused = true;
                    assert_eq!(msg.payload.as_ref().unwrap()["repo_id"], repo.id.as_str());
                }
                MessageKind::MrReverted => {
                    saw_reverted = true;
                    assert_eq!(msg.payload.as_ref().unwrap()["mr_id"], mr.id.as_str());
                }
                _ => {}
            }
        }
        assert!(saw_paused, "MergeQueuePaused event should be emitted");
        assert!(saw_reverted, "MrReverted event should be emitted");

        // 3. MR marked Reverted with reverted_at set.
        let updated = state
            .merge_requests
            .find_by_id(&mr.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, MrStatus::Reverted);
        assert!(updated.reverted_at.is_some());
        assert!(updated.revert_commit_sha.is_some());

        // 4. Author's spawner got a MrReverted notification and (re-run
        //    failed on the noop revert SHA) a MergeQueueEscalation.
        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user-recov"),
                Some(&Id::new("ws-1")),
                None,
                None,
                None,
                None,
                100,
                0,
            )
            .await
            .unwrap();
        let types: Vec<&NotificationType> = notifs.iter().map(|n| &n.notification_type).collect();
        assert!(
            types.contains(&&NotificationType::MrReverted),
            "author spawner should receive MrReverted notification, got {types:?}"
        );
        assert!(
            types.contains(&&NotificationType::MergeQueueEscalation),
            "re-run failure should escalate to human, got {types:?}"
        );

        // 4b. Workspace Admin also got the MrReverted notification
        //     (user-management.md §Who Gets Notified: "MR reverted" →
        //     MR author + workspace Admin).
        let admin_notifs = state
            .notifications
            .list_for_user(
                &Id::new("admin-recov"),
                Some(&Id::new("ws-1")),
                None,
                None,
                None,
                None,
                100,
                0,
            )
            .await
            .unwrap();
        assert!(
            admin_notifs
                .iter()
                .any(|n| n.notification_type == NotificationType::MrReverted),
            "workspace Admin should receive MrReverted notification, got {:?}",
            admin_notifs
                .iter()
                .map(|n| &n.notification_type)
                .collect::<Vec<_>>()
        );

        // 5. Remediation task created with the failure reason.
        let tasks = state.tasks.list_by_repo(&repo.id).await.unwrap();
        let reverted_task = tasks
            .iter()
            .find(|t| t.labels.contains(&"reverted-mr".to_string()))
            .expect("remediation task should exist");
        assert!(reverted_task.title.contains(mr.id.as_str()));
        assert!(reverted_task.title.contains("post-merge-tests"));
        assert_eq!(reverted_task.priority, TaskPriority::High);
        assert_eq!(reverted_task.workspace_id.as_str(), "ws-1");

        // 6. Gate results invalidated → back to Pending.
        let results = state
            .gate_results
            .list_by_mr_id(mr.id.as_str())
            .await
            .unwrap();
        assert!(
            results.iter().all(|r| r.status == GateStatus::Pending),
            "gate results should be invalidated to Pending"
        );

        // 7. Escalation branch: queue stays paused (re-run failed on noop adapter).
        assert!(merge_queue_paused(&state, &repo.id).await);
    }

    /// Task-095 R4-F1: when the recovery revert's inverse patch conflicts
    /// with later changes on the default branch, the branch is untouched —
    /// the protocol must stay paused, escalate to a human, and apply NO
    /// per-MR side effects (the MR is still merged; marking it Reverted
    /// would record a revert that never happened).
    #[tokio::test]
    async fn post_merge_recovery_revert_conflict_stays_paused_without_side_effects() {
        let git = crate::mem::ConfigurableGitOps {
            revert_conflicts: true,
            ..Default::default()
        };
        let revert_calls = git.revert_calls.clone();
        let state = crate::mem::test_state_with_git_ops(Arc::new(git));
        let (repo, mr) = setup_recovery_mr(&state).await;

        // Required post-merge gate always fails → recovery protocol runs.
        create_post_merge_gate(&state, &repo.id, "false", true).await;

        process_next(&state).await.unwrap();

        // The recovery attempted exactly one revert of the merge commit.
        assert_eq!(revert_calls.lock().len(), 1, "one revert attempt");

        // Queue stays paused (never resumed — main is not green).
        assert!(
            merge_queue_paused(&state, &repo.id).await,
            "queue must stay paused when the revert conflicts"
        );

        // No per-MR side effects: the MR remains Merged with no revert
        // recorded — the branch is untouched, so claiming a revert would
        // be a false record.
        let updated = state
            .merge_requests
            .find_by_id(&mr.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, MrStatus::Merged);
        assert!(updated.revert_commit_sha.is_none());
        assert!(updated.reverted_at.is_none());

        // Escalated to the author's spawner (a human), not silently dropped.
        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user-recov"),
                Some(&Id::new("ws-1")),
                None,
                None,
                None,
                None,
                100,
                0,
            )
            .await
            .unwrap();
        assert!(
            notifs
                .iter()
                .any(|n| n.notification_type == gyre_common::NotificationType::MergeQueueEscalation),
            "revert conflict must escalate to a human, got {:?}",
            notifs.iter().map(|n| &n.notification_type).collect::<Vec<_>>()
        );

        // No remediation task — there is no confirmed revert to remediate.
        let tasks = state.tasks.list_by_repo(&repo.id).await.unwrap();
        assert!(
            tasks.is_empty(),
            "conflicting revert must not create remediation tasks"
        );
    }

    #[tokio::test]
    async fn paused_queue_skips_entries_and_resume_processes_next() {
        let state = test_state();
        let (repo, mr) = setup_recovery_mr(&state).await;

        // Pause the queue manually with a reason.
        state
            .kv_store
            .kv_set(
                "merge_queue_pause",
                repo.id.as_str(),
                serde_json::json!({"paused": true, "reason": "manual test pause"}).to_string(),
            )
            .await
            .unwrap();

        let mut rx = state.message_broadcast_tx.subscribe();
        process_next(&state).await.unwrap();

        // Entry stays Queued and the MR stays Open — no merge while paused.
        let entry = state
            .merge_queue
            .find_by_id(&Id::new("entry-mr-recov"))
            .await
            .unwrap()
            .expect("entry should exist");
        assert_eq!(entry.status, MergeQueueEntryStatus::Queued);
        let updated = state
            .merge_requests
            .find_by_id(&mr.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, MrStatus::Open);
        assert!(rx.try_recv().is_err(), "no events expected while paused");

        // Resume → next cycle merges the MR (NoopGitOps succeeds).
        resume_merge_queue(&state, &repo).await;
        process_next(&state).await.unwrap();
        let updated = state
            .merge_requests
            .find_by_id(&mr.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            updated.status,
            MrStatus::Merged,
            "resumed queue should merge the entry"
        );
        assert!(!merge_queue_paused(&state, &repo.id).await);

        // Merge-success notification delivered after resume.
        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user-recov"),
                Some(&Id::new("ws-1")),
                None,
                None,
                None,
                None,
                100,
                0,
            )
            .await
            .unwrap();
        assert!(
            notifs
                .iter()
                .any(|n| n.notification_type == NotificationType::MrMerged),
            "author spawner should receive the merge notification, got {:?}",
            notifs
                .iter()
                .map(|n| &n.notification_type)
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn advisory_post_merge_gate_failure_does_not_pause() {
        let state = test_state();
        let (repo, mr) = setup_recovery_mr(&state).await;

        // Non-required gate fails → advisory only, no pause.
        create_post_merge_gate(&state, &repo.id, "false", false).await;

        let mut rx = state.message_broadcast_tx.subscribe();
        process_next(&state).await.unwrap();

        let updated = state
            .merge_requests
            .find_by_id(&mr.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, MrStatus::Merged);
        assert!(
            !merge_queue_paused(&state, &repo.id).await,
            "advisory failure must not pause"
        );
        assert!(
            rx.try_recv().is_err(),
            "no recovery events for advisory failure"
        );

        // Merge succeeded → merge notification, but no recovery notifications.
        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user-recov"),
                Some(&Id::new("ws-1")),
                None,
                None,
                None,
                None,
                100,
                0,
            )
            .await
            .unwrap();
        assert!(
            notifs
                .iter()
                .any(|n| n.notification_type == NotificationType::MrMerged),
            "author spawner should receive the merge notification, got {:?}",
            notifs
                .iter()
                .map(|n| &n.notification_type)
                .collect::<Vec<_>>()
        );
        assert!(
            !notifs
                .iter()
                .any(|n| n.notification_type == NotificationType::MrReverted
                    || n.notification_type == NotificationType::MergeQueueEscalation),
            "advisory failure must not produce recovery notifications"
        );
    }

    /// Test: a paused merge queue also blocks atomic-group merges — the
    /// spec's "no more merges until main is green" applies to every merge
    /// path. Entries stay Queued and members stay Open.
    #[tokio::test]
    async fn paused_queue_skips_atomic_group() {
        let state = test_state();
        let repo = create_repo_in_workspace(&state, "recovery-repo", "ws-1").await;

        let mut agent = gyre_domain::Agent::new(Id::new("agent-grp"), "agent-grp", 1000);
        agent.spawned_by = Some("user-grp".to_string());
        agent.workspace_id = Id::new("ws-1");
        state.agents.create(&agent).await.unwrap();

        create_mr_in_group(
            &state,
            "mr-grp-a",
            &repo.id,
            "ws-1",
            "bundle",
            "feat/grp-a",
            Some("agent-grp"),
        )
        .await;
        create_mr_in_group(
            &state,
            "mr-grp-b",
            &repo.id,
            "ws-1",
            "bundle",
            "feat/grp-b",
            Some("agent-grp"),
        )
        .await;
        enqueue_mr(&state, "mr-grp-a", 100, 1000).await;
        enqueue_mr(&state, "mr-grp-b", 100, 1001).await;

        // Pause the queue for this repo.
        state
            .kv_store
            .kv_set(
                "merge_queue_pause",
                repo.id.as_str(),
                serde_json::json!({"paused": true, "reason": "manual test pause"}).to_string(),
            )
            .await
            .unwrap();

        let mut rx = state.message_broadcast_tx.subscribe();
        process_next(&state).await.unwrap();

        // Entries stay Queued and MRs stay Open — no group merge while paused.
        for (entry_id, mr_id) in [
            ("entry-mr-grp-a", "mr-grp-a"),
            ("entry-mr-grp-b", "mr-grp-b"),
        ] {
            let entry = state
                .merge_queue
                .find_by_id(&Id::new(entry_id))
                .await
                .unwrap()
                .expect("entry should exist");
            assert_eq!(
                entry.status,
                MergeQueueEntryStatus::Queued,
                "{entry_id} should stay queued while paused"
            );
            let updated = state
                .merge_requests
                .find_by_id(&Id::new(mr_id))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                updated.status,
                MrStatus::Open,
                "{mr_id} should stay open while paused"
            );
        }
        assert!(rx.try_recv().is_err(), "no events expected while paused");
        // Paused queue must not notify anyone either — merge didn't happen.
        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user-grp"),
                Some(&Id::new("ws-1")),
                None,
                None,
                None,
                None,
                100,
                0,
            )
            .await
            .unwrap();
        assert!(
            notifs.is_empty(),
            "no notifications expected while paused, got {:?}",
            notifs
                .iter()
                .map(|n| &n.notification_type)
                .collect::<Vec<_>>()
        );
    }

    /// TASK-095 R4: a group member whose MR became non-mergeable while its
    /// entry sat Queued (closed, reverted, or merged elsewhere — the
    /// selection loop's 4b'' status check validates only the first
    /// candidate it dispatches on) must not be silently merged by the
    /// group path. Its entry is failed permanently, the partial group
    /// merge is rolled back (branch reset, merged members re-Open,
    /// remaining entries requeued), and the healthy member proceeds as a
    /// reduced group on the next cycle.
    #[tokio::test]
    async fn atomic_group_skips_non_mergeable_member_and_rolls_back() {
        let state = test_state();
        let repo = create_repo_in_workspace(&state, "recovery-repo", "ws-1").await;

        let mut agent = gyre_domain::Agent::new(Id::new("agent-r4"), "agent-r4", 1000);
        agent.spawned_by = Some("user-r4".to_string());
        agent.workspace_id = Id::new("ws-1");
        state.agents.create(&agent).await.unwrap();

        create_mr_in_group(
            &state,
            "mr-r4-a",
            &repo.id,
            "ws-1",
            "bundle",
            "feat/r4-a",
            Some("agent-r4"),
        )
        .await;
        create_mr_in_group(
            &state,
            "mr-r4-b",
            &repo.id,
            "ws-1",
            "bundle",
            "feat/r4-b",
            Some("agent-r4"),
        )
        .await;
        enqueue_mr(&state, "mr-r4-a", 100, 1000).await;
        enqueue_mr(&state, "mr-r4-b", 100, 1001).await;

        // Member B is closed after enqueueing — exactly the state
        // `transition_mr_status` (Open→Closed), `repos.rs` close-without-
        // merge, or `specs.rs` auto-close produce without touching the
        // queue.
        let mut mr_b = state
            .merge_requests
            .find_by_id(&Id::new("mr-r4-b"))
            .await
            .unwrap()
            .unwrap();
        mr_b.transition_status(MrStatus::Closed).unwrap();
        state.merge_requests.update(&mr_b).await.unwrap();

        let mut rx = state.message_broadcast_tx.subscribe();
        process_next(&state).await.unwrap();

        // Stale member's entry is terminally Failed, not requeued.
        let entry_b = state
            .merge_queue
            .find_by_id(&Id::new("entry-mr-r4-b"))
            .await
            .unwrap()
            .expect("entry should exist");
        assert_eq!(
            entry_b.status,
            MergeQueueEntryStatus::Failed,
            "stale member entry must be terminally Failed, got {:?} ({:?})",
            entry_b.status,
            entry_b.error_message
        );

        // Neither member merged: A rolled back to Open, B stays Closed.
        for (mr_id, expected) in [("mr-r4-a", MrStatus::Open), ("mr-r4-b", MrStatus::Closed)] {
            let updated = state
                .merge_requests
                .find_by_id(&Id::new(mr_id))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(updated.status, expected, "{mr_id} must not be merged");
        }

        // Healthy member's entry was requeued for a reduced-group retry.
        let entry_a = state
            .merge_queue
            .find_by_id(&Id::new("entry-mr-r4-a"))
            .await
            .unwrap()
            .expect("entry should exist");
        assert_eq!(
            entry_a.status,
            MergeQueueEntryStatus::Queued,
            "healthy member must be requeued after group rollback"
        );

        // The rollback path notified the author (AtomicGroupFailure) and
        // emitted the group-failure event.
        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user-r4"),
                Some(&Id::new("ws-1")),
                None,
                None,
                None,
                None,
                100,
                0,
            )
            .await
            .unwrap();
        assert!(
            notifs
                .iter()
                .any(|n| n.notification_type == NotificationType::AtomicGroupFailure),
            "author should be notified of the group rollback, got {:?}",
            notifs
                .iter()
                .map(|n| &n.notification_type)
                .collect::<Vec<_>>()
        );
        let mut saw_group_fail = false;
        while let Ok(msg) = rx.try_recv() {
            if matches!(msg.kind, MessageKind::AtomicGroupFailed) {
                saw_group_fail = true;
            }
        }
        assert!(saw_group_fail, "AtomicGroupFailed event should be emitted");
    }

    /// Test: post-merge gate failure on an atomic-group merge runs the full
    /// recovery protocol for every member: queue paused, all members marked
    /// Reverted, each author's spawner notified, remediation task per
    /// member, gate results invalidated.
    #[tokio::test]
    async fn atomic_group_post_merge_fail_reverts_all_members() {
        let state = test_state();
        // Seed the workspace record: notify_mr_reverted resolves the tenant
        // from it and skips (never fabricates a "default" tenant) when the
        // workspace is missing.
        let ws =
            gyre_domain::Workspace::new(Id::new("ws-1"), Id::new("tenant-grp"), "ws-1", "ws-1", 0);
        state.workspaces.create(&ws).await.unwrap();
        let repo = create_repo_in_workspace(&state, "recovery-repo", "ws-1").await;

        let mut agent_a = gyre_domain::Agent::new(Id::new("agent-grpa"), "agent-grpa", 1000);
        agent_a.spawned_by = Some("user-grpa".to_string());
        agent_a.workspace_id = Id::new("ws-1");
        state.agents.create(&agent_a).await.unwrap();

        let mut agent_b = gyre_domain::Agent::new(Id::new("agent-grpb"), "agent-grpb", 1000);
        agent_b.spawned_by = Some("user-grpb".to_string());
        agent_b.workspace_id = Id::new("ws-1");
        state.agents.create(&agent_b).await.unwrap();

        create_mr_in_group(
            &state,
            "mr-grpm-a",
            &repo.id,
            "ws-1",
            "bundle",
            "feat/grpm-a",
            Some("agent-grpa"),
        )
        .await;
        create_mr_in_group(
            &state,
            "mr-grpm-b",
            &repo.id,
            "ws-1",
            "bundle",
            "feat/grpm-b",
            Some("agent-grpb"),
        )
        .await;
        enqueue_mr(&state, "mr-grpm-a", 100, 1000).await;
        enqueue_mr(&state, "mr-grpm-b", 100, 1001).await;

        // Required post-merge gate that always fails.
        create_post_merge_gate(&state, &repo.id, "false", true).await;

        let mut rx = state.message_broadcast_tx.subscribe();
        process_next(&state).await.unwrap();

        // 1. Queue is paused.
        assert!(merge_queue_paused(&state, &repo.id).await);

        // 2. Every member marked Reverted with reverted_at set.
        for mr_id in ["mr-grpm-a", "mr-grpm-b"] {
            let updated = state
                .merge_requests
                .find_by_id(&Id::new(mr_id))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                updated.status,
                MrStatus::Reverted,
                "{mr_id} should be Reverted"
            );
            assert!(updated.reverted_at.is_some());
            assert!(updated.revert_commit_sha.is_some());
        }

        // 3. Events: MergeQueuePaused (workspace) + MrReverted per member.
        let mut saw_paused = false;
        let mut reverted_events = 0;
        while let Ok(msg) = rx.try_recv() {
            match msg.kind {
                MessageKind::MergeQueuePaused => saw_paused = true,
                MessageKind::MrReverted => reverted_events += 1,
                _ => {}
            }
        }
        assert!(saw_paused, "MergeQueuePaused event should be emitted");
        assert_eq!(
            reverted_events, 2,
            "each group member should emit a MrReverted event"
        );

        // 4. Both authors' spawners got a MrReverted notification.
        for user in ["user-grpa", "user-grpb"] {
            let notifs = state
                .notifications
                .list_for_user(
                    &Id::new(user),
                    Some(&Id::new("ws-1")),
                    None,
                    None,
                    None,
                    None,
                    100,
                    0,
                )
                .await
                .unwrap();
            assert!(
                notifs
                    .iter()
                    .any(|n| n.notification_type == NotificationType::MrReverted),
                "{user} should receive a MrReverted notification, got {:?}",
                notifs
                    .iter()
                    .map(|n| &n.notification_type)
                    .collect::<Vec<_>>()
            );
        }

        // 5. One remediation task per member, each naming the failure reason.
        let tasks = state.tasks.list_by_repo(&repo.id).await.unwrap();
        let reverted_tasks: Vec<_> = tasks
            .iter()
            .filter(|t| t.labels.contains(&"reverted-mr".to_string()))
            .collect();
        assert_eq!(reverted_tasks.len(), 2, "one remediation task per member");
        for t in &reverted_tasks {
            assert_eq!(t.priority, TaskPriority::High);
            assert!(
                t.title.contains("post-merge-tests"),
                "task title: {}",
                t.title
            );
        }

        // 6. Escalation branch: the noop adapter's revert SHA fails the
        // re-run, so the queue stays paused (escalated to a human).
        assert!(merge_queue_paused(&state, &repo.id).await);
    }

    /// Task-095 R4-F1: when a group member's recovery revert conflicts
    /// with later changes, the group recovery must stop (no partial revert
    /// records), stay paused, and escalate to a human. Members whose
    /// reverts never happened must stay Merged with no revert recorded.
    #[tokio::test]
    async fn atomic_group_recovery_revert_conflict_stops_group_reverts() {
        let git = crate::mem::ConfigurableGitOps {
            revert_conflicts: true,
            ..Default::default()
        };
        let revert_calls = git.revert_calls.clone();
        let state = crate::mem::test_state_with_git_ops(Arc::new(git));
        let repo = create_repo_in_workspace(&state, "recovery-repo", "ws-1").await;

        let mut agent_a = gyre_domain::Agent::new(Id::new("agent-grpa"), "agent-grpa", 1000);
        agent_a.spawned_by = Some("user-grpa".to_string());
        agent_a.workspace_id = Id::new("ws-1");
        state.agents.create(&agent_a).await.unwrap();

        let mut agent_b = gyre_domain::Agent::new(Id::new("agent-grpb"), "agent-grpb", 1000);
        agent_b.spawned_by = Some("user-grpb".to_string());
        agent_b.workspace_id = Id::new("ws-1");
        state.agents.create(&agent_b).await.unwrap();

        create_mr_in_group(
            &state,
            "mr-grpm-a",
            &repo.id,
            "ws-1",
            "bundle",
            "feat/grpm-a",
            Some("agent-grpa"),
        )
        .await;
        create_mr_in_group(
            &state,
            "mr-grpm-b",
            &repo.id,
            "ws-1",
            "bundle",
            "feat/grpm-b",
            Some("agent-grpb"),
        )
        .await;
        enqueue_mr(&state, "mr-grpm-a", 100, 1000).await;
        enqueue_mr(&state, "mr-grpm-b", 100, 1001).await;

        // Required post-merge gate always fails → group recovery runs.
        create_post_merge_gate(&state, &repo.id, "false", true).await;

        process_next(&state).await.unwrap();

        // The group revert loop attempted exactly one revert (the newest
        // member's merge commit) before the Conflict stopped it.
        assert_eq!(
            revert_calls.lock().len(),
            1,
            "group revert loop must stop at the first conflict"
        );

        // Queue stays paused — main was not restored.
        assert!(
            merge_queue_paused(&state, &repo.id).await,
            "queue must stay paused when a group revert conflicts"
        );

        // No member is marked Reverted: the side effects run only after
        // the whole group reverts, and no revert commit exists.
        for mr_id in ["mr-grpm-a", "mr-grpm-b"] {
            let updated = state
                .merge_requests
                .find_by_id(&Id::new(mr_id))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                updated.status,
                MrStatus::Merged,
                "{mr_id} should stay Merged"
            );
            assert!(
                updated.revert_commit_sha.is_none(),
                "{mr_id} must record no revert"
            );
        }

        // Escalated to a human (the first member's author spawner).
        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user-grpa"),
                Some(&Id::new("ws-1")),
                None,
                None,
                None,
                None,
                100,
                0,
            )
            .await
            .unwrap();
        assert!(
            notifs
                .iter()
                .any(|n| n.notification_type == gyre_common::NotificationType::MergeQueueEscalation),
            "group revert conflict must escalate to a human, got {:?}",
            notifs.iter().map(|n| &n.notification_type).collect::<Vec<_>>()
        );

        // No remediation tasks for a revert that did not happen.
        let tasks = state.tasks.list_by_repo(&repo.id).await.unwrap();
        assert!(
            tasks.is_empty(),
            "conflicting group revert must not create remediation tasks"
        );
    }

    #[tokio::test]
    async fn circuit_breaker_trips_at_three_reverts() {
        let state = test_state();
        let (repo, mr) = setup_recovery_mr(&state).await;

        // Passing post-merge gate: recovery runs, but the noop adapter's
        // revert SHA is not HEAD, so the re-run fails → escalation path
        // (queue stays paused) while the revert count still increments on
        // every successful revert.
        create_post_merge_gate(&state, &repo.id, "true", true).await;

        for i in 0..3 {
            recover_from_post_merge_failure(
                &state,
                &repo,
                &mr,
                &format!("fail-{i}-0000000000000000000000000000000"),
                "post-merge-tests failed",
            )
            .await;
        }

        // Revert count reached 3 → circuit breaker tripped.
        let count = state
            .kv_store
            .kv_get(REVERT_COUNTS_NS, "mr:mr-recov")
            .await
            .unwrap()
            .expect("revert count recorded");
        assert_eq!(count, "3");

        // Queue entry cancelled permanently.
        let cancelled = state
            .merge_queue
            .find_by_id(&Id::new("entry-mr-recov"))
            .await
            .unwrap()
            .expect("entry exists");
        assert_eq!(cancelled.status, MergeQueueEntryStatus::Cancelled);

        // Critical circuit-breaker escalation task created.
        let tasks = state.tasks.list_by_repo(&repo.id).await.unwrap();
        assert!(
            tasks.iter().any(|t| t.priority == TaskPriority::Critical
                && t.labels.contains(&"circuit-breaker".to_string())),
            "circuit-breaker critical task must be created"
        );
    }

    /// TASK-095 R3-F2: the remediation task tells the author to re-do the
    /// work on a fresh branch as a NEW MR. A per-MR-id counter could never
    /// fire for spec-bound work ("same MR reverted 3 times" across MRs is
    /// impossible when the MR id is the key). Two MRs bound to the same
    /// spec share one breaker; a different spec is unaffected.
    #[tokio::test]
    async fn revert_breaker_accumulates_across_resubmitted_mrs() {
        let state = test_state();
        let (repo, _mr) = setup_recovery_mr(&state).await;

        let mk = |id: &str, spec: &str| {
            let mut m = gyre_domain::MergeRequest::new(
                Id::new(id),
                repo.id.clone(),
                "resubmitted work",
                "feat/resub",
                "main",
                1000,
            );
            m.workspace_id = Id::new("ws-1");
            m.spec_ref = Some(spec.to_string());
            m
        };
        let mr_a = mk("mr-resub-a", "specs/system/foo.md");
        let mr_b = mk("mr-resub-b", "specs/system/foo.md");
        let mr_other = mk("mr-resub-other", "specs/system/bar.md");
        for m in [&mr_a, &mr_b, &mr_other] {
            state.merge_requests.create(m).await.unwrap();
        }

        assert_eq!(increment_revert_count(&state, &mr_a).await.unwrap(), 1);
        assert_eq!(increment_revert_count(&state, &mr_a).await.unwrap(), 2);
        // The resubmission of the same spec must INHERIT the count — with
        // the old per-MR-id key this would return 1 and the breaker could
        // never fire through the prescribed resubmission path.
        assert_eq!(
            increment_revert_count(&state, &mr_b).await.unwrap(),
            3,
            "resubmitted MR must accumulate on the shared spec key"
        );
        // Different spec → its own counter.
        assert_eq!(increment_revert_count(&state, &mr_other).await.unwrap(), 1);
    }

    /// TASK-095 R3-F2: a queue entry referencing an MR that became
    /// non-mergeable after enqueue (the recovery protocol reverted it, or
    /// it was closed/merged elsewhere) must fail the entry — silently
    /// re-merging a Reverted MR voids the breaker's "removed permanently"
    /// and the Reverted terminal state.
    #[tokio::test]
    async fn process_next_fails_entry_for_reverted_mr() {
        let state = test_state();
        let (_repo, mut mr) = setup_recovery_mr(&state).await;

        // The MR was reverted after enqueueing (Open→…→Reverted persisted).
        mr.transition_status(MrStatus::Approved).unwrap();
        mr.transition_status(MrStatus::Merged).unwrap();
        mr.transition_status(MrStatus::Reverted).unwrap();
        state.merge_requests.update(&mr).await.unwrap();

        process_next(&state).await.unwrap();

        let entry = state
            .merge_queue
            .find_by_id(&Id::new("entry-mr-recov"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            entry.status,
            MergeQueueEntryStatus::Failed,
            "stale entry for a Reverted MR must be failed, not merged"
        );
        let stored = state
            .merge_requests
            .find_by_id(&mr.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            stored.status,
            MrStatus::Reverted,
            "process_next must NOT re-merge a Reverted MR"
        );
    }

    // ── Verification repair: merge-time attestation ABAC enforcement ────
    //
    // check-inert-enforcement flagged the two evaluate_attestation_abac
    // call sites as F5-class (result discarded, merge proceeds regardless).
    // The decision now gates the merge. These tests prove the enforcement
    // is real AND that it does not regress valid chained merges under the
    // shipped built-in policy set.

    /// A minimal chain attestation indexed by task, for the fallback
    /// (agent-task) lookup path — no real git repo needed.
    fn task_chain_attestation(task_id: &str) -> gyre_common::Attestation {
        use gyre_common::{
            AttestationInput, AttestationMetadata, AttestationOutput, DerivedInput, KeyBinding,
        };
        gyre_common::Attestation {
            id: format!("sha256:merge-abac-{task_id}"),
            input: AttestationInput::Derived(DerivedInput {
                parent_ref: vec![0, 0],
                preconditions: vec![],
                update: "narrow_scope".to_string(),
                output_constraints: vec![],
                signature: vec![1],
                key_binding: KeyBinding {
                    public_key: vec![1, 2, 3, 4],
                    user_identity: "user:merge-abac".to_string(),
                    issuer: "https://keycloak.example.com".to_string(),
                    trust_anchor_id: "tenant-keycloak".to_string(),
                    issued_at: 1_700_000_000,
                    expires_at: 1_700_003_600,
                    user_signature: vec![10, 20],
                    platform_countersign: vec![30, 40],
                },
            }),
            output: AttestationOutput {
                content_hash: vec![1, 2, 3],
                commit_sha: "ccc000".to_string(),
                agent_signature: None,
                gate_results: vec![],
            },
            metadata: AttestationMetadata {
                created_at: 1_700_000_100,
                workspace_id: "ws-1".to_string(),
                repo_id: "merge-abac-repo".to_string(),
                task_id: task_id.to_string(),
                agent_id: "agent:merge-abac".to_string(),
                chain_depth: 0,
            },
        }
    }

    /// MR + author agent + saved attestation chain for the agent's current
    /// task, enqueued for the merge processor.
    async fn setup_mr_with_attestation_chain(
        state: &AppState,
    ) -> (gyre_domain::Repository, gyre_domain::MergeRequest) {
        let repo = create_repo_in_workspace(state, "merge-abac-repo", "ws-1").await;

        let mut agent =
            gyre_domain::Agent::new(Id::new("agent-merge-abac"), "agent-merge-abac", 1000);
        agent.spawned_by = Some("user-merge-abac".to_string());
        agent.workspace_id = Id::new("ws-1");
        agent.current_task_id = Some(Id::new("task-merge-abac"));
        state.agents.create(&agent).await.unwrap();

        state
            .chain_attestations
            .save(&task_chain_attestation("task-merge-abac"))
            .await
            .unwrap();

        let mut mr = gyre_domain::MergeRequest::new(
            Id::new("mr-merge-abac"),
            repo.id.clone(),
            "MR merge ABAC enforcement",
            "feat/abac-merge",
            "main",
            1000,
        );
        mr.workspace_id = Id::new("ws-1");
        mr.author_agent_id = Some(Id::new("agent-merge-abac"));
        state.merge_requests.create(&mr).await.unwrap();

        enqueue_mr(state, "mr-merge-abac", 100, 1000).await;

        (repo, mr)
    }

    /// A Deny decision from the merge-time attestation ABAC evaluation must
    /// fail the queue entry with a reason naming the matched policy and must
    /// NOT execute the merge. (F5 fix: the evaluation gates the merge.)
    #[tokio::test]
    async fn merge_time_abac_deny_fails_entry_and_blocks_merge() {
        let state = test_state();
        let (_repo, mr) = setup_mr_with_attestation_chain(&state).await;

        // User-created deny (200-299 band): denies merge on attestation
        // resources unconditionally.
        let deny = gyre_domain::Policy {
            id: Id::new("deny-merge-abac"),
            name: "deny-merge-abac".to_string(),
            description: "Deny all attestation merges".to_string(),
            scope: gyre_domain::PolicyScope::Tenant,
            scope_id: None,
            priority: 200,
            effect: gyre_domain::PolicyEffect::Deny,
            conditions: vec![],
            actions: vec!["merge".to_string()],
            resource_types: vec!["attestation".to_string()],
            enabled: true,
            built_in: false,
            immutable: false,
            created_by: Id::new("test"),
            created_at: 0,
            updated_at: 0,
        };
        state.policies.create(&deny).await.unwrap();

        process_next(&state).await.unwrap();

        // Entry failed with the ABAC reason naming the matched policy.
        let entry = state
            .merge_queue
            .find_by_id(&Id::new("entry-mr-merge-abac"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            entry.status,
            MergeQueueEntryStatus::Failed,
            "ABAC deny must fail the queue entry"
        );
        let reason = entry.error_message.expect("deny must record a reason");
        assert!(
            reason.contains("attestation ABAC policy denied merge"),
            "reason must name the ABAC denial: {reason}"
        );
        assert!(
            reason.contains("deny-merge-abac"),
            "reason must name the matched policy: {reason}"
        );

        // The merge was NOT executed.
        let updated = state
            .merge_requests
            .find_by_id(&mr.id)
            .await
            .unwrap()
            .unwrap();
        assert_ne!(
            updated.status,
            MrStatus::Merged,
            "ABAC-denied MR must not be merged"
        );
        assert!(
            updated.merge_commit_sha.is_none(),
            "no merge commit may be recorded for a denied merge"
        );
    }

    /// A valid attestation chain must still merge under the SHIPPED built-in
    /// policy set: `builtin:require-signed-authorization` is satisfied by the
    /// located chain (§7.2 unless-clause) and `builtin:attested-authorization`
    /// allows the merge above the default-deny catchall. This test fails if
    /// the enforcement is added without the policy interplay fix (the
    /// immutable policy would deny every chained merge).
    #[tokio::test]
    async fn merge_time_abac_valid_chain_merges_under_builtin_policies() {
        let state = test_state();
        for policy in crate::abac_middleware::m34_builtin_policies() {
            state.policies.create(&policy).await.unwrap();
        }
        let (_repo, mr) = setup_mr_with_attestation_chain(&state).await;

        process_next(&state).await.unwrap();

        let updated = state
            .merge_requests
            .find_by_id(&mr.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            updated.status,
            MrStatus::Merged,
            "a valid attestation chain must merge under the shipped built-in policy set"
        );

        // The decision was recorded in the policy-decision audit trail.
        let decisions = state
            .policies
            .list_decisions(None, Some("attestation"), 100)
            .await
            .unwrap();
        assert!(
            decisions
                .iter()
                .any(|d| d.action == "merge" && d.resource_type == "attestation"),
            "merge-time ABAC decisions must be recorded for audit"
        );
    }

    /// Verification repair (task-116): `MergeAttestation.meta_specs_used`
    /// is populated from the spawn-time prompt-set record of the MR's author
    /// agent. Reverting the wiring in `process_next` to `meta_specs_used:
    /// vec![]` (or dropping the `load_prompt_set_record` lookup) fails this
    /// test — the persisted, signed attestation bundle must carry exactly
    /// the record's meta-spec references (id, kind, content_hash, version,
    /// required, scope), and the signature must verify over that payload.
    #[tokio::test]
    async fn merge_attestation_records_author_prompt_set_meta_specs() {
        let state = test_state();
        for policy in crate::abac_middleware::m34_builtin_policies() {
            state.policies.create(&policy).await.unwrap();
        }
        let (_repo, mr) = setup_mr_with_attestation_chain(&state).await;

        // The spawn-time prompt-set record for the MR's author agent, exactly
        // as `spawn_agent_core` persists via `store_prompt_set_record`: two
        // meta-specs (required tenant persona, optional bound standard).
        let record = crate::prompt_assembly::AgentPromptSetRecord {
            agent_id: Id::new("agent-merge-abac"),
            task_id: Id::new("task-merge-abac"),
            spec_path: None,
            set_sha: "set-sha-meta-specs".to_string(),
            assembled_at: 1_700_000_000,
            meta_specs_used: vec![
                gyre_domain::MetaSpecUsed {
                    id: Id::new("ms-persona"),
                    kind: "meta:persona".to_string(),
                    content_hash: "hash-persona".to_string(),
                    version: 2,
                    required: true,
                    scope: "global".to_string(),
                },
                gyre_domain::MetaSpecUsed {
                    id: Id::new("ms-standard"),
                    kind: "meta:standard".to_string(),
                    content_hash: "hash-standard".to_string(),
                    version: 5,
                    required: false,
                    scope: "workspace".to_string(),
                },
            ],
            rendered_prompt: None,
        };
        crate::prompt_assembly::store_prompt_set_record(&state, &record).await;

        process_next(&state).await.unwrap();

        let updated = state
            .merge_requests
            .find_by_id(&mr.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, MrStatus::Merged, "MR must merge");

        let bundle = state
            .attestation_store
            .find_by_mr_id(mr.id.as_str())
            .await
            .unwrap()
            .expect("attestation bundle must be persisted for the merged MR");
        let used = &bundle.attestation.meta_specs_used;
        assert_eq!(
            used.len(),
            2,
            "both prompt-set meta-specs must be attested, got {used:?}"
        );
        for (got, want) in used.iter().zip(record.meta_specs_used.iter()) {
            assert_eq!(got.id, want.id, "meta-spec id must match the record");
            assert_eq!(got.kind, want.kind, "meta-spec kind must match the record");
            assert_eq!(
                got.content_hash, want.content_hash,
                "meta-spec content_hash must match the record"
            );
            assert_eq!(got.version, want.version, "meta-spec version must match");
            assert_eq!(got.required, want.required, "required flag must match");
            assert_eq!(got.scope, want.scope, "scope must match the record");
        }

        // The signature covers the meta_specs_used payload: the bundle
        // verifies with the server's real Ed25519 public key.
        assert!(
            crate::attestation::verify_bundle(&bundle, &state.agent_signing_key.public_key_bytes),
            "attestation signature must verify over the full payload incl. meta_specs_used"
        );
    }

    /// TASK-095 R2-1: post-merge gates must execute against the exact
    /// `head_sha` tree, in a detached worktree, not against whatever the
    /// first registered worktree happens to have checked out.
    ///
    /// Uses a real git repository: base commit has no `marker` file, head
    /// commit adds it. The gate command `test -f marker` passes only when
    /// run in a checkout of head_sha.
    #[tokio::test]
    async fn post_merge_gates_run_against_head_sha_tree() {
        use std::process::Command as ShellCommand;

        // Real git repo: init, base commit, then head commit adding `marker`.
        let dir = tempfile::tempdir().unwrap();
        let repo_path = dir.path().join("gate-repo");
        std::fs::create_dir_all(&repo_path).unwrap();
        let git = |args: &[&str]| {
            let out = ShellCommand::new("git")
                .args(["-C", repo_path.to_str().unwrap()])
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        git(&["init", "--initial-branch=main"]);
        git(&["config", "user.email", "test@gyre.local"]);
        git(&["config", "user.name", "Gyre Test"]);
        std::fs::write(repo_path.join("base.txt"), "base").unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "base"]);
        std::fs::write(repo_path.join("marker"), "post-merge content").unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "head with marker"]);
        let head_sha = git(&["rev-parse", "HEAD"]);
        let base_sha = git(&["rev-parse", "HEAD~1"]);

        // State with the real git ops adapter and the repo registered at
        // its on-disk path.
        let state = crate::mem::test_state_with_git_ops(std::sync::Arc::new(
            gyre_adapters::Git2OpsAdapter::new(),
        ));
        let repo = Repository::new(
            Id::new("repo-gate-r21"),
            Id::new("ws-1"),
            "gate-repo",
            repo_path.to_str().unwrap().to_string(),
            0,
        );
        state.repos.create(&repo).await.unwrap();

        // The gate only passes when `marker` exists in the checkout — true
        // for head_sha's tree, false for the base tree.
        create_post_merge_gate(&state, &repo.id, "test -f marker", true).await;

        // Gates run at head_sha: marker exists → pass.
        crate::gate_executor::run_post_merge_gates(&state, &repo, &head_sha)
            .await
            .expect("gates must pass at head_sha");

        // Gates run at the base SHA: marker absent → required gate fails.
        let err = crate::gate_executor::run_post_merge_gates(&state, &repo, &base_sha)
            .await
            .expect_err("gates must fail at base sha");
        assert!(
            err.contains("post-merge-tests"),
            "failure should name the gate: {err}"
        );

        // The detached gate worktree was deregistered: the only registered
        // worktree is the repo itself.
        let worktrees = state
            .git_ops
            .list_worktrees(repo_path.to_str().unwrap())
            .await
            .unwrap();
        assert_eq!(
            worktrees,
            vec![repo_path.to_str().unwrap().to_string()],
            "gate worktree should be removed"
        );
    }

    // ── meta-spec-reconciliation.md §9: merge-gate drift behavior ───────────

    /// Bind a meta-spec set to the workspace and return its current set SHA
    /// (the SHA `compute_meta_spec_set_sha` derives for the binding).
    async fn bind_drift_set(state: &AppState, ws_id: &str) -> String {
        let set = crate::api::meta_specs::MetaSpecSet {
            workspace_id: ws_id.to_string(),
            personas: std::collections::BTreeMap::from([(
                "backend-developer".to_string(),
                crate::api::meta_specs::MetaSpecPinnedEntry {
                    path: "specs/personas/backend.md".to_string(),
                    sha: "a1b2c3".to_string(),
                },
            )]),
            principles: vec![],
            standards: vec![],
            process: vec![],
        };
        let json = serde_json::to_string(&set).unwrap();
        state
            .meta_spec_sets
            .upsert(&Id::new(ws_id), &json)
            .await
            .unwrap();
        crate::compute_meta_spec_set_sha(state, &Id::new(ws_id)).await
    }

    /// A signed provenance attestation recording `meta_sha` as the set the
    /// code was produced under (same shape as the authorization chain's
    /// root `SignedInput`), attached to `task_id`.
    fn drift_attestation(
        task_id: &str,
        repo_id: &Id,
        ws_id: &str,
        meta_sha: &str,
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
            valid_until: 2000,
            expected_generation: None,
            signature: vec![1, 2, 3],
            key_binding: gyre_common::KeyBinding {
                public_key: vec![1],
                user_identity: "user:admin".to_string(),
                issuer: "https://gyre.local".to_string(),
                trust_anchor_id: "ta".to_string(),
                issued_at: 1000,
                expires_at: 2000,
                user_signature: vec![1],
                platform_countersign: vec![1],
            },
        };
        gyre_common::Attestation {
            id: format!("att-{task_id}"),
            input: AttestationInput::Signed(si),
            output: AttestationOutput {
                content_hash: vec![1],
                commit_sha: format!("{task_id:040}"),
                agent_signature: None,
                gate_results: vec![],
            },
            metadata: gyre_common::attestation::AttestationMetadata {
                created_at: 1000,
                workspace_id: ws_id.to_string(),
                repo_id: repo_id.to_string(),
                task_id: task_id.to_string(),
                agent_id: format!("agent-{task_id}"),
                chain_depth: 0,
            },
        }
    }

    /// Seed the provenance chain for a drifted MR: an author agent whose
    /// current task carries a signed attestation recording `meta_sha` as
    /// the meta-spec set the code was produced under. Returns the agent id
    /// (the task-based attestation lookup path — the repo path is not a
    /// real git repo, so the commit-based path resolves nothing).
    async fn seed_drift_provenance(
        state: &AppState,
        mr_id: &str,
        ws_id: &str,
        repo: &Repository,
        meta_sha: &str,
    ) -> Id {
        let task_id = format!("task-{mr_id}");
        let att = drift_attestation(&task_id, &repo.id, ws_id, meta_sha);
        let mut agent = gyre_domain::Agent::new(Id::new(format!("agent-{mr_id}")), "author", 1000);
        agent.workspace_id = Id::new(ws_id);
        agent.current_task_id = Some(Id::new(task_id));
        state.agents.create(&agent).await.unwrap();
        state.chain_attestations.save(&att).await.unwrap();
        agent.id.clone()
    }

    /// Create an MR whose author agent's current task carries a provenance
    /// attestation recording `meta_sha`.
    async fn create_drift_mr(
        state: &AppState,
        mr_id: &str,
        ws_id: &str,
        repo: &Repository,
        meta_sha: &str,
    ) -> gyre_domain::MergeRequest {
        let agent_id = seed_drift_provenance(state, mr_id, ws_id, repo, meta_sha).await;

        let mut mr = gyre_domain::MergeRequest::new(
            Id::new(mr_id),
            repo.id.clone(),
            format!("MR {mr_id}"),
            "feat/drift",
            "main",
            1000,
        );
        mr.workspace_id = Id::new(ws_id);
        mr.author_agent_id = Some(agent_id);
        state.merge_requests.create(&mr).await.unwrap();
        mr
    }

    /// §9: mismatched meta_spec_set_sha records a `[WARN] meta-spec-drift`
    /// gate result on the MR and the merge proceeds (warn, don't block).
    #[tokio::test]
    async fn meta_spec_drift_records_warn_gate_result_and_merge_proceeds() {
        let state = test_state();
        // Production policy set (same as
        // `guided_workspace_merge_proceeds_with_startup_seeded_builtins`):
        // a deployed server seeds the builtin ABAC policies at startup, and
        // the merge-time attestation-ABAC gate default-denies without them.
        // The drift MR carries a chain attestation, so
        // `builtin:attested-authorization` allows the merge — the §9 drift
        // behavior is under test, not the empty policy store.
        crate::abac_middleware::seed_builtin_policies(&state)
            .await
            .expect("seed built-in policies");
        let ws_id = "ws-drift-warn";
        let repo = create_repo_in_workspace(&state, "repo-drift-warn", ws_id).await;
        let current = bind_drift_set(&state, ws_id).await;
        assert!(
            !current.is_empty(),
            "set must be bound for the check to run"
        );

        // Provenance under an OLD set SHA → drift.
        create_drift_mr(&state, "mr-drift-warn", ws_id, &repo, "deadbeef").await;
        enqueue_mr(&state, "mr-drift-warn", 50, 1000).await;

        process_next(&state).await.unwrap();

        // The drift warning is recorded as a gate result in the §9 format,
        // carrying Passed — informational, never blocking by itself.
        let results = state
            .gate_results
            .list_by_mr_id("mr-drift-warn")
            .await
            .unwrap();
        let drift = results
            .iter()
            .find(|r| r.gate_id.as_str() == crate::reconciliation::META_SPEC_DRIFT_GATE_ID)
            .expect("drift gate result must be recorded");
        assert_eq!(drift.status, GateStatus::Passed);
        let output = drift.output.as_deref().expect("warning output");
        let expected = crate::reconciliation::format_meta_spec_drift_warning(
            &crate::reconciliation::MetaSpecDrift {
                produced_set_sha: "deadbeef".to_string(),
                current_set_sha: current.clone(),
            },
        );
        assert_eq!(output, expected);
        assert!(output.starts_with("[WARN] meta-spec-drift: "));

        // Warn only — the MR still merges.
        let updated = state
            .merge_requests
            .find_by_id(&Id::new("mr-drift-warn"))
            .await
            .unwrap()
            .expect("mr exists");
        assert_eq!(
            updated.status,
            MrStatus::Merged,
            "warn-only drift must not block the merge"
        );
    }

    /// §9: with `require_current_meta_spec_set: true`, drift converts to a
    /// hard block — the entry stays queued with the drift reason and the MR
    /// does not merge.
    #[tokio::test]
    async fn meta_spec_drift_hard_blocks_under_strict_workspace_policy() {
        let state = test_state();
        // Production policy set — see the warn-only test above: the relaxed
        // phase of this test must merge the attested drifted MR, which the
        // builtin `attested-authorization` Allow covers exactly as in a
        // deployed server.
        crate::abac_middleware::seed_builtin_policies(&state)
            .await
            .expect("seed built-in policies");
        let ws_id = "ws-drift-block";
        let repo = create_repo_in_workspace(&state, "repo-drift-block", ws_id).await;
        bind_drift_set(&state, ws_id).await;

        create_drift_mr(&state, "mr-drift-block", ws_id, &repo, "deadbeef").await;
        enqueue_mr(&state, "mr-drift-block", 50, 1000).await;

        // Strict workspace policy: drift blocks.
        crate::merge_policy::set_merge_policy(
            &state,
            &Id::new(ws_id),
            &crate::merge_policy::WorkspaceMergePolicy {
                require_current_meta_spec_set: true,
            },
        )
        .await
        .unwrap();

        process_next(&state).await.unwrap();

        // Entry went back to Queued carrying the drift reason — it stays in
        // the queue until reconciled or the policy is relaxed.
        let entry = state
            .merge_queue
            .find_by_id(&Id::new("entry-mr-drift-block"))
            .await
            .unwrap()
            .expect("entry exists");
        assert_eq!(entry.status, MergeQueueEntryStatus::Queued);
        let reason = entry
            .error_message
            .as_deref()
            .expect("drift reason recorded");
        assert!(reason.contains("meta-spec drift"), "reason: {reason}");
        assert!(
            reason.contains("require_current_meta_spec_set"),
            "reason: {reason}"
        );

        // The MR did not merge.
        let updated = state
            .merge_requests
            .find_by_id(&Id::new("mr-drift-block"))
            .await
            .unwrap()
            .expect("mr exists");
        assert_eq!(updated.status, MrStatus::Open);

        // The block path records no drift gate result (it returns before
        // the warn-only recording).
        let results = state
            .gate_results
            .list_by_mr_id("mr-drift-block")
            .await
            .unwrap();
        assert!(
            results
                .iter()
                .all(|r| r.gate_id.as_str() != crate::reconciliation::META_SPEC_DRIFT_GATE_ID),
            "blocked drift must not record a Passed warn gate result"
        );

        // Relaxing the policy lets the same entry merge on the next cycle
        // (the drift then downgrades to the warn-only gate result).
        crate::merge_policy::set_merge_policy(
            &state,
            &Id::new(ws_id),
            &crate::merge_policy::WorkspaceMergePolicy::default(),
        )
        .await
        .unwrap();
        process_next(&state).await.unwrap();

        let updated = state
            .merge_requests
            .find_by_id(&Id::new("mr-drift-block"))
            .await
            .unwrap()
            .expect("mr exists");
        assert_eq!(
            updated.status,
            MrStatus::Merged,
            "relaxed policy must let the drifted MR merge (warn only)"
        );
        let results = state
            .gate_results
            .list_by_mr_id("mr-drift-block")
            .await
            .unwrap();
        assert!(
            results
                .iter()
                .any(|r| r.gate_id.as_str() == crate::reconciliation::META_SPEC_DRIFT_GATE_ID),
            "warn-only path must record the drift gate result"
        );
    }

    /// §9: matching meta_spec_set_sha → pass, no warning gate result.
    #[tokio::test]
    async fn meta_spec_drift_matching_set_sha_merges_without_warning() {
        let state = test_state();
        // Production policy set — see the warn-only drift test above.
        crate::abac_middleware::seed_builtin_policies(&state)
            .await
            .expect("seed built-in policies");
        let ws_id = "ws-drift-match";
        let repo = create_repo_in_workspace(&state, "repo-drift-match", ws_id).await;
        let current = bind_drift_set(&state, ws_id).await;

        // Provenance under the CURRENT set SHA → pass.
        create_drift_mr(&state, "mr-drift-match", ws_id, &repo, &current).await;
        enqueue_mr(&state, "mr-drift-match", 50, 1000).await;

        process_next(&state).await.unwrap();

        let updated = state
            .merge_requests
            .find_by_id(&Id::new("mr-drift-match"))
            .await
            .unwrap()
            .expect("mr exists");
        assert_eq!(updated.status, MrStatus::Merged);

        let results = state
            .gate_results
            .list_by_mr_id("mr-drift-match")
            .await
            .unwrap();
        assert!(
            results
                .iter()
                .all(|r| r.gate_id.as_str() != crate::reconciliation::META_SPEC_DRIFT_GATE_ID),
            "matching set SHA must not record a drift warning"
        );
    }

    /// §9: an MR without an attestation skips the check gracefully — no
    /// crash, no drift gate result, merge proceeds.
    #[tokio::test]
    async fn meta_spec_drift_mr_without_attestation_skips_check() {
        let state = test_state();
        let ws_id = "ws-drift-noatt";
        let repo = create_repo_in_workspace(&state, "repo-drift-noatt", ws_id).await;
        bind_drift_set(&state, ws_id).await;

        // No author agent, no attestation anywhere.
        let mut mr = gyre_domain::MergeRequest::new(
            Id::new("mr-drift-noatt"),
            repo.id.clone(),
            "MR no attestation",
            "feat/noatt",
            "main",
            1000,
        );
        mr.workspace_id = Id::new(ws_id);
        state.merge_requests.create(&mr).await.unwrap();
        enqueue_mr(&state, "mr-drift-noatt", 50, 1000).await;

        process_next(&state).await.unwrap();

        let updated = state
            .merge_requests
            .find_by_id(&Id::new("mr-drift-noatt"))
            .await
            .unwrap()
            .expect("mr exists");
        assert_eq!(updated.status, MrStatus::Merged);

        let results = state
            .gate_results
            .list_by_mr_id("mr-drift-noatt")
            .await
            .unwrap();
        assert!(
            results
                .iter()
                .all(|r| r.gate_id.as_str() != crate::reconciliation::META_SPEC_DRIFT_GATE_ID),
            "no attestation → no drift gate result"
        );
    }

    /// The warn-only drift gate result is idempotent across retry cycles:
    /// an existing result for the MR is updated in place, not duplicated.
    #[tokio::test]
    async fn meta_spec_drift_warning_gate_result_is_idempotent() {
        let state = test_state();
        let ws_id = "ws-drift-idem";
        let repo = create_repo_in_workspace(&state, "repo-drift-idem", ws_id).await;
        bind_drift_set(&state, ws_id).await;

        let mr = create_drift_mr(&state, "mr-drift-idem", ws_id, &repo, "deadbeef").await;
        let drift = crate::reconciliation::mr_meta_spec_drift(&state, &mr)
            .await
            .expect("drift expected");

        record_meta_spec_drift_warning(&state, &mr, &drift).await;
        record_meta_spec_drift_warning(&state, &mr, &drift).await;

        let results = state
            .gate_results
            .list_by_mr_id("mr-drift-idem")
            .await
            .unwrap();
        let drift_results: Vec<_> = results
            .iter()
            .filter(|r| r.gate_id.as_str() == crate::reconciliation::META_SPEC_DRIFT_GATE_ID)
            .collect();
        assert_eq!(
            drift_results.len(),
            1,
            "retry cycles must not duplicate the drift gate result"
        );
    }

    /// Review regression (task-159 repair): the strict-policy drift hold is
    /// evaluated DURING candidate selection, so a strictly-blocked drifted
    /// entry at the head of the shared merge queue cannot starve later
    /// entries — the processor requeues it with the drift reason and tries
    /// the next candidate in the SAME cycle. Clean MRs behind the drifted
    /// head — both in the same workspace and in other workspaces — must
    /// merge while the drifted head stays held.
    #[tokio::test]
    async fn strict_drift_hold_does_not_starve_shared_merge_queue() {
        let state = test_state();

        // Workspace A: strict merge policy + bound meta-spec set. A drifted
        // MR sits at the HEAD of the shared queue (highest priority,
        // earliest enqueue); a clean MR (no attestation → drift check
        // skips gracefully) sits behind it in the SAME workspace.
        let ws_a = "ws-drift-starve";
        let repo_a = create_repo_in_workspace(&state, "repo-drift-starve", ws_a).await;
        bind_drift_set(&state, ws_a).await;
        create_drift_mr(&state, "mr-drift-starve-head", ws_a, &repo_a, "deadbeef").await;
        enqueue_mr(&state, "mr-drift-starve-head", 100, 1000).await;
        let mut clean_same_ws = gyre_domain::MergeRequest::new(
            Id::new("mr-drift-starve-clean"),
            repo_a.id.clone(),
            "MR clean same workspace",
            "feat/starve-clean",
            "main",
            1000,
        );
        clean_same_ws.workspace_id = Id::new(ws_a);
        state.merge_requests.create(&clean_same_ws).await.unwrap();
        enqueue_mr(&state, "mr-drift-starve-clean", 50, 1001).await;
        crate::merge_policy::set_merge_policy(
            &state,
            &Id::new(ws_a),
            &crate::merge_policy::WorkspaceMergePolicy {
                require_current_meta_spec_set: true,
            },
        )
        .await
        .unwrap();

        // Workspace B: clean MR, lowest priority, enqueued last.
        let ws_b = "ws-drift-starve-other";
        let repo_b = create_repo_in_workspace(&state, "repo-drift-starve-other", ws_b).await;
        let mut clean_other_ws = gyre_domain::MergeRequest::new(
            Id::new("mr-drift-starve-other"),
            repo_b.id.clone(),
            "MR clean other workspace",
            "feat/starve-other",
            "main",
            1000,
        );
        clean_other_ws.workspace_id = Id::new(ws_b);
        state.merge_requests.create(&clean_other_ws).await.unwrap();
        enqueue_mr(&state, "mr-drift-starve-other", 10, 1002).await;

        // One cycle: the drifted head is requeued with the drift reason and
        // the clean same-workspace MR behind it merges in the SAME cycle.
        process_next(&state).await.unwrap();

        let head = state
            .merge_requests
            .find_by_id(&Id::new("mr-drift-starve-head"))
            .await
            .unwrap()
            .expect("head mr exists");
        assert_eq!(
            head.status,
            MrStatus::Open,
            "strict drift holds the drifted MR"
        );
        let head_entry = state
            .merge_queue
            .find_by_id(&Id::new("entry-mr-drift-starve-head"))
            .await
            .unwrap()
            .expect("head entry exists");
        assert_eq!(head_entry.status, MergeQueueEntryStatus::Queued);
        let reason = head_entry
            .error_message
            .as_deref()
            .unwrap_or_else(|| panic!("held entry must carry the drift reason"));
        assert!(reason.contains("meta-spec drift"), "reason: {reason}");

        let clean = state
            .merge_requests
            .find_by_id(&Id::new("mr-drift-starve-clean"))
            .await
            .unwrap()
            .expect("clean mr exists");
        assert_eq!(
            clean.status,
            MrStatus::Merged,
            "clean MR queued behind a strictly-blocked drifted MR in the same \
             workspace must merge in the same cycle — the drift hold must not \
             starve the shared merge queue"
        );

        // Second cycle: the cross-workspace clean MR also merges while the
        // drifted head is still held.
        process_next(&state).await.unwrap();

        let other = state
            .merge_requests
            .find_by_id(&Id::new("mr-drift-starve-other"))
            .await
            .unwrap()
            .expect("other-workspace mr exists");
        assert_eq!(
            other.status,
            MrStatus::Merged,
            "clean MR in ANOTHER workspace must merge while a strict-policy \
             drifted MR is held — a workspace-scoped governance setting must \
             not deny merge service to unrelated workspaces"
        );
        let head = state
            .merge_requests
            .find_by_id(&Id::new("mr-drift-starve-head"))
            .await
            .unwrap()
            .expect("head mr exists");
        assert_eq!(
            head.status,
            MrStatus::Open,
            "the drifted MR stays held across cycles until reconciled or relaxed"
        );
    }

    /// Review regression (task-159 repair): an atomic group containing a
    /// strictly-blocked drifted member is held as a unit during candidate
    /// selection — every member entry is requeued with the drift reason —
    /// while a later independent entry in another workspace still merges
    /// in the same cycle.
    #[tokio::test]
    async fn strict_drift_hold_atomic_group_does_not_starve_later_entry() {
        let state = test_state();

        // Workspace A: strict merge policy + bound set. Two-member atomic
        // group; member A is drifted (attestation under an old set SHA),
        // member B has no attestation of its own — the group is held as a
        // unit because the drift check expands to all members.
        let ws_a = "ws-drift-group-starve";
        let group_repo = create_repo_in_workspace(&state, "repo-drift-group-starve", ws_a).await;
        bind_drift_set(&state, ws_a).await;
        let drifted_author =
            seed_drift_provenance(&state, "mr-drift-group-a", ws_a, &group_repo, "deadbeef").await;
        create_mr_in_group(
            &state,
            "mr-drift-group-a",
            &group_repo.id,
            ws_a,
            "bundle-drift-starve",
            "feat/drift-group-a",
            Some(drifted_author.as_str()),
        )
        .await;
        create_mr_in_group(
            &state,
            "mr-drift-group-b",
            &group_repo.id,
            ws_a,
            "bundle-drift-starve",
            "feat/drift-group-b",
            None,
        )
        .await;
        enqueue_mr(&state, "mr-drift-group-a", 100, 1000).await;
        enqueue_mr(&state, "mr-drift-group-b", 100, 1001).await;
        crate::merge_policy::set_merge_policy(
            &state,
            &Id::new(ws_a),
            &crate::merge_policy::WorkspaceMergePolicy {
                require_current_meta_spec_set: true,
            },
        )
        .await
        .unwrap();

        // Later independent entry in another workspace (no bound set → no
        // drift possible), lower priority, enqueued last.
        let ws_b = "ws-drift-group-later";
        let later_repo = create_repo_in_workspace(&state, "repo-drift-group-later", ws_b).await;
        let mut later_mr = gyre_domain::MergeRequest::new(
            Id::new("mr-drift-group-later"),
            later_repo.id.clone(),
            "MR behind a drift-held atomic group",
            "feat/drift-group-later",
            "main",
            1002,
        );
        later_mr.workspace_id = Id::new(ws_b);
        state.merge_requests.create(&later_mr).await.unwrap();
        enqueue_mr(&state, "mr-drift-group-later", 50, 1002).await;

        process_next(&state).await.unwrap();

        // The group is held as a unit: both members stay Open with their
        // entries requeued carrying the drift reason.
        for mr_id in ["mr-drift-group-a", "mr-drift-group-b"] {
            let updated = state
                .merge_requests
                .find_by_id(&Id::new(mr_id))
                .await
                .unwrap()
                .expect("group member should exist");
            assert_eq!(
                updated.status,
                MrStatus::Open,
                "drift-held group member {mr_id} must stay Open"
            );
            let entry = state
                .merge_queue
                .find_by_id(&Id::new(format!("entry-{mr_id}")))
                .await
                .unwrap()
                .expect("group member entry should exist");
            assert_eq!(
                entry.status,
                MergeQueueEntryStatus::Queued,
                "drift-held group member {mr_id} must stay Queued"
            );
            let reason = entry
                .error_message
                .as_deref()
                .unwrap_or_else(|| panic!("held group member {mr_id} must carry a reason"));
            assert!(
                reason.contains("meta-spec drift"),
                "member {mr_id} reason: {reason}"
            );
        }

        // The later independent entry merged in the SAME cycle.
        let later = state
            .merge_requests
            .find_by_id(&Id::new("mr-drift-group-later"))
            .await
            .unwrap()
            .expect("later mr should exist");
        assert_eq!(
            later.status,
            MrStatus::Merged,
            "MR queued behind a drift-held atomic group must still merge — a \
             held group must not monopolize the shared merge queue"
        );
    }

    // ── TASK-106: automatic jj rebase on target branch movement ────────
    // (source-control.md §4)

    use crate::mem::test_state_with_jj_ops;
    use gyre_domain::AgentWorktree;
    use gyre_ports::JjRebaseOutcome;
    use std::sync::Arc;

    /// Shared fixture: one repo, one merged MR (author agent `merged`), and
    /// one in-flight MR (author agent `inflight`) targeting the same
    /// branch. `inflight`'s worktree dir is a real tempdir so the
    /// path-exists safeguard passes. Returns (state, jj double, repo,
    /// inflight MR).
    async fn setup_rebase_fixture() -> (
        Arc<AppState>,
        Arc<crate::mem::ConfigurableJjOps>,
        Repository,
        gyre_domain::MergeRequest,
    ) {
        let jj = Arc::new(crate::mem::ConfigurableJjOps::default());
        let state = test_state_with_jj_ops(jj.clone());
        let repo = create_repo_in_workspace(&state, "rebase-repo", "ws-1").await;

        // Landed agent + MR (merged by the processor in these tests).
        let mut merged_agent = gyre_domain::Agent::new(Id::new("agent-merged"), "merged", 1000);
        merged_agent.workspace_id = Id::new("ws-1");
        state.agents.create(&merged_agent).await.unwrap();
        let mut merged_mr = gyre_domain::MergeRequest::new(
            Id::new("mr-merged"),
            repo.id.clone(),
            "merged work",
            "feat/merged",
            "main",
            1000,
        );
        merged_mr.workspace_id = Id::new("ws-1");
        merged_mr.author_agent_id = Some(Id::new("agent-merged"));
        state.merge_requests.create(&merged_mr).await.unwrap();

        // In-flight agent + MR: Active, real worktree dir on disk.
        let mut inflight = gyre_domain::Agent::new(Id::new("agent-inflight"), "inflight", 1000);
        inflight.workspace_id = Id::new("ws-1");
        inflight.status = AgentStatus::Active;
        state.agents.create(&inflight).await.unwrap();
        let mut inflight_mr = gyre_domain::MergeRequest::new(
            Id::new("mr-inflight"),
            repo.id.clone(),
            "in-flight work",
            "feat/inflight",
            "main",
            1000,
        );
        inflight_mr.workspace_id = Id::new("ws-1");
        inflight_mr.author_agent_id = Some(Id::new("agent-inflight"));
        state.merge_requests.create(&inflight_mr).await.unwrap();
        let wt_dir = tempfile::tempdir().unwrap();
        let wt = AgentWorktree::new(
            Id::new("wt-inflight"),
            Id::new("agent-inflight"),
            repo.id.clone(),
            None,
            "feat/inflight",
            wt_dir.path().to_str().unwrap(),
            1000,
        );
        state.worktrees.create(&wt).await.unwrap();
        // Leak the tempdir: the worktree must outlive this function and be
        // cleaned up by std::process::tempdir semantics at test end.
        std::mem::forget(wt_dir);

        (state, jj, repo, inflight_mr)
    }

    /// The core §4 contract: a merge landing on the target branch rebases
    /// every OTHER in-flight agent on that branch (the merged author is
    /// excluded), onto the branch that MOVED, in the agent's recorded
    /// worktree.
    #[tokio::test]
    async fn merge_triggers_rebase_of_inflight_agents() {
        let (state, jj, _repo, _inflight_mr) = setup_rebase_fixture().await;

        let mut rx = state.message_broadcast_tx.subscribe();
        enqueue_mr(&state, "mr-merged", 100, 1000).await;
        process_next(&state).await.unwrap();

        // The merged MR landed.
        let merged = state
            .merge_requests
            .find_by_id(&Id::new("mr-merged"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(merged.status, MrStatus::Merged);

        // Exactly one rebase: the in-flight agent, not the merged author.
        let calls = jj.rebase_calls.lock().clone();
        assert_eq!(calls.len(), 1, "exactly one rebase call, got {calls:?}");
        assert_eq!(calls[0].0, worktree_path_of(&state, "agent-inflight").await);
        assert_eq!(calls[0].1, "@");
        assert_eq!(calls[0].2, "main");

        // The in-flight agent was notified of the baseline movement.
        let mut saw_baseline_moved = false;
        while let Ok(msg) = rx.try_recv() {
            if let gyre_common::message::Destination::Agent(a) = &msg.to {
                if a.as_str() == "agent-inflight" && msg.kind.as_str() == "baseline_moved" {
                    saw_baseline_moved = true;
                    assert_eq!(
                        msg.payload.as_ref().unwrap().get("target_branch"),
                        Some(&serde_json::json!("main"))
                    );
                }
            }
        }
        assert!(
            saw_baseline_moved,
            "in-flight agent must receive baseline_moved"
        );
    }

    /// §4 destination pin: the rebase targets the branch that MOVED, not
    /// the repo's default branch. MRs can target non-default branches
    /// (create_mr performs no default-branch validation); rebasing an
    /// agent onto the default branch when their MR targets `develop`
    /// splices their work into an unrelated line of history. Fails on the
    /// pre-fix behavior (destination hardcoded to `repo.default_branch`).
    #[tokio::test]
    async fn rebase_destination_is_moved_target_branch_not_default() {
        let (state, jj, repo, _inflight_mr) = setup_rebase_fixture().await;

        // Point BOTH MRs at a non-default target branch.
        let non_default = "develop";
        assert_ne!(
            repo.default_branch, non_default,
            "fixture must use a non-default target to pin the destination"
        );
        for mr_id in ["mr-merged", "mr-inflight"] {
            let mut mr = state
                .merge_requests
                .find_by_id(&Id::new(mr_id))
                .await
                .unwrap()
                .unwrap();
            mr.target_branch = non_default.to_string();
            state.merge_requests.update(&mr).await.unwrap();
        }

        enqueue_mr(&state, "mr-merged", 100, 1000).await;
        process_next(&state).await.unwrap();

        let merged = state
            .merge_requests
            .find_by_id(&Id::new("mr-merged"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(merged.status, MrStatus::Merged);

        let calls = jj.rebase_calls.lock().clone();
        assert_eq!(calls.len(), 1, "exactly one rebase call, got {calls:?}");
        assert_eq!(
            calls[0].2, non_default,
            "rebase must target the moved branch ({non_default}), not the default ({})",
            repo.default_branch
        );
    }

    /// Conflicts are state, not errors (jj conflict-as-state): the MR is
    /// flagged, the workspace sees SpeculativeConflict, the agent sees
    /// Escalation with the conflicting files.
    #[tokio::test]
    async fn rebase_conflict_surfaces_as_state() {
        let (state, jj, _repo, inflight_mr) = setup_rebase_fixture().await;
        *jj.rebase_outcome.lock() = Some(Ok(JjRebaseOutcome::Conflict {
            rebased_count: 1,
            files: vec!["src/lib.rs".to_string()],
        }));

        let mut rx = state.message_broadcast_tx.subscribe();
        enqueue_mr(&state, "mr-merged", 100, 1000).await;
        process_next(&state).await.unwrap();

        // MR flagged with conflicts (consumed by the API layer).
        let updated = state
            .merge_requests
            .find_by_id(&inflight_mr.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.has_conflicts, Some(true));

        let mut workspace_saw_conflict = false;
        let mut agent_saw_escalation = false;
        while let Ok(msg) = rx.try_recv() {
            match &msg.to {
                gyre_common::message::Destination::Workspace(ws)
                    if ws.as_str() == "ws-1"
                        && msg.kind == gyre_common::message::MessageKind::SpeculativeConflict =>
                {
                    workspace_saw_conflict = true;
                    assert_eq!(
                        msg.payload.as_ref().unwrap().get("conflicting_files"),
                        Some(&serde_json::json!(["src/lib.rs"]))
                    );
                }
                gyre_common::message::Destination::Agent(a)
                    if a.as_str() == "agent-inflight"
                        && msg.kind == gyre_common::message::MessageKind::Escalation =>
                {
                    agent_saw_escalation = true;
                    assert_eq!(
                        msg.payload.as_ref().unwrap().get("conflicting_files"),
                        Some(&serde_json::json!(["src/lib.rs"]))
                    );
                }
                _ => {}
            }
        }
        assert!(
            workspace_saw_conflict,
            "workspace must see SpeculativeConflict"
        );
        assert!(agent_saw_escalation, "agent must see Escalation");
    }

    /// F10: the conflict flag is scoped to the repo whose rebase
    /// conflicted. An agent can hold Open MRs on the same-named source
    /// branch in two repos; a conflict in one repo's rebase must not flag
    /// the other repo's MR.
    #[tokio::test]
    async fn rebase_conflict_does_not_flag_other_repo_mr() {
        let (state, jj, repo, _inflight_mr) = setup_rebase_fixture().await;
        *jj.rebase_outcome.lock() = Some(Ok(JjRebaseOutcome::Conflict {
            rebased_count: 1,
            files: vec!["src/lib.rs".to_string()],
        }));

        // A SECOND repo whose MR shares the author AND the source branch
        // name — the exact ambiguity find_mr_by_author_branch must resolve
        // by repository scoping.
        let other_repo = create_repo_in_workspace(&state, "rebase-other-repo", "ws-1").await;
        let mut other_mr = gyre_domain::MergeRequest::new(
            Id::new("mr-inflight-other"),
            other_repo.id.clone(),
            "same-named branch in another repo",
            "feat/inflight",
            "main",
            1000,
        );
        other_mr.workspace_id = Id::new("ws-1");
        other_mr.author_agent_id = Some(Id::new("agent-inflight"));
        state.merge_requests.create(&other_mr).await.unwrap();

        enqueue_mr(&state, "mr-merged", 100, 1000).await;
        process_next(&state).await.unwrap();

        // The conflicted repo's MR is flagged...
        let flagged = state
            .merge_requests
            .find_by_id(&Id::new("mr-inflight"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            flagged.has_conflicts,
            Some(true),
            "the conflicted repo's MR must be flagged"
        );
        // ...and the other repo's MR is not (F10).
        let untouched = state
            .merge_requests
            .find_by_id(&Id::new("mr-inflight-other"))
            .await
            .unwrap()
            .unwrap();
        assert_ne!(
            untouched.has_conflicts,
            Some(true),
            "another repo's MR must not inherit the conflict flag (F10)"
        );

        // Direct pin of the scoping itself: the lookup is deterministic
        // per repo regardless of store iteration order (the mem store is a
        // HashMap — the end-to-end path above can pick either MR when the
        // filter is wrong; this call pins the exact contract).
        let in_repo = find_mr_by_author_branch(
            &state,
            &repo.id,
            &Id::new("agent-inflight"),
            "feat/inflight",
        )
        .await
        .unwrap()
        .expect("the repo's own MR must be found");
        assert_eq!(in_repo.id, Id::new("mr-inflight"));
        let in_other = find_mr_by_author_branch(
            &state,
            &other_repo.id,
            &Id::new("agent-inflight"),
            "feat/inflight",
        )
        .await
        .unwrap()
        .expect("the other repo's MR must be found");
        assert_eq!(in_other.id, Id::new("mr-inflight-other"));
    }

    /// R19 F3: a clean rebase clears the conflict flag an earlier
    /// conflicted rebase surfaced on the author's MR — the
    /// conflicts-as-state surface must not report state jj no longer
    /// has (the agent resolved the conflict in their workspace; the
    /// next movement of the target rebases their stack cleanly). Fails
    /// on the pre-fix behavior: the success arm never reset the flag.
    #[tokio::test]
    async fn rebase_success_clears_stale_conflict_flag() {
        let (state, jj, repo, inflight_mr) = setup_rebase_fixture().await;

        // First movement conflicts — the in-flight MR is flagged.
        *jj.rebase_outcome.lock() = Some(Ok(JjRebaseOutcome::Conflict {
            rebased_count: 1,
            files: vec!["src/lib.rs".to_string()],
        }));
        enqueue_mr(&state, "mr-merged", 100, 1000).await;
        process_next(&state).await.unwrap();
        let flagged = state
            .merge_requests
            .find_by_id(&inflight_mr.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            flagged.has_conflicts,
            Some(true),
            "precondition: the conflict round must flag the MR"
        );

        // The agent resolves the conflict; the backoff window expires
        // (backdated, as the replay tests do) so the next movement
        // rebases instead of deferring.
        let now = crate::jobs::now_secs();
        state
            .kv_store
            .kv_set(
                JJ_REBASE_BACKOFF_NS,
                "agent-inflight",
                (now - JJ_REBASE_WINDOW_SECS - 1).to_string(),
            )
            .await
            .unwrap();

        // A second MR lands on the same target branch — clean this time.
        let mut second_agent = gyre_domain::Agent::new(Id::new("agent-merged-2"), "merged-2", 1000);
        second_agent.workspace_id = Id::new("ws-1");
        state.agents.create(&second_agent).await.unwrap();
        let mut second_mr = gyre_domain::MergeRequest::new(
            Id::new("mr-merged-2"),
            repo.id.clone(),
            "second landed work",
            "feat/merged-2",
            "main",
            1000,
        );
        second_mr.workspace_id = Id::new("ws-1");
        second_mr.author_agent_id = Some(Id::new("agent-merged-2"));
        state.merge_requests.create(&second_mr).await.unwrap();

        *jj.rebase_outcome.lock() = Some(Ok(JjRebaseOutcome::Success { rebased_count: 1 }));
        enqueue_mr(&state, "mr-merged-2", 100, 2000).await;
        process_next(&state).await.unwrap();

        // The clean rebase really ran (not deferred away).
        assert_eq!(
            jj.rebase_calls.lock().len(),
            2,
            "precondition: the second movement must rebase the agent again"
        );
        let cleared = state
            .merge_requests
            .find_by_id(&inflight_mr.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            cleared.has_conflicts,
            Some(false),
            "a clean rebase must clear the stale conflict flag (R19 F3)"
        );
    }

    /// Safeguard (plan item 5): Dead/Completed agents are never rebased —
    /// their in-progress work no longer exists.
    #[tokio::test]
    async fn rebase_skips_dead_agents() {
        let (state, jj, _repo, _mr) = setup_rebase_fixture().await;

        let mut dead = state
            .agents
            .find_by_id(&Id::new("agent-inflight"))
            .await
            .unwrap()
            .unwrap();
        dead.status = AgentStatus::Dead;
        state.agents.update(&dead).await.unwrap();

        enqueue_mr(&state, "mr-merged", 100, 1000).await;
        process_next(&state).await.unwrap();

        let calls = jj.rebase_calls.lock();
        assert!(
            calls.is_empty(),
            "dead agent must not be rebased, got {calls:?}"
        );
    }

    /// R12 F1 (trigger path): the worktree-selection fallback is
    /// repo-scoped. An Active agent with an Open MR in repo A whose
    /// (repo A, source_branch) worktree record is absent — deleted after
    /// MR creation, or the branch record drifted — while an on-disk
    /// repo-B worktree exists must NOT have the repo-B worktree rebased
    /// when a merge lands on repo A's main: the agent's other-repo
    /// in-flight work is unrelated to this repo's baseline movement.
    /// Pre-fix, `.or_else(|| worktrees.first())` picked any worktree and
    /// rebased repo B's `@` onto repo B's same-named branch while telling
    /// the agent `baseline_moved` with repo A's new base SHA.
    #[tokio::test]
    async fn rebase_skips_agent_with_only_other_repo_worktree() {
        let (state, jj, _repo, _inflight_mr) = setup_rebase_fixture().await;

        // The agent's (repo A, feat/inflight) worktree record is gone...
        state
            .worktrees
            .delete(&Id::new("wt-inflight"))
            .await
            .unwrap();
        // ...but the agent has an on-disk worktree in repo B (same branch
        // name — the near-universal-name hazard from the finding).
        let other_repo = create_repo_in_workspace(&state, "rebase-other-repo-f1a", "ws-1").await;
        let other_dir = tempfile::tempdir().unwrap();
        let other_wt = AgentWorktree::new(
            Id::new("wt-inflight-other-repo-f1a"),
            Id::new("agent-inflight"),
            other_repo.id.clone(),
            None,
            "feat/inflight",
            other_dir.path().to_str().unwrap().to_string(),
            1000,
        );
        state.worktrees.create(&other_wt).await.unwrap();
        std::mem::forget(other_dir);

        let mut rx = state.message_broadcast_tx.subscribe();
        enqueue_mr(&state, "mr-merged", 100, 1000).await;
        process_next(&state).await.unwrap();

        let calls = jj.rebase_calls.lock();
        assert!(
            calls.is_empty(),
            "no worktree in the moved repo -> no rebase; the other repo's worktree must not be touched, got {calls:?}"
        );
        drop(calls);

        // And no false baseline_moved for repo A's movement.
        let mut agent_notified = false;
        while let Ok(msg) = rx.try_recv() {
            if let gyre_common::message::Destination::Agent(a) = &msg.to {
                if a.as_str() == "agent-inflight" && msg.kind.as_str() == "baseline_moved" {
                    agent_notified = true;
                }
            }
        }
        assert!(
            !agent_notified,
            "agent must not be told baseline_moved for a repo it has no worktree in"
        );
    }

    /// R12 F1 (trigger path, positive pin): when the (repo,
    /// source_branch) worktree record is absent but the agent has another
    /// worktree in the SAME repo, the fallback rebases that same-repo
    /// worktree — never a different repo's, regardless of store iteration
    /// order (the mem store is a HashMap).
    #[tokio::test]
    async fn rebase_worktree_fallback_stays_within_moved_repo() {
        let (state, jj, repo, _inflight_mr) = setup_rebase_fixture().await;

        // Replace the fixture's worktree record with a drifted-branch
        // record in the SAME repo, and add a same-named worktree in
        // repo B — the exact ambiguity the fallback must resolve by
        // repository scoping.
        state
            .worktrees
            .delete(&Id::new("wt-inflight"))
            .await
            .unwrap();
        let drift_dir = tempfile::tempdir().unwrap();
        let drift_wt = AgentWorktree::new(
            Id::new("wt-inflight-drift"),
            Id::new("agent-inflight"),
            repo.id.clone(),
            None,
            "feat/renamed",
            drift_dir.path().to_str().unwrap().to_string(),
            1000,
        );
        state.worktrees.create(&drift_wt).await.unwrap();
        std::mem::forget(drift_dir);

        let other_repo = create_repo_in_workspace(&state, "rebase-other-repo-f1b", "ws-1").await;
        let other_dir = tempfile::tempdir().unwrap();
        let other_wt = AgentWorktree::new(
            Id::new("wt-inflight-other-repo-f1b"),
            Id::new("agent-inflight"),
            other_repo.id.clone(),
            None,
            "feat/inflight",
            other_dir.path().to_str().unwrap().to_string(),
            1000,
        );
        state.worktrees.create(&other_wt).await.unwrap();
        std::mem::forget(other_dir);

        enqueue_mr(&state, "mr-merged", 100, 1000).await;
        process_next(&state).await.unwrap();

        let calls = jj.rebase_calls.lock();
        assert_eq!(
            calls.len(),
            1,
            "the same-repo fallback worktree must be rebased, got {calls:?}"
        );
        assert_eq!(
            calls[0].0,
            drift_wt.path.clone(),
            "the rebase must target the moved repo's worktree, not the other repo's"
        );
        assert_eq!(calls[0].2, "main");
    }

    /// R12 F1 (replay path): a pending rebase names its repo. When the
    /// agent's only remaining worktree is in a DIFFERENT repo, the replay
    /// drops the marker instead of rebasing the other repo's worktree.
    /// Pre-fix, `.or_else(|| worktrees.first())` replayed the deferral
    /// against whatever worktree existed.
    #[tokio::test]
    async fn replay_drops_pending_rebase_when_worktree_left_the_repo() {
        let (state, jj, _repo, _mr) = setup_rebase_fixture().await;

        // First movement: rebase runs, window starts.
        enqueue_mr(&state, "mr-merged", 100, 1000).await;
        process_next(&state).await.unwrap();
        assert_eq!(jj.rebase_calls.lock().len(), 1);

        // The agent's worktree in the pending repo is gone; only an
        // on-disk repo-B worktree remains.
        state
            .worktrees
            .delete(&Id::new("wt-inflight"))
            .await
            .unwrap();
        let other_repo = create_repo_in_workspace(&state, "rebase-other-repo-f1c", "ws-1").await;
        let other_dir = tempfile::tempdir().unwrap();
        let other_wt = AgentWorktree::new(
            Id::new("wt-inflight-other-repo-f1c"),
            Id::new("agent-inflight"),
            other_repo.id.clone(),
            None,
            "feat/inflight",
            other_dir.path().to_str().unwrap().to_string(),
            1000,
        );
        state.worktrees.create(&other_wt).await.unwrap();
        std::mem::forget(other_dir);

        // Defer a second movement (in-window), then expire the window.
        let pending = PendingRebase {
            repo_id: repo_id_of(&state).await.to_string(),
            target_branch: "main".to_string(),
            new_base_sha: "b".repeat(40),
        };
        state
            .kv_store
            .kv_set(
                JJ_REBASE_PENDING_NS,
                "agent-inflight",
                serde_json::to_string(&pending).unwrap(),
            )
            .await
            .unwrap();
        let now = crate::jobs::now_secs();
        state
            .kv_store
            .kv_set(
                JJ_REBASE_BACKOFF_NS,
                "agent-inflight",
                (now - JJ_REBASE_WINDOW_SECS - 1).to_string(),
            )
            .await
            .unwrap();

        process_next(&state).await.unwrap();

        assert_eq!(
            jj.rebase_calls.lock().len(),
            1,
            "the replay must not rebase another repo's worktree"
        );
        assert!(
            state
                .kv_store
                .kv_get(JJ_REBASE_PENDING_NS, "agent-inflight")
                .await
                .unwrap()
                .is_none(),
            "a deferral with no worktree in the pending repo is not actionable and must be dropped"
        );
    }

    /// F4: a failed rebase must not consume the backoff window — the next
    /// movement inside the window still attempts a rebase instead of
    /// silently deferring forever on a broken workspace.
    #[tokio::test]
    async fn rebase_failure_does_not_consume_backoff_window() {
        let (state, jj, _repo, _mr) = setup_rebase_fixture().await;
        *jj.rebase_outcome.lock() = Some(Err("workspace missing".to_string()));

        enqueue_mr(&state, "mr-merged", 100, 1000).await;
        process_next(&state).await.unwrap();
        assert_eq!(jj.rebase_calls.lock().len(), 1);

        // No backoff was persisted for the failed rebase.
        assert!(
            state
                .kv_store
                .kv_get(JJ_REBASE_BACKOFF_NS, "agent-inflight")
                .await
                .unwrap()
                .is_none(),
            "failed rebase must not write the backoff window"
        );

        // Program success and move the target again immediately: the
        // rebase must be attempted (no window to suppress it).
        *jj.rebase_outcome.lock() = Some(Ok(JjRebaseOutcome::Success { rebased_count: 1 }));
        let mut second_mr = gyre_domain::MergeRequest::new(
            Id::new("mr-merged-2"),
            state
                .repos
                .list()
                .await
                .unwrap()
                .into_iter()
                .find(|r| r.name == "rebase-repo")
                .unwrap()
                .id,
            "second landed work",
            "feat/merged2",
            "main",
            1001,
        );
        second_mr.workspace_id = Id::new("ws-1");
        second_mr.author_agent_id = Some(Id::new("agent-merged"));
        state.merge_requests.create(&second_mr).await.unwrap();
        enqueue_mr(&state, "mr-merged-2", 100, 1001).await;
        process_next(&state).await.unwrap();
        assert_eq!(
            jj.rebase_calls.lock().len(),
            2,
            "second movement must attempt a rebase — failure consumed no window"
        );
    }

    /// Plan item 5 (batch semantics): a movement inside the backoff window
    /// is deferred via the pending marker, not dropped; once the window
    /// expires the replay runs it on an idle processor cycle.
    #[tokio::test]
    async fn rebase_window_hit_defers_then_replays_after_expiry() {
        let (state, jj, _repo, _mr) = setup_rebase_fixture().await;

        // First movement: rebase runs, window starts.
        enqueue_mr(&state, "mr-merged", 100, 1000).await;
        process_next(&state).await.unwrap();
        assert_eq!(jj.rebase_calls.lock().len(), 1);

        // Simulate a second movement inside the window: record the pending
        // marker exactly as rebase_one_agent's window-hit path does.
        let pending = PendingRebase {
            repo_id: state
                .repos
                .list()
                .await
                .unwrap()
                .into_iter()
                .find(|r| r.name == "rebase-repo")
                .unwrap()
                .id
                .to_string(),
            target_branch: "main".to_string(),
            new_base_sha: "f".repeat(40),
        };
        state
            .kv_store
            .kv_set(
                JJ_REBASE_PENDING_NS,
                "agent-inflight",
                serde_json::to_string(&pending).unwrap(),
            )
            .await
            .unwrap();

        // Idle cycle while still inside the window: replay must NOT run
        // (kv marker intact).
        process_next(&state).await.unwrap();
        assert_eq!(
            jj.rebase_calls.lock().len(),
            1,
            "inside the window the replay must not run"
        );
        assert!(
            state
                .kv_store
                .kv_get(JJ_REBASE_PENDING_NS, "agent-inflight")
                .await
                .unwrap()
                .is_some(),
            "pending marker must survive the in-window idle cycle"
        );

        // Expire the window: backdate the backoff timestamp.
        let now = crate::jobs::now_secs();
        state
            .kv_store
            .kv_set(
                JJ_REBASE_BACKOFF_NS,
                "agent-inflight",
                (now - JJ_REBASE_WINDOW_SECS - 1).to_string(),
            )
            .await
            .unwrap();
        process_next(&state).await.unwrap();

        // The deferred rebase replayed and the marker was consumed.
        assert_eq!(
            jj.rebase_calls.lock().len(),
            2,
            "expired window must replay the deferred rebase"
        );
        assert!(
            state
                .kv_store
                .kv_get(JJ_REBASE_PENDING_NS, "agent-inflight")
                .await
                .unwrap()
                .is_none(),
            "successful replay must consume the pending marker"
        );
    }

    /// F12: the replay of expired deferrals must not depend on the merge
    /// queue being empty. Under sustained queue load — source-control.md
    /// §4's "20+ agents pushing concurrently" scenario — the queue never
    /// fully drains; idle-only replay starves the batch-retry guarantee.
    #[tokio::test]
    async fn replay_pending_rebase_runs_under_sustained_queue_load() {
        let (state, jj, _repo, _mr) = setup_rebase_fixture().await;

        // First movement: rebase runs, window starts.
        enqueue_mr(&state, "mr-merged", 100, 1000).await;
        process_next(&state).await.unwrap();
        assert_eq!(jj.rebase_calls.lock().len(), 1);

        // Defer a second movement (in-window), then expire the window.
        let pending = PendingRebase {
            repo_id: state
                .repos
                .list()
                .await
                .unwrap()
                .into_iter()
                .find(|r| r.name == "rebase-repo")
                .unwrap()
                .id
                .to_string(),
            target_branch: "main".to_string(),
            new_base_sha: "a".repeat(40),
        };
        state
            .kv_store
            .kv_set(
                JJ_REBASE_PENDING_NS,
                "agent-inflight",
                serde_json::to_string(&pending).unwrap(),
            )
            .await
            .unwrap();
        let now = crate::jobs::now_secs();
        state
            .kv_store
            .kv_set(
                JJ_REBASE_BACKOFF_NS,
                "agent-inflight",
                (now - JJ_REBASE_WINDOW_SECS - 1).to_string(),
            )
            .await
            .unwrap();

        // Keep the merge queue permanently NON-empty: an MR blocked on a
        // dependency that never merges is never ready, so every cycle
        // takes the busy path. Pre-fix, replay only ran on idle cycles and
        // the expired deferral starved until the queue drained.
        create_mr_with_deps(&state, "mr-blocked", "ws-1", vec!["mr-never-lands"]).await;
        enqueue_mr(&state, "mr-blocked", 50, 1002).await;

        process_next(&state).await.unwrap();

        // The queue really stayed busy: the blocked entry is still Queued.
        let queue = state.merge_queue.list_queue().await.unwrap();
        assert!(
            queue
                .iter()
                .any(|e| e.merge_request_id.as_str() == "mr-blocked"
                    && e.status == MergeQueueEntryStatus::Queued),
            "fixture must keep the merge queue busy (blocked entry still queued)"
        );

        // The expired deferral replayed anyway (F12), and the marker was
        // consumed by the terminal outcome.
        assert_eq!(
            jj.rebase_calls.lock().len(),
            2,
            "expired pending rebase must replay under sustained queue load (F12)"
        );
        assert!(
            state
                .kv_store
                .kv_get(JJ_REBASE_PENDING_NS, "agent-inflight")
                .await
                .unwrap()
                .is_none(),
            "successful replay must consume the pending marker"
        );
    }

    /// F8: an infrastructure failure during REPLAY must keep the pending
    /// marker — the deferral guarantee holds on every consumer of the
    /// marker, not just the trigger path.
    #[tokio::test]
    async fn replay_failure_keeps_pending_rebase() {
        let (state, jj, _repo, _mr) = setup_rebase_fixture().await;

        // First movement: rebase runs, window starts.
        enqueue_mr(&state, "mr-merged", 100, 1000).await;
        process_next(&state).await.unwrap();
        assert_eq!(jj.rebase_calls.lock().len(), 1);

        // Defer a second movement (in-window), then expire the window.
        let pending = PendingRebase {
            repo_id: state
                .repos
                .list()
                .await
                .unwrap()
                .into_iter()
                .find(|r| r.name == "rebase-repo")
                .unwrap()
                .id
                .to_string(),
            target_branch: "main".to_string(),
            new_base_sha: "e".repeat(40),
        };
        state
            .kv_store
            .kv_set(
                JJ_REBASE_PENDING_NS,
                "agent-inflight",
                serde_json::to_string(&pending).unwrap(),
            )
            .await
            .unwrap();
        let now = crate::jobs::now_secs();
        state
            .kv_store
            .kv_set(
                JJ_REBASE_BACKOFF_NS,
                "agent-inflight",
                (now - JJ_REBASE_WINDOW_SECS - 1).to_string(),
            )
            .await
            .unwrap();

        // The replay attempt itself fails (infrastructure error).
        *jj.rebase_outcome.lock() = Some(Err("jj binary vanished".to_string()));
        process_next(&state).await.unwrap();
        assert_eq!(
            jj.rebase_calls.lock().len(),
            2,
            "the replay must have been attempted"
        );
        assert!(
            state
                .kv_store
                .kv_get(JJ_REBASE_PENDING_NS, "agent-inflight")
                .await
                .unwrap()
                .is_some(),
            "a failed replay must keep the pending marker (F8)"
        );

        // Recovery: the next cycle retries and consumes it on success.
        *jj.rebase_outcome.lock() = Some(Ok(JjRebaseOutcome::Success { rebased_count: 1 }));
        process_next(&state).await.unwrap();
        assert_eq!(jj.rebase_calls.lock().len(), 3);
        assert!(
            state
                .kv_store
                .kv_get(JJ_REBASE_PENDING_NS, "agent-inflight")
                .await
                .unwrap()
                .is_none(),
            "successful retry must consume the pending marker"
        );
    }

    /// F3: a post-merge gate failure reverts the merge — no agent may be
    /// rebased onto the base that recovery just undid.
    #[tokio::test]
    async fn post_merge_gate_failure_skips_inflight_rebase() {
        let (state, jj, _repo, _mr) = setup_rebase_fixture().await;

        // Required post-merge gate that always fails.
        create_post_merge_gate(&state, &repo_id_of(&state).await, "false", true).await;

        enqueue_mr(&state, "mr-merged", 100, 1000).await;
        process_next(&state).await.unwrap();

        let calls = jj.rebase_calls.lock();
        assert!(
            calls.is_empty(),
            "gate failure + revert must skip the rebase, got {calls:?}"
        );
    }

    /// Path of the in-flight agent's recorded worktree (helper for call
    /// assertions).
    async fn worktree_path_of(state: &AppState, agent_id: &str) -> String {
        state
            .worktrees
            .find_by_agent(&Id::new(agent_id))
            .await
            .unwrap()
            .first()
            .unwrap()
            .path
            .clone()
    }

    /// Id of the fixture's rebase repo (helper for gate creation).
    async fn repo_id_of(state: &AppState) -> Id {
        state
            .repos
            .list()
            .await
            .unwrap()
            .into_iter()
            .find(|r| r.name == "rebase-repo")
            .unwrap()
            .id
    }

    /// Spec Approval Ledger regression (agent-gates.md §Forge Enforcement
    /// Policies): `require_current_spec` must block merges whose spec_ref SHA
    /// is not the current HEAD blob SHA.
    ///
    /// Spec refs carry ledger paths ("system/foo.md") while the file lives at
    /// "specs/system/foo.md" in the repo; without the specs/-prefix fallback
    /// the blob SHA never resolved, `is_stale` was always false, and the
    /// policy silently enforced nothing.
    #[tokio::test(flavor = "multi_thread")]
    async fn require_current_spec_blocks_stale_spec_ref() {
        let state = test_state();

        // Real repo whose HEAD contains the spec file at a known blob SHA.
        let tmp = tempfile::TempDir::new().unwrap();
        let repo_path = tmp.path().join("stale-spec-repo");
        std::fs::create_dir_all(repo_path.join("specs/system")).unwrap();
        std::fs::write(repo_path.join("specs/system/identity.md"), "# Identity\n").unwrap();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(&repo_path)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        git(&["init", "-b", "main"]);
        git(&["config", "user.email", "test@test.com"]);
        git(&["config", "user.name", "Test"]);
        git(&["add", "."]);
        git(&["commit", "-m", "spec v1"]);
        let head_blob = git(&["rev-parse", "HEAD:specs/system/identity.md"]);

        let repo = Repository::new(
            Id::new("repo-stale-spec"),
            Id::new("ws-1"),
            "stale-spec-repo",
            repo_path.to_str().unwrap(),
            0,
        );
        state.repos.create(&repo).await.unwrap();

        // Enable the blocking policy.
        state
            .spec_policies
            .set_for_repo(
                repo.id.as_str(),
                gyre_domain::SpecPolicy {
                    require_current_spec: true,
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        // MR referencing a LEDGER path ("system/identity.md", as spawn.rs
        // builds from the spec ledger) with a stale SHA.
        let stale_sha = "e".repeat(40);
        let mut mr = gyre_domain::MergeRequest::new(
            Id::new("mr-stale-spec"),
            repo.id.clone(),
            "Stale spec MR",
            "feat/stale",
            "main",
            1000,
        );
        mr.workspace_id = Id::new("ws-1");
        mr.spec_ref = Some(format!("system/identity.md@{stale_sha}"));
        state.merge_requests.create(&mr).await.unwrap();
        let entry = MergeQueueEntry::new(Id::new("entry-stale-spec"), mr.id.clone(), 50, 1000);
        state.merge_queue.enqueue(&entry).await.unwrap();

        run_once(&state).await.unwrap();

        // The stale entry must have reached the exact terminal status Failed.
        // Absence from list_queue() alone cannot prove this — Merged is also
        // terminal and leaves the queue identically, so a regression that
        // silently merges the stale MR would pass an absence assertion
        // (finding dcf03cee mutation probe).
        let entry_after = state
            .merge_queue
            .find_by_id(&entry.id)
            .await
            .unwrap()
            .expect("stale entry persists after processing");
        assert_eq!(
            entry_after.status,
            MergeQueueEntryStatus::Failed,
            "stale spec_ref must block the merge with a Failed entry"
        );
        let err = entry_after
            .error_message
            .as_deref()
            .expect("stale-spec failure carries a reason");
        assert!(
            err.contains("requires current spec"),
            "failure reason must be the stale-spec policy, got: {err}"
        );

        // Control: an MR referencing the CURRENT blob SHA must proceed past
        // the spec check and complete the merge (Merged, not merely
        // not-Failed — the git-ops double always succeeds, so only the
        // stale-spec policy could hold this entry back).
        let mut mr2 = gyre_domain::MergeRequest::new(
            Id::new("mr-current-spec"),
            repo.id.clone(),
            "Current spec MR",
            "feat/current",
            "main",
            1001,
        );
        mr2.workspace_id = Id::new("ws-1");
        mr2.spec_ref = Some(format!("system/identity.md@{head_blob}"));
        state.merge_requests.create(&mr2).await.unwrap();
        let entry2 = MergeQueueEntry::new(Id::new("entry-current-spec"), mr2.id.clone(), 50, 1001);
        state.merge_queue.enqueue(&entry2).await.unwrap();

        run_once(&state).await.unwrap();

        let entry2_after = state
            .merge_queue
            .find_by_id(&entry2.id)
            .await
            .unwrap()
            .expect("entry survives processing");
        assert_eq!(
            entry2_after.status,
            MergeQueueEntryStatus::Merged,
            "current spec_ref must not be blocked by require_current_spec (error: {:?})",
            entry2_after.error_message
        );
    }

    /// agent-gates.md §Forge Enforcement Policies, `require_signed_approval`:
    /// "Spec approvals must include Sigstore signature." An MR whose
    /// referenced spec SHA has only an unsigned active approval must be
    /// blocked at merge, the failure must reach the queue entry with a clear
    /// reason, and the author must be notified. A signed approval merges.
    #[tokio::test(flavor = "multi_thread")]
    async fn require_signed_approval_blocks_unsigned_spec_approval() {
        let state = test_state();
        let repo = create_repo_in_workspace(&state, "signed-spec-repo", "ws-1").await;

        // Enable the policy.
        state
            .spec_policies
            .set_for_repo(
                repo.id.as_str(),
                gyre_domain::SpecPolicy {
                    require_signed_approval: true,
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let sha = "a1".repeat(20);

        // Ledger entry for the referenced version: approved but UNSIGNED.
        let unsigned_id = Id::new("apr-unsigned");
        state
            .spec_approvals
            .create(&gyre_domain::SpecApproval::new(
                unsigned_id.clone(),
                "system/signed.md",
                &sha,
                "user:test",
            ))
            .await
            .unwrap();
        state
            .spec_approvals
            .approve(&unsigned_id, 1700000100)
            .await
            .unwrap()
            .expect("entry exists");

        let mut mr = gyre_domain::MergeRequest::new(
            Id::new("mr-unsigned-spec"),
            repo.id.clone(),
            "Unsigned spec approval MR",
            "feat/unsigned",
            "main",
            2000,
        );
        mr.workspace_id = Id::new("ws-1");
        mr.author_agent_id = Some(Id::new("author-agent-1"));
        mr.spec_ref = Some(format!("system/signed.md@{sha}"));
        state.merge_requests.create(&mr).await.unwrap();
        let entry = MergeQueueEntry::new(Id::new("entry-unsigned-spec"), mr.id.clone(), 50, 2000);
        state.merge_queue.enqueue(&entry).await.unwrap();

        run_once(&state).await.unwrap();

        let entry_after = state
            .merge_queue
            .find_by_id(&entry.id)
            .await
            .unwrap()
            .expect("unsigned entry persists after processing");
        assert_eq!(
            entry_after.status,
            MergeQueueEntryStatus::Failed,
            "unsigned approval must block the merge with a Failed entry"
        );
        let err = entry_after
            .error_message
            .as_deref()
            .expect("unsigned-approval failure carries a reason");
        assert!(
            err.contains("no Sigstore signature"),
            "failure reason must name the missing signature, got: {err}"
        );

        // The author was notified directly (agent-gates.md: enforcement
        // errors are reported clearly to the author, not just the queue).
        let author_notifications = state
            .notifications
            .list_for_user(
                &Id::new("author-agent-1"),
                None,
                None,
                None,
                None,
                None,
                50,
                0,
            )
            .await
            .unwrap();
        assert!(
            author_notifications
                .iter()
                .any(|n| n.title.contains("Spec policy blocked merge")
                    && n.title.contains("no Sigstore signature")),
            "author must receive a notification naming the violation, got: {:?}",
            author_notifications
                .iter()
                .map(|n| &n.title)
                .collect::<Vec<_>>()
        );

        // Control: the same MR with a SIGNED active approval merges.
        let sha2 = "b2".repeat(20);
        let signed_id = Id::new("apr-signed");
        let mut signed_entry = gyre_domain::SpecApproval::new(
            signed_id.clone(),
            "system/signed.md",
            &sha2,
            "user:test",
        );
        signed_entry.signature = Some("sigstore:test-signature".to_string());
        state.spec_approvals.create(&signed_entry).await.unwrap();
        state
            .spec_approvals
            .approve(&signed_id, 1700000200)
            .await
            .unwrap()
            .expect("entry exists");

        let mut mr2 = gyre_domain::MergeRequest::new(
            Id::new("mr-signed-spec"),
            repo.id.clone(),
            "Signed spec approval MR",
            "feat/signed",
            "main",
            2001,
        );
        mr2.workspace_id = Id::new("ws-1");
        mr2.spec_ref = Some(format!("system/signed.md@{sha2}"));
        state.merge_requests.create(&mr2).await.unwrap();
        let entry2 = MergeQueueEntry::new(Id::new("entry-signed-spec"), mr2.id.clone(), 50, 2001);
        state.merge_queue.enqueue(&entry2).await.unwrap();

        run_once(&state).await.unwrap();

        let entry2_after = state
            .merge_queue
            .find_by_id(&entry2.id)
            .await
            .unwrap()
            .expect("entry survives processing");
        assert_eq!(
            entry2_after.status,
            MergeQueueEntryStatus::Merged,
            "signed approval must not be blocked by require_signed_approval (error: {:?})",
            entry2_after.error_message
        );
    }

    /// `warn_stale_spec` (agent-gates.md §Forge Enforcement Policies): a
    /// stale spec_ref warns but does not block — the warning must land as an
    /// advisory MR comment the author can see, and the merge must proceed.
    #[tokio::test(flavor = "multi_thread")]
    async fn warn_stale_spec_adds_advisory_comment_without_blocking() {
        let state = test_state();

        // Real repo whose HEAD contains the spec file, then moves past it.
        let tmp = tempfile::TempDir::new().unwrap();
        let repo_path = tmp.path().join("warn-stale-repo");
        std::fs::create_dir_all(repo_path.join("specs/system")).unwrap();
        std::fs::write(
            repo_path.join("specs/system/identity.md"),
            "# Identity v1\n",
        )
        .unwrap();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(&repo_path)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        git(&["init", "-b", "main"]);
        git(&["config", "user.email", "test@test.com"]);
        git(&["config", "user.name", "Test"]);
        git(&["add", "."]);
        git(&["commit", "-m", "spec v1"]);
        let v1_blob = git(&["rev-parse", "HEAD:specs/system/identity.md"]);

        // Move HEAD past the referenced version.
        std::fs::write(
            repo_path.join("specs/system/identity.md"),
            "# Identity v2\n",
        )
        .unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "spec v2"]);

        // Seed workspace + repo with the real path.
        if state
            .workspaces
            .find_by_id(&Id::new("ws-1"))
            .await
            .unwrap()
            .is_none()
        {
            let ws = gyre_domain::Workspace::new(Id::new("ws-1"), Id::new("t1"), "ws-1", "ws-1", 0);
            state.workspaces.create(&ws).await.unwrap();
        }
        let repo = Repository::new(
            Id::new("repo-warn-stale"),
            Id::new("ws-1"),
            "warn-stale-repo",
            repo_path.to_str().unwrap(),
            0,
        );
        state.repos.create(&repo).await.unwrap();

        // Warn-only policy.
        state
            .spec_policies
            .set_for_repo(
                repo.id.as_str(),
                gyre_domain::SpecPolicy {
                    warn_stale_spec: true,
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let mut mr = gyre_domain::MergeRequest::new(
            Id::new("mr-warn-stale"),
            repo.id.clone(),
            "Stale spec (warn) MR",
            "feat/warn",
            "main",
            3000,
        );
        mr.workspace_id = Id::new("ws-1");
        mr.spec_ref = Some(format!("system/identity.md@{v1_blob}"));
        state.merge_requests.create(&mr).await.unwrap();
        let entry = MergeQueueEntry::new(Id::new("entry-warn-stale"), mr.id.clone(), 50, 3000);
        state.merge_queue.enqueue(&entry).await.unwrap();

        run_once(&state).await.unwrap();

        // The merge proceeded despite the stale reference.
        let entry_after = state
            .merge_queue
            .find_by_id(&entry.id)
            .await
            .unwrap()
            .expect("warn entry persists after processing");
        assert_eq!(
            entry_after.status,
            MergeQueueEntryStatus::Merged,
            "warn_stale_spec must not block the merge (error: {:?})",
            entry_after.error_message
        );

        // The advisory comment is on the MR.
        let comments = state.reviews.list_comments(&mr.id).await.unwrap();
        let advisory = comments
            .iter()
            .find(|c| c.author_agent_id == "forge:spec-policy")
            .unwrap_or_else(|| {
                panic!(
                    "warn_stale_spec must add an advisory MR comment, got: {:?}",
                    comments.iter().map(|c| &c.body).collect::<Vec<_>>()
                )
            });
        assert!(
            advisory.body.contains("Stale spec reference"),
            "advisory must describe the drift, got: {}",
            advisory.body
        );
        assert!(advisory.body.contains(&v1_blob));
    }

    // ── TASK-077 / HSI §2 (F5): merge-time ABAC enforcement tests ─────────
    //
    // The merge processor evaluates ABAC with its internal service identity
    // (subject.type "system", subject.id "merge-processor"). In a Supervised
    // workspace the `trust:require-human-mr-review` Deny matches and the
    // merge is held (requeued with a reason, NOT failed) until a human
    // approves the MR via the status endpoint.

    /// Seed a workspace with the trust policies for the given level.
    async fn seed_workspace_with_trust(
        state: &AppState,
        ws_id: &str,
        name: &str,
        level: gyre_domain::TrustLevel,
    ) {
        let ws = gyre_domain::Workspace::new(
            Id::new(ws_id),
            Id::new("tenant-1"),
            name,
            name.to_lowercase(),
            0,
        );
        state.workspaces.create(&ws).await.unwrap();
        for p in gyre_domain::trust_policies_for_level(&level, ws_id, "test") {
            state.policies.create(&p).await.unwrap();
        }
    }

    #[tokio::test]
    async fn supervised_workspace_open_mr_merge_is_held_and_requeued() {
        let state = test_state();

        seed_workspace_with_trust(
            &state,
            "ws-sup",
            "Supervised WS",
            gyre_domain::TrustLevel::Supervised,
        )
        .await;
        let repo = create_repo_in_workspace(&state, "sup-repo", "ws-sup").await;

        let mut mr = gyre_domain::MergeRequest::new(
            Id::new("mr-sup"),
            repo.id.clone(),
            "MR in supervised workspace",
            "feat/sup",
            "main",
            1000,
        );
        mr.workspace_id = Id::new("ws-sup");
        state.merge_requests.create(&mr).await.unwrap();
        enqueue_mr(&state, "mr-sup", 50, 1000).await;

        // Run a merge-processor cycle.
        process_next(&state).await.unwrap();

        // The MR must NOT have been merged.
        let updated = state
            .merge_requests
            .find_by_id(&Id::new("mr-sup"))
            .await
            .unwrap()
            .expect("mr should exist");
        assert_eq!(
            updated.status,
            MrStatus::Open,
            "supervised trust Deny must hold the merge — MR stays Open"
        );

        // The queue entry must be requeued (not failed, not merged) with a
        // reason naming the supervised trust hold.
        let entry = state
            .merge_queue
            .find_by_id(&Id::new("entry-mr-sup"))
            .await
            .unwrap()
            .expect("entry should exist");
        assert_eq!(
            entry.status,
            MergeQueueEntryStatus::Queued,
            "held entry must be requeued so the human-approval path stays live"
        );
        let reason = entry
            .error_message
            .as_deref()
            .unwrap_or_else(|| panic!("requeued entry must carry a hold reason"));
        assert!(
            reason.contains("supervised trust"),
            "hold reason must name supervised trust, got: {reason}"
        );
    }

    #[tokio::test]
    async fn supervised_workspace_approved_mr_merges() {
        let state = test_state();

        seed_workspace_with_trust(
            &state,
            "ws-sup2",
            "Supervised WS 2",
            gyre_domain::TrustLevel::Supervised,
        )
        .await;
        let repo = create_repo_in_workspace(&state, "sup-repo-2", "ws-sup2").await;

        // A human has approved the MR via the status endpoint
        // (Open → Approved is the only path that sets Approved before merge).
        let mut mr = gyre_domain::MergeRequest::new(
            Id::new("mr-sup-approved"),
            repo.id.clone(),
            "Approved MR in supervised workspace",
            "feat/sup-approved",
            "main",
            1000,
        );
        mr.workspace_id = Id::new("ws-sup2");
        mr.transition_status(MrStatus::Approved).unwrap();
        state.merge_requests.create(&mr).await.unwrap();
        enqueue_mr(&state, "mr-sup-approved", 50, 1000).await;

        process_next(&state).await.unwrap();

        let updated = state
            .merge_requests
            .find_by_id(&Id::new("mr-sup-approved"))
            .await
            .unwrap()
            .expect("mr should exist");
        assert_eq!(
            updated.status,
            MrStatus::Merged,
            "human-approved MR must merge in a Supervised workspace"
        );

        let entry = state
            .merge_queue
            .find_by_id(&Id::new("entry-mr-sup-approved"))
            .await
            .unwrap()
            .expect("entry should exist");
        assert_eq!(
            entry.status,
            MergeQueueEntryStatus::Merged,
            "approved MR's queue entry must be Merged"
        );
    }

    #[tokio::test]
    async fn guided_workspace_open_mr_merges() {
        let state = test_state();

        // Guided seeds no trust Deny policies — the merge processor's
        // system identity must pass the ABAC gate.
        seed_workspace_with_trust(
            &state,
            "ws-guided",
            "Guided WS",
            gyre_domain::TrustLevel::Guided,
        )
        .await;
        let repo = create_repo_in_workspace(&state, "guided-repo", "ws-guided").await;

        let mut mr = gyre_domain::MergeRequest::new(
            Id::new("mr-guided"),
            repo.id.clone(),
            "MR in guided workspace",
            "feat/guided",
            "main",
            1000,
        );
        mr.workspace_id = Id::new("ws-guided");
        state.merge_requests.create(&mr).await.unwrap();
        enqueue_mr(&state, "mr-guided", 50, 1000).await;

        process_next(&state).await.unwrap();

        let updated = state
            .merge_requests
            .find_by_id(&Id::new("mr-guided"))
            .await
            .unwrap()
            .expect("mr should exist");
        assert_eq!(
            updated.status,
            MrStatus::Merged,
            "Guided workspace (no trust Deny) must not hold the merge"
        );
    }

    // TASK-077 review F9: the supervised-trust gate must fail CLOSED on a
    // policy-store read failure. The previous implementation degraded
    // `list()` errors to an empty policy set via `unwrap_or_default()`, and
    // because the hold requires an EXPLICIT Deny match, the merge proceeded
    // in a Supervised workspace with the error silently discarded.
    #[tokio::test]
    async fn policy_store_read_failure_holds_supervised_merge_fail_closed() {
        let state = crate::mem::test_state_failing_policy_lists();

        seed_workspace_with_trust(
            &state,
            "ws-sup-fc",
            "Supervised WS fail-closed",
            gyre_domain::TrustLevel::Supervised,
        )
        .await;
        let repo = create_repo_in_workspace(&state, "sup-repo-fc", "ws-sup-fc").await;

        let mut mr = gyre_domain::MergeRequest::new(
            Id::new("mr-sup-fc"),
            repo.id.clone(),
            "MR in supervised workspace with failing policy store",
            "feat/sup-fc",
            "main",
            1000,
        );
        mr.workspace_id = Id::new("ws-sup-fc");
        state.merge_requests.create(&mr).await.unwrap();
        enqueue_mr(&state, "mr-sup-fc", 50, 1000).await;

        // Run a merge-processor cycle with the policy store unreadable.
        process_next(&state).await.unwrap();

        // The MR must NOT have been merged: an unreadable policy store means
        // the trust state cannot be determined, and the gate must not treat
        // "cannot determine" as "no policies" (fail OPEN).
        let updated = state
            .merge_requests
            .find_by_id(&Id::new("mr-sup-fc"))
            .await
            .unwrap()
            .expect("mr should exist");
        assert_eq!(
            updated.status,
            MrStatus::Open,
            "policy-store read failure must fail CLOSED — MR stays Open"
        );

        // The entry must be requeued (not failed, not merged) with a reason
        // naming the fail-closed hold, so the merge retries once the store
        // recovers and the human-approval path stays live.
        let entry = state
            .merge_queue
            .find_by_id(&Id::new("entry-mr-sup-fc"))
            .await
            .unwrap()
            .expect("entry should exist");
        assert_eq!(
            entry.status,
            MergeQueueEntryStatus::Queued,
            "fail-closed hold must requeue, not fail — store recovery must unblock it"
        );
        let reason = entry
            .error_message
            .as_deref()
            .unwrap_or_else(|| panic!("requeued entry must carry a hold reason"));
        assert!(
            reason.contains("supervised trust") && reason.contains("failing closed"),
            "hold reason must name the fail-closed supervised-trust hold, got: {reason}"
        );
    }

    // TASK-077 review F9, sibling direction: fail-closed is NOT
    // Supervised-specific. A read failure must hold even a Guided workspace's
    // merge — the inverse would make "store down" indistinguishable from
    // "no Deny policies" only for the trust-free level, reintroducing the
    // fail-open asymmetry the HTTP middleware does not have.
    #[tokio::test]
    async fn policy_store_read_failure_holds_guided_merge_fail_closed() {
        let state = crate::mem::test_state_failing_policy_lists();

        seed_workspace_with_trust(
            &state,
            "ws-guided-fc",
            "Guided WS fail-closed",
            gyre_domain::TrustLevel::Guided,
        )
        .await;
        let repo = create_repo_in_workspace(&state, "guided-repo-fc", "ws-guided-fc").await;

        let mut mr = gyre_domain::MergeRequest::new(
            Id::new("mr-guided-fc"),
            repo.id.clone(),
            "MR in guided workspace with failing policy store",
            "feat/guided-fc",
            "main",
            1000,
        );
        mr.workspace_id = Id::new("ws-guided-fc");
        state.merge_requests.create(&mr).await.unwrap();
        enqueue_mr(&state, "mr-guided-fc", 50, 1000).await;

        process_next(&state).await.unwrap();

        // With a WORKING store this MR merges (guided_workspace_open_mr_merges);
        // with an unreadable store the gate cannot prove there is no Deny, so
        // it must hold — "cannot determine state" is never "state is fine".
        let updated = state
            .merge_requests
            .find_by_id(&Id::new("mr-guided-fc"))
            .await
            .unwrap()
            .expect("mr should exist");
        assert_eq!(
            updated.status,
            MrStatus::Open,
            "policy-store read failure must hold even without trust Deny policies — fail closed"
        );

        let entry = state
            .merge_queue
            .find_by_id(&Id::new("entry-mr-guided-fc"))
            .await
            .unwrap()
            .expect("entry should exist");
        assert_eq!(
            entry.status,
            MergeQueueEntryStatus::Queued,
            "fail-closed hold must requeue so store recovery unblocks the merge"
        );
    }

    #[tokio::test]
    async fn supervised_trust_holds_atomic_group_members() {
        let state = test_state();

        seed_workspace_with_trust(
            &state,
            "ws-sup3",
            "Supervised WS 3",
            gyre_domain::TrustLevel::Supervised,
        )
        .await;
        let repo = create_repo_in_workspace(&state, "sup-repo-3", "ws-sup3").await;

        create_mr_in_group(
            &state,
            "mr-group-a",
            &repo.id,
            "ws-sup3",
            "bundle-sup",
            "feat/group-a",
            Some("agent-1"),
        )
        .await;
        create_mr_in_group(
            &state,
            "mr-group-b",
            &repo.id,
            "ws-sup3",
            "bundle-sup",
            "feat/group-b",
            Some("agent-1"),
        )
        .await;

        enqueue_mr(&state, "mr-group-a", 50, 1000).await;
        enqueue_mr(&state, "mr-group-b", 50, 1001).await;

        process_next(&state).await.unwrap();

        // No member may merge: the group is held — every member entry stays
        // Queued (never Failed, never Merged) with a reason naming the
        // supervised trust hold. With the selection-loop trust gate the
        // group is skipped before any member is marked Processing, so no
        // rollback is needed; `merge_atomic_group`'s per-member gate remains
        // as defense in depth and produces the same observable state.
        for mr_id in ["mr-group-a", "mr-group-b"] {
            let updated = state
                .merge_requests
                .find_by_id(&Id::new(mr_id))
                .await
                .unwrap()
                .expect("mr should exist");
            assert_eq!(
                updated.status,
                MrStatus::Open,
                "atomic group member {mr_id} must stay Open under supervised trust"
            );

            let entry = state
                .merge_queue
                .find_by_id(&Id::new(format!("entry-{mr_id}")))
                .await
                .unwrap()
                .expect("entry should exist");
            assert_eq!(
                entry.status,
                MergeQueueEntryStatus::Queued,
                "atomic group member {mr_id} must stay queued under the supervised trust hold"
            );
            let reason = entry
                .error_message
                .as_deref()
                .unwrap_or_else(|| panic!("held entry {mr_id} must carry a hold reason"));
            assert!(
                reason.contains("supervised trust"),
                "hold reason must name supervised trust, got: {reason}"
            );
        }
    }

    // ── TASK-077 review repair (finding 6425cc46): queue starvation ──────
    //
    // The trust hold must SKIP to the next candidate inside the selection
    // loop, exactly like gates-pending / repo-paused / group-not-ready. A
    // hold that terminates the cycle instead re-selects the same
    // head-of-queue entry every cycle (the requeue does not rotate
    // enqueued_at, so the (priority DESC, enqueued_at ASC) sort position is
    // stable) and starves every later queue entry — cross-workspace
    // autonomous merges AND same-workspace human-approved MRs (HSI §2/§13's
    // own review-approve-merge escape).

    #[tokio::test]
    async fn trust_hold_does_not_starve_later_autonomous_entry() {
        let state = test_state();

        // Supervised workspace: OLDER queue entry awaiting human approval.
        seed_workspace_with_trust(
            &state,
            "ws-mixed-sup",
            "Mixed Sup WS",
            gyre_domain::TrustLevel::Supervised,
        )
        .await;
        let sup_repo = create_repo_in_workspace(&state, "mixed-sup-repo", "ws-mixed-sup").await;
        let mut sup_mr = gyre_domain::MergeRequest::new(
            Id::new("mr-mixed-sup"),
            sup_repo.id.clone(),
            "Held MR in supervised workspace",
            "feat/mixed-sup",
            "main",
            1000,
        );
        sup_mr.workspace_id = Id::new("ws-mixed-sup");
        state.merge_requests.create(&sup_mr).await.unwrap();
        enqueue_mr(&state, "mr-mixed-sup", 50, 1000).await; // enqueued first

        // Guided workspace: NEWER, merge-ready entry at the same priority.
        seed_workspace_with_trust(
            &state,
            "ws-mixed-guided",
            "Mixed Guided WS",
            gyre_domain::TrustLevel::Guided,
        )
        .await;
        let guided_repo =
            create_repo_in_workspace(&state, "mixed-guided-repo", "ws-mixed-guided").await;
        let mut guided_mr = gyre_domain::MergeRequest::new(
            Id::new("mr-mixed-guided"),
            guided_repo.id.clone(),
            "Merge-ready MR in guided workspace",
            "feat/mixed-guided",
            "main",
            1001,
        );
        guided_mr.workspace_id = Id::new("ws-mixed-guided");
        state.merge_requests.create(&guided_mr).await.unwrap();
        enqueue_mr(&state, "mr-mixed-guided", 50, 1001).await; // enqueued later

        // Several cycles: the held entry must not monopolize the processor.
        for _ in 0..3 {
            process_next(&state).await.unwrap();
        }

        let guided = state
            .merge_requests
            .find_by_id(&Id::new("mr-mixed-guided"))
            .await
            .unwrap()
            .expect("guided mr should exist");
        assert_eq!(
            guided.status,
            MrStatus::Merged,
            "Guided workspace MR queued behind a Supervised hold must still merge \
             (HSI §12: Guided merges if gates pass; the merge queue is shared)"
        );

        let sup = state
            .merge_requests
            .find_by_id(&Id::new("mr-mixed-sup"))
            .await
            .unwrap()
            .expect("supervised mr should exist");
        assert_eq!(
            sup.status,
            MrStatus::Open,
            "supervised MR must stay held for human approval across cycles"
        );
        let entry = state
            .merge_queue
            .find_by_id(&Id::new("entry-mr-mixed-sup"))
            .await
            .unwrap()
            .expect("entry should exist");
        assert_eq!(
            entry.status,
            MergeQueueEntryStatus::Queued,
            "held entry must stay Queued so the human-approval path stays live"
        );
        assert!(
            entry
                .error_message
                .as_deref()
                .unwrap_or_default()
                .contains("supervised trust"),
            "hold reason must name supervised trust, got: {:?}",
            entry.error_message
        );
    }

    #[tokio::test]
    async fn trust_hold_does_not_starve_later_human_approved_entry() {
        let state = test_state();

        seed_workspace_with_trust(
            &state,
            "ws-escape",
            "Escape WS",
            gyre_domain::TrustLevel::Supervised,
        )
        .await;
        let repo = create_repo_in_workspace(&state, "escape-repo", "ws-escape").await;

        // OLDER unapproved MR — held for human review, sorts first.
        let mut held_mr = gyre_domain::MergeRequest::new(
            Id::new("mr-escape-held"),
            repo.id.clone(),
            "Unapproved MR awaiting human review",
            "feat/escape-held",
            "main",
            1000,
        );
        held_mr.workspace_id = Id::new("ws-escape");
        state.merge_requests.create(&held_mr).await.unwrap();
        enqueue_mr(&state, "mr-escape-held", 50, 1000).await;

        // NEWER MR a human has already Approved via the status endpoint —
        // HSI §2/§13's exact escape. It must merge past the held one.
        let mut approved_mr = gyre_domain::MergeRequest::new(
            Id::new("mr-escape-approved"),
            repo.id.clone(),
            "Human-approved MR behind a held entry",
            "feat/escape-approved",
            "main",
            1001,
        );
        approved_mr.workspace_id = Id::new("ws-escape");
        approved_mr.transition_status(MrStatus::Approved).unwrap();
        state.merge_requests.create(&approved_mr).await.unwrap();
        enqueue_mr(&state, "mr-escape-approved", 50, 1001).await;

        for _ in 0..3 {
            process_next(&state).await.unwrap();
        }

        let approved = state
            .merge_requests
            .find_by_id(&Id::new("mr-escape-approved"))
            .await
            .unwrap()
            .expect("approved mr should exist");
        assert_eq!(
            approved.status,
            MrStatus::Merged,
            "human-approved MR queued behind a held entry must merge — \
             the review-approve-merge escape must not stall on an older \
             unapproved MR (HSI §2/§13)"
        );

        let held = state
            .merge_requests
            .find_by_id(&Id::new("mr-escape-held"))
            .await
            .unwrap()
            .expect("held mr should exist");
        assert_eq!(
            held.status,
            MrStatus::Open,
            "unapproved MR must stay held for human approval"
        );
        let entry = state
            .merge_queue
            .find_by_id(&Id::new("entry-mr-escape-held"))
            .await
            .unwrap()
            .expect("entry should exist");
        assert_eq!(
            entry.status,
            MergeQueueEntryStatus::Queued,
            "held entry must stay Queued so the human-approval path stays live"
        );
    }

    #[tokio::test]
    async fn trust_held_atomic_group_does_not_starve_later_entry() {
        let state = test_state();

        // Supervised workspace: a two-member atomic group, both members
        // unapproved — the group is held as a unit.
        seed_workspace_with_trust(
            &state,
            "ws-group-starve",
            "Group Starve WS",
            gyre_domain::TrustLevel::Supervised,
        )
        .await;
        let group_repo =
            create_repo_in_workspace(&state, "group-starve-repo", "ws-group-starve").await;
        create_mr_in_group(
            &state,
            "mr-starve-a",
            &group_repo.id,
            "ws-group-starve",
            "bundle-starve",
            "feat/starve-a",
            Some("agent-1"),
        )
        .await;
        create_mr_in_group(
            &state,
            "mr-starve-b",
            &group_repo.id,
            "ws-group-starve",
            "bundle-starve",
            "feat/starve-b",
            Some("agent-1"),
        )
        .await;
        enqueue_mr(&state, "mr-starve-a", 50, 1000).await;
        enqueue_mr(&state, "mr-starve-b", 50, 1001).await;

        // Guided workspace: NEWER independent entry behind the held group.
        seed_workspace_with_trust(
            &state,
            "ws-group-guided",
            "Group Guided WS",
            gyre_domain::TrustLevel::Guided,
        )
        .await;
        let guided_repo =
            create_repo_in_workspace(&state, "group-guided-repo", "ws-group-guided").await;
        let mut guided_mr = gyre_domain::MergeRequest::new(
            Id::new("mr-starve-guided"),
            guided_repo.id.clone(),
            "Merge-ready MR behind a held atomic group",
            "feat/starve-guided",
            "main",
            1002,
        );
        guided_mr.workspace_id = Id::new("ws-group-guided");
        state.merge_requests.create(&guided_mr).await.unwrap();
        enqueue_mr(&state, "mr-starve-guided", 50, 1002).await;

        for _ in 0..3 {
            process_next(&state).await.unwrap();
        }

        let guided = state
            .merge_requests
            .find_by_id(&Id::new("mr-starve-guided"))
            .await
            .unwrap()
            .expect("guided mr should exist");
        assert_eq!(
            guided.status,
            MrStatus::Merged,
            "Guided MR queued behind a held atomic group must still merge — \
             a held group must not monopolize the shared merge queue"
        );

        for mr_id in ["mr-starve-a", "mr-starve-b"] {
            let updated = state
                .merge_requests
                .find_by_id(&Id::new(mr_id))
                .await
                .unwrap()
                .expect("group member should exist");
            assert_eq!(
                updated.status,
                MrStatus::Open,
                "held group member {mr_id} must stay Open"
            );
            let entry = state
                .merge_queue
                .find_by_id(&Id::new(format!("entry-{mr_id}")))
                .await
                .unwrap()
                .expect("entry should exist");
            assert_eq!(
                entry.status,
                MergeQueueEntryStatus::Queued,
                "held group member {mr_id} must stay Queued"
            );
        }
    }

    // ── Production-seed regression (task-077 revision): the merge gate must
    // evaluate against the SAME policy set a deployed server has after
    // `seed_builtin_policies` — including the tenant-scope catch-all
    // `builtin-default-deny`, which matches every action unconditionally.
    // If the catch-all is left in the gate's evaluation scope, the Guided
    // state (no trust Deny) returns an EXPLICIT Deny match and every
    // autonomous merge is held forever — while empty-store tests stay green.

    #[tokio::test]
    async fn guided_workspace_merge_proceeds_with_startup_seeded_builtins() {
        let state = test_state();
        // Exactly what main.rs does at startup (fail-closed).
        crate::abac_middleware::seed_builtin_policies(&state)
            .await
            .expect("seed built-in policies");

        seed_workspace_with_trust(
            &state,
            "ws-guided-seeded",
            "Guided Seeded WS",
            gyre_domain::TrustLevel::Guided,
        )
        .await;
        let repo = create_repo_in_workspace(&state, "guided-seeded-repo", "ws-guided-seeded").await;

        let mut mr = gyre_domain::MergeRequest::new(
            Id::new("mr-guided-seeded"),
            repo.id.clone(),
            "MR in seeded guided workspace",
            "feat/guided-seeded",
            "main",
            1000,
        );
        mr.workspace_id = Id::new("ws-guided-seeded");
        state.merge_requests.create(&mr).await.unwrap();
        enqueue_mr(&state, "mr-guided-seeded", 50, 1000).await;

        process_next(&state).await.unwrap();

        let updated = state
            .merge_requests
            .find_by_id(&Id::new("mr-guided-seeded"))
            .await
            .unwrap()
            .expect("mr should exist");
        assert_eq!(
            updated.status,
            MrStatus::Merged,
            "pipeline catch-all default-deny must NOT hold Guided autonomous \
             merges (HSI §2: Guided = no trust Deny → processor proceeds)"
        );
        let entry = state
            .merge_queue
            .find_by_id(&Id::new("entry-mr-guided-seeded"))
            .await
            .unwrap()
            .expect("entry should exist");
        assert_eq!(
            entry.status,
            MergeQueueEntryStatus::Merged,
            "queue entry must be Merged, not requeued on the catch-all"
        );
    }

    #[tokio::test]
    async fn supervised_hold_survives_startup_seeded_builtins() {
        let state = test_state();
        crate::abac_middleware::seed_builtin_policies(&state)
            .await
            .expect("seed built-in policies");

        seed_workspace_with_trust(
            &state,
            "ws-sup-seeded",
            "Supervised Seeded WS",
            gyre_domain::TrustLevel::Supervised,
        )
        .await;
        let repo = create_repo_in_workspace(&state, "sup-seeded-repo", "ws-sup-seeded").await;

        let mut mr = gyre_domain::MergeRequest::new(
            Id::new("mr-sup-seeded"),
            repo.id.clone(),
            "MR in seeded supervised workspace",
            "feat/sup-seeded",
            "main",
            1000,
        );
        mr.workspace_id = Id::new("ws-sup-seeded");
        state.merge_requests.create(&mr).await.unwrap();
        enqueue_mr(&state, "mr-sup-seeded", 50, 1000).await;

        process_next(&state).await.unwrap();

        let updated = state
            .merge_requests
            .find_by_id(&Id::new("mr-sup-seeded"))
            .await
            .unwrap()
            .expect("mr should exist");
        assert_eq!(
            updated.status,
            MrStatus::Open,
            "excluding the catch-all must not weaken the trust:require-human-mr-review Deny"
        );
        let entry = state
            .merge_queue
            .find_by_id(&Id::new("entry-mr-sup-seeded"))
            .await
            .unwrap()
            .expect("entry should exist");
        assert_eq!(entry.status, MergeQueueEntryStatus::Queued);
        assert!(
            entry
                .error_message
                .as_deref()
                .unwrap_or_default()
                .contains("supervised trust"),
            "hold must name supervised trust, got: {:?}",
            entry.error_message
        );
    }

    /// user-management.md §Notification Entity: pausing a merge queue
    /// notifies workspace Admins/Owners (MergeQueuePaused, High) with a deep
    /// link to the repo — and nobody else.
    #[tokio::test]
    async fn pause_merge_queue_notifies_admins_and_owners() {
        use gyre_domain::{Workspace, WorkspaceMembership, WorkspaceRole};

        let state = crate::mem::test_state();
        let ws_id = Id::new("ws-pause");
        let ws = Workspace::new(
            ws_id.clone(),
            Id::new("tenant-1"),
            "pause-ws",
            "pause-ws",
            0,
        );
        state.workspaces.create(&ws).await.unwrap();
        let repo = create_repo_in_workspace(&state, "paused-repo", "ws-pause").await;

        let members = [
            ("user-admin", WorkspaceRole::Admin),
            ("user-owner", WorkspaceRole::Owner),
            ("user-dev", WorkspaceRole::Developer),
            ("user-viewer", WorkspaceRole::Viewer),
        ];
        for (uid, role) in members {
            let m = WorkspaceMembership::new(
                Id::new(uuid::Uuid::new_v4().to_string()),
                Id::new(uid),
                ws_id.clone(),
                role,
                Id::new("inviter"),
                0,
            );
            state.workspace_memberships.create(&m).await.unwrap();
        }

        pause_merge_queue(&state, &repo, "post-merge validation failed").await;

        for uid in ["user-admin", "user-owner"] {
            let notifs = state
                .notifications
                .list_for_user(&Id::new(uid), Some(&ws_id), None, None, None, None, 10, 0)
                .await
                .unwrap();
            assert_eq!(notifs.len(), 1, "{uid} must be notified");
            let n = &notifs[0];
            assert_eq!(
                n.notification_type,
                gyre_common::NotificationType::MergeQueuePaused
            );
            assert_eq!(
                n.priority,
                gyre_common::NotificationPriority::High,
                "MergeQueuePaused is High per user-management.md"
            );
            assert_eq!(n.source.entity_type, "repo");
            assert_eq!(n.source.entity_id, repo.id.as_str());
            assert_eq!(
                n.action_url.as_deref(),
                Some("/workspaces/pause-ws/r/paused-repo"),
                "deep link resolves through the workspace slug + repo name"
            );
        }
        for uid in ["user-dev", "user-viewer"] {
            let notifs = state
                .notifications
                .list_for_user(&Id::new(uid), Some(&ws_id), None, None, None, None, 10, 0)
                .await
                .unwrap();
            assert!(
                notifs.is_empty(),
                "{uid} must not be notified for a queue pause"
            );
        }
    }
}
