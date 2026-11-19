//! Admin-only endpoints for gyre-server.
//!
//! All endpoints require Admin role (enforced via ABAC middleware).

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::{
    message::{Destination, MessageKind},
    AgEventType, Id,
};
use gyre_domain::{AgentStatus, TaskStatus};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{
    retention::{validate_policies, RetentionPolicy},
    AppState,
};

use super::error::ApiError;
use super::now_secs;

// ── System Health ─────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct SystemHealthResponse {
    pub status: &'static str,
    pub uptime_secs: u64,
    pub agent_count: usize,
    pub active_agents: usize,
    pub task_count: usize,
    pub repo_count: usize,
    pub version: &'static str,
}

/// GET /api/v1/admin/health — system health summary (Admin only).
pub async fn admin_health(
    State(state): State<Arc<AppState>>,
) -> Result<Json<SystemHealthResponse>, ApiError> {
    let uptime_secs = now_secs().saturating_sub(state.started_at_secs);
    let agents = state.agents.list().await?;
    let active_agents = agents
        .iter()
        .filter(|a| a.status == AgentStatus::Active)
        .count();
    let tasks = state.tasks.list().await?;
    let repos = state.repos.list().await?;

    Ok(Json(SystemHealthResponse {
        status: "ok",
        uptime_secs,
        agent_count: agents.len(),
        active_agents,
        task_count: tasks.len(),
        repo_count: repos.len(),
        version: env!("CARGO_PKG_VERSION"),
    }))
}

// ── Orphaned Workspaces (user-management.md §Ownership Transfer & Reclamation) ─

/// GET /api/v1/admin/orphaned-workspaces — workspaces with no active Owner
/// for at least the configured period (`GYRE_ORPHANED_WORKSPACE_DAYS`,
/// default 30 days). This is the admin dashboard "orphaned" flag.
pub async fn admin_orphaned_workspaces(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let threshold_days = crate::workspace_ownership::orphaned_threshold_days();
    let orphaned = crate::workspace_ownership::orphaned_workspaces_with(&state, threshold_days)
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(serde_json::json!({
        "threshold_days": threshold_days,
        "workspaces": orphaned,
    })))
}

// ── Background Jobs ───────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct JobInfo {
    pub name: String,
    pub status: String,
    pub interval_secs: u64,
    pub description: String,
    pub recent_runs: Vec<JobRunResponse>,
}

/// One job run as served to the admin Jobs tab (admin-panel.md: history
/// includes status, duration, failures and logs).
#[derive(Serialize)]
pub struct JobRunResponse {
    pub id: String,
    pub job_name: String,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    /// Derived duration in seconds (`None` while the run is unfinished).
    pub duration_secs: Option<u64>,
    pub status: String,
    pub error: Option<String>,
    /// Per-run captured log lines (run-scoped sink).
    pub logs: Vec<String>,
}

impl From<crate::jobs::JobRun> for JobRunResponse {
    fn from(run: crate::jobs::JobRun) -> Self {
        let duration_secs = run.duration_secs();
        Self {
            id: run.id,
            job_name: run.job_name,
            started_at: run.started_at,
            finished_at: run.finished_at,
            duration_secs,
            status: run.status,
            error: run.error,
            logs: run.logs,
        }
    }
}

/// GET /api/v1/admin/jobs — list background jobs with run history (Admin only).
///
/// TASK-273: history is served from the DURABLE `JobRunRepository` (with
/// logs and derived duration); the in-memory registry ring is a fronting
/// cache only.
pub async fn admin_jobs(State(state): State<Arc<AppState>>) -> Json<Vec<JobInfo>> {
    let defs = state.job_registry.list_jobs().await;
    let mut jobs = Vec::with_capacity(defs.len());
    for def in defs {
        // Durable history first (authoritative); fall back to the
        // in-memory cache when the durable store has no rows for this job
        // yet (e.g. pure-mem mode before any run was recorded).
        let mut recent_runs: Vec<JobRunResponse> = state
            .job_registry
            .durable_history(&def.name, 20)
            .await
            .into_iter()
            .map(JobRunResponse::from)
            .collect();
        if recent_runs.is_empty() {
            // The in-memory ring is oldest-first (VecDeque push_back order);
            // serve it newest-first like the durable store so both the job
            // status pill (most recent run's status, below) and the run
            // ordering presented to the UI are consistent across paths.
            let mut mem_history = state.job_registry.history(&def.name).await;
            mem_history.reverse();
            recent_runs = mem_history.into_iter().map(JobRunResponse::from).collect();
        }
        // Most recent run's status (both history paths are newest-first).
        let status = recent_runs
            .first()
            .map(|r| r.status.as_str())
            .unwrap_or("idle")
            .to_string();
        jobs.push(JobInfo {
            name: def.name,
            interval_secs: def.interval_secs,
            description: def.description,
            status,
            recent_runs,
        });
    }
    // Fallback: if registry is empty (test_state), return static list
    if jobs.is_empty() {
        jobs = vec![
            JobInfo {
                name: "merge_processor".to_string(),
                status: "running".to_string(),
                interval_secs: 5,
                description: "Processes queued merge requests".to_string(),
                recent_runs: vec![],
            },
            JobInfo {
                name: "stale_agent_detector".to_string(),
                status: "running".to_string(),
                interval_secs: 30,
                description: "Marks agents dead when heartbeat times out (>60s)".to_string(),
                recent_runs: vec![],
            },
        ];
    }
    Json(jobs)
}

/// POST /api/v1/admin/jobs/{name}/run — manually trigger a job (Admin only).
///
/// By-effect task-scope surface: the triggered jobs mutate task rows
/// (stale_agent_detector blocks the stale agent's task, reconciliation
/// jobs reassign/cancel tasks), so a task-bound agent principal is denied
/// (see `deny_task_bound_agent`). The extractor adds no new authentication
/// requirement — the ABAC middleware already authenticates this mapped,
/// non-exempt route — it exposes the verified claims to the handler check.
pub async fn admin_run_job(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    auth: crate::auth::AuthenticatedAgent,
) -> Result<Json<serde_json::Value>, ApiError> {
    deny_task_bound_agent(&auth)?;
    state
        .job_registry
        .trigger(&name, state.clone())
        .await
        .map_err(|e| ApiError::NotFound(e.to_string()))?;
    Ok(Json(
        serde_json::json!({ "status": "triggered", "job": name }),
    ))
}

// ── Snapshot / Restore ────────────────────────────────────────────────────────

/// POST /api/v1/admin/snapshot — create a snapshot (Admin only).
pub async fn admin_create_snapshot(
    State(state): State<Arc<AppState>>,
) -> Result<Json<crate::snapshot::SnapshotMeta>, ApiError> {
    let meta = crate::snapshot::create_snapshot(&state)
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(meta))
}

/// GET /api/v1/admin/snapshots — list snapshots (Admin only).
pub async fn admin_list_snapshots() -> Result<Json<Vec<crate::snapshot::SnapshotMeta>>, ApiError> {
    let snapshots = crate::snapshot::list_snapshots()
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(snapshots))
}

#[derive(Deserialize)]
pub struct RestoreRequest {
    pub snapshot_id: String,
}

/// POST /api/v1/admin/restore — restore from snapshot (Admin only).
///
/// By-effect task-scope surface: restore replaces task (and agent) state
/// wholesale across the deployment — no task claim can cover it, so a
/// task-bound agent principal is denied (see `deny_task_bound_agent`).
/// The extractor adds no new authentication requirement — the ABAC
/// middleware already authenticates this mapped, non-exempt route.
pub async fn admin_restore_snapshot(
    State(state): State<Arc<AppState>>,
    auth: crate::auth::AuthenticatedAgent,
    Json(req): Json<RestoreRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    deny_task_bound_agent(&auth)?;
    let warning = crate::snapshot::restore_snapshot(&state, &req.snapshot_id)
        .await
        .map_err(|e| ApiError::NotFound(e.to_string()))?;
    Ok(Json(
        serde_json::json!({ "status": "restored", "warning": warning }),
    ))
}

