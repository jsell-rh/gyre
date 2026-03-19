use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{Agent, AgentStatus, AgentWorktree, AnalyticsEvent, MergeRequest, TaskStatus};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::instrument;

use crate::{auth::AuthenticatedAgent, AppState};

use super::agents::AgentResponse;
use super::error::ApiError;
use super::merge_requests::MrResponse;
use super::{new_id, now_secs};

// ── Request / Response types ──────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SpawnAgentRequest {
    pub name: String,
    pub repo_id: String,
    pub task_id: String,
    pub branch: String,
    pub parent_id: Option<String>,
}

#[derive(Serialize)]
pub struct SpawnAgentResponse {
    pub agent: AgentResponse,
    pub token: String,
    pub worktree_path: String,
    pub clone_url: String,
    pub branch: String,
}

#[derive(Deserialize)]
pub struct CompleteAgentRequest {
    pub branch: String,
    pub title: String,
    pub target_branch: String,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// POST /api/v1/agents/spawn
///
/// Orchestrated agent provisioning in one call:
/// 1. Creates agent record (Active status, sets parent_id)
/// 2. Generates auth token
/// 3. Creates a git worktree on the repo for the agent's branch
/// 4. Assigns the task to the agent, advances task to InProgress
/// 5. Records the worktree in DB (linked to agent + task)
#[instrument(skip(state, _auth, req), fields(agent_name = %req.name, branch = %req.branch))]
pub async fn spawn_agent(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedAgent,
    Json(req): Json<SpawnAgentRequest>,
) -> Result<(StatusCode, Json<SpawnAgentResponse>), ApiError> {
    // Verify repo exists
    let repo = state
        .repos
        .find_by_id(&Id::new(&req.repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {} not found", req.repo_id)))?;

    // Verify task exists
    let mut task = state
        .tasks
        .find_by_id(&Id::new(&req.task_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("task {} not found", req.task_id)))?;

    let now = now_secs();

    // Create agent with Active status
    let mut agent = Agent::new(new_id(), req.name, now);
    agent.parent_id = req.parent_id.map(Id::new);
    agent.assign_task(Id::new(&req.task_id));
    agent
        .transition_status(AgentStatus::Active)
        .map_err(|e| ApiError::InvalidInput(e.to_string()))?;
    state.agents.create(&agent).await?;

    // Generate auth token
    let token = uuid::Uuid::new_v4().to_string();
    state
        .agent_tokens
        .lock()
        .await
        .insert(agent.id.to_string(), token.clone());

    // Compute worktree path: {repo_path}/worktrees/{branch_slug}
    let branch_slug = req.branch.replace('/', "-");
    let worktree_path = format!("{}/worktrees/{}", repo.path, branch_slug);

    // Create git worktree (best effort -- failure is logged, not fatal)
    if let Err(e) = state
        .git_ops
        .create_worktree(&repo.path, &worktree_path, &req.branch)
        .await
    {
        tracing::warn!("create_worktree failed: {e}");
    }

    // Record worktree in DB linked to agent and task
    let wt = AgentWorktree::new(
        new_id(),
        agent.id.clone(),
        Id::new(&req.repo_id),
        Some(Id::new(&req.task_id)),
        req.branch.clone(),
        worktree_path.clone(),
        now,
    );
    state.worktrees.create(&wt).await?;

    // Assign task to agent and advance to InProgress
    task.assigned_to = Some(agent.id.clone());
    if task.status == TaskStatus::Backlog {
        let _ = task.transition_status(TaskStatus::InProgress);
    }
    task.updated_at = now;
    state.tasks.update(&task).await?;

    // Build clone URL: {base_url}/git/{project_id}/{repo_name}
    let clone_url = format!("{}/git/{}/{}", state.base_url, repo.project_id, repo.name);

    // Auto-track agent spawn
    let ev = AnalyticsEvent::new(
        new_id(),
        "agent.spawned",
        Some(agent.id.to_string()),
        serde_json::json!({ "task_id": req.task_id }),
        now,
    );
    let _ = state.analytics.record(&ev).await;

    Ok((
        StatusCode::CREATED,
        Json(SpawnAgentResponse {
            agent: AgentResponse::from(agent),
            token,
            worktree_path,
            clone_url,
            branch: req.branch,
        }),
    ))
}

/// POST /api/v1/agents/{id}/complete
///
/// Agent signals it has finished its task:
/// 1. Creates a MergeRequest (source->target)
/// 2. Transitions task status to Review
/// 3. Transitions agent status to Idle
#[instrument(skip(state, req), fields(agent_id = %id, branch = %req.branch))]
pub async fn complete_agent(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<CompleteAgentRequest>,
) -> Result<(StatusCode, Json<MrResponse>), ApiError> {
    let mut agent = state
        .agents
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("agent {id} not found")))?;

    // Get repository_id from agent's worktree
    let worktrees = state.worktrees.find_by_agent(&agent.id).await?;
    let repo_id = worktrees
        .first()
        .map(|wt| wt.repository_id.clone())
        .ok_or_else(|| ApiError::InvalidInput("agent has no associated worktree".to_string()))?;

    let now = now_secs();

    // Create MergeRequest
    let mut mr = MergeRequest::new(
        new_id(),
        repo_id,
        req.title,
        req.branch,
        req.target_branch,
        now,
    );
    mr.author_agent_id = Some(agent.id.clone());
    state.merge_requests.create(&mr).await?;

    // Transition task to Review (navigate through intermediate states as needed)
    if let Some(task_id) = &agent.current_task_id {
        if let Ok(Some(mut task)) = state.tasks.find_by_id(task_id).await {
            if task.status == TaskStatus::Backlog {
                let _ = task.transition_status(TaskStatus::InProgress);
            }
            if task.status == TaskStatus::Blocked {
                let _ = task.transition_status(TaskStatus::InProgress);
            }
            let _ = task.transition_status(TaskStatus::Review);
            task.updated_at = now;
            let _ = state.tasks.update(&task).await;
        }
    }

    // Transition agent to Idle
    let _ = agent.transition_status(AgentStatus::Idle);
    state.agents.update(&agent).await?;

    // Auto-track agent completion
    let ev = AnalyticsEvent::new(
        new_id(),
        "agent.completed",
        Some(agent.id.to_string()),
        serde_json::json!({ "mr_id": mr.id.to_string() }),
        now,
    );
    let _ = state.analytics.record(&ev).await;

    // Auto-track MR creation
    let ev = AnalyticsEvent::new(
        new_id(),
        "mr.created",
        Some(agent.id.to_string()),
        serde_json::json!({ "mr_id": mr.id.to_string(), "source_branch": mr.source_branch }),
        now,
    );
    let _ = state.analytics.record(&ev).await;

    Ok((StatusCode::CREATED, Json(MrResponse::from(mr))))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

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

    async fn create_repo(app: Router) -> (Router, String) {
        let body = serde_json::json!({"project_id": "proj-1", "name": "test-repo"});
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        (app, json["id"].as_str().unwrap().to_string())
    }

    async fn create_task(app: Router, title: &str) -> (Router, String) {
        let body = serde_json::json!({"title": title});
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        (app, json["id"].as_str().unwrap().to_string())
    }

    async fn do_spawn(
        app: Router,
        repo_id: &str,
        task_id: &str,
        branch: &str,
    ) -> (Router, serde_json::Value) {
        let body = serde_json::json!({
            "name": "worker-1",
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

    #[tokio::test]
    async fn spawn_creates_agent_active_with_token() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "Build feature").await;
        let (_, json) = do_spawn(app, &repo_id, &task_id, "feat/build").await;

        assert_eq!(json["agent"]["status"], "active");
        assert!(!json["agent"]["id"].as_str().unwrap().is_empty());
        assert!(!json["token"].as_str().unwrap().is_empty());
        assert_eq!(json["branch"], "feat/build");
    }

    #[tokio::test]
    async fn spawn_creates_worktree_record() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "WT task").await;
        let (app, _) = do_spawn(app, &repo_id, &task_id, "feat/wt-test").await;

        let wt_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/worktrees"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(wt_resp.status(), StatusCode::OK);
        let wt_json = body_json(wt_resp).await;
        assert_eq!(wt_json.as_array().unwrap().len(), 1);
        assert_eq!(wt_json[0]["branch"], "feat/wt-test");
    }

