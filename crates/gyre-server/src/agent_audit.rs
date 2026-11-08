//! Agent lifecycle audit events (observability.md §Audit Event Types).
//!
//! Seven event types track an agent from spawn to death:
//! `AgentSpawned`, `AgentCompleted`, `AgentKilled`, `AgentDead`,
//! `AgentHeartbeat`, `AgentTokenRevoked`, `AgentReassigned`.
//!
//! Every emitter records the event through the `AuditRepository` port (SQLite
//! in production, mem in tests) and broadcasts it on `audit_broadcast_tx` so
//! SSE subscribers (`GET /api/v1/audit/stream`) and the SIEM forwarder see it
//! live. Emission is best-effort: an audit failure must never fail the
//! user-facing operation it documents.

use gyre_common::Id;
use gyre_domain::{Agent, AuditEvent, AuditEventType, AuditOutcome};

use crate::AppState;

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Record an agent lifecycle audit event and broadcast it to SSE subscribers.
async fn emit(
    state: &AppState,
    event_type: AuditEventType,
    agent: &Agent,
    outcome: AuditOutcome,
    detail: serde_json::Value,
) {
    let event = AuditEvent::new(
        Id::new(uuid::Uuid::new_v4().to_string()),
        event_type,
        Some(agent.id.clone()),
        None,
        None,
        Some(agent.workspace_id.clone()),
        None,
        "agent".to_string(),
        Some(agent.id.to_string()),
        outcome,
        detail,
        None,
        None,
        now_secs(),
    );
    let _ = state.audit.record(&event).await;
    if let Ok(json) = serde_json::to_string(&event) {
        let _ = state.audit_broadcast_tx.send(json);
    }
}

/// `AgentSpawned` — emitted after a successful `POST /api/v1/agents/spawn`.
///
/// Detail fields (spec): `name`, `task_id`, `compute_target`, `branch`,
/// `spawned_by`.
pub async fn emit_spawned(
    state: &AppState,
    agent: &Agent,
    task_id: &str,
    compute_target: &str,
    branch: &str,
    spawned_by: &str,
) {
    emit(
        state,
        AuditEventType::AgentSpawned,
        agent,
        AuditOutcome::Success,
        serde_json::json!({
            "name": agent.name,
            "task_id": task_id,
            "compute_target": compute_target,
            "branch": branch,
            "spawned_by": spawned_by,
        }),
    )
    .await;
}

/// `AgentCompleted` — emitted after a successful
/// `POST /api/v1/agents/{id}/complete`.
///
/// Detail fields (spec): `task_id`, `mr_id`, `branch`.
pub async fn emit_completed(
    state: &AppState,
    agent: &Agent,
    task_id: Option<&Id>,
    mr_id: &Id,
    branch: &str,
) {
    emit(
        state,
        AuditEventType::AgentCompleted,
        agent,
        AuditOutcome::Success,
        serde_json::json!({
            "task_id": task_id.map(|id| id.to_string()),
            "mr_id": mr_id.to_string(),
            "branch": branch,
        }),
    )
    .await;
}

/// `AgentKilled` — emitted when an admin force-kills an agent
/// (`POST /api/v1/admin/agents/{id}/kill`).
///
/// Detail fields (spec): `reason`, `killed_by`, `pid`. `pid` is null when the
/// agent had no locally-tracked process (e.g. it was already gone).
pub async fn emit_killed(
    state: &AppState,
    agent: &Agent,
    reason: &str,
    killed_by: &str,
    pid: Option<u32>,
) {
    emit(
        state,
        AuditEventType::AgentKilled,
        agent,
        AuditOutcome::Success,
        serde_json::json!({
            "reason": reason,
            "killed_by": killed_by,
            "pid": pid,
        }),
    )
    .await;
}

/// `AgentDead` — emitted when the stale agent detector marks an agent Dead
/// after its heartbeat timed out.
///
/// Detail fields (spec): `last_heartbeat_at`, `pid_alive`. `pid_alive` is
/// null when no PID was ever attested for the agent (liveness unknown).
pub async fn emit_dead(
    state: &AppState,
    agent: &Agent,
    last_heartbeat_at: Option<u64>,
    pid_alive: Option<bool>,
) {
    emit(
        state,
        AuditEventType::AgentDead,
        agent,
        AuditOutcome::Failure,
        serde_json::json!({
            "last_heartbeat_at": last_heartbeat_at,
            "pid_alive": pid_alive,
        }),
    )
    .await;
}

