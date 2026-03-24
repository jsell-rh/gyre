use axum::{
    extract::{Path, Query, State},
    Json,
};
use gyre_common::Id;
use gyre_domain::AgentCommit;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

use super::error::ApiError;

// ── Request / Response types ──────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ProvenanceQuery {
    pub commit: Option<String>,
    pub task_id: Option<String>,
    pub agent_id: Option<String>,
}

#[derive(Serialize)]
pub struct ProvenanceRecord {
    pub id: String,
    pub commit_sha: String,
    pub branch: String,
    pub agent_id: String,
    pub repository_id: String,
    pub timestamp: u64,
    pub task_id: Option<String>,
    pub spawned_by_user_id: Option<String>,
    pub parent_agent_id: Option<String>,
    pub model_context: Option<serde_json::Value>,
    pub attestation_level: Option<String>,
}

impl From<AgentCommit> for ProvenanceRecord {
    fn from(ac: AgentCommit) -> Self {
        let model_context = ac
            .model_context
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok());
        Self {
            id: ac.id.to_string(),
            commit_sha: ac.commit_sha,
            branch: ac.branch,
            agent_id: ac.agent_id.to_string(),
            repository_id: ac.repository_id.to_string(),
            timestamp: ac.timestamp,
            task_id: ac.task_id,
            spawned_by_user_id: ac.spawned_by_user_id,
            parent_agent_id: ac.parent_agent_id,
            model_context,
            attestation_level: ac.attestation_level,
        }
    }
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// GET /api/v1/repos/:id/provenance
///   ?commit={sha}       — full provenance for one commit
///   ?task_id={id}       — all commits for a task
///   ?agent_id={id}      — all commits by an agent in this repo
pub async fn get_provenance(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    Query(params): Query<ProvenanceQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Verify the repo exists.
    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    if let Some(sha) = params.commit {
        // Single commit lookup.
        let record = state
            .agent_commits
            .find_by_commit(&sha)
            .await?
            .filter(|ac| ac.repository_id.as_str() == repo_id)
            .map(ProvenanceRecord::from);
        return Ok(Json(serde_json::json!(record)));
    }

    if let Some(task_id) = params.task_id {
        // All commits for a task.
        let commits: Vec<ProvenanceRecord> = state
            .agent_commits
            .find_by_task(&task_id)
            .await?
            .into_iter()
            .filter(|ac| ac.repository_id.as_str() == repo_id)
            .map(ProvenanceRecord::from)
            .collect();

        return Ok(Json(serde_json::json!({
            "task_id": task_id,
            "commits": commits,
        })));
    }

    if let Some(agent_id) = params.agent_id {
        let commits: Vec<ProvenanceRecord> = state
            .agent_commits
            .find_by_agent(&Id::new(&agent_id))
            .await?
            .into_iter()
            .filter(|ac| ac.repository_id.as_str() == repo_id)
            .map(ProvenanceRecord::from)
            .collect();
        return Ok(Json(serde_json::json!(commits)));
    }

    // No filter: return all provenance records for the repo.
    let commits: Vec<ProvenanceRecord> = state
        .agent_commits
        .find_by_repo(&Id::new(&repo_id))
        .await?
        .into_iter()
        .map(ProvenanceRecord::from)
        .collect();
    Ok(Json(serde_json::json!(commits)))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use gyre_common::Id;
    use gyre_domain::AgentCommit;
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
        let body = serde_json::json!({"workspace_id": "ws-1", "name": "test-repo"});
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
        let json = body_json(resp).await;
        let repo_id = json["id"].as_str().unwrap().to_string();
        (app, repo_id)
    }

    #[tokio::test]
    async fn provenance_empty_repo() {
        let (app, repo_id) = create_repo(app()).await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/provenance"))
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
    async fn provenance_repo_not_found() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/no-such-repo/provenance")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn provenance_filter_by_commit() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let (app, repo_id) = create_repo(app).await;

        let commit = AgentCommit::new(
            Id::new("c1"),
            Id::new("agent-1"),
            Id::new(&repo_id),
            "abc123",
            "refs/heads/main",
            1000,
        )
        .with_provenance(Some("TASK-001".to_string()), None, None, None);
        state.agent_commits.record(&commit).await.unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/provenance?commit=abc123"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["commit_sha"], "abc123");
        assert_eq!(json["task_id"], "TASK-001");
    }

    #[tokio::test]
    async fn provenance_filter_by_task_id() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let (app, repo_id) = create_repo(app).await;

        for (id, sha) in [("c1", "sha1"), ("c2", "sha2")] {
            let commit = AgentCommit::new(
                Id::new(id),
                Id::new("agent-1"),
                Id::new(&repo_id),
                sha,
                "refs/heads/feat/x",
                1000,
            )
            .with_provenance(Some("TASK-007".to_string()), None, None, None);
            state.agent_commits.record(&commit).await.unwrap();
        }

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/repos/{repo_id}/provenance?task_id=TASK-007"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["task_id"], "TASK-007");
        assert_eq!(json["commits"].as_array().unwrap().len(), 2);
    }
}