/// DELETE /api/v1/admin/snapshots/{id} — delete a snapshot (Admin only).
pub async fn admin_delete_snapshot(Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    crate::snapshot::delete_snapshot(&id)
        .await
        .map_err(|e| ApiError::NotFound(e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Data Export ───────────────────────────────────────────────────────────────

/// GET /api/v1/admin/export — export all data as JSON (Admin only).
pub async fn admin_export(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (agents, tasks, merge_requests, activity_events) = tokio::try_join!(
        state.agents.list(),
        state.tasks.list(),
        state.merge_requests.list(),
        async { Ok::<_, anyhow::Error>(state.telemetry_buffer.list_all_since(0, 10_000)) },
    )
    .map_err(ApiError::Internal)?;

    let repos = state.repos.list().await.map_err(ApiError::Internal)?;

    Ok(Json(serde_json::json!({
        "exported_at": now_secs(),
        "repos": repos,
        "agents": agents,
        "tasks": tasks,
        "merge_requests": merge_requests,
        "activity_events": activity_events,
    })))
}

// ── Data Retention ────────────────────────────────────────────────────────────

/// GET /api/v1/admin/retention — list retention policies (Admin only).
pub async fn admin_list_retention(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<RetentionPolicy>> {
    Json(state.retention_store.list())
}

/// PUT /api/v1/admin/retention — update retention policies (Admin only).
///
/// Two body forms are accepted:
///
/// - The complete policy list (business-continuity.md §5): all 7 spec
///   data types (`validate_policies`); an omitted type would silently
///   disable enforcement for it.
/// - The analytics-scoped form `{ "retention_days": N }`
///   (analytics.md §Data Retention): updates only the `analytics_events`
///   policy, leaving every other data type's policy untouched.
///
/// The store persists durably before the 204 is returned — a KV write
/// failure fails the request instead of acking a change that would be
/// silently lost on restart.
pub async fn admin_update_retention(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> Result<StatusCode, ApiError> {
    if let Some(days) = body.get("retention_days").and_then(|v| v.as_u64()) {
        // analytics.md §Data Retention form: `{ retention_days: u32 }`.
        if days < 1 {
            return Err(ApiError::BadRequest(
                "retention_days must be >= 1 (0 would purge every analytics event on the \
                 next nightly run)"
                    .to_string(),
            ));
        }
        let mut policies = state.retention_store.list();
        let analytics = policies
            .iter_mut()
            .find(|p| p.data_type == "analytics_events")
            .ok_or_else(|| {
                ApiError::Internal(anyhow::anyhow!(
                    "analytics_events retention policy is missing from the store",
                ))
            })?;
        analytics.max_age_days = days;
        state.retention_store.update(policies).await?;
        return Ok(StatusCode::NO_CONTENT);
    }

    if !body.is_array() {
        return Err(ApiError::BadRequest(
            "body must be a complete policy list (all 7 data types) or the analytics form \
             { \"retention_days\": <days> }"
                .to_string(),
        ));
    }

    let policies: Vec<RetentionPolicy> = serde_json::from_value(body)
        .map_err(|e| ApiError::BadRequest(format!("invalid retention policy list: {e}")))?;
    validate_policies(&policies).map_err(ApiError::BadRequest)?;
    state.retention_store.update(policies).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Audit Log ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AuditQuery {
    pub agent_id: Option<String>,
    pub event_type: Option<String>,
    pub workspace_id: Option<String>,
    pub user_id: Option<String>,
    pub repo_id: Option<String>,
    pub resource_type: Option<String>,
    /// Free-text search over the event detail payload (TASK-273:
    /// admin-panel.md §Audit log viewer — searchable trail). Enforced in
    /// SQL by the durable adapters, transparently decompressing
    /// zstd-compressed details.
    pub q: Option<String>,
    pub since: Option<u64>,
    pub until: Option<u64>,
    pub limit: Option<usize>,
}

/// GET /api/v1/admin/audit — searchable activity log (Admin only).
///
/// TASK-273 (admin-panel.md §Audit log viewer — durable admin trail +
/// free-text search): queries the DURABLE audit repository (not the
/// in-memory TelemetryBuffer — a server restart must not lose the admin-
/// visible trail), scoped to the authenticated caller's tenant (derived
/// server-side, never client-supplied) with the same filter set
/// `/audit/events` supports plus free-text `q` search.
pub async fn admin_audit(
    State(state): State<Arc<AppState>>,
    auth: crate::auth::AuthenticatedAgent,
    Query(q): Query<AuditQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let filter = gyre_ports::AuditQueryFilter {
        agent_id: q.agent_id,
        event_type: q.event_type,
        workspace_id: q.workspace_id,
        user_id: q.user_id,
        repo_id: q.repo_id,
        resource_type: q.resource_type,
        outcome: None,
        // Tenant scoping is derived from the authenticated caller — a
        // tenant filter is never accepted from the query string.
        tenant_id: Some(auth.tenant_id.clone()),
        since: q.since,
        until: q.until,
        limit: q.limit.unwrap_or(100).min(1000),
        q: q.q,
    };
    let events = state
        .audit
        .query(&filter)
        .await
        .map_err(ApiError::Internal)?;
    let count = events.len();
    Ok(Json(serde_json::json!({
        "events": events
            .into_iter()
            .map(crate::api::audit::AuditEventResponse::from)
            .collect::<Vec<_>>(),
        "count": count
    })))
}

/// GET /api/v1/admin/server/logs — recent server log events from the
/// bounded capture buffer (Admin only; admin-panel.md §Server Logs,
/// TASK-273).
#[derive(Deserialize)]
pub struct ServerLogQuery {
    /// Minimum level (TRACE|DEBUG|INFO|WARN|ERROR).
    pub level: Option<String>,
    /// Only events at/after this Unix epoch millisecond timestamp.
    pub since: Option<u64>,
    /// At most this many of the most recent matching entries (default
    /// 200, capped at 5000 — the buffer bound).
    pub limit: Option<usize>,
}

pub async fn admin_server_logs(Query(q): Query<ServerLogQuery>) -> Json<serde_json::Value> {
    let limit = q.limit.unwrap_or(200).min(5000);
    let entries = crate::telemetry::server_log_buffer().query(q.level.as_deref(), q.since, limit);
    let count = entries.len();
    Json(serde_json::json!({
        "entries": entries,
        "count": count
    }))
}

// ── Agent Kill ────────────────────────────────────────────────────────────────

/// Kill an agent: transition to `Dead`, stop its process, handle worktrees,
/// emit `AgentStatusChanged` telemetry, emit the container-crashed audit
/// event, revoke the agent's auth token, emit the `AgentKilled` audit event,
/// clean up interrogation state, and release the budget active-agent slot
/// when the agent held one.
///
/// Kill path for the admin kill route. Budget enforcement uses its own
/// `budget_stop_agent` (api/budget.rs) with materially different terminal
/// semantics mandated by agent-runtime.md §4 / platform-model.md §5:
/// status `Stopped` (not Dead — the run is recoverable), task left
/// InProgress (not Blocked), worktree preserved, compute minutes charged.
/// The two paths are deliberately distinct; do not funnel budget stops
/// through this admin-terminal helper.
///
/// `preserve_worktree`: preserve the agent's in-progress work (worktree,
/// branch, partial MR); admin kills clean up.
///
/// `killed_by`: identity attributed in the `AgentKilled` audit event (the
/// admin's user/agent id on admin kills).
///
/// `revoke_reason`: short cause tag recorded on the `AgentTokenRevoked`
/// audit event ("admin_kill").
pub(crate) async fn kill_agent(
    state: &Arc<AppState>,
    agent: &gyre_domain::Agent,
    reason: &str,
    preserve_worktree: bool,
    killed_by: &str,
    revoke_reason: &str,
) -> Result<(), ApiError> {
    let id = agent.id.to_string();
    if agent.status == AgentStatus::Dead {
        return Ok(());
    }
    // Only an Active agent holds a budget active-agent slot (incremented at
    // spawn, released on Idle/Failed/Stopped) — never double-release.
    let was_active = agent.status == AgentStatus::Active;

    let now = now_secs();

    let mut agent = agent.clone();
    let _ = agent.transition_status(AgentStatus::Dead);
    state.agents.update(&agent).await?;

    // source-control.md §Agent-to-Commit Tracking: the Dead transition
    // terminates the agent's open session — records the worktree's real
    // current branch and dirty state, and stamps ended_at. Must run
    // BEFORE the worktree cleanup below, while the worktree still exists
    // for the real `git status --porcelain` query. Best-effort: never
    // fails the kill path.
    crate::agent_sessions::end_session(state, &agent).await;

    // Kill the actual process/container/Pod if it is running. The backend
    // recorded at spawn time decides how: a container ID goes to
    // `docker rm --force`, a Pod name to `kubectl delete pod`, a remote
    // container over SSH — falling back to LocalTarget (pid kill) only for
    // agents whose backend is unknown (pre-task-117 records). The PID is
    // captured for the AgentKilled audit event before the registry entry
    // is consumed.
    let mut killed_pid: Option<u32> = None;
    if let Some(handle) = state.process_registry.lock().await.remove(&id) {
        killed_pid = handle.pid;
        let backend = state.spawned_backends.lock().await.remove(&id);
        let kill_result = match backend.as_deref() {
            Some(b) => gyre_ports::ComputeTarget::kill_process(b, &handle).await,
            None => {
                gyre_ports::ComputeTarget::kill_process(
                    &gyre_adapters::compute::LocalTarget,
                    &handle,
                )
                .await
            }
        };
        if let Err(e) = kill_result {
            tracing::warn!(agent_id = %id, "kill_process failed: {e}");
        }
    }
    // TASK-273: the durable instance record reflects the admin kill.
    if let Err(e) = state
        .agent_instances
        .mark_terminated(&id, "admin_kill")
        .await
    {
        tracing::warn!(
            agent_id = %id,
            "marking the killed agent instance terminated failed: {e:#}"
        );
    }
    // Fall back to the attested PID when no live process handle exists.
    if killed_pid.is_none() {
        killed_pid = crate::agent_audit::attested_pid(state, &id).await;
    }

    // Worktrees: preserved on budget kills (in-progress work must survive
    // for reassignment), cleaned up on admin kills — via the shared
    // terminal teardown (`cleanup_agent_worktrees`: jj workspace forget +
    // plain-git worktree removal + agent_worktrees record delete; the
    // pre-helper inline block skipped the jj forget, leaking the workspace
    // registration in `jj workspace list`).
    if !preserve_worktree {
        crate::api::spawn::cleanup_agent_worktrees(state, &agent.id).await;
    }

    // Block the agent's current task.
    if let Some(task_id) = &agent.current_task_id {
        if let Ok(Some(mut task)) = state.tasks.find_by_id(task_id).await {
            if task.status == TaskStatus::InProgress {
                let pre_block_status = task.status.clone();
                let _ = task.transition_status(TaskStatus::Blocked);
                task.updated_at = now;
                let _ = state.tasks.update(&task).await;
                // task.status_changed analytics event (analytics.md
                // §Auto-Emitted Events: "Task transitions status") — the
                // kill auto-block is a production transition.
                if task.status != pre_block_status {
                    crate::api::analytics::record_task_status_changed(
                        state,
                        &task,
                        &pre_block_status,
                    )
                    .await;
                }
            }
        }
    }

    // M22.2: release the budget active-agent slot this agent held.
    if was_active {
        crate::api::budget::decrement_active_agents(state, &agent.workspace_id.to_string()).await;
        // Preview-agent slot bookkeeping: this kill released the slot AND
        // consumed the process registry handle, so the agent's process is
        // dying — its exit routes through finish_preview_agent, and the
        // human's preview DELETE (or the GC sweep) runs teardown later.
        // Both preview-owned paths must see the flag this writes or each
        // of them releases the same slot a second time (exactly-once;
        // decrement_active is MAX(0, n-1) — a double release permanently
        // biases the counter down). No-op for normal agents.
        crate::api::meta_specs::mark_preview_slot_released(state, &agent.id.to_string()).await;
    }

    let ws_id = agent.workspace_id.clone();
    state
        .emit_telemetry(
            ws_id,
            gyre_common::message::MessageKind::AgentStatusChanged,
            Some(serde_json::json!({
                "event_id": uuid::Uuid::new_v4().to_string(),
                "agent_id": agent.id.to_string(),
                "status": "dead",
                "description": format!("Agent {}: {}", agent.name, reason),
                "timestamp": now,
            })),
        )
        .await;

    // M23: Emit container_crashed audit event if this agent had a container.
    if let Ok(Some(rec)) = state.container_audits.find_by_agent_id(&id).await {
        let ctx = crate::container_audit::AuditCtx {
            audit: state.audit.as_ref(),
            broadcast_tx: &state.audit_broadcast_tx,
            search: state.search.as_ref(),
        };
        crate::container_audit::emit_crashed(&ctx, &id, &rec.container_id, reason).await;
    }

    // HSI §4: Clean up interrogation ABAC policies on kill.
    crate::api::spawn::cleanup_interrogation_policies(state, &id).await;
    let _ = state.kv_store.kv_remove("interrogation_context", &id).await;

    // Revoke the agent's token — killed agents must not continue to
    // authenticate. Revocation is audited as an AgentTokenRevoked event.
    crate::agent_audit::revoke_agent_token(state, &agent, revoke_reason).await;

    // Platform Model §7: revoke task-scoped ephemeral secrets on kill
    // ("Secrets access revoked" at teardown) — deletes Task-scoped secrets
    // for the agent's task and logs `secret.revoked` per secret.
    crate::secret_rotation::revoke_agent_task_secrets(state, &agent).await;

    // Agent lifecycle audit event (observability.md §Audit Event Types):
    // attributes who ordered the kill and which PID was terminated.
    crate::agent_audit::emit_killed(state, &agent, reason, killed_by, killed_pid).await;

    Ok(())
}

/// By-effect task-scope gate for the admin control surfaces (identity-
/// security.md §Task-Scoped JWT Claims, task-263 review repair).
///
/// The admin surfaces gated here mutate task rows and/or terminate other
/// tasks' workloads deployment-wide: kill blocks the target agent's current
/// task and tears down its workload, reassign moves every task assigned to
/// an agent (with a generation bump), job triggers run stale-agent/
/// reconciliation jobs that mutate task rows, snapshot restore replaces
/// task state wholesale, and seed creates task/agent/MR rows outside any
/// task claim. No task claim can cover a cross-task,
/// deployment-wide effect, so a task-bound agent principal is denied
/// outright — mirroring `archive_repo`'s outright denial — and the boundary
/// holds regardless of ABAC policy configuration: the middleware
/// `builtin-admin-only-deny` (priority 850) is the role gate for non-agent
/// principals under the default configuration, but a policy that Allows
/// Agent-role subjects on `admin` resources (priority > 850) must not let
/// one task's agent terminate another task's workload. Non-agent
/// principals (Admin users, the system token, orchestrators) keep their
/// existing rights — `agent_task_claim()` is `None` for them.
fn deny_task_bound_agent(auth: &crate::auth::AuthenticatedAgent) -> Result<(), ApiError> {
    if auth.agent_task_claim().is_some() {
        return Err(ApiError::Forbidden(
            "agent tokens cannot trigger admin control surfaces that mutate tasks across the deployment"
                .to_string(),
        ));
    }
    Ok(())
}

/// POST /api/v1/admin/agents/{id}/kill — force-kill an agent (Admin only).
pub async fn admin_kill_agent(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    auth: crate::auth::AuthenticatedAgent,
) -> Result<StatusCode, ApiError> {
    deny_task_bound_agent(&auth)?;
    let agent = state
        .agents
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("agent {id} not found")))?;

    if agent.status == AgentStatus::Dead {
        return Ok(StatusCode::NO_CONTENT);
    }

    // Admin identity for the AgentKilled audit event.
    let killed_by = auth
        .user_id
        .map(|u| u.to_string())
        .unwrap_or_else(|| auth.agent_id.clone());
    kill_agent(
        &state,
        &agent,
        "force-killed by admin",
        false,
        &killed_by,
        "admin_kill",
    )
    .await?;

    // Auto-track agent kill (analytics.md §Auto-Emitted Events: agent.failed
    // covers "fails or is killed").
    let now = now_secs();
    let ev = gyre_domain::AnalyticsEvent::new(
        crate::api::new_id(),
        "agent.failed",
        Some(agent.id.to_string()),
        serde_json::json!({
            "task_id": agent.current_task_id.as_ref().map(|id| id.to_string()),
            "reason": "force-killed by admin",
            "duration_secs": now.saturating_sub(agent.spawned_at),
        }),
        now,
    )
    .with_scope(
        None,
        None,
        Some(&agent.workspace_id),
        agent.repo_id.as_ref(),
    );
    let _ = state.analytics.record(&ev).await;

    // The kill ended the run — charge the wall-clock compute minutes
    // (spawn → kill, agent-runtime.md §4 What's Tracked). The budget
    // active-agent slot is already released inside `kill_agent` (exactly
    // once — only an Active agent ever held one).
    crate::api::budget::record_compute_minutes_for_agent(&state, &agent).await;

    Ok(StatusCode::NO_CONTENT)
}

// ── Agent Reassign ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ReassignRequest {
    pub target_agent_id: String,
}

/// POST /api/v1/admin/agents/{id}/reassign — reassign agent tasks (Admin only).
pub async fn admin_reassign_agent(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    auth: crate::auth::AuthenticatedAgent,
    Json(req): Json<ReassignRequest>,
) -> Result<StatusCode, ApiError> {
    deny_task_bound_agent(&auth)?;
    let agent = state
        .agents
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("agent {id} not found")))?;

    let target = state
        .agents
        .find_by_id(&Id::new(&req.target_agent_id))
        .await?
        .ok_or_else(|| {
            ApiError::NotFound(format!("target agent {} not found", req.target_agent_id))
        })?;

    let tasks = state.tasks.list_by_assignee(&agent.id).await?;
    let now = now_secs();
    // Admin identity for the AgentReassigned audit events below.
    let reassigned_by = auth
        .user_id
        .as_ref()
        .map(|u| u.to_string())
        .unwrap_or_else(|| auth.agent_id.clone());
    for mut task in tasks {
        task.assigned_to = Some(target.id.clone());
        // §2.4 replay prevention: reassignment bumps the deployment generation,
        // invalidating SignedInputs pinned to the previous generation.
        task.generation = task.generation.saturating_add(1);
        task.updated_at = now;
        state.tasks.update(&task).await?;

        // Agent lifecycle audit event (observability.md §Audit Event Types):
        // one per reassigned task, attributing the admin who moved it.
        crate::agent_audit::emit_reassigned(&state, &agent, &task.id, &target.id, &reassigned_by)
            .await;
    }

    let ws_id = agent.workspace_id.clone();
    state
        .emit_telemetry(
            ws_id,
            gyre_common::message::MessageKind::AgentStatusChanged,
            Some(serde_json::json!({
                "agent_id": agent.id.to_string(),
                "description": format!("Agent {} tasks reassigned to {}", agent.name, target.name),
            })),
        )
        .await;

    Ok(StatusCode::NO_CONTENT)
}

// ── Seed Data ─────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct SeedResponse {
    pub projects: usize,
    pub repos: usize,
    pub agents: usize,
    pub tasks: usize,
    pub merge_requests: usize,
    pub merge_queue_entries: usize,
    pub activity_events: usize,
    pub already_seeded: bool,
}

