use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{Agent, AgentStatus};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

use super::error::ApiError;
use super::{new_id, now_secs};

#[derive(Deserialize)]
pub struct CreateAgentRequest {
    pub name: String,
    pub parent_id: Option<String>,
}

#[derive(Deserialize)]
pub struct ListAgentsQuery {
    pub status: Option<String>,
    pub workspace_id: Option<String>,
    pub repo_id: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateStatusRequest {
    pub status: String,
}

#[derive(Serialize)]
pub struct AgentResponse {
    pub id: String,
    pub name: String,
    pub status: String,
    pub parent_id: Option<String>,
    pub current_task_id: Option<String>,
    pub spawned_at: u64,
    pub last_heartbeat: Option<u64>,
    pub workspace_id: String,
    /// Fields enriched from agent worktree tracking (populated when available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<u64>,
    /// Spec path resolved from the agent's task (enriched at query time).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec_path: Option<String>,
    /// MR ID created by this agent (enriched at query time).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mr_id: Option<String>,
    /// Attestation level resolved at push time (supply-chain.md §Attestation
    /// Levels): 3 = Gyre-managed container, 2 = registered stack, 1 = raw
    /// push. `None` until the agent's first push.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attestation_level: Option<u8>,
    /// Persona attached at spawn time (orchestrators; task-099 F2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persona_id: Option<String>,
    /// Orchestration tier (platform-model.md §3): "worker",
    /// "workspace_orchestrator", or "repo_orchestrator".
    pub orchestrator_type: String,
}

/// Returned only from POST /api/v1/agents — includes a one-time auth token.
#[derive(Serialize)]
pub struct RegisterAgentResponse {
    #[serde(flatten)]
    pub agent: AgentResponse,
    pub auth_token: String,
}

impl From<Agent> for AgentResponse {
    fn from(a: Agent) -> Self {
        Self {
            id: a.id.to_string(),
            name: a.name,
            status: format!("{:?}", a.status).to_lowercase(),
            parent_id: a.parent_id.map(|id| id.to_string()),
            current_task_id: a.current_task_id.map(|id| id.to_string()),
            spawned_at: a.spawned_at,
            last_heartbeat: a.last_heartbeat,
            workspace_id: a.workspace_id.to_string(),
            repo_id: None,
            branch: None,
            spec_path: None,
            task_id: None,
            completed_at: None,
            attestation_level: a.attestation_level,
            mr_id: None,
            persona_id: a.persona_id.map(|id| id.to_string()),
            orchestrator_type: a.orchestrator_type.to_string(),
        }
    }
}

impl AgentResponse {
    /// Enrich this response with worktree tracking data (repo_id, branch, task_id).
    pub fn with_worktree(mut self, wt: &gyre_domain::AgentWorktree) -> Self {
        self.repo_id = Some(wt.repository_id.to_string());
        self.branch = Some(wt.branch.clone());
        if self.task_id.is_none() {
            self.task_id = wt.task_id.as_ref().map(|id| id.to_string());
        }
        self
    }
}

fn parse_agent_status(s: &str) -> Result<AgentStatus, ApiError> {
    match s.to_lowercase().as_str() {
        "active" => Ok(AgentStatus::Active),
        "idle" => Ok(AgentStatus::Idle),
        "failed" => Ok(AgentStatus::Failed),
        "stopped" => Ok(AgentStatus::Stopped),
        "dead" => Ok(AgentStatus::Dead),
        // Legacy status mappings.
        "blocked" | "error" => Ok(AgentStatus::Failed),
        "paused" => Ok(AgentStatus::Stopped),
        _ => Err(ApiError::InvalidInput(format!("unknown agent status: {s}"))),
    }
}

pub async fn create_agent(
    State(state): State<Arc<AppState>>,
    auth: crate::auth::AuthenticatedAgent,
    Json(req): Json<CreateAgentRequest>,
) -> Result<(StatusCode, Json<RegisterAgentResponse>), ApiError> {
    // Preview agents may not register agents (task-206 review F2): this
    // route mints a fresh opaque agent token under `agent_tokens` for a
    // NEW agent principal that is not in the preview_agents index, so a
    // single call from a preview agent launders a fresh identity past
    // every caller-keyed preview refusal guard (the new token's agent_id
    // is not a preview agent). The middleware maps this route to
    // (agent, write), which the Agent role passes via the builtin
    // agent-scoped-access policy — the per-handler guard is the only
    // refusal layer for a same-tenant preview-agent JWT.
    if crate::api::meta_specs::caller_is_preview_agent(&state, &auth).await {
        return Err(ApiError::Forbidden(
            "preview agents may not register agents — preview runs leave no production \
             footprint"
                .to_string(),
        ));
    }
    let now = now_secs();
    let mut agent = Agent::new(new_id(), req.name, now);
    agent.parent_id = req.parent_id.map(Id::new);
    state.agents.create(&agent).await?;
    // Index for search (search.md §Searchable Entities: agent facets status,
    // workspace/repo scope).
    crate::search_indexer::index(&state, crate::search_indexer::agent_document(&agent)).await;
    state
        .emit_event(
            agent.workspace_id.clone(),
            gyre_common::message::Destination::Workspace(agent.workspace_id.clone()),
            gyre_common::message::MessageKind::AgentCreated,
            Some(serde_json::json!({"agent_id": agent.id.to_string()})),
        )
        .await;

    let token = uuid::Uuid::new_v4().to_string();
    state
        .kv_store
        .kv_set("agent_tokens", &agent.id.to_string(), token.clone())
        .await
        .map_err(ApiError::Internal)?;

    Ok((
        StatusCode::CREATED,
        Json(RegisterAgentResponse {
            agent: AgentResponse::from(agent),
            auth_token: token,
        }),
    ))
}

pub async fn list_agents(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListAgentsQuery>,
) -> Result<Json<Vec<AgentResponse>>, ApiError> {
    let agents = if let Some(ws_id) = params.workspace_id {
        state.agents.list_by_workspace(&Id::new(ws_id)).await?
    } else if let Some(status_str) = params.status {
        let status = parse_agent_status(&status_str)?;
        state.agents.list_by_status(&status).await?
    } else {
        state.agents.list().await?
    };
    let mut results: Vec<AgentResponse> = agents.into_iter().map(AgentResponse::from).collect();
    // Batch-enrich with worktree data
    for resp in &mut results {
        if let Ok(worktrees) = state.worktrees.find_by_agent(&Id::new(&resp.id)).await {
            if let Some(wt) = worktrees.first() {
                resp.repo_id = Some(wt.repository_id.to_string());
                resp.branch = Some(wt.branch.clone());
                if resp.task_id.is_none() {
                    resp.task_id = wt.task_id.as_ref().map(|id| id.to_string());
                }
            }
        }
    }
    // Apply repo_id filter after enrichment (agents are indexed by workspace, not repo).
    if let Some(ref filter_repo) = params.repo_id {
        results.retain(|r| r.repo_id.as_deref() == Some(filter_repo.as_str()));
    }
    Ok(Json(results))
}

/// GET /api/v1/workspaces/:workspace_id/agents — list agents scoped to a workspace.
/// Primary access pattern per api-conventions.md §1.1.
pub async fn list_workspace_agents(
    State(state): State<Arc<AppState>>,
    Path(workspace_id): Path<String>,
) -> Result<Json<Vec<AgentResponse>>, ApiError> {
    let agents = state
        .agents
        .list_by_workspace(&Id::new(workspace_id))
        .await?;
    let mut results: Vec<AgentResponse> = agents.into_iter().map(AgentResponse::from).collect();
    for resp in &mut results {
        if let Ok(worktrees) = state.worktrees.find_by_agent(&Id::new(&resp.id)).await {
            if let Some(wt) = worktrees.first() {
                resp.repo_id = Some(wt.repository_id.to_string());
                resp.branch = Some(wt.branch.clone());
                if resp.task_id.is_none() {
                    resp.task_id = wt.task_id.as_ref().map(|id| id.to_string());
                }
            }
        }
    }
    Ok(Json(results))
}

pub async fn get_agent(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<AgentResponse>, ApiError> {
    let agent = state
        .agents
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("agent {id} not found")))?;
    let mut resp = AgentResponse::from(agent);
    // Enrich with worktree tracking data (repo_id, branch, task_id)
    if let Ok(worktrees) = state.worktrees.find_by_agent(&Id::new(&id)).await {
        if let Some(wt) = worktrees.first() {
            resp = resp.with_worktree(wt);
        }
    }
    // Resolve spec_path from the linked task
    let task_id_str = resp
        .task_id
        .clone()
        .or_else(|| resp.current_task_id.clone());
    if let Some(ref tid) = task_id_str {
        if let Ok(Some(task)) = state.tasks.find_by_id(&Id::new(tid)).await {
            resp.spec_path = task.spec_path;
        }
    }
    // Resolve mr_id from MRs authored by this agent
    if let Some(ref repo_id) = resp.repo_id {
        if let Ok(mrs) = state.merge_requests.list_by_repo(&Id::new(repo_id)).await {
            if let Some(mr) = mrs.iter().find(|m| {
                m.author_agent_id
                    .as_ref()
                    .map(|id| id.to_string())
                    .as_deref()
                    == Some(&id)
            }) {
                resp.mr_id = Some(mr.id.to_string());
            }
        }
    }
    Ok(Json(resp))
}

