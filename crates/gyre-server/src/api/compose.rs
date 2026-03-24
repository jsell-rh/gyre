use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    Json,
};
use gyre_common::Id;
use gyre_domain::{Agent, AgentCompose, AgentStatus, Task};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

use super::error::ApiError;
use super::{new_id, now_secs};

/// Response from compose apply: the created agent tree.
#[derive(Serialize)]
pub struct ComposeApplyResponse {
    pub compose_id: String,
    pub agents: Vec<SpawnedAgentInfo>,
}

#[derive(Serialize)]
pub struct SpawnedAgentInfo {
    pub name: String,
    pub agent_id: String,
    pub task_id: Option<String>,
    pub parent_agent_id: Option<String>,
}

/// Status of the current compose tree.
#[derive(Serialize)]
pub struct ComposeStatusResponse {
    pub compose_id: String,
    pub agents: Vec<ComposeAgentStatus>,
}

#[derive(Serialize)]
pub struct ComposeAgentStatus {
    pub agent_id: String,
    pub name: String,
    pub status: String,
}

#[derive(Deserialize)]
pub struct TeardownRequest {
    pub compose_id: String,
}

/// POST /api/v1/compose/apply
/// Accepts AgentCompose as JSON or YAML. Creates all agents in topological order.
pub async fn compose_apply(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<(StatusCode, Json<ComposeApplyResponse>), ApiError> {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json");

    let spec: AgentCompose = if content_type.contains("yaml") || content_type.contains("yml") {
        serde_yaml::from_slice(&body)
            .map_err(|e| ApiError::InvalidInput(format!("YAML parse error: {e}")))?
    } else {
        serde_json::from_slice(&body)
            .map_err(|e| ApiError::InvalidInput(format!("JSON parse error: {e}")))?
    };

    // Validate repo exists
    state
        .repos
        .find_by_id(&spec.repo_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {} not found", spec.repo_id)))?;

    // Validate and topologically sort
    let ordered = spec.validate_and_sort().map_err(ApiError::InvalidInput)?;

    let now = now_secs();
    let compose_id = new_id().to_string();

    // Track name -> agent_id for parent resolution
    let mut name_to_agent_id: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut spawned = Vec::new();
    let mut agent_ids: Vec<String> = Vec::new();

    for agent_spec in ordered {
        let agent_id = new_id();
        let mut agent = Agent::new(agent_id.clone(), &agent_spec.name, now);

        // Resolve parent
        let parent_agent_id = if let Some(parent_name) = &agent_spec.parent {
            name_to_agent_id.get(parent_name).cloned()
        } else {
            None
        };
        agent.parent_id = parent_agent_id.as_deref().map(Id::new);
        agent.lifetime_budget_secs = agent_spec.lifetime_secs;

        // Create task if specified
        let task_id = if let Some(task_spec) = &agent_spec.task {
            let task_id = new_id();
            let mut task = Task::new(task_id.clone(), &task_spec.title, now);
            task.description = task_spec.description.clone();
            task.priority = task_spec.priority.clone();
            task.assigned_to = Some(agent_id.clone());
            state.tasks.create(&task).await?;
            agent.assign_task(task_id.clone());
            Some(task_id.to_string())
        } else {
            None
        };

        state.agents.create(&agent).await?;

        name_to_agent_id.insert(agent_spec.name.clone(), agent_id.to_string());
        agent_ids.push(agent_id.to_string());

        spawned.push(SpawnedAgentInfo {
            name: agent_spec.name.clone(),
            agent_id: agent_id.to_string(),
            task_id,
            parent_agent_id,
        });
    }

    // Register compose session
    state
        .compose_sessions
        .lock()
        .await
        .insert(compose_id.clone(), agent_ids);

    Ok((
        StatusCode::CREATED,
        Json(ComposeApplyResponse {
            compose_id,
            agents: spawned,
        }),
    ))
}

/// GET /api/v1/compose/status?compose_id=<id>
pub async fn compose_status(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<ComposeStatusResponse>, ApiError> {
    let compose_id = params
        .get("compose_id")
        .cloned()
        .ok_or_else(|| ApiError::InvalidInput("compose_id query param required".to_string()))?;

    let sessions = state.compose_sessions.lock().await;
    let agent_ids = sessions
        .get(&compose_id)
        .cloned()
        .ok_or_else(|| ApiError::NotFound(format!("compose session {compose_id} not found")))?;
    drop(sessions);

    let mut agents_status = Vec::new();
    for agent_id in &agent_ids {
        if let Ok(Some(agent)) = state.agents.find_by_id(&Id::new(agent_id)).await {
            agents_status.push(ComposeAgentStatus {
                agent_id: agent.id.to_string(),
                name: agent.name.clone(),
                status: format!("{:?}", agent.status).to_lowercase(),
            });
        }
    }

    Ok(Json(ComposeStatusResponse {
        compose_id,
        agents: agents_status,
    }))
}

/// POST /api/v1/compose/teardown -- stop all agents in the compose session.
pub async fn compose_teardown(
    State(state): State<Arc<AppState>>,
    Json(req): Json<TeardownRequest>,
) -> Result<StatusCode, ApiError> {
    let mut sessions = state.compose_sessions.lock().await;
    let agent_ids = sessions.remove(&req.compose_id).ok_or_else(|| {
        ApiError::NotFound(format!("compose session {} not found", req.compose_id))
    })?;
    drop(sessions);

    let now = now_secs();
    for agent_id in &agent_ids {
        if let Ok(Some(mut agent)) = state.agents.find_by_id(&Id::new(agent_id)).await {
            if agent.status != AgentStatus::Dead {
                let _ = agent.transition_status(AgentStatus::Dead);
                agent.last_heartbeat = Some(now);
                let _ = state.agents.update(&agent).await;
            }
        }
    }

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

    async fn create_project_and_repo(app: Router) -> (Router, String, String) {
        let workspace_id = "test-ws".to_string();

        // Create repo
        let repo_body = serde_json::json!({
            "name": "test-repo",
            "workspace_id": workspace_id
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&repo_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let repo = body_json(resp).await;
        let repo_id = repo["id"].as_str().unwrap().to_string();

        (app, workspace_id, repo_id)
    }

    #[tokio::test]
    async fn compose_apply_creates_agent_tree() {
        let app = app();
        let (app, workspace_id, repo_id) = create_project_and_repo(app).await;

        let compose = serde_json::json!({
            "version": "1",
            "repo_id": repo_id,
            "agents": [
                {
                    "name": "orchestrator",
                    "role": "manager",
                    "parent": null,
                    "capabilities": ["orchestration"],
                    "task": {
                        "title": "Orchestrate work",
                        "description": null,
                        "priority": "High"
                    },
                    "branch": null,
                    "lifetime_secs": null
                },
                {
                    "name": "worker",
                    "role": "developer",
                    "parent": "orchestrator",
                    "capabilities": ["rust-dev"],
                    "task": {
                        "title": "Implement feature",
                        "description": "Write the code",
                        "priority": "Medium"
                    },
                    "branch": null,
                    "lifetime_secs": 3600
                }
            ]
        });

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/compose/apply")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&compose).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert!(json["compose_id"].as_str().is_some());
        let agents = json["agents"].as_array().unwrap();
        assert_eq!(agents.len(), 2);

        // Find orchestrator and worker
        let orc = agents.iter().find(|a| a["name"] == "orchestrator").unwrap();
        let wrk = agents.iter().find(|a| a["name"] == "worker").unwrap();

        assert!(orc["parent_agent_id"].is_null());
        assert!(!wrk["parent_agent_id"].is_null());
        assert!(orc["task_id"].as_str().is_some());
        assert!(wrk["task_id"].as_str().is_some());

        // Check worker's parent_agent_id matches orchestrator's agent_id
        assert_eq!(wrk["parent_agent_id"], orc["agent_id"]);
    }

    #[tokio::test]
    async fn compose_status_returns_agent_states() {
        let app = app();
        let (app, workspace_id, repo_id) = create_project_and_repo(app).await;

        let compose = serde_json::json!({
            "version": "1",
            "repo_id": repo_id,
            "agents": [{
                "name": "status-agent",
                "role": "dev",
                "parent": null,
                "capabilities": [],
                "task": null,
                "branch": null,
                "lifetime_secs": null
            }]
        });

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/compose/apply")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&compose).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let apply_json = body_json(resp).await;
        let compose_id = apply_json["compose_id"].as_str().unwrap();

        // Get status
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/compose/status?compose_id={compose_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let status_json = body_json(resp).await;
        assert_eq!(status_json["compose_id"], compose_id);
        let agents = status_json["agents"].as_array().unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0]["name"], "status-agent");
        assert_eq!(agents[0]["status"], "idle");
    }

    #[tokio::test]
    async fn compose_teardown_stops_all_agents() {
        let app = app();
        let (app, workspace_id, repo_id) = create_project_and_repo(app).await;

        let compose = serde_json::json!({
            "version": "1",
            "repo_id": repo_id,
            "agents": [
                {
                    "name": "teardown-a",
                    "role": "dev",
                    "parent": null,
                    "capabilities": [],
                    "task": null,
                    "branch": null,
                    "lifetime_secs": null
                },
                {
                    "name": "teardown-b",
                    "role": "dev",
                    "parent": null,
                    "capabilities": [],
                    "task": null,
                    "branch": null,
                    "lifetime_secs": null
                }
            ]
        });

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/compose/apply")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&compose).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let apply_json = body_json(resp).await;
        let compose_id = apply_json["compose_id"].as_str().unwrap().to_string();

        // Teardown
        let teardown_body = serde_json::json!({ "compose_id": compose_id });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/compose/teardown")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&teardown_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // Verify agents are dead by checking status (should 404 the compose session)
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/compose/status?compose_id={compose_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn compose_apply_yaml_format() {
        let app = app();
        let (app, workspace_id, repo_id) = create_project_and_repo(app).await;

        let yaml = format!(
            r#"version: "1"
repo_id: {repo_id}
agents:
  - name: yaml-agent
    role: developer
    capabilities:
      - rust-dev
    task:
      title: YAML task
      priority: High
"#
        );

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/compose/apply")
                    .header("content-type", "application/yaml")
                    .body(Body::from(yaml))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        let agents = json["agents"].as_array().unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0]["name"], "yaml-agent");
    }

    #[tokio::test]
    async fn compose_apply_validates_missing_repo() {
        let app = app();
        let (app, _ws_id, _) = create_project_and_repo(app).await;

        let compose = serde_json::json!({
            "version": "1",
            "repo_id": "nonexistent-repo",
            "agents": [{
                "name": "a",
                "role": "dev",
                "parent": null,
                "capabilities": [],
                "task": null,
                "branch": null,
                "lifetime_secs": null
            }]
        });

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/compose/apply")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&compose).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn compose_apply_validates_cycle() {
        let app = app();
        let (app, _workspace_id, repo_id) = create_project_and_repo(app).await;

        let compose = serde_json::json!({
            "version": "1",
            "repo_id": repo_id,
            "agents": [
                {
                    "name": "a",
                    "role": "dev",
                    "parent": "b",
                    "capabilities": [],
                    "task": null,
                    "branch": null,
                    "lifetime_secs": null
                },
                {
                    "name": "b",
                    "role": "dev",
                    "parent": "a",
                    "capabilities": [],
                    "task": null,
                    "branch": null,
                    "lifetime_secs": null
                }
            ]
        });

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/compose/apply")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&compose).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn compose_status_not_found() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/compose/status?compose_id=nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn compose_teardown_not_found() {
        let teardown_body = serde_json::json!({ "compose_id": "nonexistent" });
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/compose/teardown")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&teardown_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
