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
            Some(workspace_id.clone()),
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

    // Phase 4 (agent-runtime.md §1): worker agent spawning for
    // Implementation tasks the cycles above may just have decomposed (and
    // for any other Backlog Implementation task). Same cycle lock: a
    // worker spawn claim can never interleave with another cycle's claim.
    spawn_implementation_workers(state).await;
    Ok(())
}

// ── Phase 4: task scheduler → worker agent spawning ─────────────────────────

/// Task label flagging a Backlog task that could not spawn because the
/// workspace budget is exhausted (agent-runtime.md §1 Phase 4 step 3:
/// "task stays Backlog with a BudgetExhausted flag"). Cleared when a later
/// cycle spawns the task successfully.
const BUDGET_EXHAUSTED_LABEL: &str = "budget-exhausted";

/// Exponential backoff between worker spawn attempts, in seconds
/// (agent-runtime.md §1 Phase 7 "Spawn failure": 3 retries at 1s / 5s /
/// 30s, then the task is Blocked with a priority-1 notification).
pub(crate) const SPAWN_RETRY_BACKOFF_SECS: [u64; 3] = [1, 5, 30];

/// One worker-spawn cycle (agent-runtime.md §1 Phase 4).
///
/// Detects `Backlog` tasks with `task_type: Implementation`, checks the
/// spec's preconditions (repo not Archived, `depends_on` complete, `order`
/// next, `max_agents` limit, workspace budget) and claims + spawns each
/// eligible task. Ineligible tasks stay `Backlog` — the scheduler
/// re-checks periodically as agents complete (queuing, not rejection).
///
/// Runs under the scheduler cycle lock (the caller holds it), so two
/// cycles can never both claim the same Backlog task.
async fn spawn_implementation_workers(state: &Arc<AppState>) {
    // Crash recovery first (review F2, task-118): release stale InProgress
    // claims whose agent is not Active — stranded by a dead process — so
    // the Backlog scan below (and later cycles) can re-spawn them.
    recover_stale_worker_claims(state).await;

    let now = crate::api::now_secs();
    let backlog = match state.tasks.list_by_status(&TaskStatus::Backlog).await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("signal-chain: worker scan failed: {e}");
            return;
        }
    };

    for task in backlog {
        if task.task_type != Some(TaskType::Implementation) {
            continue; // Delegation/Coordination → orchestrators; untyped → never spawned
        }
        let Some(repo) = resolve_task_repo(state, &task).await else {
            continue;
        };

        // Precondition: repo not Archived (repo-lifecycle.md §4). The task
        // stays Backlog — archiving cancels active tasks separately, and an
        // unarchived repo resumes spawning on a later cycle.
        if repo.is_archived() {
            tracing::debug!(
                task_id = %task.id,
                repo_id = %repo.id,
                "signal-chain: repo archived; worker spawn deferred"
            );
            continue;
        }

        // Ordering: every depends_on task resolved (Done or Cancelled —
        // a cancelled dependency never completes, blocking forever).
        if !depends_on_satisfied(state, &task).await {
            continue;
        }
        // Ordering: this task's `order` is next — no incomplete sibling
        // with a lower order value in the same decomposition group.
        if !order_is_next(state, &task).await {
            continue;
        }

        // Precondition: max_agents per repo (repo override → workspace
        // fallback → unlimited). At the limit the task is QUEUED (stays
        // Backlog; the scheduler re-checks as agents complete) — never
        // rejected.
        let limit = effective_max_agents(state, &repo).await;
        if let Some(limit) = limit {
            match count_active_agents_for_repo(state, &repo.id).await {
                Ok(active) if active >= limit as usize => {
                    tracing::debug!(
                        task_id = %task.id,
                        repo_id = %repo.id,
                        active,
                        limit,
                        "signal-chain: repo at max_agents; task queued"
                    );
                    continue;
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(task_id = %task.id, "signal-chain: agent count failed: {e}");
                    continue;
                }
            }
        }

        // Precondition: workspace budget. Exhausted → the task stays
        // Backlog with the BudgetExhausted flag (a label on the task —
        // the observable marker the spec calls for), cleared on the
        // successful spawn of a later cycle.
        let budget_check = match crate::api::budget::check_spawn_budget(
            state,
            &repo.workspace_id.to_string(),
        )
        .await
        {
            Ok(()) => crate::api::budget::check_repo_spawn_budget(state, &repo).await,
            Err(reason) => Err(reason),
        };
        if let Err(reason) = budget_check {
            // Flag the FRESHEST row (same stale-snapshot defense as the
            // claim below): persisting the snapshot taken at
            // list_by_status could revert a concurrent status change
            // (e.g. a cancellation) while merely adding the label.
            match state.tasks.find_by_id(&task.id).await {
                Ok(Some(current))
                    if current.status == TaskStatus::Backlog
                        && !current.labels.iter().any(|l| l == BUDGET_EXHAUSTED_LABEL) =>
                {
                    let mut flagged = current;
                    flagged.labels.push(BUDGET_EXHAUSTED_LABEL.to_string());
                    flagged.updated_at = now;
                    let _ = state.tasks.update(&flagged).await;
                    tracing::info!(
                        task_id = %task.id,
                        workspace_id = %repo.workspace_id,
                        reason = %reason,
                        "signal-chain: budget exhausted; task flagged and left Backlog"
                    );
                }
                _ => {}
            }
            continue;
        }

        // Defense in depth against a concurrent API mutation inside this
        // cycle's await window: the `task` snapshot predates the repo /
        // ordering / budget awaits above, and claiming by whole-row
        // persisting that snapshot would silently overwrite a task that
        // was cancelled, completed, or edited in the meantime (the same
        // re-read-after-lock defense the orchestrator loop applies before
        // decomposition). Only a still-Backlog Implementation task may be
        // claimed from the freshest row.
        let task = match state.tasks.find_by_id(&task.id).await {
            Ok(Some(current))
                if current.status == TaskStatus::Backlog
                    && current.task_type == Some(TaskType::Implementation) =>
            {
                current
            }
            Ok(Some(current)) => {
                tracing::debug!(
                    task_id = %task.id,
                    task_status = ?current.status,
                    "signal-chain: task changed during the spawn cycle; skipping"
                );
                continue;
            }
            Ok(None) => {
                tracing::debug!(
                    task_id = %task.id,
                    "signal-chain: task disappeared during the spawn cycle; skipping"
                );
                continue;
            }
            Err(e) => {
                tracing::warn!(
                    task_id = %task.id,
                    "signal-chain: task re-read before claim failed: {e}"
                );
                continue;
            }
        };

        // Claim: Backlog → InProgress so no concurrent cycle (or the 30 s
        // loop re-entering) can double-spawn this task. The background
        // retry task keeps the claim fresh (each attempt touches the task)
        // so the stale-claim recovery never re-claims mid-retry.
        let mut claimed = task.clone();
        if let Err(e) = claimed.transition_status(TaskStatus::InProgress) {
            tracing::warn!(task_id = %task.id, "signal-chain: worker claim failed: {e}");
            continue;
        }
        claimed.updated_at = now;
        // Clear the budget flag: this cycle is spending budget on the task.
        claimed.labels.retain(|l| l != BUDGET_EXHAUSTED_LABEL);
        if let Err(e) = state.tasks.update(&claimed).await {
            tracing::warn!(task_id = %task.id, "signal-chain: worker claim persist failed: {e}");
            continue;
        }

        // First spawn attempt synchronously in the cycle (its effect —
        // agent record, worktree, task assignment — is the cycle's
        // observable outcome); the Phase 7 retry backoffs (1s/5s/30s) run
        // off-cycle in the background so they never hold the cycle lock.
        // The claim above is the concurrency guard for both.
        let state = Arc::clone(state);
        let repo = repo.clone();
        let task = claimed;
        match try_spawn_worker(&state, &repo, &task, 0).await {
            WorkerSpawnOutcome::Launched => {}
            WorkerSpawnOutcome::Blocked(reason) => {
                release_claim(&state, &task, &reason).await;
            }
            WorkerSpawnOutcome::LaunchFailed(reason) => {
                tokio::spawn(async move {
                    retry_worker_spawn(state, repo, task, reason).await;
                });
            }
        }
    }
}

