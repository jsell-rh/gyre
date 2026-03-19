//! Admin-only endpoints for gyre-server.
//!
//! All endpoints require Admin role (enforced via [`crate::auth::AdminOnly`]).

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::{AgEventType, Id};
use gyre_domain::{AgentStatus, TaskStatus};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{auth::AdminOnly, AppState};

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
    pub project_count: usize,
    pub version: &'static str,
}

/// GET /api/v1/admin/health — system health summary (Admin only).
pub async fn admin_health(
    _admin: AdminOnly,
    State(state): State<Arc<AppState>>,
) -> Result<Json<SystemHealthResponse>, ApiError> {
    let uptime_secs = now_secs().saturating_sub(state.started_at_secs);
    let agents = state.agents.list().await?;
    let active_agents = agents
        .iter()
        .filter(|a| a.status == AgentStatus::Active)
        .count();
    let tasks = state.tasks.list().await?;
    let projects = state.projects.list().await?;

    Ok(Json(SystemHealthResponse {
        status: "ok",
        uptime_secs,
        agent_count: agents.len(),
        active_agents,
        task_count: tasks.len(),
        project_count: projects.len(),
        version: env!("CARGO_PKG_VERSION"),
    }))
}

// ── Background Jobs ───────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct JobInfo {
    pub name: String,
    pub status: String,
    pub interval_secs: u64,
    pub description: String,
}

/// GET /api/v1/admin/jobs — list background jobs (Admin only).
pub async fn admin_jobs(_admin: AdminOnly) -> Json<Vec<JobInfo>> {
    Json(vec![
        JobInfo {
            name: "merge_processor".to_string(),
            status: "running".to_string(),
            interval_secs: 5,
            description: "Processes queued merge requests".to_string(),
        },
        JobInfo {
            name: "stale_agent_detector".to_string(),
            status: "running".to_string(),
            interval_secs: 30,
            description: "Marks agents dead when heartbeat times out (>60s)".to_string(),
        },
    ])
}

// ── Audit Log ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AuditQuery {
    pub agent_id: Option<String>,
    pub event_type: Option<String>,
    pub since: Option<u64>,
    pub limit: Option<usize>,
}

/// GET /api/v1/admin/audit — searchable activity log (Admin only).
pub async fn admin_audit(
    _admin: AdminOnly,
    State(state): State<Arc<AppState>>,
    Query(q): Query<AuditQuery>,
) -> Json<serde_json::Value> {
    let events = state.activity_store.query(q.since, q.limit);
    let filtered: Vec<_> = events
        .into_iter()
        .filter(|e| {
            q.agent_id.as_deref().is_none_or(|id| e.agent_id == id)
                && q.event_type.as_deref().is_none_or(|et| e.event_type.as_str() == et)
        })
        .collect();

    let count = filtered.len();
    Json(serde_json::json!({
        "events": filtered,
        "count": count
    }))
}

// ── Agent Kill ────────────────────────────────────────────────────────────────

/// POST /api/v1/admin/agents/{id}/kill — force-kill an agent (Admin only).
pub async fn admin_kill_agent(
    _admin: AdminOnly,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let mut agent = state
        .agents
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("agent {id} not found")))?;

    if agent.status == AgentStatus::Dead {
        return Ok(StatusCode::NO_CONTENT);
    }

    let now = now_secs();

    let _ = agent.transition_status(AgentStatus::Dead);
    state.agents.update(&agent).await?;

    // Clean up worktrees.
    if let Ok(worktrees) = state.worktrees.find_by_agent(&agent.id).await {
        for wt in worktrees {
            if let Ok(Some(repo)) = state.repos.find_by_id(&wt.repository_id).await {
                let _ = state.git_ops.remove_worktree(&repo.path, &wt.path).await;
            }
            let _ = state.worktrees.delete(&wt.id).await;
        }
    }

    // Block the agent's current task.
    if let Some(task_id) = &agent.current_task_id {
        if let Ok(Some(mut task)) = state.tasks.find_by_id(task_id).await {
            if task.status == TaskStatus::InProgress {
                let _ = task.transition_status(TaskStatus::Blocked);
                task.updated_at = now;
                let _ = state.tasks.update(&task).await;
            }
        }
    }

    state.activity_store.record(gyre_common::ActivityEventData {
        event_id: uuid::Uuid::new_v4().to_string(),
        agent_id: agent.id.to_string(),
        event_type: AgEventType::StateChanged,
        description: format!("Agent {} force-killed by admin", agent.name),
        timestamp: now,
    });

    Ok(StatusCode::NO_CONTENT)
}

// ── Agent Reassign ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ReassignRequest {
    pub target_agent_id: String,
}

/// POST /api/v1/admin/agents/{id}/reassign — reassign agent tasks (Admin only).
pub async fn admin_reassign_agent(
    _admin: AdminOnly,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<ReassignRequest>,
) -> Result<StatusCode, ApiError> {
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
    for mut task in tasks {
        task.assigned_to = Some(target.id.clone());
        task.updated_at = now;
        state.tasks.update(&task).await?;
    }

    state.activity_store.record(gyre_common::ActivityEventData {
        event_id: uuid::Uuid::new_v4().to_string(),
        agent_id: agent.id.to_string(),
        event_type: AgEventType::StateChanged,
        description: format!("Agent {} tasks reassigned to {}", agent.name, target.name),
        timestamp: now,
    });

    Ok(StatusCode::NO_CONTENT)
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

    #[tokio::test]
    async fn admin_health_requires_auth() {
        let resp = app_no_jwt()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
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
    async fn admin_health_developer_gets_403() {
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
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
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
    async fn admin_jobs_developer_gets_403() {
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
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_audit_returns_events() {
        let state = test_state();
        state.activity_store.record(gyre_common::ActivityEventData {
            event_id: "test-event-1".to_string(),
            agent_id: "agent-1".to_string(),
            event_type: gyre_common::AgEventType::StateChanged,
            description: "Test event".to_string(),
            timestamp: 1000,
        });

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
        assert_eq!(json["events"][0]["event_id"], "test-event-1");
    }

    #[tokio::test]
    async fn admin_audit_filters_by_agent_id() {
        let state = test_state();
        state.activity_store.record(gyre_common::ActivityEventData {
            event_id: "e1".to_string(),
            agent_id: "agent-x".to_string(),
            event_type: gyre_common::AgEventType::RunStarted,
            description: "From agent-x".to_string(),
            timestamp: 1000,
        });
        state.activity_store.record(gyre_common::ActivityEventData {
            event_id: "e2".to_string(),
            agent_id: "agent-y".to_string(),
            event_type: gyre_common::AgEventType::RunStarted,
            description: "From agent-y".to_string(),
            timestamp: 2000,
        });

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
    async fn admin_kill_requires_admin() {
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
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
