//! Spec approval signal chain (agent-runtime.md §1 Phases 1–3).
//!
//! The chain:
//!   human approves spec
//!     → `SpecApproved` message on the bus (Phase 1, emitted in `api/specs.rs`)
//!     → server intercepts the workspace-destined message and routes it through
//!       the [`OrchestratorRegistry`] to the workspace orchestrator (Phase 2)
//!     → workspace orchestrator creates a Delegation task for the spec's repo
//!       and Coordination tasks for dependent repos
//!     → task scheduler detects Delegation/Coordination tasks and runs the
//!       repo orchestrator (Phase 3), which decomposes the spec into ordered
//!       Implementation sub-tasks and completes the delegation task
//!
//! Both orchestrators are real [`gyre_domain::Agent`] entities with scoped
//! JWTs minted by the task-093 spawn cores (`api/orchestrator.rs`); the
//! registry only adds the in-process serialization the spec calls for
//! ("exactly-one-active semantics via a per-workspace mutex"). The
//! DB-based one-live check in the spawn core remains the durable guard —
//! the registry mutex serializes concurrent signals within this process.

/// Age after which a claimed (InProgress) Delegation/Coordination task is
/// considered stale and re-claimed by the scheduler. This is **crash
/// recovery** for a task left InProgress by a dead process — it can never
/// fire mid-run, because a whole scheduler cycle (scan → claim → process)
/// holds the cycle run lock
/// ([`OrchestratorRegistry::scheduler_run_lock`]): no cycle can scan
/// candidates while another is executing, so a live in-flight run is never
/// re-selected. Every orchestrator LLM call is additionally latency-bounded
/// (`LLM_CALL_TIMEOUT` plus the adapter's HTTP client timeout), so an
/// in-flight run always finishes or fails within bounded time and releases
/// the run lock.
const SCHEDULER_CLAIM_STALENESS_SECS: u64 = 60;

/// Latency bound for one orchestrator LLM judgment call (decomposition or
/// coordination assessment). reqwest has no default total timeout, so an
/// unbounded adapter call would hold the scheduler's cycle run lock and the
/// per-repo mutex indefinitely, stalling every later cycle. On timeout the
/// caller degrades to its deterministic fallback — the same handling as an
/// LLM error.
const LLM_CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

use std::collections::HashMap;
use std::sync::Arc;

use gyre_common::message::{Destination, MessageKind};
use gyre_common::Id;
use gyre_domain::{
    Agent, AgentStatus, Notification, NotificationType, OrchestratorType, Repository, Task,
    TaskPriority, TaskStatus, TaskType, WorkspaceRole,
};
use tokio::sync::{Mutex, RwLock};

use crate::AppState;

/// Per-scope tokio mutexes held only for the duration of a signal's
/// processing. Process-local locks, not a durable data store: the registry
/// tracks no agent state — the agents table is the source of truth for
/// which orchestrator is live — so there is nothing to persist and nothing
/// to orphan on restart.
// in-memory-state-stores:ok — process-local serialization locks only;
// agent liveness state lives in the port-backed agents store.
/// Cheap to clone: the maps live behind an `Arc` so every clone observes
/// the same per-scope locks (stable lock identity across clones matters —
/// see the middleware/state clones in tests).
#[derive(Clone, Default)]
pub struct OrchestratorRegistry {
    inner: Arc<RwLock<RegistryInner>>,
    /// Serializes whole task-scheduler cycles (scan → claim → process).
    /// The 30 s loop and the operator trigger
    /// (`POST /api/v1/admin/jobs/task_scheduler/run`, whose
    /// `JobRegistry::trigger` runs the handler without serialization) both
    /// enter through `scheduler_run_once`, which holds this lock for the
    /// entire cycle — so two concurrent cycles can never both claim the
    /// same Backlog task. A plain lock, not durable state: nothing is
    /// stored here.
    cycle_lock: Arc<Mutex<()>>,
}

#[derive(Default)]
struct RegistryInner {
    /// workspace_id → per-workspace mutex (exactly one active workspace
    /// orchestrator processing at a time).
    workspace_locks: HashMap<Id, Arc<Mutex<()>>>,
    /// repo_id → per-repo mutex (exactly one active repo orchestrator
    /// processing at a time).
    repo_locks: HashMap<Id, Arc<Mutex<()>>>,
}

