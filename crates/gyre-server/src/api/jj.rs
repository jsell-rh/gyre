use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_ports::JjChange;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::commit_signatures::{self, CommitSignature};
use crate::AppState;

use super::error::ApiError;

#[derive(Serialize)]
pub struct JjChangeResponse {
    pub change_id: String,
    pub commit_id: String,
    pub description: String,
    pub author: String,
    pub timestamp: u64,
    pub bookmarks: Vec<String>,
}

impl From<JjChange> for JjChangeResponse {
    fn from(c: JjChange) -> Self {
        Self {
            change_id: c.change_id,
            commit_id: c.commit_id,
            description: c.description,
            author: c.author,
            timestamp: c.timestamp,
            bookmarks: c.bookmarks,
        }
    }
}

#[derive(Deserialize)]
pub struct NewChangeRequest {
    pub description: String,
}

#[derive(Deserialize)]
pub struct BookmarkRequest {
    pub name: String,
    pub change_id: String,
}

#[derive(Deserialize)]
pub struct LogQuery {
    pub limit: Option<usize>,
}

async fn repo_path(state: &AppState, repo_id: &str) -> Result<String, ApiError> {
    let repo = state
        .repos
        .find_by_id(&Id::new(repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;
    Ok(repo.path)
}

/// POST /api/v1/repos/:id/jj/init
pub async fn jj_init(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let path = repo_path(&state, &repo_id).await?;
    state
        .jj_ops
        .jj_init(&path)
        .await
        .map_err(ApiError::Internal)?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/repos/:id/jj/log
pub async fn jj_log(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    Query(q): Query<LogQuery>,
) -> Result<Json<Vec<JjChangeResponse>>, ApiError> {
    let path = repo_path(&state, &repo_id).await?;
    let limit = q.limit.unwrap_or(20);
    let changes = state
        .jj_ops
        .jj_log(&path, limit)
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(changes.into_iter().map(Into::into).collect()))
}

/// POST /api/v1/repos/:id/jj/new
pub async fn jj_new(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    Json(req): Json<NewChangeRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let path = repo_path(&state, &repo_id).await?;
    let change_id = state
        .jj_ops
        .jj_new(&path, &req.description)
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(serde_json::json!({ "change_id": change_id })))
}

/// POST /api/v1/repos/:id/jj/squash
///
/// Squashes the working copy into its parent change and signs the resulting
/// commit SHA with the forge's Ed25519 key (M13.8 Sigstore local signing).
pub async fn jj_squash(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
) -> Result<Json<CommitSignature>, ApiError> {
    let path = repo_path(&state, &repo_id).await?;
    let commit_sha = state
        .jj_ops
        .jj_squash(&path)
        .await
        .map_err(ApiError::Internal)?;

    // Sign the resulting commit SHA with the forge's Ed25519 key.
    let record = commit_signatures::sign_commit(
        &commit_sha,
        "forge",
        &state.agent_signing_key,
        state.sigstore_mode.clone(),
    );

    state
        .commit_signatures
        .lock()
        .await
        .insert(commit_sha.clone(), record.clone());

    tracing::info!(
        commit_sha = %commit_sha,
        signing_key_id = %record.signing_key_id,
        mode = ?record.sigstore_mode,
        "jj squash: commit signed (M13.8)"
    );

    Ok(Json(record))
}

/// GET /api/v1/repos/:id/commits/:sha/signature
///
/// Return the Sigstore commit signature for a specific commit SHA, if one exists.
pub async fn get_commit_signature(
    State(state): State<Arc<AppState>>,
    Path((repo_id, sha)): Path<(String, String)>,
) -> Result<Json<CommitSignature>, ApiError> {
    // Verify the repo exists.
    let _ = repo_path(&state, &repo_id).await?;

    let store = state.commit_signatures.lock().await;
    store
        .get(&sha)
        .cloned()
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("no signature found for commit {sha}")))
}

