use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{Agent, AgentStatus};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::domain_events::DomainEvent;
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
        }
    }
}

fn parse_agent_status(s: &str) -> Result<AgentStatus, ApiError> {
    match s.to_lowercase().as_str() {
        "idle" => Ok(AgentStatus::Idle),
        "active" => Ok(AgentStatus::Active),
        "blocked" => Ok(AgentStatus::Blocked),
        "error" => Ok(AgentStatus::Error),
        "dead" => Ok(AgentStatus::Dead),
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
    let _ = state.event_tx.send(DomainEvent::AgentCreated {
        id: agent.id.to_string(),
    });

    let token = uuid::Uuid::new_v4().to_string();
    state
        .agent_tokens
        .lock()
        .await
        .insert(agent.id.to_string(), token.clone());

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
    let agents = if let Some(status_str) = params.status {
        let status = parse_agent_status(&status_str)?;
        state.agents.list_by_status(&status).await?
    } else {
        state.agents.list().await?
    };
    Ok(Json(agents.into_iter().map(AgentResponse::from).collect()))
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
    Ok(Json(AgentResponse::from(agent)))
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
    let _ = state.event_tx.send(DomainEvent::AgentStatusChanged {
        id: agent.id.to_string(),
        status: req.status.clone(),
    });
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
    agent.heartbeat(now_secs());
    state.agents.update(&agent).await?;
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
        let (app, id) = create_test_agent(app, "ceo").await;

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
        assert_eq!(json["name"], "ceo");
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
    async fn update_agent_status_invalid_transition() {
        let app = app();
        let (app, id) = create_test_agent(app, "worker2").await;

        // Idle -> Blocked is invalid
        let body = serde_json::json!({ "status": "blocked" });
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