/// PUT /api/v1/agents/:id/status
///
/// identity-security.md §Task-Scoped JWT Claims — by-effect task-scope
/// surface: a terminal transition (Dead/Failed/Stopped) revokes the target
/// task's Task-scoped secrets, so an agent principal may only act on its
/// own agent or an agent whose task is covered by its `task_id` claim.
/// Non-agent principals are unaffected.
pub async fn update_agent_status(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    auth: crate::auth::AuthenticatedAgent,
    Json(req): Json<UpdateStatusRequest>,
) -> Result<Json<AgentResponse>, ApiError> {
    // Preview agents may not flip agent status (meta-spec reconciliation §5):
    // a preview agent resetting itself to Idle would defeat the
    // killed-on-completion semantics finish_preview_agent enforces (the run
    // ends Stopped, never Idle). Same Agent-role 'write'-on-'*' escape class
    // as the task/MR guards.
    if crate::api::meta_specs::caller_is_preview_agent(&state, &auth).await {
        return Err(ApiError::Forbidden(
            "preview agents may not change agent status — preview runs skip ceremony \
             and never return to Idle"
                .to_string(),
        ));
    }
    let mut agent = state
        .agents
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("agent {id} not found")))?;
    if let Some(claim) = auth.agent_task_claim() {
        let own = auth.agent_id == agent.id.to_string();
        let task_covered = agent
            .current_task_id
            .as_ref()
            .is_some_and(|t| t.to_string() == claim);
        if !own && !task_covered {
            return Err(ApiError::Forbidden(format!(
                "agent token is scoped to task {claim}; cannot act on agent {}",
                agent.id
            )));
        }
    }
    let new_status = parse_agent_status(&req.status)?;
    agent
        .transition_status(new_status)
        .map_err(|e| ApiError::InvalidInput(e.to_string()))?;
    state.agents.update(&agent).await?;
    // search.md §Index Updates: agent status changed -> index updated.
    crate::search_indexer::index(&state, crate::search_indexer::agent_document(&agent)).await;
    // Platform Model §7 (task-100 review round-3 F2): a terminal status
    // transition (Dead/Failed/Stopped — Stopped has no exit transitions)
    // ends the agent's run, so the task-scoped ephemeral secrets must be
    // revoked here like the dedicated stop/fail/kill endpoints (teardown;
    // logs `secret.revoked` per secret).
    if matches!(
        agent.status,
        AgentStatus::Dead | AgentStatus::Failed | AgentStatus::Stopped
    ) {
        crate::secret_rotation::revoke_agent_task_secrets(&state, &agent).await;
        // Auto-track the death (analytics.md §Auto-Emitted Events:
        // agent.failed covers "fails or is killed" — this endpoint is the
        // operator/orchestrator kill path for Failed/Dead/Stopped, the
        // same event the dedicated fail/stop endpoints emit).
        crate::api::analytics::record_agent_failed(
            &state,
            &agent,
            &format!("agent status set to {} via agent status API", req.status),
        )
        .await;
    }
    state
        .emit_event(
            agent.workspace_id.clone(),
            gyre_common::message::Destination::Workspace(agent.workspace_id.clone()),
            gyre_common::message::MessageKind::AgentStatusChanged,
            Some(serde_json::json!({"agent_id": agent.id.to_string(), "status": req.status})),
        )
        .await;
    Ok(Json(AgentResponse::from(agent)))
}