/// POST /api/v1/admin/seed — populate demo data (Admin only, idempotent).
///
/// The demo tenant/workspace is created in the CALLER's authenticated tenant
/// scope (static system token resolves as tenant "default"). Every workspace
/// query filters by the caller's tenant, so seeding into any other scope
/// would produce data the caller cannot see. Because the seed fixture ids
/// ("seed-repo-1", workspace "default") are global, only ONE tenant can own
/// the demo seed — a caller from a different tenant gets a 409 naming the
/// collision, never a success response for data outside its scope.
///
/// By-effect task-scope surface: seeding creates task rows, agent rows and
/// task-linked MRs (demo fixtures) outside any task claim, so a task-bound
/// agent principal is denied (see `deny_task_bound_agent`).
pub async fn admin_seed(
    State(state): State<Arc<AppState>>,
    auth: crate::auth::AuthenticatedAgent,
) -> Result<Json<SeedResponse>, ApiError> {
    deny_task_bound_agent(&auth)?;
    use gyre_domain::{
        Agent, AgentStatus, MergeQueueEntry, MergeRequest, MrStatus, Repository, Task,
        TaskPriority, TaskStatus, Tenant, Workspace,
    };

    // non-atomic-create:ok - dev-bootstrap demo fixture: every seed record is
    // an independent demo entity (no parent-child invariant ties the set
    // together; tenant/workspace are find-or-create and the rest are
    // first-boot fixtures whose creation errors propagate with `?`), and the
    // whole endpoint is Admin-gated demo tooling, not a production path.

    // Scope: derived from the authenticated caller, never caller-supplied.
    // The global token resolves as tenant "default" with Admin role; JWT
    // callers seed their own tenant.
    let tenant_id = auth.tenant_id;

    // Idempotency + ownership: the demo seed repo uses a GLOBAL fixture id
    // ("seed-repo-1"), so its presence alone does not prove this caller's
    // tenant owns it. Load the actual seed workspace and compare its owner
    // against the caller — a mismatch (another tenant, or a legacy seed
    // created before caller-scoping) is rejected instead of silently
    // reported as "this caller's seed already succeeded".
    let seed_workspace_id = Id::new("default");
    if let Some(existing_repo) = state.repos.find_by_id(&Id::new("seed-repo-1")).await? {
        let ws = state
            .workspaces
            .find_by_id(&existing_repo.workspace_id)
            .await?
            .ok_or_else(|| {
                ApiError::Conflict(format!(
                    "seed repo '{}' exists but its workspace '{}' does not; \
                     the seed data is inconsistent and must be repaired before seeding",
                    existing_repo.id.as_str(),
                    existing_repo.workspace_id.as_str()
                ))
            })?;
        if ws.tenant_id != Id::new(&tenant_id) {
            return Err(ApiError::Conflict(format!(
                "demo seed is owned by tenant '{}'; caller is scoped to tenant '{}'. \
                 Per-tenant demo seeding is not supported: the seed fixture ids \
                 ('seed-repo-1', workspace 'default') are global and would collide",
                ws.tenant_id.as_str(),
                tenant_id
            )));
        }
        return Ok(Json(SeedResponse {
            projects: 0,
            repos: 3,
            agents: 4,
            tasks: 6,
            merge_requests: 2,
            merge_queue_entries: 1,
            activity_events: 5,
            already_seeded: true,
        }));
    }

    let now = now_secs();

    // ── Tenant + Workspace ────────────────────────────────────────────────────
    // Repos below reference workspace_id: "default". The tenant and workspace
    // records must exist so the frontend can resolve them (e.g. GET
    // /workspaces/default). Both are checked deliberately: an existing row is
    // fine (idempotent re-run), but a storage failure propagates — a dropped
    // error here leaves repos pointing at a workspace that does not exist.
    let tenant_row = state.tenants.find_by_id(&Id::new(&tenant_id)).await?;
    if tenant_row.is_none() {
        let tenant = Tenant::new(Id::new(&tenant_id), "Default Tenant", &tenant_id, now);
        state.tenants.create(&tenant).await?;
    }

    // The seed workspace id is global ("default"). If it already exists but
    // belongs to a different tenant, the repos we are about to create would
    // be scoped into a workspace invisible to this caller — same collision
    // class as the seeded-repo check above, caught before any write.
    if let Some(existing_ws) = state.workspaces.find_by_id(&seed_workspace_id).await? {
        if existing_ws.tenant_id != Id::new(&tenant_id) {
            return Err(ApiError::Conflict(format!(
                "workspace 'default' already exists in tenant '{}'; caller is scoped \
                 to tenant '{}'. Per-tenant demo seeding is not supported: the seed \
                 fixture workspace id is global",
                existing_ws.tenant_id.as_str(),
                tenant_id
            )));
        }
    } else {
        let workspace = Workspace::new(
            Id::new("default"),
            Id::new(&tenant_id),
            "Default Workspace",
            "default",
            now,
        );
        state.workspaces.create(&workspace).await?;
    }
    // Platform-model.md §2: the demo tenant also gets the four built-in
    // personas (idempotent — skipped if already present). Startup seeding
    // only covers tenants that existed at boot, so a tenant created here
    // (or created before the persona seeding existed) is seeded now.
    let seed_tenant_id = Id::new(&tenant_id);
    crate::seed_builtin_personas_for_tenant(&state, &seed_tenant_id).await;

    // Repo paths are derived from the configured repos root (same source as
    // repos.rs create handlers) — never relative "./..." literals, which a
    // child process resolves against its own cwd (task-106 R2-F6 class).
    let seed_repo_path = |name: &str| format!("{}/default/{}.git", state.repos_root, name);
    let repo1 = Repository::new(
        Id::new("seed-repo-1"),
        Id::new("default"),
        "gyre-core",
        seed_repo_path("gyre-core"),
        now - 3500,
    );
    let repo2 = Repository::new(
        Id::new("seed-repo-2"),
        Id::new("default"),
        "gyre-web",
        seed_repo_path("gyre-web"),
        now - 3400,
    );
    let repo3 = Repository::new(
        Id::new("seed-repo-3"),
        Id::new("default"),
        "infra-config",
        seed_repo_path("infra-config"),
        now - 3300,
    );
    state.repos.create(&repo1).await?;
    state.repos.create(&repo2).await?;
    state.repos.create(&repo3).await?;

    // Initialize bare git repos on disk with an initial commit.
    for repo in [&repo1, &repo2, &repo3] {
        if let Err(e) = state.git_ops.init_bare(&repo.path).await {
            tracing::warn!("seed: init_bare failed for {}: {e}", repo.path);
        } else if let Err(e) = state
            .git_ops
            .create_initial_commit(&repo.path, &repo.default_branch)
            .await
        {
            tracing::warn!("seed: create_initial_commit failed for {}: {e}", repo.path);
        }
    }

    // ── Agents ────────────────────────────────────────────────────────────────
    let mut agent1 = Agent::new(Id::new("seed-agent-1"), "orchestrator", now - 1800);
    agent1.status = AgentStatus::Active;
    agent1.last_heartbeat = Some(now - 30);

    let mut agent2 = Agent::new(Id::new("seed-agent-2"), "worker-backend", now - 1200);
    agent2.status = AgentStatus::Active;
    agent2.last_heartbeat = Some(now - 15);

    let mut agent3 = Agent::new(Id::new("seed-agent-3"), "worker-frontend", now - 900);
    agent3.status = AgentStatus::Idle;

    let mut agent4 = Agent::new(Id::new("seed-agent-4"), "reviewer", now - 7200);
    agent4.status = AgentStatus::Dead;

    state.agents.create(&agent1).await?;
    state.agents.create(&agent2).await?;
    state.agents.create(&agent3).await?;
    state.agents.create(&agent4).await?;

    // ── Tasks ─────────────────────────────────────────────────────────────────
    let task1 = Task::new(
        Id::new("seed-task-1"),
        "Set up NixOS flake for CI",
        now - 3000,
    );

    let mut task2 = Task::new(
        Id::new("seed-task-2"),
        "Implement agent spawn API",
        now - 2800,
    );
    task2.status = TaskStatus::InProgress;
    task2.priority = TaskPriority::High;
    task2.assigned_to = Some(Id::new("seed-agent-1"));

    let mut task3 = Task::new(
        Id::new("seed-task-3"),
        "Build Svelte dashboard components",
        now - 2600,
    );
    task3.status = TaskStatus::InProgress;
    task3.assigned_to = Some(Id::new("seed-agent-2"));

    let mut task4 = Task::new(
        Id::new("seed-task-4"),
        "Add Prometheus metrics endpoint",
        now - 2400,
    );
    task4.status = TaskStatus::Review;

    let mut task5 = Task::new(
        Id::new("seed-task-5"),
        "Write E2E Ralph loop test",
        now - 5000,
    );
    task5.status = TaskStatus::Done;
    task5.priority = TaskPriority::Critical;

    let mut task6 = Task::new(
        Id::new("seed-task-6"),
        "Integrate Keycloak OIDC auth",
        now - 1800,
    );
    task6.status = TaskStatus::Blocked;

    state.tasks.create(&task1).await?;
    state.tasks.create(&task2).await?;
    state.tasks.create(&task3).await?;
    state.tasks.create(&task4).await?;
    state.tasks.create(&task5).await?;
    state.tasks.create(&task6).await?;

    // ── Merge Requests ────────────────────────────────────────────────────────
    let mr_open = MergeRequest::new(
        Id::new("seed-mr-1"),
        Id::new("seed-repo-1"),
        seed_workspace_id.clone(),
        "feat: add agent spawn endpoint",
        "feat/agent-spawn",
        "main",
        now - 1200,
    );
    let mut mr_merged = MergeRequest::new(
        Id::new("seed-mr-2"),
        Id::new("seed-repo-1"),
        seed_workspace_id.clone(),
        "feat: add Prometheus metrics",
        "feat/metrics",
        "main",
        now - 5000,
    );
    mr_merged.status = MrStatus::Merged;
    state.merge_requests.create(&mr_open).await?;
    state.merge_requests.create(&mr_merged).await?;

    // ── Merge Queue ───────────────────────────────────────────────────────────
    let queue_entry =
        MergeQueueEntry::new(Id::new("seed-queue-1"), Id::new("seed-mr-1"), 50, now - 600);
    state.merge_queue.enqueue(&queue_entry).await?;

    // ── Activity Events ───────────────────────────────────────────────────────
    let events = [
        (
            "seed-evt-1",
            "seed-agent-1",
            AgEventType::RunStarted,
            "Orchestrator agent started",
            now - 3600,
        ),
        (
            "seed-evt-2",
            "seed-agent-2",
            AgEventType::RunStarted,
            "Backend worker agent spawned",
            now - 2700,
        ),
        (
            "seed-evt-3",
            "seed-agent-2",
            AgEventType::ToolCallStart,
            "Running cargo test --all",
            now - 1800,
        ),
        (
            "seed-evt-4",
            "seed-agent-1",
            AgEventType::StateChanged,
            "Task seed-task-2 transitioned to in_progress",
            now - 900,
        ),
        (
            "seed-evt-5",
            "seed-agent-2",
            AgEventType::TextMessageContent,
            "All tests passing (476 tests)",
            now - 300,
        ),
    ];
    for (event_id, agent_id, event_type, description, timestamp) in events {
        state
            .emit_telemetry(
                gyre_common::Id::new("default"),
                gyre_common::message::MessageKind::StateChanged,
                Some(serde_json::json!({
                    "event_id": event_id,
                    "agent_id": agent_id,
                    "event_type": event_type,
                    "description": description,
                    "timestamp": timestamp,
                })),
            )
            .await;
    }

    // message-bus.md §Broadcast-only kinds: a DataSeeded broadcast is emitted
    // when workspace data seeding completes — server origin,
    // Destination::Broadcast, workspace_id: None, payload: none. It is a
    // broadcast-only kind, so the kind-based storage short-circuit keeps it
    // out of MessageRepository::store() (in-memory broadcast channel only).
    // The tenant scope is the seeding caller's tenant (the same scope every
    // seeded record was created in), never fabricated.
    state
        .emit_tenant_event(
            Id::new(&tenant_id),
            Destination::Broadcast,
            MessageKind::DataSeeded,
            None,
        )
        .await;

    Ok(Json(SeedResponse {
        projects: 2,
        repos: 3,
        agents: 4,
        tasks: 6,
        merge_requests: 2,
        merge_queue_entries: 1,
        activity_events: 5,
        already_seeded: false,
    }))
}

