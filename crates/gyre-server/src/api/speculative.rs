//! M13.5 Speculative Merging API endpoints.
//!
//! - `GET /api/v1/repos/{id}/speculative` — list all speculative merge results
//! - `GET /api/v1/repos/{id}/speculative/{branch}` — result for a specific branch

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use serde::Serialize;
use std::sync::Arc;

use crate::speculative_merge::{ConflictType, SpeculativeResult, SpeculativeStatus};
use crate::AppState;

use super::error::ApiError;

#[derive(Serialize)]
pub struct SpeculativeResponse {
    pub repo_id: String,
    pub branch: String,
    pub status: String,
    pub conflicting_branch: Option<String>,
    pub conflicting_agent_id: Option<String>,
    pub conflicting_files: Vec<String>,
    /// `"order_independent"` or `"order_dependent"` when status is `"conflict"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflict_type: Option<String>,
    pub detected_at: u64,
}

impl From<SpeculativeResult> for SpeculativeResponse {
    fn from(r: SpeculativeResult) -> Self {
        let status = match r.status {
            SpeculativeStatus::Clean => "clean",
            SpeculativeStatus::Conflict => "conflict",
            SpeculativeStatus::Skipped => "skipped",
        }
        .to_string();
        let conflict_type = r.conflict_type.map(|ct| match ct {
            ConflictType::OrderIndependent => "order_independent".to_string(),
            ConflictType::OrderDependent => "order_dependent".to_string(),
        });
        Self {
            repo_id: r.repo_id,
            branch: r.branch,
            status,
            conflicting_branch: r.conflicting_branch,
            conflicting_agent_id: r.conflicting_agent_id,
            conflicting_files: r.conflicting_files,
            conflict_type,
            detected_at: r.detected_at,
        }
    }
}

/// GET /api/v1/repos/:id/speculative
///
/// Lists all speculative merge results for the repository.
pub async fn list_speculative(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
) -> Result<Json<Vec<SpeculativeResponse>>, ApiError> {
    // Verify the repo exists.
    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    let results = state.speculative_results.lock().await;
    let entries: Vec<SpeculativeResponse> = results
        .iter()
        .filter(|((rid, _), _)| rid == &repo_id)
        .map(|(_, v)| SpeculativeResponse::from(v.clone()))
        .collect();

    Ok(Json(entries))
}

/// GET /api/v1/repos/:id/speculative/:branch
///
/// Returns the speculative merge result for a specific branch.
/// The branch path segment may be URL-encoded (slashes become %2F).
pub async fn get_speculative_branch(
    State(state): State<Arc<AppState>>,
    Path((repo_id, branch)): Path<(String, String)>,
) -> Result<(StatusCode, Json<SpeculativeResponse>), ApiError> {
    // Verify the repo exists.
    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    let results = state.speculative_results.lock().await;
    let result = results
        .get(&(repo_id.clone(), branch.clone()))
        .cloned()
        .ok_or_else(|| {
            ApiError::NotFound(format!(
                "no speculative result for branch {branch} in repo {repo_id}"
            ))
        })?;

    Ok((StatusCode::OK, Json(SpeculativeResponse::from(result))))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use crate::speculative_merge::{ConflictType, SpeculativeResult, SpeculativeStatus};
    use axum::{body::Body, Router};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app_with_state() -> (Router, std::sync::Arc<crate::AppState>) {
        let state = test_state();
        let router = crate::api::api_router().with_state(state.clone());
        (router, state)
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn create_repo(app: &Router) -> String {
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
        json["id"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn list_speculative_empty() {
        let (app, _state) = app_with_state();
        let repo_id = create_repo(&app).await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/speculative"))
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
    async fn list_speculative_not_found() {
        let (app, _) = app_with_state();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/no-such/speculative")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn list_speculative_with_results() {
        let (app, state) = app_with_state();
        let repo_id = create_repo(&app).await;

        // Insert a speculative result directly.
        {
            let mut results = state.speculative_results.lock().await;
            results.insert(
                (repo_id.clone(), "feat/x".to_string()),
                SpeculativeResult {
                    repo_id: repo_id.clone(),
                    branch: "feat/x".to_string(),
                    status: SpeculativeStatus::Clean,
                    conflicting_files: vec![],
                    conflicting_branch: None,
                    conflicting_agent_id: None,
                    conflict_type: None,
                    detected_at: 1000,
                },
            );
        }

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/speculative"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json.as_array().unwrap().len(), 1);
        assert_eq!(json[0]["branch"], "feat/x");
        assert_eq!(json[0]["status"], "clean");
    }

    #[tokio::test]
    async fn get_speculative_branch_not_found() {
        let (app, _) = app_with_state();
        let repo_id = create_repo(&app).await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/speculative/feat-x"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_speculative_branch_conflict() {
        let (app, state) = app_with_state();
        let repo_id = create_repo(&app).await;

        {
            let mut results = state.speculative_results.lock().await;
            results.insert(
                (repo_id.clone(), "feat-y".to_string()),
                SpeculativeResult {
                    repo_id: repo_id.clone(),
                    branch: "feat-y".to_string(),
                    status: SpeculativeStatus::Conflict,
                    conflicting_files: vec!["src/lib.rs".to_string()],
                    conflicting_branch: Some("feat-z".to_string()),
                    conflicting_agent_id: Some("agent-1".to_string()),
                    conflict_type: Some(ConflictType::OrderIndependent),
                    detected_at: 2000,
                },
            );
        }

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/speculative/feat-y"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["status"], "conflict");
        assert_eq!(json["conflicting_branch"], "feat-z");
        assert_eq!(json["conflicting_agent_id"], "agent-1");
        assert_eq!(json["conflicting_files"][0], "src/lib.rs");
        assert_eq!(json["conflict_type"], "order_independent");
    }
}