/// Crash recovery for claimed Implementation tasks (agent-runtime.md §1
/// Phase 4/7; review F2, task-118). A task left InProgress by a dead
/// process — e.g. a server crash during the spawn-retry backoff, when the
/// last attempt's agent is already `Failed` so the stale-agent detector
/// has no Active agent to reap — would be stranded forever: the worker
/// scan selects only Backlog, and the orchestrator stale-claim recovery
/// deliberately skips Implementation tasks. Such claims are released back
/// to Backlog through the domain's dedicated `Task::release_claim` edge
/// (the only InProgress→Backlog route), so a later cycle re-spawns them.
///
/// A claim held by an **Active** agent is never released: task rows are
/// not touched while a worker runs (only the claim and the completion
/// transition write them), so task age alone must not requeue live work.
/// A crashed-but-still-Active agent is first reaped by the stale-agent
/// detector (heartbeat timeout → Dead), after which the next cycle
/// releases the orphaned claim here.
async fn recover_stale_worker_claims(state: &Arc<AppState>) {
    let now = crate::api::now_secs();
    let stale_before = now.saturating_sub(SCHEDULER_CLAIM_STALENESS_SECS);
    let claimed = match state.tasks.list_by_status(&TaskStatus::InProgress).await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("signal-chain: stale worker-claim scan failed: {e}");
            return;
        }
    };
    for task in claimed
        .into_iter()
        .filter(|t| t.task_type == Some(TaskType::Implementation))
        .filter(|t| t.updated_at <= stale_before)
    {
        // An Active agent holds the claim — never release it from under a
        // live worker.
        if let Some(aid) = task.assigned_to.as_ref() {
            if let Ok(Some(a)) = state.agents.find_by_id(aid).await {
                if a.status == AgentStatus::Active {
                    continue;
                }
            }
        }
        tracing::warn!(
            task_id = %task.id,
            agent_id = ?task.assigned_to,
            "signal-chain: stale worker claim with no active agent; releasing to Backlog (crash recovery)"
        );
        release_claim(state, &task, "stale worker claim (crash recovery)").await;
    }
}

/// Resolve a task's repo, warning once for unresolvable rows (a Backlog
/// task whose repo vanished would otherwise retry-scan forever silently).
async fn resolve_task_repo(state: &Arc<AppState>, task: &Task) -> Option<Repository> {
    if task.repo_id.as_str().is_empty() {
        return None;
    }
    match state.repos.find_by_id(&task.repo_id).await {
        Ok(Some(repo)) => Some(repo),
        Ok(None) => {
            tracing::warn!(
                task_id = %task.id,
                repo_id = %task.repo_id,
                "signal-chain: worker task has no resolvable repo; leaving Backlog"
            );
            None
        }
        Err(e) => {
            tracing::warn!(task_id = %task.id, "signal-chain: repo lookup failed: {e}");
            None
        }
    }
}

/// All `depends_on` tasks resolved: Done or Cancelled.
async fn depends_on_satisfied(state: &Arc<AppState>, task: &Task) -> bool {
    for dep_id in &task.depends_on {
        match state.tasks.find_by_id(dep_id).await {
            Ok(Some(dep)) => {
                if !matches!(dep.status, TaskStatus::Done | TaskStatus::Cancelled) {
                    tracing::debug!(
                        task_id = %task.id,
                        dep_id = %dep_id,
                        dep_status = ?dep.status,
                        "signal-chain: dependency incomplete; worker spawn deferred"
                    );
                    return false;
                }
            }
            Ok(None) => {
                // A deleted dependency can never complete — treat as
                // satisfied rather than blocking the task forever.
                tracing::warn!(
                    task_id = %task.id,
                    dep_id = %dep_id,
                    "signal-chain: depends_on task missing; treating as resolved"
                );
            }
            Err(e) => {
                tracing::warn!(task_id = %task.id, "signal-chain: dependency lookup failed: {e}");
                return false;
            }
        }
    }
    true
}

/// The task's `order` is the next to run: no incomplete task in the same
/// repo + decomposition group (parent_task_id) with a strictly lower
/// order. Tasks without an order never block ordered siblings.
async fn order_is_next(state: &Arc<AppState>, task: &Task) -> bool {
    let Some(order) = task.order else {
        return true;
    };
    let siblings = match state.tasks.list_by_repo(&task.repo_id).await {
        Ok(t) => t,
        Err(_) => return true, // cannot determine ordering — do not starve the task
    };
    for sib in siblings {
        if sib.id == task.id {
            continue;
        }
        if sib.parent_task_id != task.parent_task_id {
            continue;
        }
        let Some(sib_order) = sib.order else {
            continue;
        };
        if sib_order < order && !matches!(sib.status, TaskStatus::Done | TaskStatus::Cancelled) {
            tracing::debug!(
                task_id = %task.id,
                blocking_task_id = %sib.id,
                blocking_order = sib_order,
                "signal-chain: lower-order sibling incomplete; worker spawn deferred"
            );
            return false;
        }
    }
    true
}