/// `AgentHeartbeat` — emitted on `PUT /api/v1/agents/{id}/heartbeat`.
///
/// Detail fields (spec): `pid`, `pid_alive`. Both are null when the agent has
/// no workload attestation (no PID was recorded at spawn).
pub async fn emit_heartbeat(
    state: &AppState,
    agent: &Agent,
    pid: Option<u32>,
    pid_alive: Option<bool>,
) {
    emit(
        state,
        AuditEventType::AgentHeartbeat,
        agent,
        AuditOutcome::Success,
        serde_json::json!({
            "pid": pid,
            "pid_alive": pid_alive,
        }),
    )
    .await;
}

/// `AgentTokenRevoked` — emitted whenever an agent's auth token is revoked
/// (agent completion, operator stop, admin kill).
///
/// Detail fields (spec): `jti`, `reason`. `jti` is null when the revoked
/// credential predates JWT IDs (e.g. UUID fallback tokens).
pub async fn emit_token_revoked(
    state: &AppState,
    agent: &Agent,
    jti: Option<String>,
    reason: &str,
) {
    emit(
        state,
        AuditEventType::AgentTokenRevoked,
        agent,
        AuditOutcome::Success,
        serde_json::json!({
            "jti": jti,
            "reason": reason,
        }),
    )
    .await;
}

/// `AgentReassigned` — emitted when an admin reassigns an agent's tasks
/// (`POST /api/v1/admin/agents/{id}/reassign`).
///
/// Detail fields (spec): `old_task_id`, `new_task_id`, `reassigned_by`.
/// Task identity is stable across reassignment (the task keeps its id; only
/// the assignee changes), so `old_task_id` and `new_task_id` both carry the
/// reassigned task's id and the assignee move is recorded alongside as
/// `from_agent_id`/`to_agent_id`.
pub async fn emit_reassigned(
    state: &AppState,
    agent: &Agent,
    task_id: &Id,
    target_agent_id: &Id,
    reassigned_by: &str,
) {
    emit(
        state,
        AuditEventType::AgentReassigned,
        agent,
        AuditOutcome::Success,
        serde_json::json!({
            "old_task_id": task_id.to_string(),
            "new_task_id": task_id.to_string(),
            "reassigned_by": reassigned_by,
            "from_agent_id": agent.id.to_string(),
            "to_agent_id": target_agent_id.to_string(),
        }),
    )
    .await;
}

/// Revoke an agent's auth token and audit the revocation.
///
/// Deletes the token from the `agent_tokens` KV bucket (so the agent can no
/// longer authenticate) and emits an `AgentTokenRevoked` audit event carrying
/// the token's `jti`. The token is read before deletion so its JWT ID can be
/// attributed; tokens that were never JWTs (UUID fallback) record a null jti.
pub async fn revoke_agent_token(state: &AppState, agent: &Agent, reason: &str) {
    let agent_id = agent.id.to_string();
    let token = state
        .kv_store
        .kv_get("agent_tokens", &agent_id)
        .await
        .ok()
        .flatten();
    let _ = state.kv_store.kv_remove("agent_tokens", &agent_id).await;
    let jti = token
        .as_deref()
        .and_then(|t| state.agent_signing_key.extract_jti_unverified(t));
    emit_token_revoked(state, agent, jti, reason).await;
}

/// Best-effort PID liveness lookup for audit detail fields.
///
/// Reads the agent's workload attestation (if any) and checks whether the
/// attested OS PID is still alive. Returns `None` when no PID was attested —
/// liveness is unknown, not assumed.
pub async fn attested_pid_alive(state: &AppState, agent_id: &str) -> Option<bool> {
    let json = state
        .kv_store
        .kv_get("workload_attestations", agent_id)
        .await
        .ok()
        .flatten()?;
    let att: crate::workload_attestation::WorkloadAttestation = serde_json::from_str(&json).ok()?;
    att.pid.map(crate::workload_attestation::pid_is_alive)
}

/// Best-effort attested PID lookup for audit detail fields.
pub async fn attested_pid(state: &AppState, agent_id: &str) -> Option<u32> {
    let json = state
        .kv_store
        .kv_get("workload_attestations", agent_id)
        .await
        .ok()
        .flatten()?;
    let att: crate::workload_attestation::WorkloadAttestation = serde_json::from_str(&json).ok()?;
    att.pid
}

/// Test helper: fetch recorded audit events of one type from the audit port.
#[cfg(test)]
pub(crate) async fn recorded_events(state: &AppState, event_type: &str) -> Vec<AuditEvent> {
    state
        .audit
        .query(&gyre_ports::AuditQueryFilter {
            event_type: Some(event_type.to_string()),
            limit: 100,
            ..Default::default()
        })
        .await
        .expect("audit query must not fail")
}