/// POST /api/v1/repos/:id/jj/undo
pub async fn jj_undo(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let path = repo_path(&state, &repo_id).await?;
    state
        .jj_ops
        .jj_undo(&path)
        .await
        .map_err(ApiError::Internal)?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/repos/:id/jj/bookmark
pub async fn jj_bookmark(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    Json(req): Json<BookmarkRequest>,
) -> Result<StatusCode, ApiError> {
    let path = repo_path(&state, &repo_id).await?;
    state
        .jj_ops
        .jj_bookmark_create(&path, &req.name, &req.change_id)
        .await
        .map_err(ApiError::Internal)?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app() -> Router {
        crate::build_router(test_state())
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn create_project_and_repo(app: Router) -> (Router, String) {
        let proj = serde_json::json!({ "name": "test-project", "description": null });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/projects")
                    .header("Authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&proj).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let proj_json = body_json(resp).await;
        let project_id = proj_json["id"].as_str().unwrap().to_string();

        let repo = serde_json::json!({ "name": "test-repo", "project_id": project_id, "description": null });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos")
                    .header("Authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&repo).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let repo_json = body_json(resp).await;
        let repo_id = repo_json["id"].as_str().unwrap().to_string();

        (app, repo_id)
    }

    /// jj log returns empty list (NoopJjOps returns []).
    #[tokio::test]
    async fn jj_log_returns_empty_for_noop() {
        let app = app();
        let (app, repo_id) = create_project_and_repo(app).await;

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/jj/log"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json.as_array().unwrap().is_empty());
    }

    /// jj init returns 204 (NoopJjOps succeeds).
    #[tokio::test]
    async fn jj_init_returns_no_content() {
        let app = app();
        let (app, repo_id) = create_project_and_repo(app).await;

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/jj/init"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    /// jj new returns a change_id.
    #[tokio::test]
    async fn jj_new_returns_change_id() {
        let app = app();
        let (app, repo_id) = create_project_and_repo(app).await;

        let body = serde_json::json!({ "description": "test change" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/jj/new"))
                    .header("Authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json["change_id"].as_str().is_some());
    }

    /// jj squash returns 200 with a commit signature (M13.8 Sigstore).
    #[tokio::test]
    async fn jj_squash_returns_commit_signature() {
        let app = app();
        let (app, repo_id) = create_project_and_repo(app).await;

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/jj/squash"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json["commit_sha"].as_str().is_some(), "missing commit_sha");
        assert!(json["signature"].as_str().is_some(), "missing signature");
        assert!(
            json["signing_key_id"].as_str().is_some(),
            "missing signing_key_id"
        );
        assert_eq!(json["algorithm"].as_str().unwrap(), "EdDSA");
        assert_eq!(json["sigstore_mode"].as_str().unwrap(), "local");
    }

    /// jj squash signature can be retrieved via GET /commits/:sha/signature (M13.8).
    #[tokio::test]
    async fn commit_signature_retrievable_after_squash() {
        use tower::ServiceExt as _;
        let state = crate::mem::test_state();
        let app = crate::build_router(state);
        let (app, repo_id) = create_project_and_repo(app).await;

        // First squash to produce a signature.
        let squash_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/jj/squash"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(squash_resp.status(), StatusCode::OK);
        let squash_json = body_json(squash_resp).await;
        let sha = squash_json["commit_sha"].as_str().unwrap().to_string();

        // Then retrieve the signature by SHA.
        let sig_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/commits/{sha}/signature"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(sig_resp.status(), StatusCode::OK);
        let sig_json = body_json(sig_resp).await;
        assert_eq!(sig_json["commit_sha"].as_str().unwrap(), sha);
        assert!(sig_json["signature"].as_str().is_some());
    }

    /// GET /commits/:sha/signature returns 404 for an unknown SHA (M13.8).
    #[tokio::test]
    async fn commit_signature_unknown_sha_returns_404() {
        let app = app();
        let (app, repo_id) = create_project_and_repo(app).await;

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/repos/{repo_id}/commits/deadbeef/signature"
                    ))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    /// jj undo returns 204.
    #[tokio::test]
    async fn jj_undo_returns_no_content() {
        let app = app();
        let (app, repo_id) = create_project_and_repo(app).await;

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/jj/undo"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    /// jj bookmark returns 204.
    #[tokio::test]
    async fn jj_bookmark_returns_no_content() {
        let app = app();
        let (app, repo_id) = create_project_and_repo(app).await;

        let body = serde_json::json!({ "name": "my-feature", "change_id": "abc123" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/jj/bookmark"))
                    .header("Authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    /// jj init on unknown repo returns 404.
    #[tokio::test]
    async fn jj_init_unknown_repo_404() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos/nonexistent/jj/init")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    /// jj log with limit query parameter.
    #[tokio::test]
    async fn jj_log_with_limit() {
        let app = app();
        let (app, repo_id) = create_project_and_repo(app).await;

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/jj/log?limit=5"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