/// Effective per-repo agent limit: `Repository.max_agents` →
/// `Workspace.max_agents_per_repo` → None (unlimited) — agent-runtime.md
/// §1 Phase 4 / §4 Cascade fallback chain.
async fn effective_max_agents(state: &Arc<AppState>, repo: &Repository) -> Option<u32> {
    if let Some(max) = repo.max_agents {
        return Some(max);
    }
    state
        .workspaces
        .find_by_id(&repo.workspace_id)
        .await
        .ok()
        .flatten()
        .and_then(|ws| ws.max_agents_per_repo)
}

/// Count Active worker agents scoped to a repo. Workers link repos via
/// their worktree records (only orchestrators carry `repo_id` directly),
/// so the count is the number of distinct agents with a worktree in the
/// repo whose status is Active.
async fn count_active_agents_for_repo(
    state: &Arc<AppState>,
    repo_id: &Id,
) -> anyhow::Result<usize> {
    let worktrees = state.worktrees.find_by_repo(repo_id).await?;
    let mut agent_ids: Vec<Id> = Vec::new();
    for wt in worktrees {
        if !agent_ids.contains(&wt.agent_id) {
            agent_ids.push(wt.agent_id);
        }
    }
    let mut active = 0;
    for aid in agent_ids {
        if let Ok(Some(a)) = state.agents.find_by_id(&aid).await {
            if a.status == AgentStatus::Active {
                active += 1;
            }
        }
    }
    Ok(active)
}

/// The system principal the scheduler spawns workers as. Same shape as
/// the global-token auth path (auth.rs): agent_id "system", no user row,
/// no JWT claims — ABAC checks bypass for it, and spawn-side tenant
/// containment treats it as the bootstrap principal.
fn scheduler_system_auth() -> crate::auth::AuthenticatedAgent {
    crate::auth::AuthenticatedAgent {
        agent_id: "system".to_string(),
        user_id: None,
        roles: vec![gyre_domain::UserRole::Admin],
        tenant_id: crate::auth::system_principal_tenant(),
        jwt_claims: None,
        bearer_token: None,
        deprecated_token_auth: false,
        impersonation: None,
    }
}

/// Outcome of one worker spawn attempt.
#[derive(Debug)]
enum WorkerSpawnOutcome {
    /// The agent process launched on the compute target.
    Launched,
    /// Spawning is blocked by a precondition (budget, task/repo state) —
    /// release the claim, no retry.
    Blocked(String),
    /// `ComputeTarget::spawn_process()` failed — retry with backoff.
    LaunchFailed(String),
}

/// Retry a failed worker spawn, applying the Phase 7 spawn-failure policy:
/// up to 3 retries with exponential backoff (1s / 5s / 30s), then the task
/// is Blocked, the orchestrator notified, and a priority-1 notification
/// created for workspace admins. Attempt 0 already failed (`reason`) —
/// this loop covers attempts 1..=3.
pub(crate) async fn retry_worker_spawn(
    state: Arc<AppState>,
    repo: Repository,
    task: Task,
    first_reason: String,
) {
    let max_attempts = SPAWN_RETRY_BACKOFF_SECS.len();
    let mut reason = first_reason;
    for attempt in 1..=max_attempts {
        let backoff = SPAWN_RETRY_BACKOFF_SECS[attempt - 1];
        tracing::warn!(
            task_id = %task.id,
            attempt,
            backoff_secs = backoff,
            reason = %reason,
            "signal-chain: worker spawn failed; retrying with backoff"
        );
        // Keep the claim fresh so the stale-claim recovery (60 s) never
        // re-claims a task whose retry is still sleeping.
        touch_task(&state, &task.id).await;
        tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;

        match try_spawn_worker(&state, &repo, &task, attempt).await {
            WorkerSpawnOutcome::Launched => return,
            WorkerSpawnOutcome::Blocked(blocked) => {
                release_claim(&state, &task, &blocked).await;
                return;
            }
            WorkerSpawnOutcome::LaunchFailed(r) => reason = r,
        }
    }
    final_spawn_failure(&state, &repo, &task, &reason).await;
}

/// One spawn attempt through the shared spawn core. On launch failure the
/// just-created agent record is torn down (Failed + token removed + budget
/// counter released) so a failed attempt leaves no orphaned Active agent.
async fn try_spawn_worker(
    state: &Arc<AppState>,
    repo: &Repository,
    task: &Task,
    attempt: usize,
) -> WorkerSpawnOutcome {
    // Unique per attempt: a failed attempt's agent record keeps its name.
    // Char-boundary-safe prefix (task ids are server-generated UUIDs, but
    // the repo invariant forbids fixed byte-index slicing on principle).
    let id_prefix: String = task.id.as_str().chars().take(8).collect();
    let name = format!("worker-{id_prefix}-a{attempt}");
    let branch = task
        .branch
        .clone()
        .unwrap_or_else(|| format!("task/{id_prefix}"));
    let req = crate::api::spawn::SpawnAgentRequest {
        name,
        repo_id: repo.id.to_string(),
        task_id: task.id.to_string(),
        branch,
        parent_id: task.parent_task_id.as_ref().map(|p| p.to_string()),
        compute_target_id: None, // system spawn: workspace/tenant resolution only
        disconnected_behavior: None,
        loop_config: None,
        agent_type: None,
        conversation_sha: None,
    };
    match crate::api::spawn::spawn_agent_core(
        state,
        req,
        &scheduler_system_auth(),
        "task scheduler (agent-runtime.md §1 Phase 4)",
    )
    .await
    {
        Ok(resp) if resp.process_launched => WorkerSpawnOutcome::Launched,
        Ok(resp) => {
            let reason =
                "compute target spawn_process failed (agent-runtime.md §1 Phase 7)".to_string();
            teardown_failed_spawn(state, &Id::new(&resp.agent.id), repo).await;
            WorkerSpawnOutcome::LaunchFailed(reason)
        }
        // Budget exhausted mid-flight (race between the pre-check and the
        // spawn core's own check): release the claim with the flag set.
        Err(crate::api::error::ApiError::TooManyRequests(reason)) => {
            WorkerSpawnOutcome::Blocked(reason)
        }
        Err(e) => WorkerSpawnOutcome::LaunchFailed(format!("{e:?}")),
    }
}