impl OrchestratorRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Acquire the per-workspace processing lock. Concurrent SpecApproved
    /// signals for the same workspace serialize here (agent-runtime.md §1
    /// Phase 2): the second message waits until the first finishes or a new
    /// orchestrator session is spawned.
    pub async fn workspace_lock(&self, workspace_id: &Id) -> Arc<Mutex<()>> {
        // Read-fast path: the lock already exists.
        if let Some(lock) = self.inner.read().await.workspace_locks.get(workspace_id) {
            return lock.clone();
        }
        // Slow path: insert under the write lock, return whichever lock
        // won the race (stable identity per scope either way).
        self.inner
            .write()
            .await
            .workspace_locks
            .entry(workspace_id.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// Acquire the per-repo processing lock (Phase 3): concurrent
    /// Delegation/Coordination tasks for the same repo serialize here.
    pub async fn repo_lock(&self, repo_id: &Id) -> Arc<Mutex<()>> {
        if let Some(lock) = self.inner.read().await.repo_locks.get(repo_id) {
            return lock.clone();
        }
        self.inner
            .write()
            .await
            .repo_locks
            .entry(repo_id.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// The whole-cycle scheduler lock: `scheduler_run_once` acquires this
    /// before scanning candidates and holds it until the cycle completes,
    /// so concurrent cycles (background loop + admin trigger) serialize
    /// instead of racing on the non-atomic Backlog claim. Shared across
    /// clones like the scope locks above.
    pub async fn scheduler_run_lock(&self) -> Arc<Mutex<()>> {
        self.cycle_lock.clone()
    }
}

/// Whether an agent slot counts as occupying its scope (same predicate as
/// the task-093 spawn core; kept local so the registry cannot drift from
/// the DB-based one-live check).
fn is_live(a: &Agent) -> bool {
    !matches!(
        a.status,
        AgentStatus::Dead | AgentStatus::Stopped | AgentStatus::Failed
    )
}

/// Find the live workspace orchestrator for a workspace, if any.
async fn find_live_workspace_orchestrator(
    state: &AppState,
    workspace_id: &Id,
) -> anyhow::Result<Option<Agent>> {
    let peers = state.agents.list_by_workspace(workspace_id).await?;
    Ok(peers
        .into_iter()
        .find(|a| a.orchestrator_type == OrchestratorType::WorkspaceOrchestrator && is_live(a)))
}

/// Find the live repo orchestrator for a repo, if any.
async fn find_live_repo_orchestrator(
    state: &AppState,
    repo: &Repository,
) -> anyhow::Result<Option<Agent>> {
    let peers = state.agents.list_by_workspace(&repo.workspace_id).await?;
    Ok(peers.into_iter().find(|a| {
        a.orchestrator_type == OrchestratorType::RepoOrchestrator
            && a.repo_id.as_ref() == Some(&repo.id)
            && is_live(a)
    }))
}

// ── Phase 1→2: SpecApproved interception ─────────────────────────────────────

/// Intercept a workspace-destined `SpecApproved` message (agent-runtime.md
/// §1 Phase 2 "orchestrator spawn and message routing"): ensure a live
/// workspace orchestrator exists — spawning one through the task-093 core
/// when none does — deliver the message to its inbox, and process it under
/// the per-workspace registry mutex.
///
/// This is an internal server mechanism, not a message bus feature: the
/// bus message (Workspace-destined, emitted by the approval handler) stays
/// on the bus untouched; this hook is invoked from the same handler right
/// after emission.
pub async fn on_spec_approved(state: &Arc<AppState>, payload: &serde_json::Value) {
    // Signal chain only fires for workspace-scoped specs: the ledger entry
    // had no workspace_id (Broadcast destination) means no orchestrator can
    // be scoped to receive it. Mirrors the fallback in the emit side.
    let Some(ws_id_str) = payload
        .get("workspace_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    else {
        tracing::debug!(
            "signal-chain: SpecApproved without workspace_id; skipping orchestrator routing"
        );
        return;
    };
    let workspace_id = Id::new(ws_id_str.to_string());

    // Serialize per workspace: concurrent approvals queue behind the first
    // (the registry mutex holds the "exactly-one-active" semantics).
    let lock = state
        .orchestrator_registry
        .workspace_lock(&workspace_id)
        .await;
    let _guard = lock.lock().await;

    // Ensure a live workspace orchestrator exists (spawn on demand — the
    // spec says spawned on demand, not long-lived).
    let orchestrator = match ensure_workspace_orchestrator(state, &workspace_id).await {
        Ok(o) => o,
        Err(e) => {
            // Budget-exceeded and spawn failures must not take down the
            // approval handler; the signal stays emitted on the bus and a
            // later approval (or an operator spawn) re-enters the chain.
            tracing::warn!(
                workspace_id = %workspace_id,
                "signal-chain: failed to ensure workspace orchestrator: {e:#}"
            );
            return;
        }
    };

    // Deliver the message to the orchestrator's inbox (Directed tier,
    // persisted, ack-based — same delivery the agent polls via
    // GET /api/v1/agents/:id/messages).
    state
        .emit_event(
            workspace_id.clone(),
            Destination::Agent(orchestrator.id.clone()),
            MessageKind::SpecApproved,
            Some(payload.clone()),
        )
        .await;

    // Drive the orchestrator's processing server-side (it "completes after
    // processing its inbox" per the spec).
    if let Err(e) = run_workspace_orchestrator(state, &orchestrator, payload).await {
        tracing::error!(
            orchestrator_id = %orchestrator.id,
            "signal-chain: workspace orchestrator run failed: {e:#}"
        );
    }
}

/// Spawn a workspace orchestrator if none is live, reusing the task-093
/// spawn core (scoped JWT, keypair, budget, restart_on_failure). Returns
/// the live orchestrator either way.
async fn ensure_workspace_orchestrator(
    state: &Arc<AppState>,
    workspace_id: &Id,
) -> anyhow::Result<Agent> {
    if let Some(existing) = find_live_workspace_orchestrator(state, workspace_id).await? {
        return Ok(existing);
    }
    let (agent, _token, _launch) = crate::api::orchestrator::spawn_workspace_orchestrator_core(
        state,
        &workspace_id.to_string(),
        crate::api::orchestrator::SpawnOrchestratorRequest::default(),
        "system",
    )
    .await
    .map_err(|e| anyhow::anyhow!("workspace orchestrator spawn: {e:?}"))?;
    tracing::info!(
        orchestrator_id = %agent.id,
        workspace_id = %workspace_id,
        "signal-chain: spawned workspace orchestrator on SpecApproved"
    );
    Ok(agent)
}

// ── Phase 2: workspace orchestrator processing ───────────────────────────────

/// One workspace-orchestrator run: cross-repo impact analysis and
/// delegation for a SpecApproved signal.
///
/// Reads the approved spec content, queries `spec_links` for cross-repo and
/// cross-workspace dependencies, then creates:
/// - a Delegation task for the spec's own repo (`spec_ref: path@sha`)
/// - a Coordination task per dependent repo
/// - priority-4 `CrossWorkspaceSpecChange` notifications for dependent
///   workspace admins when a dependency crosses workspaces (HSI §8)
///
/// The orchestrator only creates tasks — never spawns agents.
async fn run_workspace_orchestrator(
    state: &AppState,
    orchestrator: &Agent,
    payload: &serde_json::Value,
) -> anyhow::Result<()> {
    let workspace_id = orchestrator.workspace_id.clone();
    let now = crate::api::now_secs();

    let repo_id = payload
        .get("repo_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(Id::new);
    let spec_path = payload
        .get("spec_path")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let spec_sha = payload
        .get("spec_sha")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    // 1. Read the approved spec content (real git object read at the
    //    approved SHA — agent-runtime.md §1 Phase 2 step 1). The content
    //    feeds the delegation task's metadata below (title, section
    //    count, size): the workspace orchestrator's own run only needs
    //    the reference for task creation — the repo orchestrator reads
    //    the full content during decomposition (Phase 3) — so a failed
    //    read here is non-fatal (warned below); Phase 3 re-reads and
    //    surfaces hard failures there.
    let mut spec_summary: Option<String> = None;
    if let (Some(rid), true) = (&repo_id, !spec_sha.is_empty()) {
        if let Ok(Some(repo)) = state.repos.find_by_id(rid).await {
            let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());
            match crate::spec_registry::read_git_file(&git_bin, &repo.path, &spec_sha, &spec_path)
                .await
            {
                Some(content) => {
                    spec_summary = Some(summarize_spec_content(&content));
                }
                None => {
                    tracing::warn!(
                        repo_id = %rid,
                        spec_ref = format!("{spec_path}@{spec_sha}"),
                        "signal-chain: approved spec blob unreadable at delegation time; the repo orchestrator re-reads it during decomposition"
                    );
                }
            }
        }
    }

    // 2. Query spec_links for cross-repo dependencies: links whose target
    //    resolves to this spec's repo. `source_repo_id` is the repo that
    //    depends on us; it may live in another workspace.
    let dependents = {
        let links = state.spec_links_store.lock().await;
        links
            .iter()
            .filter(|l| l.target_repo_id.as_deref() == repo_id.as_ref().map(|r| r.as_str()))
            .filter(|l| l.target_path == spec_path)
            .cloned()
            .collect::<Vec<_>>()
    };

    // 3. Delegation task for the spec's own repo.
    if let Some(rid) = &repo_id {
        let spec_ref = format!("{spec_path}@{spec_sha}");
        let mut task = Task::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            format!("Delegated implementation of {spec_path}"),
            now,
        );
        task.task_type = Some(TaskType::Delegation);
        task.repo_id = rid.clone();
        task.workspace_id = workspace_id.clone();
        // spec_ref pins the approved SHA (agent-runtime.md §1 Phase 2.3):
        // the repo orchestrator decomposes against this exact blob, so a
        // later push (which invalidates the approval) cannot redirect
        // decomposition to unapproved content. spec_path stays the bare
        // path — list_by_spec_path consumers (rejection cancellation,
        // spec progress) match on it exactly.
        task.spec_path = Some(spec_path.clone());
        task.spec_ref = Some(spec_ref.clone());
        task.description = Some(format!(
            "Delegation task created by workspace orchestrator {} for approved spec {spec_ref} (agent-runtime.md §1 Phase 2).{}",
            orchestrator.id,
            spec_summary
                .as_ref()
                .map(|s| format!(" Approved spec: {s}."))
                .unwrap_or_default()
        ));
        task.priority = TaskPriority::High;
        task.assigned_to = Some(orchestrator.id.clone());
        state.tasks.create(&task).await?;
        tracing::info!(
            task_id = %task.id,
            repo_id = %rid,
            spec_ref = %spec_ref,
            "signal-chain: workspace orchestrator created delegation task"
        );
    }

    // 4. Coordination tasks for dependent repos (+ 5. cross-workspace p4
    //    notifications for dependent workspace admins).
    for link in &dependents {
        let Some(dep_repo_id) = link.source_repo_id.clone() else {
            continue;
        };
        let dep_rid = Id::new(dep_repo_id);
        let Ok(Some(dep_repo)) = state.repos.find_by_id(&dep_rid).await else {
            tracing::warn!(
                repo_id = %dep_rid,
                "signal-chain: dependent repo not found; skipping coordination task"
            );
            continue;
        };

        let mut task = Task::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            format!("Coordinate dependency change: {}", link.source_path),
            now,
        );
        task.task_type = Some(TaskType::Coordination);
        task.repo_id = dep_repo.id.clone();
        task.workspace_id = dep_repo.workspace_id.clone();
        task.spec_path = Some(link.source_path.clone());
        task.description = Some(format!(
            "Dependency {} changed (approved as {spec_path}@{spec_sha}). Assess impact: documentation updates, spec amendments, or re-implementation as needed (agent-runtime.md §1 Phase 2).",
            link.source_path
        ));
        task.priority = TaskPriority::High;
        task.assigned_to = Some(orchestrator.id.clone());
        state.tasks.create(&task).await?;
        tracing::info!(
            task_id = %task.id,
            repo_id = %dep_repo.id,
            "signal-chain: workspace orchestrator created coordination task"
        );

        // Cross-workspace dependency: priority-4 notification for the
        // dependent workspace's admins (HSI §8). The coordination task
        // above still lands in the dependent workspace because the repo
        // (and its orchestrator scope) travels with its workspace.
        if dep_repo.workspace_id != workspace_id {
            notify_cross_workspace_change(state, &workspace_id, &dep_repo, &spec_path, &spec_sha)
                .await;
        }
    }

    // Acknowledge ONLY the SpecApproved signal this run consumed. Other
    // inbox messages — e.g. Escalations the stale-agent detector delivers
    // to a live workspace orchestrator — must stay unacked for their real
    // consumers: bulk-acking here would falsely record them as processed,
    // hiding them from the documented crash-recovery poll
    // (GET /api/v1/agents/:id/messages?acknowledged=false).
    match state.messages.list_unacked(&orchestrator.id, 1000).await {
        Ok(unacked) => {
            for msg in unacked {
                if msg.kind == MessageKind::SpecApproved && msg.payload.as_ref() == Some(payload) {
                    if let Err(e) = state.messages.acknowledge(&msg.id, &orchestrator.id).await {
                        tracing::warn!(
                            message_id = %msg.id,
                            "signal-chain: failed to acknowledge consumed SpecApproved message: {e}"
                        );
                    }
                }
            }
        }
        Err(e) => {
            tracing::warn!(
                orchestrator_id = %orchestrator.id,
                "signal-chain: could not list unacked inbox messages for targeted ack: {e}"
            );
        }
    }

    // The orchestrator "completes after processing its inbox": transition
    // to Idle so the next SpecApproved spawns a fresh session (spawned on
    // demand, not long-lived). restart_on_failure stays armed for crashes.
    let mut completed = orchestrator.clone();
    if completed.transition_status(AgentStatus::Idle).is_ok() {
        let _ = state.agents.update(&completed).await;
        crate::api::budget::decrement_active_agents(state, &workspace_id.to_string()).await;
    }
    Ok(())
}

/// Create priority-4 `CrossWorkspaceSpecChange` notifications for the
/// dependent workspace's Admin/Owner members (HSI §8). Tenant is resolved
/// from the dependent workspace record; when it cannot be resolved the
/// notifications are skipped and the miss is logged (never fabricated —
/// see the fabricated-scope-defaults invariant).
async fn notify_cross_workspace_change(
    state: &AppState,
    source_workspace_id: &Id,
    dep_repo: &Repository,
    spec_path: &str,
    spec_sha: &str,
) {
    let dep_ws_id = dep_repo.workspace_id.clone();
    let tenant_id = match state.workspaces.find_by_id(&dep_ws_id).await {
        Ok(Some(ws)) => ws.tenant_id.to_string(),
        Ok(None) => {
            tracing::warn!(
                workspace_id = %dep_ws_id,
                "signal-chain: dependent workspace not found; skipping cross-workspace notification"
            );
            return;
        }
        Err(e) => {
            tracing::warn!(
                workspace_id = %dep_ws_id,
                "signal-chain: failed to resolve dependent workspace tenant: {e}"
            );
            return;
        }
    };

    let members = match state
        .workspace_memberships
        .list_by_workspace(&dep_ws_id)
        .await
    {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(
                workspace_id = %dep_ws_id,
                "signal-chain: failed to list dependent workspace members: {e}"
            );
            return;
        }
    };

    let now = crate::api::now_secs();
    for member in &members {
        if !matches!(member.role, WorkspaceRole::Admin | WorkspaceRole::Owner) {
            continue;
        }
        let notif = Notification::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            dep_ws_id.clone(),
            member.user_id.clone(),
            NotificationType::CrossWorkspaceSpecChange,
            format!(
                "Cross-workspace spec change: {spec_path}@{spec_sha} (source workspace {source_workspace_id})"
            ),
            &tenant_id,
            now as i64,
        );
        if let Err(e) = state.notifications.create(&notif).await {
            tracing::warn!(
                user_id = %member.user_id,
                "signal-chain: failed to create CrossWorkspaceSpecChange notification: {e}"
            );
        }
    }
}

