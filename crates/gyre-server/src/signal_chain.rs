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

use std::collections::HashMap;
use std::sync::Arc;

use gyre_common::Id;
use gyre_common::message::{Destination, MessageKind};
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
    Ok(peers
        .into_iter()
        .find(|a| {
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
pub async fn on_spec_approved(state: &AppState, payload: &serde_json::Value) {
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
    let lock = state.orchestrator_registry.workspace_lock(&workspace_id).await;
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
    state: &AppState,
    workspace_id: &Id,
) -> anyhow::Result<Agent> {
    if let Some(existing) = find_live_workspace_orchestrator(state, workspace_id).await? {
        return Ok(existing);
    }
    let (agent, _token) = crate::api::orchestrator::spawn_workspace_orchestrator_core(
        state,
        &workspace_id.to_string(),
        crate::api::orchestrator::SpawnOrchestratorRequest::default(),
        "system",
    )
    .await?;
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
    //    approved SHA; None when the repo/sha is not available here).
    let mut spec_content: Option<String> = None;
    if let (Some(rid), true) = (&repo_id, !spec_sha.is_empty()) {
        if let Ok(Some(repo)) = state.repos.find_by_id(rid).await {
            let git_bin =
                std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());
            spec_content =
                crate::spec_registry::read_git_file(&git_bin, &repo.path, &spec_sha, &spec_path)
                    .await;
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
        task.spec_path = Some(spec_path.clone());
        task.description = Some(format!(
            "Delegation task created by workspace orchestrator {} for approved spec {spec_ref} (agent-runtime.md §1 Phase 2).",
            orchestrator.id
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

    // Mark the inbox delivery acknowledged — this run consumed it.
    let _ = state
        .messages
        .acknowledge_all(&orchestrator.id, "processed by workspace orchestrator run")
        .await;

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
                "Cross-workspace spec change: {spec_path} (source workspace {source_workspace_id})"
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
/// Finds Backlog tasks with `task_type` Delegation or Coordination, claims
/// each (Backlog→InProgress — the claim makes cycle re-entry idempotent),
/// and runs the repo orchestrator for its repo under the per-repo registry
/// mutex. `Implementation` tasks are NOT handled here — they are Phase 4's
/// worker-spawn path, intentionally excluded by the task_type
/// discriminator. Tasks with unresolved repos are left Backlog.
pub async fn scheduler_run_once(state: &AppState) -> anyhow::Result<()> {
    let now = crate::api::now_secs();
    let backlog = state.tasks.list_by_status(&TaskStatus::Backlog).await?;

    for mut task in backlog {
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
            tracing::warn!(
                task_id = %task.id,
                task_type = %task_type,
                "signal-chain scheduler: task has no resolvable repo; leaving Backlog"
            );
            continue;
        };

        // Claim: Backlog→InProgress so concurrent cycles (and the 30 s loop
        // re-entering while a run is in flight) cannot double-process.
        if task.transition_status(TaskStatus::InProgress).is_err() {
            continue;
        }
        task.updated_at = now;
        if let Err(e) = state.tasks.update(&task).await {
            tracing::warn!(task_id = %task.id, "signal-chain scheduler: claim failed: {e}");
            continue;
        }

        // Serialize per repo: a second Delegation for the same repo waits
        // here (per-repo mutex, exactly-one-active repo orchestrator).
        let lock = state.orchestrator_registry.repo_lock(&repo.id).await;
        let _guard = lock.lock().await;

        if let Err(e) = run_repo_orchestrator(state, &repo, &task).await {
            // Release the claim so a later cycle retries after the failure.
            let _ = task.transition_status(TaskStatus::Backlog);
            tracing::error!(
                task_id = %task.id,
                repo_id = %repo.id,
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
    state: &AppState,
    repo: &Repository,
    task: &Task,
) -> anyhow::Result<()> {
    let orchestrator = match find_live_repo_orchestrator(state, repo).await? {
        Some(existing) => existing,
        None => {
            let (agent, _token) = crate::api::orchestrator::spawn_repo_orchestrator_core(
                state,
                &repo.id.to_string(),
                crate::api::orchestrator::SpawnOrchestratorRequest {
                    name: None,
                    parent_id: task.assigned_to.map(|a| a.to_string()),
                },
                "system",
            )
            .await?;
            tracing::info!(
                orchestrator_id = %agent.id,
                repo_id = %repo.id,
                "signal-chain: spawned repo orchestrator for {:?} task",
                task.task_type
            );
            agent
        }
    };

    match task.task_type {
        Some(TaskType::Delegation) => {
            decompose_delegation(state, repo, task).await?
        }
        Some(TaskType::Coordination) => {
            assess_coordination(state, repo, task).await?
        }
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

    // 1. Read the delegation task's approved spec. `spec_path` carries
    //    `path@sha` from Phase 2 when set; fall back to the ledger's
    //    current SHA for tasks created by other paths (e.g. MCP).
    let (spec_path, spec_sha) = match task.spec_path.as_deref() {
        Some(p) if p.contains('@') => {
            let (path, sha) = p.split_once('@').unwrap();
            (path.to_string(), sha.to_string())
        }
        Some(p) => {
            let sha = state
                .spec_ledger
                .find_by_path(p)
                .await?
                .map(|e| e.current_sha)
                .unwrap_or_default();
            (p.to_string(), sha)
        }
        None => {
            // No spec reference: nothing to decompose from. Complete the
            // delegation as a no-op rather than leaving it InProgress
            // forever, and record why in the description.
            let mut done = task.clone();
            done.transition_status(TaskStatus::Review)?;
            done.transition_status(TaskStatus::Done)?;
            done.updated_at = now;
            state.tasks.update(&done).await?;
            tracing::warn!(
                task_id = %task.id,
                "signal-chain: delegation task has no spec_path; marked Done without decomposition"
            );
            return Ok(());
        }
    };

    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());
    let spec_content = if spec_sha.is_empty() {
        None
    } else {
        crate::spec_registry::read_git_file(&git_bin, &repo.path, &spec_sha, &spec_path).await
    };

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
        sub.spec_path = Some(spec_ref.clone());
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
    let user_prompt = format!(
        "Decompose the following approved spec into ordered implementation sub-tasks.\n\n\
         Repo: {repo_name} ({repo_id})\nSpec: {spec_path}@{spec_sha}\n\n\
         Spec content:\n```\n{content}\n```\n\n\
         Respond with valid JSON only: an array of objects \
         {{\"title\": string, \"description\": string, \"order\": number}}, \
         where lower order runs first and same-order tasks may run in parallel.",
        repo_name = repo.name,
    );
    let parsed: Result<serde_json::Value> = port
        .predict_json(&persona, &user_prompt)
        .await
        .map_err(Into::into);
    let value = match parsed {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                "signal-chain: LLM decomposition failed ({e:#}); falling back to deterministic decomposition"
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
        tracing::warn!("signal-chain: LLM decomposition produced no usable sub-tasks; falling back");
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
    let parsed: Result<serde_json::Value> = port
        .predict_json(&persona, &user_prompt)
        .await
        .map_err(Into::into);
    let value = match parsed {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("signal-chain: LLM coordination assessment failed ({e:#}); assuming action needed");
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
        state
            .job_registry
            .mark_scheduled("task_scheduler")
            .await;
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
        assert!(!Arc::ptr_eq(&a, &d), "workspace and repo locks are distinct");
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
        assert!(task.await.unwrap(), "second acquisition completes after release");
    }
}