/// Tear down the agent record a failed launch left behind: Failed status,
/// token removed, budget counter released.
async fn teardown_failed_spawn(state: &Arc<AppState>, agent_id: &Id, repo: &Repository) {
    if let Ok(Some(mut agent)) = state.agents.find_by_id(agent_id).await {
        let _ = agent.transition_status(AgentStatus::Failed);
        let _ = state.agents.update(&agent).await;
    }
    let _ = state
        .kv_store
        .kv_remove("agent_tokens", agent_id.as_str())
        .await;
    crate::api::budget::decrement_active_agents(state, &repo.workspace_id.to_string()).await;
}

/// Release a worker claim (precondition blocked mid-flight, after the
/// claim was taken — e.g. workspace budget exhausted between the
/// scheduler's pre-check and the spawn core's own check). The task
/// returns to Backlog via `Task::release_claim` — the domain's ONLY
/// InProgress→Backlog edge, reserved for this system-side release — so
/// agent-runtime.md §1 Phase 4 step 3 ("task stays Backlog with a
/// BudgetExhausted flag") holds on this path exactly as on the
/// pre-check path, and the next cycle's Backlog scan re-spawns the
/// task once the precondition clears.
async fn release_claim(state: &Arc<AppState>, task: &Task, reason: &str) {
    // Re-read the freshest row: the claim snapshot predates the spawn
    // attempt's awaits, and releasing a task that was cancelled,
    // completed, or otherwise mutated in the meantime must not
    // overwrite that change.
    let Ok(Some(mut t)) = state.tasks.find_by_id(&task.id).await else {
        return;
    };
    if t.status != TaskStatus::InProgress {
        tracing::debug!(
            task_id = %task.id,
            task_status = ?t.status,
            reason,
            "signal-chain: worker claim already changed; not releasing"
        );
        return;
    }
    let now = crate::api::now_secs();
    if let Err(e) = t.release_claim(now) {
        tracing::error!(
            task_id = %task.id,
            reason,
            "signal-chain: could not release worker claim to Backlog: {e}"
        );
        return;
    }
    // Budget-blocked releases carry the flag so the observable marker
    // matches the pre-check path (spawn core reasons: "workspace budget
    // exceeded: ..." / "tenant budget exceeded: ...").
    if (reason.starts_with("workspace budget") || reason.starts_with("tenant budget"))
        && !t.labels.iter().any(|l| l == BUDGET_EXHAUSTED_LABEL)
    {
        t.labels.push(BUDGET_EXHAUSTED_LABEL.to_string());
    }
    if let Err(e) = state.tasks.update(&t).await {
        tracing::error!(
            task_id = %task.id,
            reason,
            "signal-chain: persisting released worker claim failed: {e}"
        );
        return;
    }
    tracing::info!(
        task_id = %task.id,
        reason,
        "signal-chain: worker claim released to Backlog; a later cycle re-spawns"
    );
}

/// Bump a task's updated_at so the stale-claim recovery window restarts.
/// Shared with the Ralph loop's re-spawn retry ladder, which must keep its
/// claim fresh across the same 1s/5s/30s backoff sleeps this module's own
/// retry ladder touches through.
pub(crate) async fn touch_task(state: &Arc<AppState>, task_id: &Id) {
    if let Ok(Some(mut t)) = state.tasks.find_by_id(task_id).await {
        t.updated_at = crate::api::now_secs();
        let _ = state.tasks.update(&t).await;
    }
}

/// All spawn retries exhausted (agent-runtime.md §1 Phase 7 "Spawn
/// failure"): task Blocked, the failed attempt's agent already Failed,
/// orchestrator notified via TaskBlocked, priority-1 notification created
/// for workspace admins ("Compute target unavailable: {target}").
async fn final_spawn_failure(state: &Arc<AppState>, repo: &Repository, task: &Task, reason: &str) {
    let now = crate::api::now_secs();
    let mut blocked = task.clone();
    let _ = blocked.transition_status(TaskStatus::Blocked);
    blocked.updated_at = now;
    let _ = state.tasks.update(&blocked).await;

    // TaskBlocked → repo orchestrator inbox (spawning one when none is
    // live — same routing as SpecApproved).
    crate::ralph_loop::notify_task_blocked(
        state,
        &blocked,
        Some(repo),
        format!("worker spawn failed after all retries: {reason}"),
    )
    .await;

    // Priority-1 notification for workspace admins.
    notify_compute_target_unavailable(state, repo, &blocked, reason).await;
    tracing::error!(
        task_id = %blocked.id,
        repo_id = %repo.id,
        reason,
        "signal-chain: worker spawn failed after all retries; task Blocked"
    );
}

/// Priority-1 `ComputeTargetUnavailable` notification for workspace admins
/// (agent-runtime.md §1 Phase 7 "Spawn failure": "priority-1 notification
/// created for workspace admins"). Recipients: workspace Owner/Admin
/// members (same resolution as the budget notifications). Shared by the
/// scheduler's first-spawn retry ladder ([`final_spawn_failure`]) and the
/// Ralph loop's re-spawn retry ladder — the spawn-failure policy is not
/// path-scoped.
pub(crate) async fn notify_compute_target_unavailable(
    state: &Arc<AppState>,
    repo: &Repository,
    task: &Task,
    reason: &str,
) {
    let target_name = state
        .workspaces
        .find_by_id(&repo.workspace_id)
        .await
        .ok()
        .flatten()
        .and_then(|ws| ws.compute_target_id.map(|ct| ct.to_string()))
        .unwrap_or_else(|| "local".to_string());
    let mut recipients: Vec<Id> = Vec::new();
    if let Ok(members) = state
        .workspace_memberships
        .list_by_workspace(&repo.workspace_id)
        .await
    {
        recipients.extend(
            members
                .into_iter()
                .filter(|m| {
                    matches!(
                        m.role,
                        gyre_domain::WorkspaceRole::Owner | gyre_domain::WorkspaceRole::Admin
                    )
                })
                .map(|m| m.user_id),
        );
    }
    let title = format!("Compute target unavailable: {target_name}");
    for user_id in recipients {
        crate::notifications::notify_rich(
            state.as_ref(),
            repo.workspace_id.clone(),
            user_id,
            gyre_common::NotificationType::ComputeTargetUnavailable,
            title.clone(),
            "default",
            Some(
                serde_json::json!({
                    "task_id": task.id.to_string(),
                    "repo_id": repo.id.to_string(),
                    "reason": reason,
                    "target": target_name,
                })
                .to_string(),
            ),
            Some(task.id.to_string()),
            Some(repo.id.to_string()),
            "task",
            None,
        )
        .await;
    }
}