// ── Phase 3: task scheduler → repo orchestrator ──────────────────────────────

/// Run one task-scheduler cycle (agent-runtime.md §1 Phases 3–4 routing).
///
/// The whole cycle — scan, claim, process — holds the registry's cycle
/// lock, so the 30 s loop and the operator trigger
/// (`POST /api/v1/admin/jobs/task_scheduler/run`) serialize: two concurrent
/// cycles can never both claim one Backlog task (the claim is a plain
/// read-modify-write against the task store, not a compare-and-swap, so
/// serialization is what makes it safe). Cycles also never re-select a task
/// mid-run: no cycle scans while another executes, and every LLM call is
/// bounded (`LLM_CALL_TIMEOUT`), so a run cannot hold the cycle lock
/// indefinitely.
///
/// Finds Backlog tasks with `task_type` Delegation or Coordination, claims
/// each (Backlog→InProgress), and runs the repo orchestrator for its repo
/// under the per-repo registry mutex. Stale InProgress tasks (crash
/// recovery: left claimed by a dead process) and Blocked tasks (released by
/// a failed run) are re-selected. `Implementation` tasks are NOT handled
/// here — they are Phase 4's worker-spawn path, intentionally excluded by
/// the task_type discriminator. Tasks with unresolved repos are left
/// Backlog.
pub async fn scheduler_run_once(state: &Arc<AppState>) -> anyhow::Result<()> {
    // One cycle at a time: the scan-claim-process sequence must not
    // interleave with another cycle (see the doc comment above).
    let cycle_lock = state.orchestrator_registry.scheduler_run_lock().await;
    let _cycle_guard = cycle_lock.lock().await;

    // Candidate set: Backlog (never claimed), Blocked (the failure-release
    // state — a later cycle retries the orchestrator run), and InProgress
    // only when stale (crash recovery: a task left claimed by a dead
    // process; the staleness bound matches the stale-agent detector's
    // heartbeat timeout). Because this cycle holds the cycle lock for the
    // whole scan-claim-process sequence, a task being processed by the
    // current cycle is never re-selected by a concurrent one.
    let now = crate::api::now_secs();
    let mut candidates = state.tasks.list_by_status(&TaskStatus::Backlog).await?;
    candidates.extend(state.tasks.list_by_status(&TaskStatus::Blocked).await?);
    let stale_before = now - SCHEDULER_CLAIM_STALENESS_SECS;
    candidates.extend(
        state
            .tasks
            .list_by_status(&TaskStatus::InProgress)
            .await?
            .into_iter()
            .filter(|t| t.updated_at <= stale_before),
    );

    for mut task in candidates {
        let Some(task_type) = task.task_type.clone() else {
            continue; // pre-approval push-hook tasks have no task_type (Phase 4)
        };
        if !matches!(task_type, TaskType::Delegation | TaskType::Coordination) {
            continue; // Implementation → worker spawn path, not orchestrators
        }

        // Resolve the repo this task targets.
        let repo_id = task.repo_id.clone();
        let repo = match repo_id {
            rid if rid.as_str().is_empty() => None,
            rid => state.repos.find_by_id(&rid).await?,
        };
        let Some(repo) = repo else {
            // A re-claimed (InProgress/Blocked) task with no resolvable
            // repo would otherwise retry forever; surface it and leave it.
            if task.status != TaskStatus::Backlog {
                tracing::error!(
                    task_id = %task.id,
                    task_status = ?task.status,
                    "signal-chain scheduler: claimed task has no resolvable repo; leaving as-is"
                );
            } else {
                tracing::warn!(
                    task_id = %task.id,
                    task_type = ?task_type,
                    "signal-chain scheduler: task has no resolvable repo; leaving Backlog"
                );
            }
            continue;
        };

        // Claim: →InProgress so concurrent cycles (and the 30 s loop
        // re-entering while a run is in flight) cannot double-process.
        // Backlog→InProgress is the fresh claim; Blocked→InProgress and a
        // stale InProgress re-claim use the same terminal state.
        if task.status != TaskStatus::InProgress {
            if let Err(e) = task.transition_status(TaskStatus::InProgress) {
                tracing::warn!(
                    task_id = %task.id,
                    "signal-chain scheduler: claim transition failed: {e}"
                );
                continue;
            }
            task.updated_at = now;
            if let Err(e) = state.tasks.update(&task).await {
                tracing::warn!(task_id = %task.id, "signal-chain scheduler: claim failed: {e}");
                continue;
            }
        }

        // Serialize per repo: a second Delegation for the same repo waits
        // here (per-repo mutex, exactly-one-active repo orchestrator).
        let lock = state.orchestrator_registry.repo_lock(&repo.id).await;
        let _guard = lock.lock().await;

        // Defense in depth against a stale or double claim: re-read the
        // task AFTER acquiring the repo lock and process only the freshest
        // snapshot. If anything finished, cancelled, or re-claimed it since
        // this cycle's claim (a prior run completing under this lock, spec
        // rejection cancellation), re-running decomposition from the stale
        // snapshot would duplicate the sub-task set.
        match state.tasks.find_by_id(&task.id).await {
            Ok(Some(current)) if current.status == TaskStatus::InProgress => {
                task = current;
            }
            Ok(Some(current)) => {
                tracing::info!(
                    task_id = %task.id,
                    task_status = ?current.status,
                    "signal-chain scheduler: task is no longer claimed InProgress; skipping (already processed, cancelled, or released)"
                );
                continue;
            }
            Ok(None) => {
                tracing::info!(
                    task_id = %task.id,
                    "signal-chain scheduler: task disappeared since claim; skipping"
                );
                continue;
            }
            Err(e) => {
                tracing::warn!(
                    task_id = %task.id,
                    "signal-chain scheduler: re-read after claim failed: {e}; skipping"
                );
                continue;
            }
        }

        if let Err(e) = run_repo_orchestrator(state, &repo, &task).await {
            // Release the claim so a later cycle retries. InProgress→Blocked
            // is the valid domain transition (InProgress→Backlog is not —
            // the pre-fix code discarded that Err and left the task
            // stranded InProgress forever with no retry and no surfaced
            // error). Blocked candidates are re-selected by the scan
            // above, so the retry actually happens.
            let released = match task.transition_status(TaskStatus::Blocked) {
                Err(te) => {
                    tracing::error!(
                        task_id = %task.id,
                        "signal-chain scheduler: could not release claim to Blocked (task stays InProgress until the stale re-claim): {te}"
                    );
                    false
                }
                Ok(()) => {
                    task.updated_at = now;
                    if let Err(ue) = state.tasks.update(&task).await {
                        tracing::error!(
                            task_id = %task.id,
                            "signal-chain scheduler: persisting Blocked release failed (task stays InProgress until the stale re-claim): {ue}"
                        );
                        false
                    } else {
                        true
                    }
                }
            };
            tracing::error!(
                task_id = %task.id,
                repo_id = %repo.id,
                released_to_blocked = released,
                "signal-chain: repo orchestrator run failed: {e:#}"
            );
        }
    }
    Ok(())
}

/// Spawn a repo orchestrator for the repo if none is live (task-093 core:
/// repo-scoped JWT, keypair, budget, restart_on_failure) and run one
/// processing pass for the claimed task.
async fn run_repo_orchestrator(
    state: &Arc<AppState>,
    repo: &Repository,
    task: &Task,
) -> anyhow::Result<()> {
    // Spawn (or find) the repo orchestrator for this repo. The agent handle
    // itself is not needed beyond existence — the spawn core enforces the
    // durable one-live check and the per-repo registry lock (held by the
    // caller) serializes concurrent signals.
    if find_live_repo_orchestrator(state, repo).await?.is_none() {
        let (agent, _token, _launch) = crate::api::orchestrator::spawn_repo_orchestrator_core(
            state,
            &repo.id.to_string(),
            crate::api::orchestrator::SpawnOrchestratorRequest {
                name: None,
                parent_id: task.assigned_to.as_ref().map(|a| a.to_string()),
            },
            "system",
        )
        .await
        .map_err(|e| anyhow::anyhow!("repo orchestrator spawn: {e:?}"))?;
        tracing::info!(
            orchestrator_id = %agent.id,
            repo_id = %repo.id,
            "signal-chain: spawned repo orchestrator for {:?} task",
            task.task_type
        );
    }

    match task.task_type {
        Some(TaskType::Delegation) => decompose_delegation(state, repo, task).await?,
        Some(TaskType::Coordination) => assess_coordination(state, repo, task).await?,
        _ => unreachable!("scheduler_run_once filters to Delegation/Coordination"),
    }
    Ok(())
}