    #[tokio::test]
    async fn spawn_assigns_task_to_agent() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "Assigned task").await;
        let (app, json) = do_spawn(app, &repo_id, &task_id, "feat/assign").await;
        let agent_id = json["agent"]["id"].as_str().unwrap().to_string();

        let task_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/tasks/{task_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let task_json = body_json(task_resp).await;
        assert_eq!(task_json["assigned_to"].as_str().unwrap(), &agent_id);
        assert_eq!(task_json["status"], "in_progress");
    }

    #[tokio::test]
    async fn spawn_returns_correct_clone_url_format() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "URL task").await;
        let (_, json) = do_spawn(app, &repo_id, &task_id, "feat/url-test").await;

        let clone_url = json["clone_url"].as_str().unwrap();
        assert!(
            clone_url.contains("/git/"),
            "clone_url should contain /git/: {clone_url}"
        );
        assert!(
            clone_url.contains("proj-1"),
            "clone_url should contain project id: {clone_url}"
        );
        assert!(
            clone_url.contains("test-repo"),
            "clone_url should contain repo name: {clone_url}"
        );
    }

    #[tokio::test]
    async fn spawn_branch_slashes_become_dashes_in_worktree_path() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "Slash task").await;
        let (_, json) = do_spawn(app, &repo_id, &task_id, "feat/sub/feature").await;

        let wt_path = json["worktree_path"].as_str().unwrap();
        assert!(
            !wt_path.ends_with("feat/sub/feature"),
            "worktree path should not contain raw branch slashes: {wt_path}"
        );
    }

    #[tokio::test]
    async fn spawn_with_parent_id() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "Child task").await;

        let body = serde_json::json!({
            "name": "child-worker",
            "repo_id": repo_id,
            "task_id": task_id,
            "branch": "feat/child",
            "parent_id": "parent-agent-123",
        });
        let resp = app
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
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["agent"]["parent_id"], "parent-agent-123");
    }

    #[tokio::test]
    async fn spawn_requires_valid_auth_token() {
        let app = app();
        let body = serde_json::json!({
            "name": "worker",
            "repo_id": "r",
            "task_id": "t",
            "branch": "feat/x",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/agents/spawn")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn spawn_bad_token_rejected() {
        let app = app();
        let body = serde_json::json!({
            "name": "worker",
            "repo_id": "r",
            "task_id": "t",
            "branch": "feat/x",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/agents/spawn")
                    .header("content-type", "application/json")
                    .header("Authorization", "Bearer bad-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn spawn_repo_not_found() {
        let app = app();
        let body = serde_json::json!({
            "name": "worker",
            "repo_id": "no-such-repo",
            "task_id": "task-1",
            "branch": "feat/x",
        });
        let resp = app
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
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn spawn_task_not_found() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let body = serde_json::json!({
            "name": "worker",
            "repo_id": repo_id,
            "task_id": "no-such-task",
            "branch": "feat/x",
        });
        let resp = app
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
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn complete_creates_merge_request() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "Feature task").await;
        let (app, spawn_json) = do_spawn(app, &repo_id, &task_id, "feat/complete-test").await;
        let agent_id = spawn_json["agent"]["id"].as_str().unwrap().to_string();

        let body = serde_json::json!({
            "branch": "feat/complete-test",
            "title": "Add my feature",
            "target_branch": "main",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/agents/{agent_id}/complete"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let mr_json = body_json(resp).await;
        assert_eq!(mr_json["source_branch"], "feat/complete-test");
        assert_eq!(mr_json["target_branch"], "main");
        assert_eq!(mr_json["title"], "Add my feature");
        assert_eq!(mr_json["status"], "open");
    }

    #[tokio::test]
    async fn complete_transitions_task_to_review() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "Review task").await;
        let (app, spawn_json) = do_spawn(app, &repo_id, &task_id, "feat/review-test").await;
        let agent_id = spawn_json["agent"]["id"].as_str().unwrap().to_string();

        let body = serde_json::json!({
            "branch": "feat/review-test",
            "title": "Done",
            "target_branch": "main",
        });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/agents/{agent_id}/complete"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let task_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/tasks/{task_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let task_json = body_json(task_resp).await;
        assert_eq!(task_json["status"], "review");
    }

    #[tokio::test]
    async fn complete_transitions_agent_to_idle() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "Idle task").await;
        let (app, spawn_json) = do_spawn(app, &repo_id, &task_id, "feat/idle-test").await;
        let agent_id = spawn_json["agent"]["id"].as_str().unwrap().to_string();

        let body = serde_json::json!({
            "branch": "feat/idle-test",
            "title": "Done",
            "target_branch": "main",
        });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/agents/{agent_id}/complete"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let agent_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/agents/{agent_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let agent_json = body_json(agent_resp).await;
        assert_eq!(agent_json["status"], "idle");
    }

    #[tokio::test]
    async fn complete_agent_not_found() {
        let body = serde_json::json!({
            "branch": "feat/x",
            "title": "Done",
            "target_branch": "main",
        });
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/agents/no-such/complete")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn complete_no_worktree_returns_bad_request() {
        let app = app();
        // Create agent directly (no worktree)
        let body = serde_json::json!({"name": "bare-agent"});
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
        let agent_id = json["id"].as_str().unwrap().to_string();

        let body = serde_json::json!({
            "branch": "main",
            "title": "Done",
            "target_branch": "main",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/agents/{agent_id}/complete"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn complete_mr_sets_author_agent_id() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "Author task").await;
        let (app, spawn_json) = do_spawn(app, &repo_id, &task_id, "feat/author-test").await;
        let agent_id = spawn_json["agent"]["id"].as_str().unwrap().to_string();

        let body = serde_json::json!({
            "branch": "feat/author-test",
            "title": "Feature",
            "target_branch": "main",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/agents/{agent_id}/complete"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let mr_json = body_json(resp).await;
        assert_eq!(mr_json["author_agent_id"].as_str().unwrap(), &agent_id);
    }
}