/// Ensure a live repo orchestrator exists for a failure notification
/// (agent-runtime.md §1 Phase 7: "if no orchestrator is active, the
/// server spawns one to process the failure"). Returns None when one
/// cannot be ensured (budget, spawn failure) — the caller falls back to a
/// workspace-destined message.
pub async fn ensure_repo_orchestrator_for_failure(
    state: &Arc<AppState>,
    repo: &Repository,
) -> Option<Agent> {
    if let Ok(Some(existing)) = find_live_repo_orchestrator(state, repo).await {
        return Some(existing);
    }
    match crate::api::orchestrator::spawn_repo_orchestrator_core(
        state,
        &repo.id.to_string(),
        crate::api::orchestrator::SpawnOrchestratorRequest::default(),
        "system",
    )
    .await
    {
        Ok((agent, _token, _launch)) => Some(agent),
        Err(e) => {
            tracing::warn!(
                repo_id = %repo.id,
                "signal-chain: could not spawn repo orchestrator for failure routing: {e:?}"
            );
            None
        }
    }
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

        // Phase 4 (task-118): the SAME cycle now claims Implementation
        // sub-tasks and spawns worker agents for them — the decomposition
        // above and the worker spawn below run in one scheduler cycle, so
        // a just-decomposed spec flows straight to an agent.
        scheduler_run_once(&state).await.unwrap();
        let after = state.tasks.list().await.unwrap();
        let sub = after
            .iter()
            .find(|t| t.task_type == Some(TaskType::Implementation))
            .expect("implementation sub-task exists");
        assert_eq!(
            sub.status,
            TaskStatus::InProgress,
            "implementation sub-task claimed by the Phase 4 worker path"
        );
        assert!(
            sub.assigned_to.is_some(),
            "implementation sub-task assigned to a spawned worker agent"
        );
        let worker = state
            .agents
            .find_by_id(sub.assigned_to.as_ref().unwrap())
            .await
            .unwrap()
            .expect("worker agent record exists");
        assert_eq!(worker.current_task_id, Some(sub.id.clone()));
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

    // ── Phase 4 (task-118): worker agent spawning ──────────────────────────

    /// Helper: a Backlog Implementation task in repo r-1 (workspace ws-1).
    async fn impl_task(state: &Arc<crate::AppState>, id: &str) -> Task {
        let mut t = Task::new(Id::new(id), "Implement thing", 0);
        t.task_type = Some(TaskType::Implementation);
        t.workspace_id = Id::new("ws-1");
        t.repo_id = Id::new("r-1");
        t.spec_path = Some("specs/system/auth.md".to_string());
        state.tasks.create(&t).await.unwrap();
        t
    }

    /// Phase 4 acceptance: the scheduler detects a Backlog Implementation
    /// task and spawns a worker agent for it mechanically — claim to
    /// InProgress, agent record created and assigned, worktree provisioned.
    /// (The process launch itself is best-effort in this environment; the
    /// spawn attempt's durable effects are what the scheduler owns.)
    #[tokio::test]
    async fn scheduler_spawns_worker_for_implementation_task() {
        let state = chain_state().await;
        let task = impl_task(&state, "impl-1").await;

        scheduler_run_once(&state).await.unwrap();

        let after = state.tasks.find_by_id(&task.id).await.unwrap().unwrap();
        assert_eq!(
            after.status,
            TaskStatus::InProgress,
            "Phase 4 claims the Implementation task"
        );
        let worker_id = after
            .assigned_to
            .as_ref()
            .expect("task assigned to spawned worker");
        let worker = state
            .agents
            .find_by_id(worker_id)
            .await
            .unwrap()
            .expect("worker agent record exists");
        assert_eq!(worker.current_task_id, Some(task.id.clone()));
        assert_eq!(worker.workspace_id, Id::new("ws-1"));
        let wts = state.worktrees.find_by_agent(&worker.id).await.unwrap();
        assert!(
            wts.iter().any(|wt| wt.task_id.as_ref() == Some(&task.id)),
            "worker has a worktree provisioned for the task"
        );
    }

    /// Phase 4 precondition: Archived repos never spawn workers — the task
    /// stays Backlog (queuing, not rejection; unarchiving resumes).
    #[tokio::test]
    async fn scheduler_skips_archived_repo() {
        let state = chain_state().await;
        let mut repo = state
            .repos
            .find_by_id(&Id::new("r-1"))
            .await
            .unwrap()
            .unwrap();
        repo.archive();
        state.repos.update(&repo).await.unwrap();
        let task = impl_task(&state, "impl-arch").await;

        scheduler_run_once(&state).await.unwrap();

        let after = state.tasks.find_by_id(&task.id).await.unwrap().unwrap();
        assert_eq!(after.status, TaskStatus::Backlog, "archived repo defers");
        assert!(after.assigned_to.is_none(), "no agent spawned");
    }

    /// Phase 4 ordering: a task whose depends_on dependency is incomplete
    /// stays Backlog; completing the dependency unblocks it.
    #[tokio::test]
    async fn scheduler_defers_until_depends_on_complete() {
        let state = chain_state().await;
        let mut dep = Task::new(Id::new("dep-1"), "Dependency", 0);
        dep.task_type = Some(TaskType::Implementation);
        dep.workspace_id = Id::new("ws-1");
        dep.repo_id = Id::new("r-1");
        state.tasks.create(&dep).await.unwrap();

        let mut task = Task::new(Id::new("impl-dep"), "Dependent", 0);
        task.task_type = Some(TaskType::Implementation);
        task.workspace_id = Id::new("ws-1");
        task.repo_id = Id::new("r-1");
        task.depends_on.push(dep.id.clone());
        state.tasks.create(&task).await.unwrap();

        scheduler_run_once(&state).await.unwrap();
        let after = state.tasks.find_by_id(&task.id).await.unwrap().unwrap();
        assert_eq!(after.status, TaskStatus::Backlog, "dependent task defers");
        assert!(after.assigned_to.is_none());

        // Resolve the dependency (Done) — the next cycle spawns. The
        // first cycle spawned the dependency ITSELF (it is an eligible
        // Backlog Implementation task), so walk it to Done from whichever
        // state that spawn left it in — a direct Backlog→InProgress
        // transition would be rejected as an invalid same-state edge when
        // the claim already happened.
        let mut done = state.tasks.find_by_id(&dep.id).await.unwrap().unwrap();
        if done.status == TaskStatus::Backlog {
            done.transition_status(TaskStatus::InProgress).unwrap();
        }
        if done.status == TaskStatus::InProgress {
            done.transition_status(TaskStatus::Review).unwrap();
        }
        done.transition_status(TaskStatus::Done).unwrap();
        state.tasks.update(&done).await.unwrap();

        scheduler_run_once(&state).await.unwrap();
        let after = state.tasks.find_by_id(&task.id).await.unwrap().unwrap();
        assert_eq!(
            after.status,
            TaskStatus::InProgress,
            "dependent task spawns once its dependency is Done"
        );
    }

    /// Phase 4 ordering: among ordered siblings of one decomposition group,
    /// only the next `order` value runs; the later one queues.
    #[tokio::test]
    async fn scheduler_defers_lower_order_sibling() {
        let state = chain_state().await;
        let parent = Id::new("deleg-1");
        let mut first = Task::new(Id::new("ord-0"), "First", 0);
        first.task_type = Some(TaskType::Implementation);
        first.workspace_id = Id::new("ws-1");
        first.repo_id = Id::new("r-1");
        first.parent_task_id = Some(parent.clone());
        first.order = Some(0);
        state.tasks.create(&first).await.unwrap();

        let mut second = Task::new(Id::new("ord-1"), "Second", 0);
        second.task_type = Some(TaskType::Implementation);
        second.workspace_id = Id::new("ws-1");
        second.repo_id = Id::new("r-1");
        second.parent_task_id = Some(parent.clone());
        second.order = Some(1);
        state.tasks.create(&second).await.unwrap();

        scheduler_run_once(&state).await.unwrap();

        let first_after = state.tasks.find_by_id(&first.id).await.unwrap().unwrap();
        let second_after = state.tasks.find_by_id(&second.id).await.unwrap().unwrap();
        assert_eq!(first_after.status, TaskStatus::InProgress, "order 0 runs");
        assert_eq!(
            second_after.status,
            TaskStatus::Backlog,
            "order 1 queues behind the incomplete order-0 sibling"
        );
    }

    /// Phase 4 precondition: max_agents. The repo override queues the task
    /// when the repo already runs its limit of Active agents; the
    /// workspace-level fallback applies when the repo has no override.
    #[tokio::test]
    async fn scheduler_queues_at_max_agents() {
        let state = chain_state().await;

        // Seed one Active agent with a worktree in r-1 (workers link repos
        // via worktrees).
        let mut existing = Agent::new(Id::new("busy-1"), "busy-worker", 0);
        existing.transition_status(AgentStatus::Active).unwrap();
        existing.workspace_id = Id::new("ws-1");
        state.agents.create(&existing).await.unwrap();
        state
            .worktrees
            .create(&gyre_domain::AgentWorktree::new(
                Id::new("wt-busy"),
                existing.id.clone(),
                Id::new("r-1"),
                None,
                "task/branch",
                "/tmp/wt-busy",
                0,
            ))
            .await
            .unwrap();

        // Repo override: limit 1 (already taken).
        let mut repo = state
            .repos
            .find_by_id(&Id::new("r-1"))
            .await
            .unwrap()
            .unwrap();
        repo.max_agents = Some(1);
        state.repos.update(&repo).await.unwrap();
        let task = impl_task(&state, "impl-max").await;

        scheduler_run_once(&state).await.unwrap();
        let after = state.tasks.find_by_id(&task.id).await.unwrap().unwrap();
        assert_eq!(
            after.status,
            TaskStatus::Backlog,
            "task queued at the repo max_agents limit (no 429, no claim)"
        );
        assert!(after.assigned_to.is_none());

        // Workspace fallback: repo override cleared, workspace limit 1.
        let mut repo = state
            .repos
            .find_by_id(&Id::new("r-1"))
            .await
            .unwrap()
            .unwrap();
        repo.max_agents = None;
        state.repos.update(&repo).await.unwrap();
        let mut ws = state
            .workspaces
            .find_by_id(&Id::new("ws-1"))
            .await
            .unwrap()
            .unwrap();
        ws.max_agents_per_repo = Some(1);
        state.workspaces.update(&ws).await.unwrap();

        scheduler_run_once(&state).await.unwrap();
        let after = state.tasks.find_by_id(&task.id).await.unwrap().unwrap();
        assert_eq!(
            after.status,
            TaskStatus::Backlog,
            "task queued at the workspace max_agents_per_repo fallback"
        );

        // The queued task spawns once the slot frees (agent completes).
        let mut idle = state
            .agents
            .find_by_id(&existing.id)
            .await
            .unwrap()
            .unwrap();
        idle.transition_status(AgentStatus::Idle).unwrap();
        state.agents.update(&idle).await.unwrap();
        scheduler_run_once(&state).await.unwrap();
        let after = state.tasks.find_by_id(&task.id).await.unwrap().unwrap();
        assert_eq!(
            after.status,
            TaskStatus::InProgress,
            "queued task spawns when an agent completes and frees the slot"
        );
    }

    /// Phase 4 precondition: workspace budget. An exhausted budget leaves
    /// the task Backlog with the BudgetExhausted flag (label), and the flag
    /// clears when a later cycle spawns the task.
    #[tokio::test]
    async fn scheduler_flags_budget_exhausted() {
        let state = chain_state().await;
        let task = impl_task(&state, "impl-budget").await;

        // Exhaust the workspace budget: zero concurrent agents allowed.
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

        scheduler_run_once(&state).await.unwrap();
        let after = state.tasks.find_by_id(&task.id).await.unwrap().unwrap();
        assert_eq!(after.status, TaskStatus::Backlog, "task stays Backlog");
        assert!(
            after.labels.iter().any(|l| l == "budget-exhausted"),
            "task carries the BudgetExhausted flag: {:?}",
            after.labels
        );
        assert!(after.assigned_to.is_none(), "no agent spawned");

        // Budget restored: the next cycle spawns the task and clears the flag.
        state
            .budget_configs
            .set_config(
                "workspace:ws-1",
                &gyre_domain::BudgetConfig {
                    max_tokens_per_day: None,
                    max_cost_per_day: None,
                    max_concurrent_agents: None,
                    max_agent_lifetime_secs: None,
                },
            )
            .await
            .unwrap();
        scheduler_run_once(&state).await.unwrap();
        let after = state.tasks.find_by_id(&task.id).await.unwrap().unwrap();
        assert_eq!(after.status, TaskStatus::InProgress, "task spawns");
        assert!(
            !after.labels.iter().any(|l| l == "budget-exhausted"),
            "budget flag cleared on spawn"
        );
    }

    /// Review finding (task-118 repair round): the worker-claim RELEASE
    /// path — budget exhausted AFTER the claim was taken, the race
    /// between the scheduler's pre-check and the spawn core's own check
    /// (a concurrent REST spawn consuming the last slot inside the
    /// cycle's await window, or budget exhaustion during the 1s/5s/30s
    /// retry backoff) — must actually release the claim. The pre-fix
    /// code routed InProgress→Review→InProgress→Backlog, but the domain
    /// transition table has no edge into Backlog, so the final
    /// transition was always rejected and nothing was persisted: the
    /// task was stranded InProgress forever (the worker scan selects
    /// only Backlog; the stale-claim recovery skips Implementation
    /// tasks; no API transition can requeue). The fix releases via the
    /// domain's dedicated `Task::release_claim` edge — the only
    /// InProgress→Backlog route — so Phase 4 step 3 ("task stays Backlog
    /// with a BudgetExhausted flag") holds on this path exactly as on
    /// the pre-check path, and a later cycle re-spawns the task.
    #[tokio::test]
    async fn budget_exhausted_mid_flight_releases_claim_and_respawns() {
        let state = chain_state().await;
        let task = impl_task(&state, "impl-midflight").await;

        // Claim as the scheduler does — the race this test pins is
        // everything that happens AFTER this point.
        let mut claimed = state.tasks.find_by_id(&task.id).await.unwrap().unwrap();
        claimed.transition_status(TaskStatus::InProgress).unwrap();
        state.tasks.update(&claimed).await.unwrap();

        // Exhaust the workspace budget inside the claim's window: the
        // spawn core's own check now fails where the pre-check passed.
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

        let repo = state
            .repos
            .find_by_id(&Id::new("r-1"))
            .await
            .unwrap()
            .unwrap();
        match try_spawn_worker(&state, &repo, &claimed, 0).await {
            WorkerSpawnOutcome::Blocked(reason) => {
                release_claim(&state, &claimed, &reason).await;
            }
            other => panic!("expected the mid-flight budget block, got {other:?}"),
        }

        // The claim is actually released: Backlog (not stranded
        // InProgress) with the BudgetExhausted flag.
        let after = state.tasks.find_by_id(&task.id).await.unwrap().unwrap();
        assert_eq!(
            after.status,
            TaskStatus::Backlog,
            "mid-flight budget block releases the claim to Backlog"
        );
        assert!(
            after.labels.iter().any(|l| l == "budget-exhausted"),
            "released claim carries the BudgetExhausted flag: {:?}",
            after.labels
        );

        // Budget restored: a later cycle re-spawns the task — the
        // pre-fix bug was that it never could.
        state
            .budget_configs
            .set_config(
                "workspace:ws-1",
                &gyre_domain::BudgetConfig {
                    max_tokens_per_day: None,
                    max_cost_per_day: None,
                    max_concurrent_agents: None,
                    max_agent_lifetime_secs: None,
                },
            )
            .await
            .unwrap();
        scheduler_run_once(&state).await.unwrap();
        let respawned = state.tasks.find_by_id(&task.id).await.unwrap().unwrap();
        assert_eq!(
            respawned.status,
            TaskStatus::InProgress,
            "a later cycle re-spawns the released task"
        );
        assert!(
            !respawned.labels.iter().any(|l| l == "budget-exhausted"),
            "budget flag cleared on the re-spawn"
        );
    }

    /// Crash recovery for claimed Implementation tasks (review F2,
    /// task-118): a stale InProgress claim whose agent is not Active —
    /// e.g. a server crash during the spawn-retry backoff, when the last
    /// attempt's agent is already Failed so the stale-agent detector has
    /// no Active agent to reap — is released to Backlog (the worker scan
    /// selects only Backlog, so it would otherwise be stranded forever),
    /// and a later cycle re-spawns it with a fresh worker.
    #[tokio::test]
    async fn stale_worker_claim_released_and_respawned() {
        let state = chain_state().await;
        let task = impl_task(&state, "impl-stale").await;

        // Claim as the scheduler does; the crash left the last attempt's
        // agent Failed and the task row stale.
        let mut claimed = state.tasks.find_by_id(&task.id).await.unwrap().unwrap();
        claimed.transition_status(TaskStatus::InProgress).unwrap();
        claimed.assigned_to = Some(Id::new("agent-dead"));
        claimed.updated_at = 0; // far in the past → stale
        state.tasks.update(&claimed).await.unwrap();
        let mut dead = Agent::new(Id::new("agent-dead"), "worker-stale", 0);
        dead.transition_status(AgentStatus::Active).unwrap();
        dead.transition_status(AgentStatus::Failed).unwrap();
        dead.workspace_id = Id::new("ws-1");
        state.agents.create(&dead).await.unwrap();

        // An incomplete dependency holds the released task in Backlog for
        // the assertion (the same cycle would otherwise re-spawn it
        // immediately — proven by the second phase below).
        let mut dep = Task::new(Id::new("dep-stale"), "Dependency", 0);
        dep.task_type = Some(TaskType::Implementation);
        dep.workspace_id = Id::new("ws-1");
        dep.repo_id = Id::new("r-1");
        state.tasks.create(&dep).await.unwrap();
        let mut gated = state.tasks.find_by_id(&task.id).await.unwrap().unwrap();
        gated.depends_on = vec![dep.id.clone()];
        state.tasks.update(&gated).await.unwrap();

        scheduler_run_once(&state).await.unwrap();

        let after = state.tasks.find_by_id(&task.id).await.unwrap().unwrap();
        assert_eq!(
            after.status,
            TaskStatus::Backlog,
            "stale claim with a dead agent is released to Backlog (crash recovery)"
        );

        // Dependency complete: a later cycle re-spawns the released task
        // with a FRESH worker (the dead agent does not keep the claim).
        // The first cycle spawned the dependency ITSELF (it is an eligible
        // Backlog Implementation task), so walk it to Done from whichever
        // state that spawn left it in — the same walk
        // scheduler_defers_until_depends_on_complete uses; a direct
        // →Done transition is invalid from Backlog and InProgress alike.
        let mut done = state.tasks.find_by_id(&dep.id).await.unwrap().unwrap();
        if done.status == TaskStatus::Backlog {
            done.transition_status(TaskStatus::InProgress).unwrap();
        }
        if done.status == TaskStatus::InProgress {
            done.transition_status(TaskStatus::Review).unwrap();
        }
        done.transition_status(TaskStatus::Done).unwrap();
        state.tasks.update(&done).await.unwrap();
        scheduler_run_once(&state).await.unwrap();
        let respawned = state.tasks.find_by_id(&task.id).await.unwrap().unwrap();
        assert_eq!(
            respawned.status,
            TaskStatus::InProgress,
            "the released task is re-spawned by a later cycle"
        );
        assert_ne!(
            respawned.assigned_to,
            Some(Id::new("agent-dead")),
            "a fresh worker takes over the released claim"
        );
    }

    /// The stale worker-claim recovery never releases a claim held by an
    /// ACTIVE agent: task rows are not touched while a worker runs, so
    /// task age alone must not requeue live work under its agent.
    #[tokio::test]
    async fn stale_worker_claim_with_active_agent_is_kept() {
        let state = chain_state().await;
        let task = impl_task(&state, "impl-live").await;

        let mut claimed = state.tasks.find_by_id(&task.id).await.unwrap().unwrap();
        claimed.transition_status(TaskStatus::InProgress).unwrap();
        claimed.assigned_to = Some(Id::new("agent-live"));
        claimed.updated_at = 0; // stale by age — but the agent is live
        state.tasks.update(&claimed).await.unwrap();
        let mut live = Agent::new(Id::new("agent-live"), "worker-live", 0);
        live.transition_status(AgentStatus::Active).unwrap();
        live.workspace_id = Id::new("ws-1");
        state.agents.create(&live).await.unwrap();

        scheduler_run_once(&state).await.unwrap();

        let after = state.tasks.find_by_id(&task.id).await.unwrap().unwrap();
        assert_eq!(
            after.status,
            TaskStatus::InProgress,
            "a claim held by an Active agent is never released"
        );
        assert_eq!(after.assigned_to, Some(Id::new("agent-live")));
    }

    /// Phase 7 spawn-failure policy: when ComputeTarget::spawn_process()
    /// fails on every attempt, the server retries 3 times with exponential
    /// backoff (1s/5s/30s), then marks the task Blocked, the failed
    /// attempts' agents Failed, notifies the repo orchestrator with a
    /// TaskBlocked message, and creates the priority-1 notification for
    /// workspace admins. Deterministic launch failure: the workspace's
    /// compute target is a Kubernetes target — `kubectl apply` cannot
    /// succeed without a cluster in any test environment.
    #[tokio::test(start_paused = true)]
    async fn worker_spawn_failure_retries_then_blocks() {
        let state = chain_state().await;

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

        // Kubernetes compute target assigned to the workspace: spawn_process
        // runs `kubectl apply`, which fails without a cluster.
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

        let task = impl_task(&state, "impl-fail").await;
        let task_id = task.id.clone();
        // Claim as the scheduler would.
        let mut claimed = state.tasks.find_by_id(&task.id).await.unwrap().unwrap();
        claimed.transition_status(TaskStatus::InProgress).unwrap();
        state.tasks.update(&claimed).await.unwrap();

        let repo = state
            .repos
            .find_by_id(&Id::new("r-1"))
            .await
            .unwrap()
            .unwrap();
        retry_worker_spawn(
            state.clone(),
            repo,
            claimed.clone(),
            "attempt 0 failed (test seeding)".to_string(),
        )
        .await;

        // Task Blocked after all retries.
        let after = state.tasks.find_by_id(&task.id).await.unwrap().unwrap();
        assert_eq!(
            after.status,
            TaskStatus::Blocked,
            "task Blocked after 3 failed retries"
        );

        // Four attempts (0-3) each left a Failed agent behind.
        let agents = state
            .agents
            .list_by_workspace(&Id::new("ws-1"))
            .await
            .unwrap();
        let workers: Vec<_> = agents
            .iter()
            .filter(|a| a.current_task_id == Some(task_id.clone()))
            .collect();
        assert_eq!(
            workers.len(),
            SPAWN_RETRY_BACKOFF_SECS.len(),
            "one agent record per retry attempt (attempts 1-3; attempt 0 is              the seeded failure this test resumes from)"
        );
        assert!(
            workers.iter().all(|a| a.status == AgentStatus::Failed),
            "every failed attempt's agent is Failed"
        );

        // Priority-1 notification for the workspace admin.
        let notifs = state
            .notifications
            .list_for_user(&Id::new("admin-user"), None, None, None, None, 100, 0)
            .await
            .unwrap();
        assert!(
            notifs.iter().any(|n| {
                n.notification_type == gyre_common::NotificationType::ComputeTargetUnavailable
                    && n.priority_rank == 1
            }),
            "priority-1 ComputeTargetUnavailable notification created: {notifs:?}"
        );

        // TaskBlocked message on the bus, routed to the repo orchestrator
        // the failure path ensured (spawned on demand when none was live).
        // Directed (orchestrator-inbox) delivery is invisible to
        // list_by_workspace by the port contract, so collect both tiers.
        let ws_msgs =
            crate::ralph_loop::collect_task_blocked_messages(&state, &Id::new("ws-1")).await;
        assert!(
            ws_msgs
                .iter()
                .any(|m| m.payload.as_ref().and_then(|p| p["task_id"].as_str())
                    == Some(task.id.as_str())),
            "TaskBlocked message emitted for the blocked task: {ws_msgs:?}"
        );
    }

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
                Some(Id::new("ws-1")),
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