/// Phase 3 for Delegation tasks: read the approved spec the task
/// references, decompose it into ordered Implementation sub-tasks, then
/// mark the delegation task Completed (Done).
async fn decompose_delegation(
    state: &AppState,
    repo: &Repository,
    task: &Task,
) -> anyhow::Result<()> {
    let now = crate::api::now_secs();

    // 1. Read the delegation task's approved spec. `spec_ref` carries the
    //    pinned "path@sha" from Phase 2; when it is absent, fall back to
    //    the bare `spec_path` + the ledger's current SHA for tasks created
    //    by other paths (e.g. MCP task.create). A missing or empty SHA is
    //    a hard failure — never an empty one: pinning "path@" would decompose
    //    against a blob that cannot be identified, converting "cannot
    //    determine the approved blob" into "empty blob is fine" (the same
    //    failure class the fail-open-ref-resolution invariant bans). The
    //    error releases the claim to Blocked so a later cycle retries once
    //    the ledger resolves.
    let (spec_path, spec_sha) = match task.spec_ref.as_deref() {
        Some(r) if r.contains('@') => {
            let (path, sha) = r.split_once('@').unwrap();
            if sha.is_empty() {
                anyhow::bail!(
                    "delegation task {} has a malformed spec_ref {r:?} with an \
                     empty SHA; cannot pin the approved blob for decomposition",
                    task.id
                );
            }
            (path.to_string(), sha.to_string())
        }
        _ => match task.spec_path.as_deref() {
            Some(p) if !p.is_empty() => {
                let sha = state
                    .spec_ledger
                    .find_by_path(p)
                    .await?
                    .map(|e| e.current_sha)
                    .filter(|s| !s.is_empty());
                let Some(sha) = sha else {
                    anyhow::bail!(
                        "delegation task {} references spec {p} with no approved SHA in \
                         the ledger; cannot pin the approved blob for decomposition",
                        task.id
                    );
                };
                (p.to_string(), sha)
            }
            _ => {
                // No spec reference: nothing to decompose from. Complete
                // the delegation as a no-op rather than leaving it
                // InProgress forever, and record why in the log.
                let mut done = task.clone();
                done.transition_status(TaskStatus::Review)?;
                done.transition_status(TaskStatus::Done)?;
                done.updated_at = now;
                state.tasks.update(&done).await?;
                tracing::warn!(
                    task_id = %task.id,
                    "signal-chain: delegation task has no spec reference; marked Done without decomposition"
                );
                return Ok(());
            }
        },
    };

    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());
    // spec_sha is guaranteed non-empty by the resolution above; an
    // unreadable blob (None) degrades to the deterministic decomposition
    // inside `decompose`, which warns rather than failing.
    let spec_content =
        crate::spec_registry::read_git_file(&git_bin, &repo.path, &spec_sha, &spec_path).await;

    // 2. Decompose. Judgment comes from the LLM when configured (the spec
    //    says "the orchestrator is an LLM"); when `state.llm` is None
    //    (deployment without GYRE_VERTEX_PROJECT) fall back to a
    //    deterministic decomposition derived from the real spec content —
    //    one ordered sub-task per `##`-level section with depends_on
    //    chaining the previous section. Both paths create real tasks with
    //    real spec data; the fallback fabricates no LLM output.
    let subtasks = decompose(state, repo, &spec_path, &spec_sha, spec_content).await?;

    // 3. Create the Implementation sub-tasks with spec_ref, parent_task_id,
    //    order, and depends_on chaining.
    let spec_ref = format!("{spec_path}@{spec_sha}");
    let mut prev_id: Option<Id> = None;
    for (i, st) in subtasks.iter().enumerate() {
        let mut sub = Task::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            st.title.clone(),
            now,
        );
        sub.description = Some(st.description.clone());
        sub.task_type = Some(TaskType::Implementation);
        sub.repo_id = repo.id.clone();
        sub.workspace_id = repo.workspace_id.clone();
        sub.spec_path = Some(spec_path.clone());
        sub.spec_ref = Some(spec_ref.clone());
        sub.parent_task_id = Some(task.id.clone());
        sub.order = Some(st.order.unwrap_or(i as u32));
        if let Some(dep) = &prev_id {
            sub.depends_on.push(dep.clone());
        }
        state.tasks.create(&sub).await?;
        prev_id = Some(sub.id.clone());
        tracing::debug!(
            subtask_id = %sub.id,
            order = ?sub.order,
            depends_on = ?sub.depends_on,
            "signal-chain: created implementation sub-task"
        );
    }

    // 4. Mark the delegation task Completed (Backlog→InProgress→Review→Done).
    let mut done = task.clone();
    done.transition_status(TaskStatus::Review)?;
    done.transition_status(TaskStatus::Done)?;
    done.updated_at = now;
    state.tasks.update(&done).await?;
    tracing::info!(
        task_id = %task.id,
        subtasks = subtasks.len(),
        "signal-chain: repo orchestrator decomposed delegation task"
    );

    // 5. Complete: the repo orchestrator only creates tasks (Phase 3) and
    //    completes. It stays live for future coordination tasks in the
    //    same repo (exactly-one-active; stale detector restarts on death).
    Ok(())
}

/// Coordination-task processing (Phase 2 "coordination task processing"):
/// the repo orchestrator reads the coordination task, assesses the impact
/// of the dependency change, and may create sub-tasks or mark the
/// coordination task Completed if no action is needed. Without a live LLM
/// the deterministic assessment creates one documentation-update sub-task
/// when the spec content is reachable and marks the task Done otherwise.
async fn assess_coordination(
    state: &AppState,
    repo: &Repository,
    task: &Task,
) -> anyhow::Result<()> {
    let now = crate::api::now_secs();

    let assessment = coordinate(state, repo, task).await?;
    if assessment.create_subtask {
        let mut sub = Task::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            assessment.title,
            now,
        );
        sub.description = Some(assessment.description);
        sub.task_type = Some(TaskType::Implementation);
        sub.repo_id = repo.id.clone();
        sub.workspace_id = repo.workspace_id.clone();
        sub.parent_task_id = Some(task.id.clone());
        sub.spec_path = task.spec_path.clone();
        sub.spec_ref = task.spec_ref.clone();
        state.tasks.create(&sub).await?;
    }

    let mut done = task.clone();
    done.transition_status(TaskStatus::Review)?;
    done.transition_status(TaskStatus::Done)?;
    done.updated_at = now;
    state.tasks.update(&done).await?;
    tracing::info!(
        task_id = %task.id,
        subtask_created = assessment.create_subtask,
        "signal-chain: repo orchestrator processed coordination task"
    );
    Ok(())
}

/// A sub-task proposed by decomposition.
struct ProposedSubtask {
    title: String,
    description: String,
    order: Option<u32>,
}

/// Summarize approved spec content for the delegation task's metadata:
/// the first `#` heading (title), the `##` section count, and the size —
/// all derived from the real blob the workspace orchestrator read, so the
/// delegation record carries what was actually read (Phase 2 step 1)
/// instead of discarding the content.
fn summarize_spec_content(content: &str) -> String {
    let title = content
        .lines()
        .find_map(|l| l.strip_prefix("# ").map(|t| t.trim().to_string()))
        .unwrap_or_else(|| "(untitled)".to_string());
    let sections = content.lines().filter(|l| l.starts_with("## ")).count();
    format!("\"{title}\" — {sections} sections, {} bytes", content.len())
}

/// Read a bounded summary of the repo's current codebase state: the
/// tracked file tree at HEAD (`git ls-tree -r --name-only HEAD`),
/// truncated to `max_entries` paths with an explicit "(N more files)"
/// marker. Returns `None` when the repo or HEAD is unreadable — the
/// caller then decomposes from spec content only (logged).
async fn read_repo_tree(git_bin: &str, repo_path: &str, max_entries: usize) -> Option<String> {
    let out = tokio::process::Command::new(git_bin)
        .arg("-C")
        .arg(repo_path)
        .args(["ls-tree", "-r", "--name-only", "HEAD"])
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let total = text.lines().filter(|l| !l.trim().is_empty()).count();
    let shown: Vec<&str> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(max_entries)
        .collect();
    if shown.is_empty() {
        return None;
    }
    let mut summary = shown.join("\n");
    if total > max_entries {
        summary.push_str(&format!("\n... ({} more files)", total - max_entries));
    }
    Some(summary)
}

/// Deterministic fallback: one sub-task per `##` section, order = section
/// index, depends_on chains the previous section (set by the caller).
fn deterministic_decomposition(spec_path: &str, content: &str) -> Vec<ProposedSubtask> {
    let mut sections: Vec<(String, Vec<&str>)> = Vec::new();
    let mut current: Option<(String, Vec<&str>)> = None;
    for line in content.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            if let Some(sec) = current.take() {
                sections.push(sec);
            }
            current = Some((heading.trim().to_string(), Vec::new()));
        } else if let Some((_, body)) = current.as_mut() {
            body.push(line);
        }
    }
    if let Some(sec) = current.take() {
        sections.push(sec);
    }

    if sections.is_empty() {
        // Flat spec: single implementation task for the whole spec.
        return vec![ProposedSubtask {
            title: format!("Implement {spec_path}"),
            description: format!(
                "Implement the approved spec {spec_path} in full. Acceptance: every requirement in the spec is met."
            ),
            order: Some(0),
        }];
    }

    sections
        .into_iter()
        .enumerate()
        .map(|(i, (heading, body))| {
            let body_text = body.join("\n").trim().to_string();
            let excerpt: String = {
                let mut s = body_text.chars().take(500).collect::<String>();
                if body_text.chars().count() > 500 {
                    s.push('…');
                }
                s
            };
            ProposedSubtask {
                title: format!("{heading} — {spec_path}"),
                description: format!(
                    "Implement section '{heading}' of the approved spec.\n\nSpec excerpt:\n{excerpt}\n\nAcceptance: the section's requirements are implemented and verified."
                ),
                order: Some(i as u32),
            }
        })
        .collect()
}