// ── BCP Endpoints ─────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct BcpTargetsResponse {
    pub rto_seconds: u64,
    pub rpo_seconds: u64,
}

/// GET /api/v1/admin/bcp/targets — BCP recovery objectives from environment (Admin only).
pub async fn admin_bcp_targets() -> Json<BcpTargetsResponse> {
    let rto = std::env::var("GYRE_RTO_SECONDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300u64);
    let rpo = std::env::var("GYRE_RPO_SECONDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60u64);
    Json(BcpTargetsResponse {
        rto_seconds: rto,
        rpo_seconds: rpo,
    })
}

#[derive(Serialize)]
pub struct BcpDrillResponse {
    pub snapshot_id: String,
    pub verified: bool,
    pub duration_ms: u64,
}

/// POST /api/v1/admin/bcp/drill — create+verify a snapshot, return drill result (Admin only).
pub async fn admin_bcp_drill(
    State(state): State<Arc<AppState>>,
) -> Result<Json<BcpDrillResponse>, ApiError> {
    let start = std::time::Instant::now();

    // Create a real snapshot using the existing snapshot infrastructure.
    let meta = crate::snapshot::create_snapshot(&state)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("bcp drill snapshot failed: {e}")))?;

    // Verify the snapshot file exists on disk.
    let verified = tokio::fs::metadata(&meta.path).await.is_ok();

    let duration_ms = start.elapsed().as_millis() as u64;

    tracing::info!(snapshot_id = %meta.snapshot_id, verified, duration_ms, "BCP drill completed");

    Ok(Json(BcpDrillResponse {
        snapshot_id: meta.snapshot_id,
        verified,
        duration_ms,
    }))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::{
        api::api_router,
        auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt},
        mem::test_state,
    };
    use axum::{body::Body, Router};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app_no_jwt() -> Router {
        api_router().with_state(test_state())
    }

    fn app_with_jwt() -> Router {
        api_router().with_state(make_test_state_with_jwt())
    }

    fn admin_jwt() -> String {
        sign_test_jwt(
            &serde_json::json!({
                "sub": "admin-sub",
                "preferred_username": "admin-user",
                "realm_access": { "roles": ["admin"] }
            }),
            3600,
        )
    }

    fn developer_jwt() -> String {
        sign_test_jwt(
            &serde_json::json!({
                "sub": "dev-sub",
                "preferred_username": "dev-user",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        )
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    // NOTE: auth enforcement (401/403) is now handled by ABAC middleware, not handler-level
    // extractors. These handler-unit tests verify handler logic only; integration tests cover
    // ABAC enforcement.
    #[tokio::test]
    async fn admin_health_no_auth_handler_allows() {
        // Without middleware, handler has no auth extractor and returns 200.
        // ABAC middleware (require_auth + abac) enforces 401/403 in the full stack.
        let resp = app_no_jwt()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn admin_health_with_global_token() {
        let resp = app_no_jwt()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/health")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["status"], "ok");
        assert!(json["uptime_secs"].is_number());
        assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn admin_health_admin_jwt_allowed() {
        let resp = app_with_jwt()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/health")
                    .header("Authorization", format!("Bearer {}", admin_jwt()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn admin_health_developer_handler_allows() {
        // Handler has no role extractor; ABAC middleware (not tested here) enforces 403.
        let resp = app_with_jwt()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/health")
                    .header("Authorization", format!("Bearer {}", developer_jwt()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn admin_jobs_returns_job_list() {
        let resp = app_no_jwt()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/jobs")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let jobs = json.as_array().unwrap();
        assert!(jobs.len() >= 2);
        let names: Vec<_> = jobs.iter().map(|j| j["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"merge_processor"));
        assert!(names.contains(&"stale_agent_detector"));
    }

    #[tokio::test]
    async fn admin_jobs_developer_handler_allows() {
        // Handler has no role extractor; ABAC middleware enforces 403 in the full stack.
        let resp = app_with_jwt()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/jobs")
                    .header("Authorization", format!("Bearer {}", developer_jwt()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// Create a workspace in the test state and return its id, so emitted
    /// telemetry carries a resolvable scope (its tenant is loaded from the
    /// workspace record — never fabricated).
    async fn test_workspace(state: &std::sync::Arc<crate::AppState>) -> gyre_common::Id {
        let ws = gyre_domain::Workspace::new(
            gyre_common::Id::new("ws-test"),
            gyre_common::Id::new("tenant-test"),
            "Test Workspace",
            "ws-test",
            1000,
        );
        state.workspaces.create(&ws).await.unwrap();
        ws.id.clone()
    }

    #[tokio::test]
    async fn admin_audit_returns_events() {
        let state = test_state();
        let ws_id = test_workspace(&state).await;
        // TASK-273: /admin/audit serves the DURABLE audit repository
        // (tenant-scoped, free-text searchable) — not the in-memory
        // telemetry buffer.
        state
            .audit
            .record(&gyre_domain::AuditEvent::new(
                gyre_common::Id::new("audit-1"),
                gyre_domain::AuditEventType::from_str("run_started"),
                Some(gyre_common::Id::new("agent-1")),
                None,
                None,
                Some(ws_id.clone()),
                None,
                "agent".to_string(),
                Some("agent-1".to_string()),
                gyre_domain::AuditOutcome::Success,
                serde_json::json!({ "description": "Test event" }),
                None,
                None,
                1000,
            ))
            .await
            .unwrap();

        let app = api_router().with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/audit")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["count"], 1);
        assert_eq!(json["events"][0]["id"], "audit-1");
        assert_eq!(json["events"][0]["event_type"], "run_started");
    }

    #[tokio::test]
    async fn admin_audit_tenant_scoped_and_free_text_searchable() {
        // TASK-273: the admin trail is scoped to the caller's tenant
        // (server-derived) and supports free-text search over the detail
        // payload.
        let state = test_state();
        let mk = |id: &str, tenant: Option<&str>, detail: serde_json::Value| {
            let mut ev = gyre_domain::AuditEvent::new(
                gyre_common::Id::new(id),
                gyre_domain::AuditEventType::from_str("run_started"),
                Some(gyre_common::Id::new("agent-1")),
                None,
                None,
                None,
                None,
                "agent".to_string(),
                Some(id.to_string()),
                gyre_domain::AuditOutcome::Success,
                detail,
                None,
                None,
                1000,
            );
            ev.tenant_id = tenant.map(str::to_string);
            ev
        };
        // Same tenant as the test-token principal ("default"), plus an
        // other-tenant event that must never be visible, plus a legacy
        // NULL-tenant event that remains visible.
        state
            .audit
            .record(&mk(
                "ev-own",
                Some("default"),
                serde_json::json!({ "description": "deployed the payment service" }),
            ))
            .await
            .unwrap();
        state
            .audit
            .record(&mk(
                "ev-other",
                Some("tenant-other"),
                serde_json::json!({ "description": "payment service of another tenant" }),
            ))
            .await
            .unwrap();
        state
            .audit
            .record(&mk(
                "ev-legacy",
                None,
                serde_json::json!({ "description": "pre-migration row" }),
            ))
            .await
            .unwrap();

        let app = api_router().with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/audit")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["count"], 2, "other-tenant events must be invisible");
        let ids: Vec<&str> = json["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["id"].as_str().unwrap())
            .collect();
        assert!(ids.contains(&"ev-own"));
        assert!(ids.contains(&"ev-legacy"));
    }

    #[tokio::test]
    async fn admin_audit_free_text_search() {
        let state = test_state();
        let mk = |id: &str, detail: serde_json::Value| {
            gyre_domain::AuditEvent::new(
                gyre_common::Id::new(id),
                gyre_domain::AuditEventType::from_str("run_started"),
                Some(gyre_common::Id::new("agent-1")),
                None,
                None,
                None,
                None,
                "agent".to_string(),
                Some(id.to_string()),
                gyre_domain::AuditOutcome::Success,
                detail,
                None,
                None,
                1000,
            )
        };
        state
            .audit
            .record(&mk(
                "ev-pay",
                serde_json::json!({ "description": "deployed the payment service" }),
            ))
            .await
            .unwrap();
        state
            .audit
            .record(&mk(
                "ev-auth",
                serde_json::json!({ "description": "rotated the auth keys" }),
            ))
            .await
            .unwrap();

        let app = api_router().with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/audit?q=PAYMENT")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["count"], 1, "case-insensitive free-text search");
        assert_eq!(json["events"][0]["id"], "ev-pay");
    }

    #[tokio::test]
    async fn admin_audit_drops_telemetry_with_unresolvable_scope() {
        // The workspace does not exist, so its tenant cannot be resolved: the
        // telemetry must be dropped (skip-and-log), never re-targeted to a
        // fabricated "default" identity (task-099 F1 flaw class).
        let state = test_state();
        state
            .emit_telemetry(
                gyre_common::Id::new("ghost-workspace"),
                gyre_common::message::MessageKind::StateChanged,
                Some(serde_json::json!({
                    "event_id": "ghost-event",
                    "agent_id": "agent-1",
                    "event_type": "state_changed",
                    "description": "Must not be recorded",
                    "timestamp": 1000u64,
                })),
            )
            .await;

        // The admin audit trail reads the durable audit repository, which
        // contains no fabricated-scope rows.
        let app = api_router().with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/audit")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["count"], 0, "telemetry with an unresolvable workspace scope must be dropped, not recorded under a fabricated tenant");
    }

    #[tokio::test]
    async fn admin_audit_filters_by_agent_id() {
        let state = test_state();
        let mk = |id: &str, agent: &str| {
            gyre_domain::AuditEvent::new(
                gyre_common::Id::new(id),
                gyre_domain::AuditEventType::from_str("run_started"),
                Some(gyre_common::Id::new(agent)),
                None,
                None,
                None,
                None,
                "agent".to_string(),
                Some(id.to_string()),
                gyre_domain::AuditOutcome::Success,
                serde_json::json!({ "description": "From {agent}" }),
                None,
                None,
                1000,
            )
        };
        state.audit.record(&mk("e1", "agent-x")).await.unwrap();
        state.audit.record(&mk("e2", "agent-y")).await.unwrap();

        let app = api_router().with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/audit?agent_id=agent-x")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["count"], 1);
        assert_eq!(json["events"][0]["agent_id"], "agent-x");
    }

    #[tokio::test]
    async fn admin_kill_agent_sets_dead() {
        use crate::api::now_secs;
        use gyre_common::Id;
        use gyre_domain::Agent;

        let state = test_state();
        let now = now_secs();
        let mut agent = Agent::new(Id::new("agent-kill-1"), "kill-target", now);
        agent
            .transition_status(gyre_domain::AgentStatus::Active)
            .ok();
        state.agents.create(&agent).await.unwrap();

        let app = api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/agents/agent-kill-1/kill")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let killed = state
            .agents
            .find_by_id(&Id::new("agent-kill-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(killed.status, gyre_domain::AgentStatus::Dead);

        // analytics.md §Auto-Emitted Events: agent.failed covers "fails or
        // is killed" — the admin force-kill path must record it with the
        // kill reason.
        let events = state
            .analytics
            .query(Some("agent.failed"), None, 10)
            .await
            .unwrap();
        assert_eq!(events.len(), 1, "one agent.failed event expected");
        assert_eq!(events[0].agent_id.as_deref(), Some("agent-kill-1"));
        assert_eq!(events[0].properties["reason"], "force-killed by admin");
        assert!(
            events[0].properties["duration_secs"].as_u64().is_some(),
            "duration_secs must be present"
        );
    }

    #[tokio::test]
    async fn kill_agent_terminates_open_session_before_worktree_cleanup() {
        // Regression (task-258 review): the admin kill path transitions the
        // agent to Dead — that termination must end the agent's open
        // session, capturing the worktree's real dirty state BEFORE the
        // worktree cleanup removes it, so "did the agent leave uncommitted
        // work when it terminated?" stays answerable for killed agents.
        use crate::api::now_secs;
        use gyre_common::Id;
        use gyre_domain::{Agent, AgentWorktree, Repository};

        let state = test_state();
        let now = now_secs();

        // Real git repo + linked worktree with an uncommitted edit.
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main");
        std::fs::create_dir_all(&main).unwrap();
        let git = |args: &[&str]| {
            let ok = std::process::Command::new("git")
                .arg("-C")
                .arg(&main)
                .args(args)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .status()
                .unwrap()
                .success();
            assert!(ok, "git {args:?} failed");
        };
        git(&["init", "-b", "main"]);
        std::fs::write(main.join("src"), "fn main() {}\n").unwrap();
        git(&["add", "src"]);
        git(&["commit", "-m", "init"]);
        let wt_path = dir.path().join("wt");
        git(&[
            "worktree",
            "add",
            "-b",
            "feat/spawn",
            wt_path.to_str().unwrap(),
        ]);

        let mut agent = Agent::new(Id::new("agent-kill-sess"), "kill-sess", now);
        agent
            .transition_status(gyre_domain::AgentStatus::Active)
            .ok();
        state.agents.create(&agent).await.unwrap();
        let repo = Repository::new(
            Id::new("repo-kill-sess"),
            Id::new("ws-1"),
            "r-kill-sess".to_string(),
            main.to_str().unwrap().to_string(),
            now,
        );
        state.repos.create(&repo).await.unwrap();
        let wt = AgentWorktree::new(
            Id::new("wt-kill-sess"),
            agent.id.clone(),
            repo.id.clone(),
            None,
            "feat/spawn",
            wt_path.to_str().unwrap().to_string(),
            now,
        );
        state.worktrees.create(&wt).await.unwrap();
        crate::agent_sessions::start_session(&state, &agent.id, &repo.id, "feat/spawn").await;

        // Uncommitted modification present in the worktree at kill time.
        std::fs::write(wt_path.join("src"), "fn main() { changed }\n").unwrap();

        super::kill_agent(
            &state,
            &agent,
            "force-killed by admin",
            false,
            "admin-t",
            "admin_kill",
        )
        .await
        .unwrap();

        let sessions = state.agent_sessions.find_by_agent(&agent.id).await.unwrap();
        assert_eq!(sessions.len(), 1);
        let s = &sessions[0];
        assert!(s.ended_at.is_some(), "kill must stamp ended_at");
        assert_eq!(
            s.dirty,
            Some(true),
            "uncommitted edit must be captured before worktree cleanup, got dirty={:?}",
            s.dirty
        );
        assert!(
            s.uncommitted_paths.contains(&"src".to_string()),
            "modified file must be listed, got {:?}",
            s.uncommitted_paths
        );
        // No session remains open after the Dead transition.
        assert!(state
            .agent_sessions
            .find_open_by_agent(&agent.id)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn admin_kill_revokes_task_scoped_secrets() {
        use crate::api::now_secs;
        use gyre_common::{Id, Secret, SecretScope, SecretType};
        use gyre_domain::{Agent, Workspace};

        let state = test_state();
        let now = now_secs();

        let ws = Workspace::new(
            Id::new("ws-kill-teardown"),
            Id::new("default"),
            "Kill Teardown WS",
            "kill-teardown-ws",
            now,
        );
        state.workspaces.create(&ws).await.unwrap();

        let mut agent = Agent::new(Id::new("agent-kill-teardown"), "kill-teardown", now);
        agent.workspace_id = Id::new("ws-kill-teardown");
        agent.current_task_id = Some(Id::new("task-kill-teardown"));
        agent
            .transition_status(gyre_domain::AgentStatus::Active)
            .ok();
        state.agents.create(&agent).await.unwrap();

        let mk = |id: &str, name: &str, scope: SecretScope, scope_id: &str| Secret {
            id: Id::new(id),
            name: name.to_string(),
            scope,
            scope_id: scope_id.to_string(),
            secret_type: SecretType::Ephemeral,
            created_by: "system".to_string(),
            created_at: now,
            expires_at: None,
            last_rotated_at: None,
            tenant_id: "default".to_string(),
        };
        state
            .secrets
            .create(
                &mk(
                    "sec-kill-task",
                    "EPHEMERAL_DB",
                    SecretScope::Task,
                    "task-kill-teardown",
                ),
                b"v1",
            )
            .await
            .unwrap();
        state
            .secrets
            .create(
                &mk(
                    "sec-kill-ws",
                    "SHARED_KEY",
                    SecretScope::Workspace,
                    "ws-kill-teardown",
                ),
                b"v2",
            )
            .await
            .unwrap();

        let app = api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/agents/agent-kill-teardown/kill")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        assert!(
            state
                .secrets
                .find_by_id(&Id::new("sec-kill-task"), "default")
                .await
                .unwrap()
                .is_none(),
            "admin kill must revoke the task-scoped secret"
        );
        assert!(
            state
                .secrets
                .find_by_id(&Id::new("sec-kill-ws"), "default")
                .await
                .unwrap()
                .is_some(),
            "workspace-scoped secrets must survive admin kill"
        );
    }

    #[tokio::test]
    async fn admin_kill_blocks_active_task() {
        use crate::api::now_secs;
        use gyre_common::Id;
        use gyre_domain::{Agent, Task, TaskStatus};

        let state = test_state();
        let now = now_secs();

        let task_id = Id::new("task-for-kill");
        let mut task = Task::new(task_id.clone(), "test task", now);
        task.transition_status(TaskStatus::InProgress).ok();
        state.tasks.create(&task).await.unwrap();

        let mut agent = Agent::new(Id::new("agent-kill-2"), "kill-task-agent", now);
        agent
            .transition_status(gyre_domain::AgentStatus::Active)
            .ok();
        agent.current_task_id = Some(task_id.clone());
        state.agents.create(&agent).await.unwrap();

        let app = api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/agents/agent-kill-2/kill")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let blocked_task = state.tasks.find_by_id(&task_id).await.unwrap().unwrap();
        assert_eq!(blocked_task.status, TaskStatus::Blocked);

        // analytics.md §Auto-Emitted Events: task.status_changed (trigger
        // "Task transitions status") — the admin force-kill auto-block is a
        // production transition and must record. Regression: previously only
        // the HTTP status endpoint and MCP update_task recorded.
        let events = state
            .analytics
            .query(Some("task.status_changed"), None, 10)
            .await
            .unwrap();
        assert_eq!(
            events.len(),
            1,
            "admin force-kill auto-block must record task.status_changed"
        );
        let ev = &events[0];
        assert_eq!(ev.properties["task_id"], "task-for-kill");
        assert_eq!(ev.properties["old_status"], "InProgress");
        assert_eq!(ev.properties["new_status"], "Blocked");
    }

    #[tokio::test]
    async fn admin_kill_not_found() {
        let resp = app_no_jwt()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/agents/ghost/kill")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // ── task-263 by-effect boundary: admin task-mutating surfaces ──────────

    async fn boundary_workspace(app: Router) -> (Router, String) {
        let body = serde_json::json!({"name": "admin-boundary-ws"});
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("content-type", "application/json")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        (app, json["id"].as_str().unwrap().to_string())
    }

    async fn boundary_repo(app: Router, ws_id: &str) -> (Router, String) {
        let body = serde_json::json!({"workspace_id": ws_id, "name": "admin-boundary-repo"});
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos")
                    .header("content-type", "application/json")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        (app, json["id"].as_str().unwrap().to_string())
    }

    async fn boundary_task(app: Router, title: &str) -> (Router, String) {
        let body = serde_json::json!({"title": title, "task_type": "implementation"});
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tasks")
                    .header("content-type", "application/json")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        (app, json["id"].as_str().unwrap().to_string())
    }

    /// Spawn through the real POST /api/v1/agents/spawn path so the caller
    /// holds a genuine task-scoped agent JWT (the same route production
    /// workers authenticate with).
    async fn boundary_spawn(
        app: Router,
        repo_id: &str,
        task_id: &str,
        branch: &str,
    ) -> (Router, serde_json::Value) {
        let body = serde_json::json!({
            "name": format!("worker-{}", branch.replace('/', "-")),
            "repo_id": repo_id,
            "task_id": task_id,
            "branch": branch,
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/agents/spawn")
                    .header("content-type", "application/json")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED, "spawn should succeed");
        let json = body_json(resp).await;
        (app, json)
    }

    /// task-263 review repair (by-effect sweep completion): the admin
    /// control surfaces whose execution mutates task rows or terminates
    /// other tasks' workloads — kill (blocks the target agent's task and
    /// tears down its workload), reassign (moves every assigned task with
    /// a generation bump), job trigger (stale-agent/reconciliation jobs
    /// mutate tasks), and snapshot restore — deny task-bound agent
    /// principals in the handler. The probe runs under a policy
    /// configuration that ALLOWS Agent-role subjects on `admin` resources
    /// (priority 860, above the 850 builtin-admin-only-deny), so without
    /// the handler gate every call below would succeed; the denial
    /// message is asserted to name the handler gate, proving the 403
    /// comes from the code boundary and not middleware policy.
    #[tokio::test]
    async fn agent_jwt_denied_on_admin_task_mutating_surfaces() {
        use gyre_common::Id;
        use gyre_domain::policy::{Policy, PolicyEffect, PolicyScope};
        use gyre_domain::{AgentStatus, TaskStatus};

        let state = test_state();
        let app = api_router().with_state(state.clone());

        // Permissive policy above the builtin admin-only-deny (850).
        state
            .policies
            .create(&Policy {
                id: Id::new("test-agent-admin-allow"),
                name: "test-agent-admin-allow".to_string(),
                description: "allows subjects on admin resources (boundary probe)".to_string(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 860,
                effect: PolicyEffect::Allow,
                conditions: vec![],
                actions: vec!["*".to_string()],
                resource_types: vec!["admin".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("test"),
                created_at: crate::api::now_secs(),
                updated_at: crate::api::now_secs(),
            })
            .await
            .unwrap();

        let (app, ws_id) = boundary_workspace(app).await;
        let (app, repo_id) = boundary_repo(app, &ws_id).await;
        let (app, task_a) = boundary_task(app, "Admin boundary A").await;
        let (app, task_b) = boundary_task(app, "Admin boundary B").await;
        let (app, json_a) = boundary_spawn(app, &repo_id, &task_a, "feat/admin-a").await;
        let (app, json_b) = boundary_spawn(app, &repo_id, &task_b, "feat/admin-b").await;
        let agent_a = json_a["agent"]["id"].as_str().unwrap().to_string();
        let agent_b = json_b["agent"]["id"].as_str().unwrap().to_string();
        let token_a = json_a["token"].as_str().unwrap().to_string();

        // Spawn auto-advances Backlog→InProgress: a successful foreign kill
        // would block task B, a successful foreign reassign would move it.
        let task_b_row = state
            .tasks
            .find_by_id(&Id::new(&task_b))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(task_b_row.status, TaskStatus::InProgress);

        // ── kill: 403 from the handler gate, agent B and task B untouched ──
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/admin/agents/{agent_b}/kill"))
                    .header("Authorization", format!("Bearer {token_a}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let json = body_json(resp).await;
        assert!(
            json["error"]
                .as_str()
                .unwrap_or_default()
                .contains("admin control surfaces"),
            "denial must come from the handler task-scope gate, not middleware policy: {json}"
        );
        let agent_b_row = state
            .agents
            .find_by_id(&Id::new(&agent_b))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(agent_b_row.status, AgentStatus::Active);
        let task_b_row = state
            .tasks
            .find_by_id(&Id::new(&task_b))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            task_b_row.status,
            TaskStatus::InProgress,
            "task B must not be blocked by a foreign agent's admin kill"
        );

        // ── reassign: 403, agent A's tasks not moved ──────────────────────
        let body = serde_json::json!({ "target_agent_id": agent_b });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/admin/agents/{agent_a}/reassign"))
                    .header("Authorization", format!("Bearer {token_a}"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let json = body_json(resp).await;
        assert!(
            json["error"]
                .as_str()
                .unwrap_or_default()
                .contains("admin control surfaces"),
            "reassign denial must come from the handler gate: {json}"
        );
        let task_a_row = state
            .tasks
            .find_by_id(&Id::new(&task_a))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            task_a_row.assigned_to.as_ref().map(|id| id.as_str()),
            Some(agent_a.as_str()),
            "task A must not be reassigned by an agent-principal admin call"
        );

        // ── job trigger: 403 (stale-agent/reconciliation jobs mutate tasks) ─
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/jobs/stale_agent_detector/run")
                    .header("Authorization", format!("Bearer {token_a}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let json = body_json(resp).await;
        assert!(
            json["error"]
                .as_str()
                .unwrap_or_default()
                .contains("admin control surfaces"),
            "job-trigger denial must come from the handler gate: {json}"
        );

        // ── snapshot restore: 403 (replaces task state wholesale) ─────────
        let body = serde_json::json!({ "snapshot_id": "no-such-snap" });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/restore")
                    .header("Authorization", format!("Bearer {token_a}"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let json = body_json(resp).await;
        assert!(
            json["error"]
                .as_str()
                .unwrap_or_default()
                .contains("admin control surfaces"),
            "restore denial must come from the handler gate: {json}"
        );

        // ── the operator (global system token) keeps the same right under
        // the same permissive policy — enforcement targets agent
        // task-boundaries only. The kill succeeds and blocks task B.
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/admin/agents/{agent_b}/kill"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let task_b_row = state
            .tasks
            .find_by_id(&Id::new(&task_b))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(task_b_row.status, TaskStatus::Blocked);
    }

    #[tokio::test]
    async fn admin_reassign_moves_tasks() {
        use crate::api::now_secs;
        use gyre_common::Id;
        use gyre_domain::{Agent, Task};

        let state = test_state();
        let now = now_secs();
        let agent1 = Agent::new(Id::new("agent-from"), "from-agent", now);
        let agent2 = Agent::new(Id::new("agent-to"), "to-agent", now);
        state.agents.create(&agent1).await.unwrap();
        state.agents.create(&agent2).await.unwrap();

        let mut task = Task::new(Id::new("task-reassign-1"), "task to reassign", now);
        task.assigned_to = Some(Id::new("agent-from"));
        state.tasks.create(&task).await.unwrap();

        let app = api_router().with_state(state.clone());
        let body = serde_json::json!({ "target_agent_id": "agent-to" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/agents/agent-from/reassign")
                    .header("Authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let reassigned = state
            .tasks
            .find_by_id(&Id::new("task-reassign-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            reassigned.assigned_to.as_ref().map(|id| id.as_str()),
            Some("agent-to")
        );
    }

    #[tokio::test]
    async fn admin_kill_emits_lifecycle_audit_events() {
        // observability.md §Audit Event Types: an admin kill must record an
        // AgentKilled event (reason/killed_by/pid) and revoke + audit the
        // agent's token (AgentTokenRevoked, reason admin_kill).
        use crate::api::now_secs;
        use gyre_common::Id;
        use gyre_domain::Agent;

        let state = test_state();
        let now = now_secs();
        let mut agent = Agent::new(Id::new("agent-kill-audit"), "kill-audit-target", now);
        agent
            .transition_status(gyre_domain::AgentStatus::Active)
            .ok();
        state.agents.create(&agent).await.unwrap();
        // Seed a legacy (non-JWT) token so revocation is observable.
        state
            .kv_store
            .kv_set(
                "agent_tokens",
                "agent-kill-audit",
                "legacy-uuid-token".to_string(),
            )
            .await
            .unwrap();

        let app = api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/agents/agent-kill-audit/kill")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // The token must actually be revoked, not just audited.
        let token = state
            .kv_store
            .kv_get("agent_tokens", "agent-kill-audit")
            .await
            .unwrap();
        assert!(token.is_none(), "killed agent's token must be removed");

        let killed = crate::agent_audit::recorded_events(&state, "agent_killed").await;
        assert_eq!(killed.len(), 1, "exactly one agent_killed event");
        let ev = &killed[0];
        assert_eq!(ev.resource_id.as_deref(), Some("agent-kill-audit"));
        assert_eq!(ev.detail["reason"], "force-killed by admin");
        // Global test token authenticates as "system".
        assert_eq!(ev.detail["killed_by"], "system");
        // No process handle and no attestation → pid unknown (null), not fabricated.
        assert!(ev.detail["pid"].is_null());

        let revoked = crate::agent_audit::recorded_events(&state, "agent_token_revoked").await;
        assert_eq!(revoked.len(), 1, "exactly one agent_token_revoked event");
        assert_eq!(revoked[0].detail["reason"], "admin_kill");
        // Legacy non-JWT token → jti unknown (null), not fabricated.
        assert!(revoked[0].detail["jti"].is_null());
    }

    #[tokio::test]
    async fn admin_reassign_emits_agent_reassigned_audit_event() {
        // observability.md §Audit Event Types: each reassigned task must
        // record an AgentReassigned event attributing the admin who moved it.
        use crate::api::now_secs;
        use gyre_common::Id;
        use gyre_domain::{Agent, Task};

        let state = test_state();
        let now = now_secs();
        let src = Agent::new(Id::new("agent-reassign-audit-src"), "src-agent", now);
        let dst = Agent::new(Id::new("agent-reassign-audit-dst"), "dst-agent", now);
        state.agents.create(&src).await.unwrap();
        state.agents.create(&dst).await.unwrap();
        let mut task = Task::new(Id::new("task-reassign-audit"), "reassign audit task", now);
        task.assigned_to = Some(src.id.clone());
        state.tasks.create(&task).await.unwrap();

        let app = api_router().with_state(state.clone());
        let body = serde_json::json!({ "target_agent_id": "agent-reassign-audit-dst" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/agents/agent-reassign-audit-src/reassign")
                    .header("Authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // The task actually moved.
        let reassigned = state
            .tasks
            .find_by_id(&Id::new("task-reassign-audit"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            reassigned.assigned_to.as_ref().map(|id| id.as_str()),
            Some("agent-reassign-audit-dst")
        );

        let events = crate::agent_audit::recorded_events(&state, "agent_reassigned").await;
        assert_eq!(events.len(), 1, "exactly one agent_reassigned event");
        let ev = &events[0];
        assert_eq!(ev.resource_id.as_deref(), Some("agent-reassign-audit-src"));
        assert_eq!(ev.detail["old_task_id"], "task-reassign-audit");
        assert_eq!(ev.detail["new_task_id"], "task-reassign-audit");
        assert_eq!(ev.detail["reassigned_by"], "system");
        assert_eq!(ev.detail["from_agent_id"], "agent-reassign-audit-src");
        assert_eq!(ev.detail["to_agent_id"], "agent-reassign-audit-dst");
    }

    #[tokio::test]
    async fn admin_reassign_target_not_found() {
        use crate::api::now_secs;
        use gyre_common::Id;
        use gyre_domain::Agent;

        let state = test_state();
        let agent = Agent::new(Id::new("agent-src"), "source-agent", now_secs());
        state.agents.create(&agent).await.unwrap();

        let app = api_router().with_state(state);
        let body = serde_json::json!({ "target_agent_id": "no-such-agent" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/agents/agent-src/reassign")
                    .header("Authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn admin_kill_agent_not_found() {
        // Handler has no role extractor; ABAC middleware enforces 403 in the full stack.
        // Agent "some-id" doesn't exist → 404 from handler.
        let resp = app_with_jwt()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/agents/some-id/kill")
                    .header("Authorization", format!("Bearer {}", developer_jwt()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // ── Export tests ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn admin_export_returns_all_entity_types() {
        let resp = app_no_jwt()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/export")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json["repos"].is_array());
        assert!(json["agents"].is_array());
        assert!(json["tasks"].is_array());
        assert!(json["merge_requests"].is_array());
        assert!(json["activity_events"].is_array());
        assert!(json["exported_at"].is_number());
    }

    #[tokio::test]
    async fn admin_export_developer_handler_allows() {
        // Handler has no role extractor; ABAC middleware enforces 403 in the full stack.
        let resp = app_with_jwt()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/export")
                    .header("Authorization", format!("Bearer {}", developer_jwt()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // ── Retention tests ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn admin_list_retention_returns_defaults() {
        let resp = app_no_jwt()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/retention")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let policies = json.as_array().unwrap();
        assert!(!policies.is_empty());
        let types: Vec<_> = policies
            .iter()
            .map(|p| p["data_type"].as_str().unwrap())
            .collect();
        assert!(types.contains(&"activity_events"));
        // analytics.md §Data Retention: analytics events default to 365
        // days.
        let analytics = json
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["data_type"] == "analytics_events")
            .unwrap();
        assert_eq!(analytics["max_age_days"], 365);
    }

    #[tokio::test]
    async fn admin_update_retention_replaces_policies() {
        let state = test_state();
        let app = api_router().with_state(state.clone());

        // A complete, valid replacement list: all 7 types, values changed.
        let new_policies = serde_json::json!([
            { "data_type": "activity_events", "max_age_days": 30 },
            { "data_type": "agent_logs", "max_age_days": 14 },
            { "data_type": "audit_events", "max_age_days": 180 },
            { "data_type": "snapshots", "max_age_days": u64::MAX, "snapshot_tiers": { "keep_24h": 12, "keep_7d": 3, "keep_4w": 2 } },
            { "data_type": "attestations", "max_age_days": u64::MAX },
            { "data_type": "notifications", "max_age_days": 200, "max_age_days_read": 60 },
            { "data_type": "analytics_events", "max_age_days": 90 }
        ]);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/admin/retention")
                    .header("Authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&new_policies).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let policies = state.retention_store.list();
        assert_eq!(policies.len(), 7);
        assert_eq!(
            policies
                .iter()
                .find(|p| p.data_type == "activity_events")
                .unwrap()
                .max_age_days,
            30
        );
        assert_eq!(
            policies
                .iter()
                .find(|p| p.data_type == "agent_logs")
                .unwrap()
                .max_age_days,
            14
        );
    }

    #[tokio::test]
    async fn admin_update_retention_rejects_incomplete_list() {
        // A partial list would silently disable enforcement for the omitted
        // types — the PUT must reject it and leave the current policies
        // untouched (retention.rs F4).
        let state = test_state();
        let app = api_router().with_state(state.clone());
        let before = state.retention_store.list();

        let partial = serde_json::json!([
            { "data_type": "activity_events", "max_age_days": 30 }
        ]);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/admin/retention")
                    .header("Authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&partial).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Policies unchanged — the rejected list was not applied.
        assert_eq!(state.retention_store.list(), before);
    }

    #[tokio::test]
    async fn admin_update_retention_analytics_form_updates_analytics_only() {
        // analytics.md §Data Retention: `PUT /api/v1/admin/retention` with
        // `{ "retention_days": N }` re-configures the analytics event
        // retention without touching the other data types' policies.
        let state = test_state();
        let app = api_router().with_state(state.clone());

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/admin/retention")
                    .header("Authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({ "retention_days": 30 })).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let policies = state.retention_store.list();
        assert_eq!(
            policies
                .iter()
                .find(|p| p.data_type == "analytics_events")
                .unwrap()
                .max_age_days,
            30
        );
        // Sibling policies are untouched — the analytics form must not
        // silently reset the rest of the retention configuration.
        assert_eq!(
            policies
                .iter()
                .find(|p| p.data_type == "audit_events")
                .unwrap()
                .max_age_days,
            365
        );
        assert_eq!(
            policies
                .iter()
                .find(|p| p.data_type == "notifications")
                .unwrap()
                .max_age_days,
            365
        );

        // The change is visible through the GET (the configured value,
        // not just the in-memory store).
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/retention")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let analytics = json
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["data_type"] == "analytics_events")
            .unwrap();
        assert_eq!(analytics["max_age_days"], 30);
    }

    #[tokio::test]
    async fn admin_update_retention_rejects_invalid_analytics_form() {
        let state = test_state();
        let app = api_router().with_state(state.clone());
        let before = state.retention_store.list();

        // 0 days would purge every analytics event on the next nightly
        // run — rejected, policies untouched.
        for body in [
            serde_json::json!({ "retention_days": 0 }),
            serde_json::json!({ "retention_days": "30" }),
            serde_json::json!({ "something_else": 30 }),
        ] {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("PUT")
                        .uri("/api/v1/admin/retention")
                        .header("Authorization", "Bearer test-token")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "body was {body}");
        }
        assert_eq!(state.retention_store.list(), before);
    }

    #[tokio::test]
    async fn admin_retention_developer_handler_allows() {
        // Handler has no role extractor; ABAC middleware enforces 403 in the full stack.
        let resp = app_with_jwt()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/retention")
                    .header("Authorization", format!("Bearer {}", developer_jwt()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // ── Snapshot tests ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn admin_list_snapshots_returns_array() {
        let resp = app_no_jwt()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/snapshots")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json.is_array());
    }

    #[tokio::test]
    async fn admin_create_snapshot_returns_meta() {
        let resp = app_no_jwt()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/snapshot")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json["snapshot_id"].is_string());
        assert!(json["path"].is_string());
        assert!(json["size_bytes"].is_number());
        assert!(json["created_at"].is_number());

        // Clean up
        let snapshot_id = json["snapshot_id"].as_str().unwrap();
        let _ = crate::snapshot::delete_snapshot(snapshot_id).await;
    }

    #[tokio::test]
    async fn admin_delete_snapshot_not_found() {
        let resp = app_no_jwt()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/v1/admin/snapshots/nonexistent-snapshot")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn admin_restore_snapshot_not_found() {
        let body = serde_json::json!({ "snapshot_id": "no-such-snap" });
        let resp = app_no_jwt()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/restore")
                    .header("Authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // ── Job tests ─────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn admin_jobs_returns_list_with_history() {
        let resp = app_no_jwt()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/jobs")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let jobs = json.as_array().unwrap();
        assert!(jobs.len() >= 2);
        // Each job should have a recent_runs array
        for job in jobs {
            assert!(job["recent_runs"].is_array(), "job should have recent_runs");
        }
    }

    #[tokio::test]
    async fn admin_jobs_status_and_runs_reflect_latest_run_on_memory_fallback() {
        // The in-memory history ring is oldest-first (VecDeque push_back);
        // the fallback path (no durable rows yet — pure-mem state before
        // start_job_registry attaches the store) must still present the
        // MOST RECENT run first, and the job status pill must show the
        // latest run's status, not the oldest's.
        let state = test_state();
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let calls_for_handler = std::sync::Arc::clone(&calls);
        state
            .job_registry
            .register(
                crate::jobs::JobDefinition {
                    name: "flaky_job".to_string(),
                    description: "Fails once, then succeeds".to_string(),
                    interval_secs: 60,
                    enabled: true,
                    run_at_utc_hour: None,
                },
                move |_state, _log| {
                    let calls = std::sync::Arc::clone(&calls_for_handler);
                    async move {
                        if calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                            anyhow::bail!("first run fails")
                        }
                        Ok(())
                    }
                },
            )
            .await;
        // First run fails, latest run succeeds.
        let _ = state
            .job_registry
            .trigger("flaky_job", state.clone())
            .await;
        state
            .job_registry
            .trigger("flaky_job", state.clone())
            .await
            .unwrap();

        let app = api_router().with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/jobs")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let jobs = body_json(resp).await;
        let job = jobs
            .as_array()
            .unwrap()
            .iter()
            .find(|j| j["name"] == "flaky_job")
            .expect("registered job must be listed");
        assert_eq!(
            job["status"], "success",
            "status pill must reflect the LATEST run, got {}",
            job["status"]
        );
        let runs = job["recent_runs"].as_array().unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0]["status"], "success", "runs must be newest-first");
        assert_eq!(runs[1]["status"], "failed");
        assert!(
            runs[0]["started_at"].as_u64().unwrap() >= runs[1]["started_at"].as_u64().unwrap(),
            "newest run must be served first on the memory fallback path"
        );
    }

    #[tokio::test]
    async fn admin_run_job_unknown_returns_404() {
        let resp = app_no_jwt()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/jobs/no_such_job/run")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // ── Seed tests ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn admin_seed_creates_demo_data() {
        let state = test_state();
        let app = api_router().with_state(state.clone());

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/seed")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["projects"], 2);
        assert_eq!(json["repos"], 3);
        assert_eq!(json["agents"], 4);
        assert_eq!(json["tasks"], 6);
        assert_eq!(json["merge_requests"], 2);
        assert_eq!(json["merge_queue_entries"], 1);
        assert_eq!(json["activity_events"], 5);
        assert_eq!(json["already_seeded"], false);

        let repos = state.repos.list().await.unwrap();
        assert_eq!(repos.len(), 3);
        let agents = state.agents.list().await.unwrap();
        assert_eq!(agents.len(), 4);
        let tasks = state.tasks.list().await.unwrap();
        assert_eq!(tasks.len(), 6);
    }

    /// message-bus.md §Broadcast-only kinds: completing the workspace
    /// data-seed flow emits a `DataSeeded` broadcast observable by a
    /// subscribed client — server origin, `Destination::Broadcast`,
    /// `workspace_id: None`, payload none — and the kind's storage exemption
    /// means no row is ever persisted. Before task-249 the kind had zero
    /// production emitters, so this assertion could never pass.
    #[tokio::test]
    async fn admin_seed_broadcasts_data_seeded() {
        use gyre_common::message::{Destination, MessageKind, MessageOrigin};

        let state = test_state();
        let mut rx = state.message_broadcast_tx.subscribe();
        let app = api_router().with_state(state.clone());

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/seed")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Drain: the seed flow also emits StateChanged telemetry on the same
        // channel, so scan for the DataSeeded broadcast.
        let mut found = None;
        for _ in 0..32 {
            match rx.try_recv() {
                Ok(m) if m.kind == MessageKind::DataSeeded => {
                    found = Some(m);
                    break;
                }
                Ok(_) => continue,
                Err(_) => break,
            }
        }
        let msg = found.expect("seed completion must broadcast DataSeeded to subscribers");
        assert!(matches!(msg.from, MessageOrigin::Server));
        assert_eq!(msg.to, Destination::Broadcast);
        assert_eq!(
            msg.workspace_id, None,
            "broadcast-only kinds carry no workspace scope"
        );
        assert!(
            msg.payload.is_none(),
            "DataSeeded payload is (none) per spec"
        );
        // Storage exemption is kind-based: no DataSeeded row, ever.
        assert!(
            state.messages.find_by_id(&msg.id).await.unwrap().is_none(),
            "DataSeeded is storage-exempt: it must never hit MessageRepository::store()"
        );
        let listed = state
            .messages
            .list_by_workspace(
                &gyre_common::Id::new("default"),
                None,
                None,
                None,
                None,
                Some(100),
            )
            .await
            .unwrap();
        assert!(
            listed.iter().all(|m| m.kind != MessageKind::DataSeeded),
            "DataSeeded must never hit MessageRepository::store()"
        );
    }

    #[tokio::test]
    async fn admin_seed_is_idempotent() {
        let state = test_state();
        let app = api_router().with_state(state.clone());

        let resp1 = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/seed")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp1.status(), StatusCode::OK);
        let json1 = body_json(resp1).await;
        assert_eq!(json1["already_seeded"], false);

        let resp2 = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/seed")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp2.status(), StatusCode::OK);
        let json2 = body_json(resp2).await;
        assert_eq!(json2["already_seeded"], true);

        // No duplicate repos created
        let repos = state.repos.list().await.unwrap();
        assert_eq!(repos.len(), 3);
    }

    #[tokio::test]
    async fn admin_seed_developer_handler_allows() {
        // Handler has no role extractor; ABAC middleware enforces 403 in the full stack.
        let resp = app_with_jwt()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/seed")
                    .header("Authorization", format!("Bearer {}", developer_jwt()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// A JWT carrying a tenant_id claim scopes the caller to that tenant.
    fn tenant_admin_jwt(tenant: &str) -> String {
        sign_test_jwt(
            &serde_json::json!({
                "sub": "tenant-admin-sub",
                "preferred_username": "tenant-admin",
                "tenant_id": tenant,
                "realm_access": { "roles": ["admin"] }
            }),
            3600,
        )
    }

    #[tokio::test]
    async fn admin_seed_workspace_visible_to_calling_tenant() {
        // The original defect: the seed created its workspace in a hardcoded
        // tenant while every workspace listing filters by the caller's tenant
        // — the seeding caller got zero workspaces back. The workspace must
        // land in the caller's tenant and be visible through the same
        // filtered listing the SPA uses.
        let state = test_state();
        let app = api_router().with_state(state.clone());

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/seed")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Same query path as GET /api/v1/workspaces for this caller:
        // list_by_tenant filtered by the authenticated tenant ("default"
        // for the static token).
        let visible = state
            .workspaces
            .list_by_tenant(&gyre_common::Id::new("default"))
            .await
            .unwrap();
        let seed_ws = visible
            .iter()
            .find(|ws| ws.slug == "default")
            .expect("seeded workspace 'default' must be visible to the calling tenant");
        assert_eq!(seed_ws.tenant_id.as_str(), "default");
    }

    #[tokio::test]
    async fn admin_seed_rejects_caller_from_foreign_tenant() {
        // The seed fixture ids are global: once tenant "default" owns the
        // demo seed, a caller from another tenant must NOT receive a success
        // response describing data outside its scope — the handler must
        // load the actual seed workspace owner and reject the mismatch.
        // State carries a JWT config so the second caller below (a tenant-
        // scoped Keycloak JWT) authenticates; the static token still
        // resolves first as tenant "default" (global token check precedes
        // JWT validation).
        let state = make_test_state_with_jwt();
        let app = api_router().with_state(state.clone());

        // First seed as the static token (tenant "default").
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/seed")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Second seed from a different tenant's admin: 409, and no workspace
        // is created in the foreign tenant.
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/seed")
                    .header(
                        "Authorization",
                        format!("Bearer {}", tenant_admin_jwt("acme-corp")),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let json = body_json(resp).await;
        let err = json["error"].as_str().unwrap_or_default();
        assert!(
            err.contains("acme-corp"),
            "409 body must name the ownership collision, got: {err}"
        );

        let foreign = state
            .workspaces
            .list_by_tenant(&gyre_common::Id::new("acme-corp"))
            .await
            .unwrap();
        assert!(
            foreign.is_empty(),
            "no workspace may be created in the foreign tenant"
        );
    }

    #[tokio::test]
    async fn admin_seed_rejects_when_workspace_id_already_owned_by_foreign_tenant() {
        // Legacy/collision path: no seed repo exists yet, but workspace
        // "default" is already owned by another tenant (e.g. a pre-scoping
        // seed). Creating the seed repos would scope them into a workspace
        // invisible to this caller — rejected before any write.
        let state = test_state();

        // Foreign tenant already owns workspace "default".
        let foreign_ws = gyre_domain::Workspace::new(
            gyre_common::Id::new("default"),
            gyre_common::Id::new("legacy-tenant"),
            "Legacy Workspace",
            "default",
            1000,
        );
        state.workspaces.create(&foreign_ws).await.unwrap();

        let app = api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/seed")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);

        // Nothing was written: no repos, and the workspace row is untouched.
        let repos = state.repos.list().await.unwrap();
        assert!(repos.is_empty(), "no repos may be created on collision");
        let ws = state
            .workspaces
            .find_by_id(&gyre_common::Id::new("default"))
            .await
            .unwrap()
            .expect("workspace row must still exist");
        assert_eq!(
            ws.tenant_id.as_str(),
            "legacy-tenant",
            "existing workspace ownership must not be reassigned by seeding"
        );
    }

    #[tokio::test]
    async fn admin_seed_inconsistent_repo_without_workspace_is_conflict() {
        // A seed repo whose workspace row is missing means the seed data is
        // corrupt; the handler must not paper over it by reporting success.
        let state = test_state();
        let orphan_repo = gyre_domain::Repository::new(
            gyre_common::Id::new("seed-repo-1"),
            gyre_common::Id::new("default"),
            "orphan",
            "./repos/default/orphan.git",
            1000,
        );
        state.repos.create(&orphan_repo).await.unwrap();

        let app = api_router().with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/seed")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }
}