/// Heartbeat request body (identity-security.md Layer 1: the SVID is the
/// workload proof, presented and verified on the attestation path —
/// spawn/attest/heartbeat/verify).
#[derive(Debug, Default, Deserialize)]
pub struct HeartbeatBody {
    /// The agent session's X.509-SVID (PEM chain, leaf first), from the
    /// `GYRE_SVID_PEM` environment variable injected at spawn.
    #[serde(default)]
    pub svid: String,
    /// The workload's current stack fingerprint (from `GYRE_STACK_HASH`
    /// when present; bound against the SVID-backed attestation record).
    #[serde(default)]
    pub stack_fingerprint: String,
}

/// Why a liveness heartbeat was refused. Shared by every heartbeat
/// surface (REST `PUT /agents/{id}/heartbeat` and the agent WebSocket
/// transport's `Heartbeat` message) so the domain liveness path — and
/// its failure semantics — is one code path, not one per transport.
#[derive(Debug)]
pub(crate) enum HeartbeatFailure {
    NotFound(String),
    /// Workload-identity verification failed (identity-security.md
    /// Layer 1) — the agent's liveness is NOT advanced.
    Verification(String),
    /// The agent store rejected the read/update.
    Storage(String),
}

/// Record one agent liveness heartbeat through the shared domain path:
/// fail-closed SVID workload verification, `Agent::heartbeat`,
/// persistence, WireGuard peer `last_seen`, and the agent-heartbeat
/// audit trail. This is the single liveness entry point behind
/// `PUT /api/v1/agents/{id}/heartbeat` and the agent WebSocket channel
/// (architecture.md §API: "WebSocket as the primary transport for
/// agent ↔ server communication") — stale-agent detection behaves
/// identically whichever channel delivered the heartbeat.
///
/// `agent_id` MUST be the verified connection/caller identity, never a
/// client-supplied value (forged-scope invariant).
pub(crate) async fn record_agent_heartbeat(
    state: &Arc<AppState>,
    agent_id: &str,
    svid: &str,
    stack_fingerprint: &str,
) -> Result<(), HeartbeatFailure> {
    let mut agent = state
        .agents
        .find_by_id(&Id::new(agent_id))
        .await
        .map_err(|e| HeartbeatFailure::Storage(e.to_string()))?
        .ok_or_else(|| HeartbeatFailure::NotFound(format!("agent {agent_id} not found")))?;

    // Layer 1 workload-identity verification (identity-security.md): the
    // heartbeat carries the session's SVID, verified fail-closed against
    // the published trust bundle — replacing the retired G10 /proc probe
    // and skip-on-empty string compare. A heartbeat that presents no SVID,
    // an invalid SVID, or a tampered/empty stack fingerprint is rejected:
    // there is no bypass path for unattested workloads on this surface.
    // (Liveness of the agent record itself is still recorded on success.)
    let verification =
        crate::spiffe::attest_session(state, agent_id, svid, stack_fingerprint).await;
    if let Err(reason) = &verification {
        tracing::warn!(
            agent_id = %agent_id,
            reason = %reason,
            "heartbeat rejected: workload-identity verification failed"
        );
        // Rejected heartbeats are audited (Failure) — the workload identity
        // did not verify; the agent record's liveness is NOT advanced.
        let (audited_pid, _) = attestation_pid_and_status(state, agent_id).await;
        crate::agent_audit::emit_heartbeat_rejected(state, &agent, audited_pid, reason).await;
        return Err(HeartbeatFailure::Verification(reason.clone()));
    }

    let now = now_secs();
    agent.heartbeat(now);
    state
        .agents
        .update(&agent)
        .await
        .map_err(|e| HeartbeatFailure::Storage(e.to_string()))?;

    // M26.4: Update WireGuard peer last_seen on each agent heartbeat.
    if let Ok(Some(peer)) = state.network_peers.find_by_agent(&agent.id).await {
        let _ = state.network_peers.update_last_seen(&peer.id, now).await;
    }

    // Agent lifecycle audit event (observability.md §Audit Event Types).
    // pid is the attestation record's metadata (null when none was
    // recorded); pid_alive carries the SVID verification result.
    let (audited_pid, audited_pid_alive) = attestation_pid_and_status(state, agent_id).await;
    crate::agent_audit::emit_heartbeat(state, &agent, audited_pid, audited_pid_alive).await;

    Ok(())
}