/// LLM decomposition judgment (or deterministic fallback when `state.llm`
/// is None). The LLM receives the real spec content and the orchestrator's
/// persona prompt and returns a JSON array of sub-task proposals.
async fn decompose(
    state: &AppState,
    repo: &Repository,
    spec_path: &str,
    spec_sha: &str,
    spec_content: Option<String>,
) -> anyhow::Result<Vec<ProposedSubtask>> {
    let content = match spec_content {
        Some(c) => c,
        None => {
            // Spec content unreachable (bare repo path in a unit-test env,
            // or SHA not present locally): decompose from the spec
            // reference alone so the chain still produces work.
            return Ok(vec![ProposedSubtask {
                title: format!("Implement {spec_path}"),
                description: format!(
                    "Implement the approved spec {spec_path}@{spec_sha}. Acceptance: the spec's requirements are implemented and verified."
                ),
                order: Some(0),
            }]);
        }
    };

    let Some(factory) = state.llm.as_ref() else {
        // Deployment without an LLM: deterministic decomposition from real
        // spec content. Honest degradation — no fabricated model output.
        return Ok(deterministic_decomposition(spec_path, &content));
    };

    let persona = load_persona_prompt(state, "repo-orchestrator").await;
    let (model, _) =
        crate::llm_helpers::resolve_llm_model(state, &repo.workspace_id, "decompose_spec").await;
    let port = factory.for_model(&model);

    // Read the repo's current codebase state (agent-runtime.md \u00a71 Phase 3
    // step 2): the tracked file tree at HEAD, fed to the LLM alongside the
    // spec so decomposition considers what already exists. Equivalent
    // effect to the spec's "via MCP tools" wording \u2014 the orchestrator run
    // is server-driven, so it reads git directly instead of calling its
    // own MCP tools (same stance as task creation via the task store).
    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());
    let codebase_state = read_repo_tree(&git_bin, &repo.path, 200).await;
    if codebase_state.is_none() {
        tracing::debug!(
            repo_id = %repo.id,
            "signal-chain: repo tree unreadable; decomposing from spec content only"
        );
    }

    let user_prompt = format!(
        "Decompose the following approved spec into ordered implementation sub-tasks.\n\n\
         Repo: {repo_name} ({repo_id})\nSpec: {spec_path}@{spec_sha}\n\n\
         Spec content:\n```\n{content}\n```\n\n{codebase_section}\n\
         Respond with valid JSON only: an array of objects \
         {{\"title\": string, \"description\": string, \"order\": number}}, \
         where lower order runs first and same-order tasks may run in parallel.",
        repo_name = repo.name,
        repo_id = repo.id,
        codebase_section = codebase_state
            .map(|tree| format!("Current codebase state (tracked files at HEAD):\n{tree}\n"))
            .unwrap_or_default(),
    );
    // Latency-bounded: a hung model must not hold the scheduler's cycle
    // lock and the per-repo mutex indefinitely (LLM_CALL_TIMEOUT).
    let value = match tokio::time::timeout(
        LLM_CALL_TIMEOUT,
        port.predict_json(&persona, &user_prompt),
    )
    .await
    {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            tracing::warn!(
                "signal-chain: LLM decomposition failed ({e:#}); falling back to deterministic decomposition"
            );
            return Ok(deterministic_decomposition(spec_path, &content));
        }
        Err(_) => {
            tracing::warn!(
                timeout_secs = LLM_CALL_TIMEOUT.as_secs(),
                "signal-chain: LLM decomposition timed out; falling back to deterministic decomposition"
            );
            return Ok(deterministic_decomposition(spec_path, &content));
        }
    };

    let Some(arr) = value.as_array() else {
        tracing::warn!("signal-chain: LLM decomposition returned non-array; falling back");
        return Ok(deterministic_decomposition(spec_path, &content));
    };
    let mut subs: Vec<ProposedSubtask> = Vec::new();
    for item in arr {
        let Some(title) = item.get("title").and_then(|v| v.as_str()) else {
            continue;
        };
        let description = item
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("Implement per the approved spec.")
            .to_string();
        let order = item.get("order").and_then(|v| v.as_u64()).map(|o| o as u32);
        subs.push(ProposedSubtask {
            title: title.to_string(),
            description,
            order,
        });
    }
    if subs.is_empty() {
        tracing::warn!(
            "signal-chain: LLM decomposition produced no usable sub-tasks; falling back"
        );
        return Ok(deterministic_decomposition(spec_path, &content));
    }
    Ok(subs)
}

/// Coordination impact assessment (LLM judgment with deterministic
/// fallback): decide whether the dependency change warrants a sub-task.
struct CoordinationAssessment {
    create_subtask: bool,
    title: String,
    description: String,
}

async fn coordinate(
    state: &AppState,
    repo: &Repository,
    task: &Task,
) -> anyhow::Result<CoordinationAssessment> {
    let Some(factory) = state.llm.as_ref() else {
        // Without an LLM the deterministic assessment is conservative:
        // a dependency changed under this repo — create one
        // documentation-update sub-task so a human (or worker agent)
        // reviews the dependent spec, rather than silently marking the
        // coordination Done.
        return Ok(CoordinationAssessment {
            create_subtask: true,
            title: format!("Review dependency change: {}", task.spec_path.as_deref().unwrap_or("")),
            description: format!(
                "A spec this repo depends on changed. Review the dependent spec and update documentation, amend the spec, or re-implement as needed. Coordination task: {}.",
                task.id
            ),
        });
    };

    let persona = load_persona_prompt(state, "repo-orchestrator").await;
    let (model, _) =
        crate::llm_helpers::resolve_llm_model(state, &repo.workspace_id, "coordinate").await;
    let port = factory.for_model(&model);
    let user_prompt = format!(
        "A dependency of repo {repo_name} changed (coordination task {task_id}).\n\
         Task description: {desc}\n\n\
         Assess the impact. Respond with valid JSON only: \
         {{\"action_needed\": boolean, \"title\": string, \"description\": string}}. \
         action_needed=false means mark the coordination task Completed with no sub-tasks.",
        repo_name = repo.name,
        task_id = task.id,
        desc = task.description.as_deref().unwrap_or(""),
    );

    // Latency-bounded like the decomposition call (LLM_CALL_TIMEOUT): a
    // hung model must not stall the scheduler cycle.
    let value = match tokio::time::timeout(
        LLM_CALL_TIMEOUT,
        port.predict_json(&persona, &user_prompt),
    )
    .await
    {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            tracing::warn!(
                "signal-chain: LLM coordination assessment failed ({e:#}); assuming action needed"
            );
            return Ok(CoordinationAssessment {
                create_subtask: true,
                title: format!("Review dependency change: {}", task.spec_path.as_deref().unwrap_or("")),
                description: format!(
                    "A spec this repo depends on changed. Review and update as needed. Coordination task: {}.",
                    task.id
                ),
            });
        }
        Err(_) => {
            tracing::warn!(
                timeout_secs = LLM_CALL_TIMEOUT.as_secs(),
                "signal-chain: LLM coordination assessment timed out; assuming action needed"
            );
            return Ok(CoordinationAssessment {
                create_subtask: true,
                title: format!("Review dependency change: {}", task.spec_path.as_deref().unwrap_or("")),
                description: format!(
                    "A spec this repo depends on changed. Review and update as needed. Coordination task: {}.",
                    task.id
                ),
            });
        }
    };
    let action_needed = value
        .get("action_needed")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    Ok(CoordinationAssessment {
        create_subtask: action_needed,
        title: value
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("Review dependency change")
            .to_string(),
        description: value
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("Dependency changed; review and update as needed.")
            .to_string(),
    })
}

/// Load the persona prompt from the seeded meta-specs; fall back to a
/// functional default when the persona is missing (soft validation, same
/// stance as the task-093 spawn core).
async fn load_persona_prompt(state: &AppState, name: &str) -> String {
    use gyre_domain::meta_spec::{MetaSpecApprovalStatus, MetaSpecKind};
    use gyre_ports::MetaSpecFilter;
    let filter = MetaSpecFilter {
        kind: Some(MetaSpecKind::Persona),
        ..Default::default()
    };
    if let Ok(personas) = state.meta_specs.list(&filter).await {
        if let Some(p) = personas.iter().find(|p| p.name == name) {
            if matches!(p.approval_status, MetaSpecApprovalStatus::Approved) {
                return p.prompt.clone();
            }
        }
    }
    format!("You are a {name} agent for the Gyre platform.")
}

