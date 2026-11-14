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
            task_id: None,
            completed_at: None,
            spec_path: None,
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
    Json(req): Json<CreateAgentRequest>,
) -> Result<(StatusCode, Json<RegisterAgentResponse>), ApiError> {
    let now = now_secs();
    let mut agent = Agent::new(new_id(), req.name, now);
    agent.parent_id = req.parent_id.map(Id::new);
    state.agents.create(&agent).await?;
    // Index for search (search.md §Searchable Entities: agent facets status,
    // workspace/repo scope).
    crate::search_indexer::index(&state, crate::search_indexer::agent_document(&agent)).await;
    state
        .emit_event(
            Some(agent.workspace_id.clone()),
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

pub async fn update_agent_status(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateStatusRequest>,
) -> Result<Json<AgentResponse>, ApiError> {
    let mut agent = state
        .agents
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("agent {id} not found")))?;
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
            Some(agent.workspace_id.clone()),
            gyre_common::message::Destination::Workspace(agent.workspace_id.clone()),
            gyre_common::message::MessageKind::AgentStatusChanged,
            Some(serde_json::json!({"agent_id": agent.id.to_string(), "status": req.status})),
        )
        .await;
    Ok(Json(AgentResponse::from(agent)))
}

pub async fn agent_heartbeat(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let mut agent = state
        .agents
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("agent {id} not found")))?;
    let now = now_secs();
    agent.heartbeat(now);
    state.agents.update(&agent).await?;

    // M26.4: Update WireGuard peer last_seen on each agent heartbeat.
    if let Ok(Some(peer)) = state.network_peers.find_by_agent(&agent.id).await {
        let _ = state.network_peers.update_last_seen(&peer.id, now).await;
    }

    // G10: Verify workload attestation liveness on each heartbeat.
    // Checks PID is still alive; stack hash is left empty (stack re-check
    // is only done when agent self-reports via POST /api/v1/agents/{id}/stack).
    //
    // The attested PID and liveness result also feed the AgentHeartbeat
    // audit event (observability.md §Audit Event Types).
    let mut audited_pid: Option<u32> = None;
    let mut audited_pid_alive: Option<bool> = None;
    if let Ok(Some(json)) = state.kv_store.kv_get("workload_attestations", &id).await {
        if let Ok(mut att) =
            serde_json::from_str::<crate::workload_attestation::WorkloadAttestation>(&json)
        {
            audited_pid = att.pid;
            // M19.4: If a container_id is present, verify container liveness
            // via `docker inspect --format='{{.State.Running}}'` in addition to
            // (or instead of) the PID check.
            let container_alive = if let Some(ref cid) = att.container_id {
                // Detect runtime from the container audit record (best-effort).
                let runtime = state
                    .container_audits
                    .find_by_agent_id(&id)
                    .await
                    .ok()
                    .flatten()
                    .map(|r| r.runtime)
                    .unwrap_or_else(|| "docker".to_string());
                let result = tokio::process::Command::new(&runtime)
                    .args(["inspect", "--format={{.State.Running}}", cid.as_str()])
                    .output()
                    .await;
                match result {
                    Ok(out) if out.status.success() => {
                        String::from_utf8_lossy(&out.stdout).trim() == "true"
                    }
                    _ => {
                        tracing::warn!(
                            agent_id = %id,
                            container_id = %cid,
                            "Container liveness check via {} inspect failed on heartbeat",
                            runtime
                        );
                        false
                    }
                }
            } else {
                match att.pid {
                    Some(pid) => {
                        let alive = crate::workload_attestation::pid_is_alive(pid);
                        audited_pid_alive = Some(alive);
                        alive
                    }
                    // No PID recorded → assume alive (attestation cannot disprove).
                    None => true,
                }
            };

            // Container liveness result feeds the AgentHeartbeat audit event.
            if att.container_id.is_some() {
                audited_pid_alive = Some(container_alive);
            }

            let still_ok =
                crate::workload_attestation::verify_attestation(&mut att, container_alive, "");
            if !still_ok {
                tracing::warn!(
                    agent_id = %id,
                    pid = ?att.pid,
                    container_id = ?att.container_id,
                    "Workload attestation liveness check failed on heartbeat"
                );
            }
            // Persist updated attestation (alive + last_verified_at updated).
            if let Ok(updated_json) = serde_json::to_string(&att) {
                let _ = state
                    .kv_store
                    .kv_set("workload_attestations", &id, updated_json)
                    .await;
            }
        }
    }

    // Agent lifecycle audit event (observability.md §Audit Event Types).
    // pid/pid_alive are null when the agent has no workload attestation.
    crate::agent_audit::emit_heartbeat(&state, &agent, audited_pid, audited_pid_alive).await;

    Ok(StatusCode::NO_CONTENT)
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
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn agent_heartbeat() {
        let app = app();
        let (app, id) = create_test_agent(app, "hb-agent").await;

        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/agents/{id}/heartbeat"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn heartbeat_emits_agent_heartbeat_audit_event() {
        // observability.md §Audit Event Types: each heartbeat must record an
        // AgentHeartbeat audit event. Without a workload attestation the
        // pid/pid_alive detail fields are null (unknown), never fabricated.
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let (app, id) = create_test_agent(app, "hb-audit-agent").await;

        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/agents/{id}/heartbeat"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let events = crate::agent_audit::recorded_events(&state, "agent_heartbeat").await;
        assert_eq!(events.len(), 1, "exactly one agent_heartbeat event");
        let ev = &events[0];
        assert_eq!(ev.resource_id.as_deref(), Some(id.as_str()));
        assert!(ev.detail["pid"].is_null(), "no attestation → pid unknown");
        assert!(
            ev.detail["pid_alive"].is_null(),
            "no attestation → pid_alive unknown"
        );
    }

    #[tokio::test]
    async fn heartbeat_audit_reports_attested_pid_liveness() {
        // With a workload attestation present, the AgentHeartbeat audit event
        // must carry the attested PID and its real liveness. A live PID (this
        // test process) reports true; a dead PID (an exited child) reports
        // false — proving the value is measured, not hardcoded.
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let (app, id) = create_test_agent(app, "hb-pid-agent").await;

        // Live PID: this test process itself.
        let live_pid = std::process::id();
        let att = crate::workload_attestation::attest_agent(&id, Some(live_pid), "local", "");
        state
            .kv_store
            .kv_set(
                "workload_attestations",
                &id,
                serde_json::to_string(&att).unwrap(),
            )
            .await
            .unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/agents/{id}/heartbeat"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let events = crate::agent_audit::recorded_events(&state, "agent_heartbeat").await;
        let ev = events
            .iter()
            .find(|e| e.resource_id.as_deref() == Some(id.as_str()))
            .expect("agent_heartbeat event for hb-pid-agent");
        assert_eq!(ev.detail["pid"], live_pid);
        assert_eq!(ev.detail["pid_alive"], true);

        // Dead PID: a child process that has already exited.
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let dead_pid = child.id();
        child.wait().unwrap();
        let att = crate::workload_attestation::attest_agent(&id, Some(dead_pid), "local", "");
        state
            .kv_store
            .kv_set(
                "workload_attestations",
                &id,
                serde_json::to_string(&att).unwrap(),
            )
            .await
            .unwrap();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/agents/{id}/heartbeat"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let events = crate::agent_audit::recorded_events(&state, "agent_heartbeat").await;
        let mine: Vec<_> = events
            .iter()
            .filter(|e| e.resource_id.as_deref() == Some(id.as_str()))
            .collect();
        // Two heartbeats were sent for this agent, no more, no fewer.
        assert_eq!(mine.len(), 2, "two agent_heartbeat events for hb-pid-agent");
        // Audit queries return newest-first, but equal second-granularity
        // timestamps tie under the mem adapter's stable sort, so positional
        // selection is flaky. The first heartbeat was pinned to
        // (live_pid, true) above; identify the second by its content: it
        // must carry the dead child's PID. A handler that re-read a stale
        // attestation would leave no dead-pid event and fail here.
        let second = mine
            .iter()
            .find(|e| e.detail["pid"] == serde_json::json!(dead_pid))
            .expect("second heartbeat reports the attested dead pid");
        assert_eq!(second.detail["pid_alive"], false);
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
