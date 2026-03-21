use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{AgentCommit, AgentWorktree};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::auth::AuthenticatedAgent;
use crate::AppState;

use super::error::ApiError;
use super::{new_id, now_secs};

// ── Request / Response types ──────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct RecordCommitRequest {
    pub agent_id: String,
    pub commit_sha: String,
    pub branch: String,
}

#[derive(Serialize)]
pub struct AgentCommitResponse {
    pub id: String,
    pub agent_id: String,
    pub repository_id: String,
    pub commit_sha: String,
    pub branch: String,
    pub timestamp: u64,
}

impl From<AgentCommit> for AgentCommitResponse {
    fn from(ac: AgentCommit) -> Self {
        Self {
            id: ac.id.to_string(),
            agent_id: ac.agent_id.to_string(),
            repository_id: ac.repository_id.to_string(),
            commit_sha: ac.commit_sha,
            branch: ac.branch,
            timestamp: ac.timestamp,
        }
    }
}

#[derive(Deserialize)]
pub struct CommitFilterQuery {
    pub agent_id: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateWorktreeRequest {
    pub agent_id: String,
    pub task_id: Option<String>,
    pub branch: String,
    pub path: String,
}

#[derive(Serialize)]
pub struct AgentWorktreeResponse {
    pub id: String,
    pub agent_id: String,
    pub repository_id: String,
    pub task_id: Option<String>,
    pub branch: String,
    pub path: String,
    pub created_at: u64,
}

impl From<AgentWorktree> for AgentWorktreeResponse {
    fn from(wt: AgentWorktree) -> Self {
        Self {
            id: wt.id.to_string(),
            agent_id: wt.agent_id.to_string(),
            repository_id: wt.repository_id.to_string(),
            task_id: wt.task_id.map(|id| id.to_string()),
            branch: wt.branch,
            path: wt.path,
            created_at: wt.created_at,
        }
    }
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// POST /api/v1/repos/:id/commits/record
pub async fn record_commit(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    Json(req): Json<RecordCommitRequest>,
) -> Result<(StatusCode, Json<AgentCommitResponse>), ApiError> {
    // Verify the repo exists.
    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    let ac = AgentCommit::new(
        new_id(),
        Id::new(req.agent_id),
        Id::new(repo_id),
        req.commit_sha,
        req.branch,
        now_secs(),
    );
    state.agent_commits.record(&ac).await?;
    Ok((StatusCode::CREATED, Json(AgentCommitResponse::from(ac))))
}

/// GET /api/v1/repos/:id/commits?agent_id=<optional>
pub async fn list_commits(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    Query(params): Query<CommitFilterQuery>,
) -> Result<Json<Vec<AgentCommitResponse>>, ApiError> {
    // Verify the repo exists.
    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    let commits = if let Some(agent_id) = params.agent_id {
        // Filter by both repo AND agent: get agent's commits, keep matching repo.
        state
            .agent_commits
            .find_by_agent(&Id::new(agent_id))
            .await?
            .into_iter()
            .filter(|ac| ac.repository_id.as_str() == repo_id)
            .collect()
    } else {
        state.agent_commits.find_by_repo(&Id::new(&repo_id)).await?
    };

    Ok(Json(
        commits.into_iter().map(AgentCommitResponse::from).collect(),
    ))
}

/// POST /api/v1/repos/:id/worktrees
pub async fn create_worktree(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    auth: AuthenticatedAgent,
    Json(req): Json<CreateWorktreeRequest>,
) -> Result<(StatusCode, Json<AgentWorktreeResponse>), ApiError> {
    // Verify the repo exists.
    let repo = state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    // G6: ABAC enforcement — check repo access policies against the caller's JWT claims.
    crate::abac::check_repo_abac(&state, &repo_id, &auth)
        .await
        .map_err(ApiError::Forbidden)?;

    // Create the worktree via git ops.
    if let Err(e) = state
        .git_ops
        .create_worktree(&repo.path, &req.path, &req.branch)
        .await
    {
        tracing::warn!("git worktree add failed: {e}");
    }

    let wt = AgentWorktree::new(
        new_id(),
        Id::new(req.agent_id),
        Id::new(repo_id),
        req.task_id.map(Id::new),
        req.branch,
        req.path,
        now_secs(),
    );
    state.worktrees.create(&wt).await?;
    Ok((StatusCode::CREATED, Json(AgentWorktreeResponse::from(wt))))
}

/// GET /api/v1/repos/:id/worktrees
pub async fn list_worktrees(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
) -> Result<Json<Vec<AgentWorktreeResponse>>, ApiError> {
    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    let worktrees = state.worktrees.find_by_repo(&Id::new(&repo_id)).await?;
    Ok(Json(
        worktrees
            .into_iter()
            .map(AgentWorktreeResponse::from)
            .collect(),
    ))
}

/// DELETE /api/v1/repos/:id/worktrees/:wt_id
pub async fn delete_worktree(
    State(state): State<Arc<AppState>>,
    Path((repo_id, wt_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    state.worktrees.delete(&Id::new(&wt_id)).await?;
    Ok(StatusCode::NO_CONTENT)
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
        let repo_id = json["id"].as_str().unwrap().to_string();
        (app, repo_id)
    }

    #[tokio::test]
    async fn record_commit_and_list() {
        let (app, repo_id) = create_repo(app()).await;

        let body = serde_json::json!({
            "agent_id": "agent-1",
            "commit_sha": "abc123",
            "branch": "main"
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/commits/record"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let list_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/agent-commits"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(list_resp.status(), StatusCode::OK);
        let json = body_json(list_resp).await;
        assert_eq!(json.as_array().unwrap().len(), 1);
        assert_eq!(json[0]["commit_sha"], "abc123");
    }

    #[tokio::test]
    async fn list_commits_filter_by_agent() {
        let (app, repo_id) = create_repo(app()).await;

        for (agent, sha) in [("agent-1", "sha1"), ("agent-2", "sha2")] {
            let body = serde_json::json!({
                "agent_id": agent,
                "commit_sha": sha,
                "branch": "main"
            });
            app.clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/api/v1/repos/{repo_id}/commits/record"))
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
        }

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/repos/{repo_id}/agent-commits?agent_id=agent-1"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json.as_array().unwrap().len(), 1);
        assert_eq!(json[0]["agent_id"], "agent-1");
    }

    #[tokio::test]
    async fn record_commit_repo_not_found() {
        let app = app();
        let body = serde_json::json!({
            "agent_id": "agent-1",
            "commit_sha": "abc",
            "branch": "main"
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos/no-such/commits/record")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn create_and_list_worktrees() {
        let (app, repo_id) = create_repo(app()).await;

        let body = serde_json::json!({
            "agent_id": "agent-1",
            "branch": "feat/x",
            "path": "/tmp/wt/agent-1"
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/worktrees"))
                    .header("content-type", "application/json")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let created = body_json(resp).await;
        let wt_id = created["id"].as_str().unwrap().to_string();

        let list_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/worktrees"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(list_resp.status(), StatusCode::OK);
        let json = body_json(list_resp).await;
        assert_eq!(json.as_array().unwrap().len(), 1);
        assert_eq!(json[0]["branch"], "feat/x");

        // Delete the worktree.
        let del_resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/repos/{repo_id}/worktrees/{wt_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(del_resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn create_worktree_with_task_id() {
        let (app, repo_id) = create_repo(app()).await;

        let body = serde_json::json!({
            "agent_id": "agent-1",
            "task_id": "task-42",
            "branch": "feat/task42",
            "path": "/tmp/wt/task42"
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/worktrees"))
                    .header("content-type", "application/json")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["task_id"], "task-42");
    }

    #[tokio::test]
    async fn list_worktrees_empty() {
        let (app, repo_id) = create_repo(app()).await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/worktrees"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json.as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn list_worktrees_repo_not_found() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/no-such/worktrees")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