/// Start the task-scheduler background loop (30 s interval, same shape as
/// the stale-agent detector). Every cycle is recorded in the job registry
/// so /healthz can detect a dead loop.
pub fn spawn_task_scheduler(state: Arc<AppState>) {
    const CHECK_INTERVAL_SECS: u64 = 30;

    tokio::spawn(async move {
        state.job_registry.mark_scheduled("task_scheduler").await;
        let mut interval =
            tokio::time::interval(tokio::time::Duration::from_secs(CHECK_INTERVAL_SECS));
        loop {
            interval.tick().await;
            let started_at = crate::jobs::now_secs();
            let result = scheduler_run_once(&state).await;
            state
                .job_registry
                .record_cycle("task_scheduler", started_at, &result)
                .await;
            if let Err(e) = result {
                tracing::error!("task scheduler cycle failed: {e:#}");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    // Registry semantics: distinct locks per scope, stable identity across
    // calls, no cross-contamination between workspaces/repos.
    #[tokio::test]
    async fn registry_locks_are_per_scope_and_stable() {
        let reg = OrchestratorRegistry::new();
        let ws1 = Id::new("ws-1");
        let ws2 = Id::new("ws-2");
        let r1 = Id::new("r-1");

        let a = reg.workspace_lock(&ws1).await;
        let b = reg.workspace_lock(&ws1).await;
        let c = reg.workspace_lock(&ws2).await;
        let d = reg.repo_lock(&r1).await;
        let e = reg.repo_lock(&r1).await;

        assert!(Arc::ptr_eq(&a, &b), "same workspace must share one lock");
        assert!(Arc::ptr_eq(&d, &e), "same repo must share one lock");
        assert!(!Arc::ptr_eq(&a, &c), "different workspaces must not share");
        assert!(
            !Arc::ptr_eq(&a, &d),
            "workspace and repo locks are distinct"
        );
    }

    // Concurrent acquisitions of the same workspace lock serialize: the
    // second guard waits until the first is released.
    #[tokio::test]
    async fn registry_workspace_lock_serializes() {
        let reg = Arc::new(OrchestratorRegistry::new());
        let ws = Id::new("ws-1");
        let lock = reg.workspace_lock(&ws).await;

        let g1 = lock.lock().await;
        let lock2 = Arc::clone(&lock);
        let task = tokio::spawn(async move {
            let _g2 = lock2.lock().await;
            true
        });
        // While g1 is held the second acquisition must not complete.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(
            !task.is_finished(),
            "second acquisition must block while the first holds the lock"
        );
        drop(g1);
        assert!(
            task.await.unwrap(),
            "second acquisition completes after release"
        );
    }

    // ── Full-chain integration tests ─────────────────────────────────────
    //
    // These exercise the real production path with the mem test state:
    // on_spec_approved → workspace orchestrator spawn → delegation +
    // coordination task creation → scheduler_run_once → repo orchestrator
    // → decomposition into ordered sub-tasks.

    use crate::mem::test_state;
    use gyre_domain::spec_ledger::ApprovalStatus;

    /// Seed ws-1 (tenant t1) with repos r-1 (own repo) and r-2 (dependent
    /// repo in the same workspace), a spec ledger entry for
    /// `specs/system/auth.md` in r-1, and a depends_on link from r-2's
    /// `specs/system/api.md` to it.
    async fn chain_state() -> Arc<crate::AppState> {
        let state = test_state();
        let ws = gyre_domain::Workspace::new(Id::new("ws-1"), Id::new("t1"), "Ws", "ws", 0);
        state.workspaces.create(&ws).await.unwrap();
        for rid in ["r-1", "r-2"] {
            let repo = gyre_domain::Repository::new(
                Id::new(rid),
                Id::new("ws-1"),
                rid,
                format!("/tmp/gyre-test-{rid}"),
                0,
            );
            state.repos.create(&repo).await.unwrap();
        }
        // Production seeds these at startup (lib.rs seed_builtin_personas);
        // the spawn cores fail closed without an approved persona in the
        // tenant/workspace/repo scope chain (task-099 F2).
        for slug in ["workspace-orchestrator", "repo-orchestrator"] {
            let mut persona = gyre_domain::Persona::new(
                Id::new(format!("{slug}-persona")),
                slug,
                slug,
                gyre_domain::PersonaScope::Tenant(Id::new("t1")),
                "test orchestrator persona",
                0,
            );
            persona.approval_status = gyre_domain::PersonaApprovalStatus::Approved;
            state.personas.create(&persona).await.unwrap();
        }
        let entry = gyre_domain::SpecLedgerEntry {
            path: "specs/system/auth.md".to_string(),
            title: "Auth".to_string(),
            owner: "owner".to_string(),
            kind: None,
            current_sha: "abc123".to_string(),
            approval_mode: "single".to_string(),
            approval_status: ApprovalStatus::Pending,
            linked_tasks: vec![],
            linked_mrs: vec![],
            drift_status: "clean".to_string(),
            created_at: 0,
            updated_at: 0,
            repo_id: Some("r-1".to_string()),
            workspace_id: Some("ws-1".to_string()),
        };
        state.spec_ledger.save(&entry).await.unwrap();
        state
            .spec_links_store
            .lock()
            .await
            .push(crate::spec_registry::SpecLinkEntry {
                id: "link-1".to_string(),
                source_path: "specs/system/api.md".to_string(),
                source_repo_id: Some("r-2".to_string()),
                link_type: crate::spec_registry::SpecLinkType::DependsOn,
                target_path: "specs/system/auth.md".to_string(),
                target_repo_id: Some("r-1".to_string()),
                target_display: None,
                target_sha: None,
                reason: None,
                status: "active".to_string(),
                created_at: 0,
                stale_since: None,
            });
        state
    }

    fn approved_payload() -> serde_json::Value {
        serde_json::json!({
            "repo_id": "r-1",
            "spec_path": "specs/system/auth.md",
            "spec_sha": "abc123",
            "approved_by": "user:alice",
            "approval_id": "approval-1",
            "workspace_id": "ws-1",
        })
    }

    // Phase 2 acceptance: SpecApproved spawns exactly one workspace
    // orchestrator, delivers the message to its inbox, creates a Delegation
    // task for the spec's repo and a Coordination task for the dependent
    // repo, then completes the orchestrator (Idle).
    #[tokio::test]
    async fn spec_approved_creates_orchestrator_and_tasks() {
        let state = chain_state().await;

        on_spec_approved(&state, &approved_payload()).await;

        // Exactly one live workspace orchestrator was spawned for ws-1.
        let agents = state
            .agents
            .list_by_workspace(&Id::new("ws-1"))
            .await
            .unwrap();
        let orch: Vec<_> = agents
            .iter()
            .filter(|a| a.orchestrator_type == OrchestratorType::WorkspaceOrchestrator)
            .collect();
        assert_eq!(orch.len(), 1, "exactly one workspace orchestrator spawned");
        assert_eq!(
            orch[0].status,
            AgentStatus::Idle,
            "completes after processing inbox"
        );

        // The SpecApproved message was delivered to the orchestrator's inbox
        // (Directed tier, persisted).
        let inbox = state
            .messages
            .list_after(&orch[0].id, 0, None, 100)
            .await
            .unwrap();
        assert!(
            inbox.iter().any(|m| m.kind == MessageKind::SpecApproved
                && m.payload.as_ref().and_then(|p| p.get("approval_id"))
                    == Some(&serde_json::json!("approval-1"))),
            "SpecApproved delivered to orchestrator inbox, got {inbox:?}"
        );

        // Delegation task created for the spec's own repo (r-1), with the
        // spec_ref path@sha and the orchestrator as assignee.
        let tasks = state.tasks.list().await.unwrap();
        let delegation = tasks
            .iter()
            .find(|t| t.task_type == Some(TaskType::Delegation))
            .expect("delegation task created for r-1");
        assert_eq!(delegation.repo_id, Id::new("r-1"));
        assert_eq!(delegation.workspace_id, Id::new("ws-1"));
        assert_eq!(
            delegation.spec_path.as_deref(),
            Some("specs/system/auth.md"),
            "spec_path is the bare path (list_by_spec_path match key)"
        );
        assert_eq!(
            delegation.spec_ref.as_deref(),
            Some("specs/system/auth.md@abc123"),
            "spec_ref pins the approved blob"
        );
        assert_eq!(delegation.assigned_to.as_ref(), Some(&orch[0].id));
        assert_eq!(delegation.status, TaskStatus::Backlog);

        // Coordination task created for the dependent repo (r-2).
        let coordination = tasks
            .iter()
            .find(|t| t.task_type == Some(TaskType::Coordination))
            .expect("coordination task created for dependent repo r-2");
        assert_eq!(coordination.repo_id, Id::new("r-2"));
        assert_eq!(coordination.workspace_id, Id::new("ws-1"));
    }

    // Phase 2 routing: a SpecApproved without a workspace scope must not
    // spawn anything (Broadcast-destined specs have no orchestrator scope).
    #[tokio::test]
    async fn spec_approved_without_workspace_is_noop() {
        let state = chain_state().await;
        let mut payload = approved_payload();
        payload["workspace_id"] = serde_json::json!(null);

        on_spec_approved(&state, &payload).await;

        let agents = state
            .agents
            .list_by_workspace(&Id::new("ws-1"))
            .await
            .unwrap();
        assert!(
            agents
                .iter()
                .all(|a| a.orchestrator_type != OrchestratorType::WorkspaceOrchestrator),
            "no orchestrator spawned for unscoped spec"
        );
        let tasks = state.tasks.list().await.unwrap();
        assert!(tasks.is_empty(), "no tasks created");
    }

    // Phase 2 exactly-one-active: a second SpecApproved reuses the live
    // (Idle) orchestrator — its message lands in the same inbox — instead
    // of spawning a second session.
    #[tokio::test]
    async fn second_approval_reuses_then_respawns() {
        let state = chain_state().await;

        on_spec_approved(&state, &approved_payload()).await;
        on_spec_approved(&state, &approved_payload()).await;

        // Two runs → two delegation tasks (one per signal). Exactly-one-
        // active: the first run's orchestrator is Idle (live, not Active)
        // when the second signal arrives, so the second run reuses it —
        // same agent record, both messages in one inbox — rather than
        // spawning a second session (agent-runtime.md §1 Phase 2 "if an
        // active workspace orchestrator exists, the message is delivered
        // to its inbox").
        let tasks = state.tasks.list().await.unwrap();
        let delegations: Vec<_> = tasks
            .iter()
            .filter(|t| t.task_type == Some(TaskType::Delegation))
            .collect();
        assert_eq!(
            delegations.len(),
            2,
            "each signal creates its delegation task"
        );
        let agents = state
            .agents
            .list_by_workspace(&Id::new("ws-1"))
            .await
            .unwrap();
        let orch: Vec<_> = agents
            .iter()
            .filter(|a| a.orchestrator_type == OrchestratorType::WorkspaceOrchestrator)
            .collect();
        assert_eq!(orch.len(), 1, "no second orchestrator session spawned");
        assert_eq!(orch[0].status, AgentStatus::Idle);
        let inbox = state
            .messages
            .list_after(&orch[0].id, 0, None, 100)
            .await
            .unwrap();
        let delivered: Vec<_> = inbox
            .iter()
            .filter(|m| m.kind == MessageKind::SpecApproved)
            .collect();
        assert_eq!(
            delivered.len(),
            2,
            "both SpecApproved signals delivered to the same orchestrator inbox"
        );
        assert!(
            delivered.iter().all(|m| m.acknowledged),
            "both signals processed (acked) by the orchestrator runs"
        );
    }

    // Phase 3 acceptance: the scheduler claims the Backlog Delegation task,
    // spawns a repo orchestrator for its repo, decomposes into ordered
    // Implementation sub-tasks chained with depends_on, and marks the
    // delegation task Done.
    #[tokio::test]
    async fn scheduler_decomposes_delegation_into_ordered_subtasks() {
        let state = chain_state().await;

        on_spec_approved(&state, &approved_payload()).await;
        scheduler_run_once(&state).await.unwrap();

        // Repo orchestrator spawned for r-1 (Delegation task's repo).
        let agents = state
            .agents
            .list_by_workspace(&Id::new("ws-1"))
            .await
            .unwrap();
        assert!(
            agents.iter().any(
                |a| a.orchestrator_type == OrchestratorType::RepoOrchestrator
                    && a.repo_id.as_ref() == Some(&Id::new("r-1"))
            ),
            "repo orchestrator spawned for r-1"
        );

        let tasks = state.tasks.list().await.unwrap();
        let delegation = tasks
            .iter()
            .find(|t| t.task_type == Some(TaskType::Delegation))
            .expect("delegation task exists");
        assert_eq!(
            delegation.status,
            TaskStatus::Done,
            "delegation task marked Completed after decomposition"
        );

        // Implementation sub-tasks: spec_ref, parent_task_id, order, and
        // depends_on chaining the previous sub-task. The spec content is
        // unreachable at /tmp/gyre-test-r-1 (no git repo), so decomposition
        // falls back to the single-task proposal — the chain still creates
        // a real ordered sub-task.
        let subs: Vec<_> = tasks
            .iter()
            .filter(|t| {
                t.parent_task_id.as_ref() == Some(&delegation.id)
                    && t.task_type == Some(TaskType::Implementation)
            })
            .collect();
        assert!(
            !subs.is_empty(),
            "implementation sub-tasks created under the delegation task"
        );
        for sub in &subs {
            assert_eq!(sub.parent_task_id.as_ref(), Some(&delegation.id));
            assert_eq!(sub.repo_id, Id::new("r-1"));
            assert!(sub.order.is_some(), "sub-task has an order");
            assert_eq!(sub.spec_path.as_deref(), Some("specs/system/auth.md"));
            assert_eq!(
                sub.spec_ref.as_deref(),
                Some("specs/system/auth.md@abc123"),
                "sub-task pins the approved spec blob"
            );
        }
        // Ordering: depends_on chains previous sub-task IDs in order.
        let mut sorted: Vec<&gyre_domain::Task> = subs.clone();
        sorted.sort_by_key(|t| t.order.unwrap_or(0));
        for pair in sorted.windows(2) {
            assert!(
                pair[1].depends_on.contains(&pair[0].id),
                "sub-task {:?} must depend on order-{:?} predecessor",
                pair[1].order,
                pair[0].order
            );
        }

        // The scheduler must not touch Implementation tasks: re-running the
        // cycle leaves the sub-tasks Backlog (they are Phase 4's path).
        let before = tasks.len();
        scheduler_run_once(&state).await.unwrap();
        let after = state.tasks.list().await.unwrap();
        assert_eq!(before, after.len(), "no new tasks on second cycle");
        assert!(
            after
                .iter()
                .filter(|t| t.task_type == Some(TaskType::Implementation))
                .all(|t| t.status == TaskStatus::Backlog),
            "implementation tasks stay Backlog for the worker-spawn path"
        );
    }

    // Phase 2 coordination processing: the scheduler runs the repo
    // orchestrator for the Coordination task in the dependent repo (r-2)
    // and marks it Done with a review sub-task (deterministic no-LLM path
    // is conservative: action needed).
    #[tokio::test]
    async fn scheduler_processes_coordination_task() {
        let state = chain_state().await;

        on_spec_approved(&state, &approved_payload()).await;
        // First cycle claims the Delegation (r-1); the Coordination task
        // (r-2) is also Backlog and gets claimed in the same cycle.
        scheduler_run_once(&state).await.unwrap();

        let tasks = state.tasks.list().await.unwrap();
        let coordination = tasks
            .iter()
            .find(|t| t.task_type == Some(TaskType::Coordination))
            .expect("coordination task exists");
        assert_eq!(
            coordination.status,
            TaskStatus::Done,
            "coordination task processed to Done"
        );
        let review_subs: Vec<_> = tasks
            .iter()
            .filter(|t| t.parent_task_id.as_ref() == Some(&coordination.id))
            .collect();
        assert_eq!(
            review_subs.len(),
            1,
            "conservative assessment creates one review sub-task"
        );
        assert_eq!(review_subs[0].repo_id, Id::new("r-2"));
        assert_eq!(review_subs[0].task_type, Some(TaskType::Implementation));
    }

    // The scheduler ignores tasks without a task_type (pre-approval
    // push-hook tasks) — they stay Backlog untouched.
    #[tokio::test]
    async fn scheduler_ignores_untyped_tasks() {
        let state = chain_state().await;

        let mut t = Task::new(Id::new("push-hook-1"), "Pre-approval hook task", 0);
        t.workspace_id = Id::new("ws-1");
        t.repo_id = Id::new("r-1");
        t.spec_path = Some("specs/system/auth.md".to_string());
        state.tasks.create(&t).await.unwrap();

        scheduler_run_once(&state).await.unwrap();

        let after = state
            .tasks
            .find_by_id(&Id::new("push-hook-1"))
            .await
            .unwrap()
            .expect("task survives");
        assert_eq!(after.status, TaskStatus::Backlog, "untyped task untouched");
    }

    /// Review finding F2 regression (task-115): a repo-orchestrator run
    /// failure must RELEASE the claim so a later cycle retries. The
    /// pre-fix code attempted InProgress→Backlog (an invalid domain
    /// transition) and discarded the Err, stranding the task InProgress
    /// forever. The fix releases to Blocked (valid) which the scheduler
    /// scan re-selects. Failure is injected deterministically: the
    /// workspace budget is capped at max_concurrent_agents=0 so the repo
    /// orchestrator spawn fails; raising the cap makes the retry succeed.
    #[tokio::test]
    async fn scheduler_failure_releases_claim_and_retries() {
        let state = chain_state().await;

        // Run the chain once to create the Delegation task for r-1.
        on_spec_approved(&state, &approved_payload()).await;
        let delegation = state
            .tasks
            .list()
            .await
            .unwrap()
            .into_iter()
            .find(|t| t.task_type == Some(TaskType::Delegation))
            .expect("delegation task created");
        assert_eq!(delegation.status, TaskStatus::Backlog);

        // Cap the workspace budget: no concurrent agents allowed, so the
        // repo-orchestrator spawn inside run_repo_orchestrator fails.
        state
            .budget_configs
            .set_config(
                "workspace:ws-1",
                &gyre_domain::BudgetConfig {
                    max_tokens_per_day: None,
                    max_cost_per_day: None,
                    max_concurrent_agents: Some(0),
                    max_agent_lifetime_secs: None,
                },
            )
            .await
            .unwrap();

        // Cycle 1: claim + run fails + release. The task must NOT be
        // stranded InProgress (the pre-fix bug).
        scheduler_run_once(&state).await.unwrap();
        let after_fail = state
            .tasks
            .find_by_id(&delegation.id)
            .await
            .unwrap()
            .unwrap();
        assert_ne!(
            after_fail.status,
            TaskStatus::InProgress,
            "failed run must not leave the task stranded InProgress (review F2)"
        );
        assert_eq!(
            after_fail.status,
            TaskStatus::Blocked,
            "failed run releases the claim to Blocked for retry"
        );

        // Cycle 2 with the same budget: the Blocked task is re-selected
        // and retried (still failing, still released — no stranding).
        scheduler_run_once(&state).await.unwrap();
        let retried = state
            .tasks
            .find_by_id(&delegation.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            retried.status,
            TaskStatus::Blocked,
            "Blocked task is re-selected by the scan and retried"
        );

        // Lift the cap: the next cycle's retry now spawns the repo
        // orchestrator and completes the delegation task.
        state
            .budget_configs
            .set_config(
                "workspace:ws-1",
                &gyre_domain::BudgetConfig {
                    max_tokens_per_day: None,
                    max_cost_per_day: None,
                    max_concurrent_agents: Some(10),
                    max_agent_lifetime_secs: None,
                },
            )
            .await
            .unwrap();
        scheduler_run_once(&state).await.unwrap();
        let done = state
            .tasks
            .find_by_id(&delegation.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            done.status,
            TaskStatus::Done,
            "retry after transient failure completes the delegation task"
        );
    }

    // ── Review-finding regressions (task-115 repair round) ───────────────────

    /// Review finding F1: concurrent scheduler cycles (the 30 s loop plus
    /// `POST /api/v1/admin/jobs/task_scheduler/run`, whose
    /// `JobRegistry::trigger` runs the handler without serialization) must
    /// not double-process one Delegation task. The pre-fix claim was a
    /// non-atomic read-modify-write with no cycle serialization: two
    /// concurrently entered cycles could both claim the same Backlog task
    /// and decompose it twice (duplicate Implementation sub-tasks, later
    /// duplicate worker agents). The cycle run lock serializes whole
    /// cycles — both entries here run concurrently and produce exactly one
    /// sub-task set. (The mid-run race itself — one cycle scanning while
    /// another's LLM call is in flight — cannot be reproduced
    /// deterministically in-process, which is exactly why the
    /// serialization lock below is asserted directly rather than hoped
    /// for.)
    #[tokio::test]
    async fn concurrent_scheduler_cycles_do_not_double_process() {
        let state = chain_state().await;
        on_spec_approved(&state, &approved_payload()).await;

        let (a, b) = tokio::join!(scheduler_run_once(&state), scheduler_run_once(&state));
        a.unwrap();
        b.unwrap();

        let tasks = state.tasks.list().await.unwrap();
        let delegation = tasks
            .iter()
            .find(|t| t.task_type == Some(TaskType::Delegation))
            .expect("delegation task exists");
        assert_eq!(delegation.status, TaskStatus::Done);
        let subs: Vec<_> = tasks
            .iter()
            .filter(|t| {
                t.parent_task_id.as_ref() == Some(&delegation.id)
                    && t.task_type == Some(TaskType::Implementation)
            })
            .collect();
        assert_eq!(
            subs.len(),
            1,
            "exactly one sub-task set — no duplicate decomposition from concurrent cycles"
        );
    }

    /// Review finding F1, deterministic half: `scheduler_run_once` holds
    /// the registry's cycle lock for the whole scan-claim-process sequence,
    /// so a second cycle entered while one is in flight blocks on the same
    /// lock instead of scanning (and racing on the non-atomic Backlog
    /// claim). Fails if the cycle-lock acquisition is removed from
    /// `scheduler_run_once`.
    #[tokio::test]
    async fn scheduler_cycles_serialize_on_the_run_lock() {
        let state = chain_state().await;

        // Simulate an in-flight cycle: the run lock is held.
        let lock = state.orchestrator_registry.scheduler_run_lock().await;
        let guard = lock.lock().await;

        let bg = tokio::spawn(async move { scheduler_run_once(&state).await });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(
            !bg.is_finished(),
            "a second cycle must block while the first holds the run lock"
        );

        drop(guard);
        bg.await.unwrap().unwrap();
    }

    /// Review finding F1 (crash-recovery half): a Delegation task left
    /// InProgress by a dead process is re-claimed (stale claim) and
    /// processed exactly once — one sub-task set, delegation Done.
    #[tokio::test]
    async fn stale_claim_crash_recovery_processes_once() {
        let state = chain_state().await;
        on_spec_approved(&state, &approved_payload()).await;

        // Simulate a crashed process: the delegation was claimed
        // InProgress long ago and never completed.
        let mut delegation = state
            .tasks
            .list()
            .await
            .unwrap()
            .into_iter()
            .find(|t| t.task_type == Some(TaskType::Delegation))
            .expect("delegation task created");
        delegation
            .transition_status(TaskStatus::InProgress)
            .unwrap();
        delegation.updated_at = 0; // far in the past → stale
        state.tasks.update(&delegation).await.unwrap();

        scheduler_run_once(&state).await.unwrap();

        let tasks = state.tasks.list().await.unwrap();
        let delegation = tasks
            .iter()
            .find(|t| t.task_type == Some(TaskType::Delegation))
            .expect("delegation task exists");
        assert_eq!(
            delegation.status,
            TaskStatus::Done,
            "stale claim is recovered and completed"
        );
        let subs: Vec<_> = tasks
            .iter()
            .filter(|t| {
                t.parent_task_id.as_ref() == Some(&delegation.id)
                    && t.task_type == Some(TaskType::Implementation)
            })
            .collect();
        assert_eq!(subs.len(), 1, "recovered claim processes exactly once");
    }

    /// Review finding F2: the workspace orchestrator run must acknowledge
    /// ONLY the SpecApproved signal it consumed. The pre-fix
    /// `acknowledge_all` marked every inbox message processed — including
    /// Escalations the stale-agent detector delivers to a live workspace
    /// orchestrator, which no consumer had seen — hiding them from the
    /// documented crash-recovery poll
    /// (GET /api/v1/agents/:id/messages?acknowledged=false).
    #[tokio::test]
    async fn orchestrator_run_does_not_ack_escalations() {
        let state = chain_state().await;
        on_spec_approved(&state, &approved_payload()).await;

        let orch = state
            .agents
            .list_by_workspace(&Id::new("ws-1"))
            .await
            .unwrap()
            .into_iter()
            .find(|a| a.orchestrator_type == OrchestratorType::WorkspaceOrchestrator)
            .expect("workspace orchestrator spawned");

        // Deliver an Escalation to the live orchestrator's inbox (as the
        // stale-agent detector does on repo-orchestrator death).
        state
            .emit_event(
                Id::new("ws-1"),
                Destination::Agent(orch.id.clone()),
                MessageKind::Escalation,
                Some(serde_json::json!({
                    "event": "repo_orchestrator_dead",
                    "agent_id": "agent-x",
                })),
            )
            .await;

        // A second, different approval reuses the orchestrator and runs it.
        let mut second = approved_payload();
        second["approval_id"] = serde_json::json!("approval-2");
        on_spec_approved(&state, &second).await;

        let unacked = state.messages.list_unacked(&orch.id, 100).await.unwrap();
        assert!(
            unacked.iter().any(|m| m.kind == MessageKind::Escalation),
            "escalation must stay unacked for its real consumer, got {unacked:?}"
        );
        assert!(
            unacked.iter().all(|m| m.kind != MessageKind::SpecApproved),
            "both SpecApproved signals were consumed and acked by their runs"
        );
    }

    /// Review finding F3(2): a Delegation task whose spec has no approved
    /// SHA in the ledger must NOT be decomposed against a malformed
    /// `path@` spec_ref (empty SHA) — "cannot determine the approved blob"
    /// is not "empty blob is fine". The run fails, the claim is released to
    /// Blocked for retry, and no sub-tasks are created.
    #[tokio::test]
    async fn delegation_without_resolvable_sha_stays_blocked() {
        let state = chain_state().await;

        let mut t = Task::new(Id::new("del-nosha"), "Delegation without ledger SHA", 0);
        t.task_type = Some(TaskType::Delegation);
        t.repo_id = Id::new("r-1");
        t.workspace_id = Id::new("ws-1");
        t.spec_path = Some("specs/system/never-approved.md".to_string());
        state.tasks.create(&t).await.unwrap();

        scheduler_run_once(&state).await.unwrap();

        let after = state
            .tasks
            .find_by_id(&Id::new("del-nosha"))
            .await
            .unwrap()
            .expect("task survives");
        assert_eq!(
            after.status,
            TaskStatus::Blocked,
            "unpinnable delegation is released for retry, not completed"
        );
        let tasks = state.tasks.list().await.unwrap();
        let subs: Vec<_> = tasks
            .iter()
            .filter(|t| t.parent_task_id.as_ref() == Some(&Id::new("del-nosha")))
            .collect();
        assert!(
            subs.is_empty(),
            "no sub-tasks pinned to a malformed empty-SHA spec_ref"
        );
    }

    /// Review finding F2 (task-115 round 3): the sibling branch of
    /// `delegation_without_resolvable_sha_stays_blocked`. A spec_ref that
    /// carries a `@` but an EMPTY SHA (e.g. "specs/foo.md@", reachable via
    /// POST /api/v1/tasks or MCP task.create, which do not shape-validate
    /// spec_ref) must fail the same way: no decomposition against a blob
    /// that cannot be identified, claim released to Blocked, no sub-tasks
    /// pinned to the malformed "path@" spec_ref.
    #[tokio::test]
    async fn delegation_with_empty_sha_spec_ref_stays_blocked() {
        let state = chain_state().await;

        let mut t = Task::new(
            Id::new("del-emptysha"),
            "Delegation with empty-SHA spec_ref",
            0,
        );
        t.task_type = Some(TaskType::Delegation);
        t.repo_id = Id::new("r-1");
        t.workspace_id = Id::new("ws-1");
        t.spec_path = Some("specs/system/auth.md".to_string());
        t.spec_ref = Some("specs/system/auth.md@".to_string());
        state.tasks.create(&t).await.unwrap();

        scheduler_run_once(&state).await.unwrap();

        let after = state
            .tasks
            .find_by_id(&Id::new("del-emptysha"))
            .await
            .unwrap()
            .expect("task survives");
        assert_eq!(
            after.status,
            TaskStatus::Blocked,
            "delegation with a malformed empty-SHA spec_ref is released for retry, not completed"
        );
        let tasks = state.tasks.list().await.unwrap();
        let subs: Vec<_> = tasks
            .iter()
            .filter(|t| t.parent_task_id.as_ref() == Some(&Id::new("del-emptysha")))
            .collect();
        assert!(
            subs.is_empty(),
            "no sub-tasks fabricated from an unpinnable empty-SHA spec_ref"
        );
    }

    /// Phase 2 step 1 (agent-runtime.md §1): the workspace orchestrator
    /// READS the approved spec content and the read is consumed — the
    /// delegation task's description carries the title/section/size
    /// summary derived from the real blob (the pre-fix code discarded the
    /// read result: a decorative git subprocess per approval).
    #[tokio::test]
    async fn delegation_description_carries_spec_summary() {
        let state = chain_state().await;

        // A real git repo holding the approved spec blob.
        let tmp = tempfile::tempdir().unwrap();
        let repo_path = tmp.path();
        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@gyre.dev"],
            vec!["config", "user.name", "Test"],
        ] {
            std::process::Command::new("git")
                .args(&args)
                .current_dir(repo_path)
                .output()
                .unwrap();
        }
        std::fs::create_dir_all(repo_path.join("specs/system")).unwrap();
        std::fs::write(
            repo_path.join("specs/system/auth.md"),
            "# Auth Spec Title\n\nIntro.\n\n## Section One\n\nBody.\n\n## Section Two\n\nBody.\n",
        )
        .unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(repo_path)
            .output()
            .unwrap();
        let sha = String::from_utf8(
            std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(repo_path)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_string();

        // Point repo r-1 at the real git repo and pin the ledger SHA.
        let mut repo = state
            .repos
            .find_by_id(&Id::new("r-1"))
            .await
            .unwrap()
            .unwrap();
        repo.path = repo_path.to_string_lossy().to_string();
        state.repos.update(&repo).await.unwrap();
        let mut entry = state
            .spec_ledger
            .find_by_path("specs/system/auth.md")
            .await
            .unwrap()
            .expect("ledger entry seeded");
        entry.current_sha = sha.clone();
        state.spec_ledger.save(&entry).await.unwrap();

        let mut payload = approved_payload();
        payload["spec_sha"] = serde_json::json!(sha);
        on_spec_approved(&state, &payload).await;

        let tasks = state.tasks.list().await.unwrap();
        let delegation = tasks
            .iter()
            .find(|t| t.task_type == Some(TaskType::Delegation))
            .expect("delegation task created");
        let description = delegation.description.as_deref().unwrap_or("");
        assert!(
            description.contains("Auth Spec Title"),
            "delegation description carries the spec title derived from the read blob: {description}"
        );
        assert!(
            description.contains("2 sections"),
            "delegation description carries the section count: {description}"
        );
    }

    /// Phase 3 step 2 (agent-runtime.md §1): the repo orchestrator's
    /// decomposition input includes the repo's current codebase state —
    /// the tracked file tree at HEAD, read via a real git subprocess.
    #[tokio::test]
    async fn read_repo_tree_lists_tracked_files() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_path = tmp.path();
        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@gyre.dev"],
            vec!["config", "user.name", "Test"],
        ] {
            std::process::Command::new("git")
                .args(&args)
                .current_dir(repo_path)
                .output()
                .unwrap();
        }
        std::fs::create_dir_all(repo_path.join("src")).unwrap();
        std::fs::write(repo_path.join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(repo_path.join("README.md"), "# t\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(repo_path)
            .output()
            .unwrap();

        let tree = read_repo_tree("git", &repo_path.to_string_lossy(), 200)
            .await
            .expect("tracked files listed at HEAD");
        assert!(tree.contains("src/main.rs"), "tree: {tree}");
        assert!(tree.contains("README.md"), "tree: {tree}");

        // Bounded listing marks truncation explicitly.
        let truncated = read_repo_tree("git", &repo_path.to_string_lossy(), 1)
            .await
            .unwrap();
        assert!(
            truncated.contains("more files"),
            "bounded listing marks truncation: {truncated}"
        );

        // Unreadable repo (no git dir) → None; the caller degrades to
        // spec-content-only decomposition.
        let none = read_repo_tree("git", "/nonexistent/gyre-path", 200).await;
        assert!(none.is_none());
    }
}