pub async fn agent_heartbeat(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    body: axum::body::Bytes,
) -> Result<StatusCode, ApiError> {
    let hb: HeartbeatBody = if body.is_empty() {
        HeartbeatBody::default()
    } else {
        serde_json::from_slice(&body)
            .map_err(|e| ApiError::BadRequest(format!("heartbeat body: {e}")))?
    };

    // The REST path addresses the agent by URL; route scoping for agent
    // principals is the middleware's job (same as every /agents/{id}
    // route). The shared liveness path below records the heartbeat or
    // refuses it with the same semantics as every other channel.
    match record_agent_heartbeat(&state, &id, &hb.svid, &hb.stack_fingerprint).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(HeartbeatFailure::NotFound(m)) => Err(ApiError::NotFound(m)),
        Err(HeartbeatFailure::Verification(reason)) => Err(ApiError::Forbidden(format!(
            "workload-identity verification failed: {reason}"
        ))),
        Err(HeartbeatFailure::Storage(e)) => Err(ApiError::Internal(anyhow::anyhow!(e))),
    }
}

/// Read the attestation record's PID (metadata) and the verification
/// status that was just applied, for the AgentHeartbeat audit event.
async fn attestation_pid_and_status(
    state: &Arc<AppState>,
    id: &str,
) -> (Option<u32>, Option<bool>) {
    match state
        .kv_store
        .kv_get("workload_attestations", id)
        .await
        .ok()
        .flatten()
        .and_then(|json| {
            serde_json::from_str::<crate::workload_attestation::WorkloadAttestation>(&json).ok()
        }) {
        Some(att) => (att.pid, Some(att.alive)),
        None => (None, None),
    }
}

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app() -> Router {
        crate::api::api_router().with_state(test_state())
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn create_test_agent(app: Router, name: &str) -> (Router, String) {
        let body = serde_json::json!({ "name": name });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/agents")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        let id = json["id"].as_str().unwrap().to_string();
        (app, id)
    }

    #[tokio::test]
    async fn create_agent_returns_auth_token() {
        let app = app();
        let body = serde_json::json!({ "name": "token-agent" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/agents")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert!(json["auth_token"].as_str().is_some());
        assert!(!json["auth_token"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn create_agent_and_get() {
        let app = app();
        let (app, id) = create_test_agent(app, "workspace-orchestrator").await;

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/agents/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["name"], "workspace-orchestrator");
        assert_eq!(json["status"], "idle");
    }

    #[tokio::test]
    async fn update_agent_status_valid() {
        let app = app();
        let (app, id) = create_test_agent(app, "worker").await;

        let body = serde_json::json!({ "status": "active" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/agents/{id}/status"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["status"], "active");
    }

    #[tokio::test]
    async fn update_agent_status_terminal_revokes_task_scoped_secrets() {
        // platform-model.md §7 (task-100 review round-3 F2): PUT
        // /api/v1/agents/{id}/status with a terminal status (Dead, Failed,
        // or Stopped — Stopped has no exit transitions) ends the agent's
        // run and must revoke the task-scoped ephemeral secrets, like the
        // dedicated stop/fail/kill endpoints. A non-terminal transition
        // (Active -> Idle) must not revoke.
        use gyre_common::{Id, Secret, SecretScope, SecretType};

        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        state
            .tenants
            .create(&gyre_domain::Tenant::new(
                Id::new("default"),
                "Default",
                "default",
                now,
            ))
            .await
            .unwrap();
        let ws = gyre_domain::Workspace::new(
            Id::new("ws-status"),
            Id::new("default"),
            "Status WS",
            "status-ws",
            now,
        );
        state.workspaces.create(&ws).await.unwrap();

        // Terminal-transition agent and a non-terminal control agent, each
        // with its own task so the control's secret is independently
        // observable.
        let mut agent =
            gyre_domain::Agent::new(Id::new("agent-status-dead"), "status-dead-agent", now);
        agent
            .transition_status(gyre_domain::AgentStatus::Active)
            .ok();
        agent.workspace_id = Id::new("ws-status");
        agent.current_task_id = Some(Id::new("task-status-dead"));
        state.agents.create(&agent).await.unwrap();
        let mut control =
            gyre_domain::Agent::new(Id::new("agent-status-idle"), "status-idle-agent", now);
        control
            .transition_status(gyre_domain::AgentStatus::Active)
            .ok();
        control.workspace_id = Id::new("ws-status");
        control.current_task_id = Some(Id::new("task-status-idle"));
        state.agents.create(&control).await.unwrap();

        let mk = |id: &str, name: &str, scope: SecretScope, scope_id: String| Secret {
            id: Id::new(id),
            name: name.to_string(),
            scope,
            scope_id,
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
                    "sec-status-dead",
                    "EPHEMERAL_DB",
                    SecretScope::Task,
                    "task-status-dead".to_string(),
                ),
                b"v1",
            )
            .await
            .unwrap();
        state
            .secrets
            .create(
                &mk(
                    "sec-status-idle",
                    "EPHEMERAL_DB",
                    SecretScope::Task,
                    "task-status-idle".to_string(),
                ),
                b"v2",
            )
            .await
            .unwrap();
        state
            .secrets
            .create(
                &mk(
                    "sec-status-ws",
                    "SHARED_KEY",
                    SecretScope::Workspace,
                    "ws-status".to_string(),
                ),
                b"v3",
            )
            .await
            .unwrap();
        state
            .secrets
            .create(
                &mk(
                    "sec-status-other",
                    "OTHER_TASK",
                    SecretScope::Task,
                    "task-unrelated".to_string(),
                ),
                b"v4",
            )
            .await
            .unwrap();

        // Terminal transition: Dead — the run ends, task secrets revoked.
        let body = serde_json::json!({ "status": "dead" });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/agents/agent-status-dead/status")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Non-terminal transition: Active -> Idle keeps the secrets.
        let body = serde_json::json!({ "status": "idle" });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/agents/agent-status-idle/status")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let updated = state
            .agents
            .find_by_id(&Id::new("agent-status-dead"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, gyre_domain::AgentStatus::Dead);
        let control_updated = state
            .agents
            .find_by_id(&Id::new("agent-status-idle"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(control_updated.status, gyre_domain::AgentStatus::Idle);

        assert!(
            state
                .secrets
                .find_by_id(&Id::new("sec-status-dead"), "default")
                .await
                .unwrap()
                .is_none(),
            "terminal status transition must revoke the task-scoped secret"
        );
        assert!(
            state
                .secrets
                .find_by_id(&Id::new("sec-status-idle"), "default")
                .await
                .unwrap()
                .is_some(),
            "non-terminal status transition must keep the task-scoped secret"
        );
        assert!(
            state
                .secrets
                .find_by_id(&Id::new("sec-status-ws"), "default")
                .await
                .unwrap()
                .is_some(),
            "workspace-scoped secrets must survive the terminal transition"
        );
        assert!(
            state
                .secrets
                .find_by_id(&Id::new("sec-status-other"), "default")
                .await
                .unwrap()
                .is_some(),
            "other tasks' secrets must survive the terminal transition"
        );
    }

    #[tokio::test]
    async fn update_agent_status_invalid_transition() {
        let app = app();
        let (app, id) = create_test_agent(app, "worker2").await;

        // First move to Dead (terminal state).
        let body = serde_json::json!({ "status": "dead" });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/agents/{id}/status"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Dead -> Active is invalid (Dead is terminal).
        let body = serde_json::json!({ "status": "active" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/agents/{id}/status"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    /// Issue a session SVID for a test agent and return the
    /// (chain_pem, stack_fingerprint) pair the workload presents on the
    /// heartbeat path (identity-security.md Layer 1).
    async fn issue_test_svid(
        state: &std::sync::Arc<crate::AppState>,
        id: &str,
    ) -> (String, String) {
        let svid = crate::spiffe::issue_session_svid(
            state,
            id,
            "test-persona",
            "local",
            "sha256:test-stack-fingerprint",
        )
        .await
        .expect("test SVID issuance must succeed");
        (
            svid.record.chain_pem.clone(),
            svid.record.stack_fingerprint.clone(),
        )
    }

    async fn send_heartbeat(
        app: &Router,
        id: &str,
        body: serde_json::Value,
    ) -> axum::response::Response {
        app.clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/agents/{id}/heartbeat"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn agent_heartbeat() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let (app, id) = create_test_agent(app, "hb-agent").await;
        let (svid_pem, stack) = issue_test_svid(&state, &id).await;

        let resp = send_heartbeat(
            &app,
            &id,
            serde_json::json!({
                "svid": svid_pem,
                "stack_fingerprint": stack,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn agent_heartbeat_without_svid_is_rejected() {
        // Secure-by-default (identity-security.md): a heartbeat that
        // presents no SVID is rejected with 403 — there is no bypass path
        // for unattested workloads on the verification surface. The
        // rejection is audited and the agent's liveness is NOT advanced.
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let (app, id) = create_test_agent(app, "hb-reject-agent").await;

        let before = state
            .agents
            .find_by_id(&crate::Id::new(&id))
            .await
            .unwrap()
            .expect("agent exists")
            .last_heartbeat;
        let resp = send_heartbeat(&app, &id, serde_json::json!({})).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        let after = state
            .agents
            .find_by_id(&crate::Id::new(&id))
            .await
            .unwrap()
            .expect("agent exists");
        assert_eq!(
            after.last_heartbeat, before,
            "rejected heartbeat must not advance agent liveness"
        );

        // The rejection is audited as an AgentHeartbeat event with a
        // Failure outcome (and the rejection reason in the detail).
        let events = crate::agent_audit::recorded_events(&state, "agent_heartbeat").await;
        let mine: Vec<_> = events
            .iter()
            .filter(|e| e.resource_id.as_deref() == Some(id.as_str()))
            .collect();
        assert_eq!(mine.len(), 1, "exactly one heartbeat audit event");
        assert_eq!(
            mine[0].outcome,
            gyre_domain::AuditOutcome::Failure,
            "rejected heartbeat → Failure outcome"
        );
        assert!(
            !mine[0].detail["reason"].as_str().unwrap_or("").is_empty(),
            "rejection reason recorded"
        );
    }

    #[tokio::test]
    async fn heartbeat_with_tampered_stack_is_rejected() {
        // The stack fingerprint is bound to the SVID-backed record at
        // issuance; presenting a different value is a verification
        // failure (fail closed), not a skip.
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let (app, id) = create_test_agent(app, "hb-tamper-agent").await;
        let (svid_pem, _) = issue_test_svid(&state, &id).await;

        let resp = send_heartbeat(
            &app,
            &id,
            serde_json::json!({
                "svid": svid_pem.clone(),
                "stack_fingerprint": "sha256:attacker-stack",
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // An empty stack presentation is likewise rejected (the empty
        // string is not a valid fingerprint to skip on — fail closed).
        let resp = send_heartbeat(
            &app,
            &id,
            serde_json::json!({ "svid": svid_pem, "stack_fingerprint": "" }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn heartbeat_emits_agent_heartbeat_audit_event() {
        // observability.md §Audit Event Types: each heartbeat must record an
        // AgentHeartbeat audit event. Without a workload attestation record
        // the pid detail field is null (unknown), never fabricated; with a
        // verified SVID the pid_alive detail carries the verification
        // result (true).
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let (app, id) = create_test_agent(app, "hb-audit-agent").await;
        let (svid_pem, stack) = issue_test_svid(&state, &id).await;
        // Seed the spawn-time metadata record (as the spawn path writes):
        // the audit event's pid/pid_alive detail fields are read from it.
        let att = crate::workload_attestation::attest_agent_with_container(
            &id,
            None,
            "local",
            "sha256:test-stack-fingerprint",
            None,
            None,
            None,
            None,
        );
        state
            .kv_store
            .kv_set(
                "workload_attestations",
                &id,
                serde_json::to_string(&att).unwrap(),
            )
            .await
            .unwrap();

        let resp = send_heartbeat(
            &app,
            &id,
            serde_json::json!({
                "svid": svid_pem,
                "stack_fingerprint": stack,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let events = crate::agent_audit::recorded_events(&state, "agent_heartbeat").await;
        assert_eq!(events.len(), 1, "exactly one agent_heartbeat event");
        let ev = &events[0];
        assert_eq!(ev.resource_id.as_deref(), Some(id.as_str()));
        assert!(
            ev.detail["pid"].is_null(),
            "no pid metadata recorded → pid unknown"
        );
        assert_eq!(
            ev.detail["pid_alive"], true,
            "verified SVID → pid_alive carries the verification result"
        );
    }

    #[tokio::test]
    async fn heartbeat_audit_carries_attestation_record_details() {
        // With a workload attestation record present, the AgentHeartbeat
        // audit event carries the record's PID metadata and the SVID
        // verification result as pid_alive. A rejected heartbeat (dead
        // workload identity: SVID that does not verify) produces a
        // heartbeat_rejected event instead, with pid_alive false on the
        // attestation record — proving the value is measured, not
        // hardcoded.
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let (app, id) = create_test_agent(app, "hb-pid-agent").await;

        // Seed an attestation record with PID metadata (as the spawn path
        // writes), then verify a heartbeat over the SVID path.
        let att = crate::workload_attestation::attest_agent_with_container(
            &id,
            Some(4242),
            "local",
            "sha256:test-stack-fingerprint",
            None,
            None,
            None,
            None,
        );
        state
            .kv_store
            .kv_set(
                "workload_attestations",
                &id,
                serde_json::to_string(&att).unwrap(),
            )
            .await
            .unwrap();
        let (svid_pem, stack) = issue_test_svid(&state, &id).await;
        let resp = send_heartbeat(
            &app,
            &id,
            serde_json::json!({
                "svid": svid_pem,
                "stack_fingerprint": stack,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let events = crate::agent_audit::recorded_events(&state, "agent_heartbeat").await;
        let ev = events
            .iter()
            .find(|e| e.resource_id.as_deref() == Some(id.as_str()))
            .expect("agent_heartbeat event for hb-pid-agent");
        assert_eq!(ev.detail["pid"], 4242);
        assert_eq!(ev.detail["pid_alive"], true);

        // Dead workload identity: an SVID signed by a foreign CA is
        // rejected (403) — the heartbeat does not advance and the
        // attestation record's alive flag flips false.
        let foreign_ca = gyre_spiffe::TrustDomainCa::generate("attacker.example")
            .expect("foreign CA generation must succeed");
        let foreign = foreign_ca
            .issue_svid(
                &gyre_spiffe::SpiffeId::parse(&format!(
                    "spiffe://attacker.example/agent/test-persona/session/{}",
                    crate::auth::agent_subject(&id)
                ))
                .expect("foreign SPIFFE ID must parse"),
                gyre_spiffe::BASE_SVID_TTL_SECS,
            )
            .expect("foreign SVID issuance must succeed");
        let resp = send_heartbeat(
            &app,
            &id,
            serde_json::json!({
                "svid": foreign.chain_pem,
                "stack_fingerprint": stack,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let events = crate::agent_audit::recorded_events(&state, "agent_heartbeat").await;
        let mine: Vec<_> = events
            .iter()
            .filter(|e| e.resource_id.as_deref() == Some(id.as_str()))
            .collect();
        assert_eq!(mine.len(), 2, "one success + one rejection event");
        assert_eq!(
            mine.iter()
                .filter(|e| e.outcome == gyre_domain::AuditOutcome::Failure)
                .count(),
            1,
            "one rejection event for the foreign SVID"
        );
    }

    #[tokio::test]
    async fn list_agents_by_status() {
        let app = app();
        // Create agent then activate it
        let (app, id) = create_test_agent(app, "status-agent").await;
        let body = serde_json::json!({ "status": "active" });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/agents/{id}/status"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/agents?status=active")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn agent_response_exposes_orchestrator_type() {
        // HSI §4 scoped-chat recipient lookup (requirements gap d382faa3):
        // the agents API must expose the domain orchestration tier
        // (platform-model.md §3) so the workspace overview can select the
        // workspace orchestrator by ROLE, not by name convention. The
        // custom-named orchestrator ("coord") must be distinguishable from
        // a name-matching repo orchestrator ("payments-orchestrator") and
        // a plain worker.
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        state
            .tenants
            .create(&gyre_domain::Tenant::new(
                gyre_common::Id::new("default"),
                "Default",
                "default",
                now,
            ))
            .await
            .unwrap();
        let ws = gyre_domain::Workspace::new(
            gyre_common::Id::new("ws-orch-role"),
            gyre_common::Id::new("default"),
            "Orch Role WS",
            "orch-role-ws",
            now,
        );
        state.workspaces.create(&ws).await.unwrap();

        let mk = |id: &str, name: &str, ty: gyre_domain::OrchestratorType| {
            let mut a = gyre_domain::Agent::new(gyre_common::Id::new(id), name, now);
            a.workspace_id = gyre_common::Id::new("ws-orch-role");
            a.orchestrator_type = ty;
            a
        };
        for agent in [
            mk(
                "ws-orch-1",
                "coord",
                gyre_domain::OrchestratorType::WorkspaceOrchestrator,
            ),
            mk(
                "repo-orch-1",
                "payments-orchestrator",
                gyre_domain::OrchestratorType::RepoOrchestrator,
            ),
            mk(
                "worker-1",
                "search-orchestrator-worker",
                gyre_domain::OrchestratorType::Worker,
            ),
        ] {
            state.agents.create(&agent).await.unwrap();
        }

        // List scoped to the workspace.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces/ws-orch-role/agents")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let by_id = |v: &serde_json::Value, id: &str| {
            v.as_array()
                .unwrap()
                .iter()
                .find(|a| a["id"] == id)
                .unwrap_or_else(|| panic!("agent {id} missing from list: {v:?}"))
                .clone()
        };
        assert_eq!(
            by_id(&json, "ws-orch-1")["orchestrator_type"],
            "workspace_orchestrator"
        );
        assert_eq!(
            by_id(&json, "repo-orch-1")["orchestrator_type"],
            "repo_orchestrator"
        );
        assert_eq!(by_id(&json, "worker-1")["orchestrator_type"], "worker");

        // Single-agent GET carries the same field.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/agents/ws-orch-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["orchestrator_type"], "workspace_orchestrator");
    }

    #[tokio::test]
    async fn get_agent_not_found() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/agents/ghost")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
