use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{BranchInfo, CommitInfo, DiffResult, Repository};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

use super::error::ApiError;
use super::{new_id, now_secs};

/// Strip credentials from a URL to prevent leaking secrets in API responses (H-5).
fn redact_url_credentials(url: String) -> String {
    // Match https://user:pass@host/path or https://token@host/path
    if let Some(at_pos) = url.find('@') {
        if let Some(scheme_end) = url.find("://") {
            let prefix = &url[..scheme_end + 3]; // "https://"
            let after_at = &url[at_pos + 1..]; // "host/path"
            return format!("{prefix}***@{after_at}");
        }
    }
    url
}

#[derive(Deserialize)]
pub struct CreateRepoRequest {
    pub project_id: String,
    pub name: String,
    /// Ignored — path is always computed server-side (C-4 security fix).
    #[serde(default)]
    pub _path: Option<String>,
    pub default_branch: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateMirrorRequest {
    pub project_id: String,
    pub name: String,
    pub url: String,
    pub interval_secs: Option<u64>,
}

#[derive(Deserialize)]
pub struct ListReposQuery {
    pub project_id: Option<String>,
}

#[derive(Serialize)]
pub struct RepoResponse {
    pub id: String,
    pub project_id: String,
    pub name: String,
    // path is intentionally omitted — it is a server-internal filesystem path.
    pub default_branch: String,
    pub created_at: u64,
    pub is_mirror: bool,
    pub mirror_url: Option<String>,
    pub mirror_interval_secs: Option<u64>,
    pub last_mirror_sync: Option<u64>,
}

impl From<Repository> for RepoResponse {
    fn from(r: Repository) -> Self {
        Self {
            id: r.id.to_string(),
            project_id: r.project_id.to_string(),
            name: r.name,
            default_branch: r.default_branch,
            created_at: r.created_at,
            is_mirror: r.is_mirror,
            mirror_url: r.mirror_url.map(redact_url_credentials),
            mirror_interval_secs: r.mirror_interval_secs,
            last_mirror_sync: r.last_mirror_sync,
        }
    }
}

#[derive(Deserialize)]
pub struct CommitLogQuery {
    pub branch: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Deserialize)]
pub struct DiffQuery {
    pub from: String,
    pub to: String,
}

pub async fn create_repo(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateRepoRequest>,
) -> Result<(StatusCode, Json<RepoResponse>), ApiError> {
    // Reject path traversal in project_id and name.
    for field in [req.project_id.as_str(), req.name.as_str()] {
        if field.contains("..") || field.contains('/') {
            return Err(ApiError::InvalidInput(
                "project_id and name must not contain '..' or '/'".to_string(),
            ));
        }
    }
    let repos_root = std::env::var("GYRE_REPOS_PATH").unwrap_or_else(|_| "./repos".to_string());
    // C-4 fix: always compute path server-side, never from user input.
    let repo_path = format!("{}/{}/{}.git", repos_root, req.project_id, req.name);

    let now = now_secs();
    let mut repo = Repository::new(
        new_id(),
        Id::new(req.project_id),
        req.name,
        repo_path.clone(),
        now,
    );
    if let Some(branch) = req.default_branch {
        repo.default_branch = branch;
    }
    state.repos.create(&repo).await?;

    // Initialize the bare git repository; log on failure but don't block the response.
    if let Err(e) = state.git_ops.init_bare(&repo_path).await {
        tracing::warn!("init_bare failed for {repo_path}: {e}");
    }

    Ok((StatusCode::CREATED, Json(RepoResponse::from(repo))))
}

pub async fn list_repos(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListReposQuery>,
) -> Result<Json<Vec<RepoResponse>>, ApiError> {
    let repos = if let Some(project_id) = params.project_id {
        state.repos.list_by_project(&Id::new(project_id)).await?
    } else {
        state.repos.list().await?
    };
    Ok(Json(repos.into_iter().map(RepoResponse::from).collect()))
}

pub async fn get_repo(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<RepoResponse>, ApiError> {
    let repo = state
        .repos
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {id} not found")))?;
    Ok(Json(RepoResponse::from(repo)))
}

pub async fn list_branches(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<BranchInfo>>, ApiError> {
    let repo = state
        .repos
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {id} not found")))?;
    let branches = state.git_ops.list_branches(&repo.path).await?;
    Ok(Json(branches))
}

pub async fn commit_log(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(params): Query<CommitLogQuery>,
) -> Result<Json<Vec<CommitInfo>>, ApiError> {
    let repo = state
        .repos
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {id} not found")))?;
    let branch = params.branch.unwrap_or_else(|| repo.default_branch.clone());
    let limit = params.limit.unwrap_or(50);
    let commits = state.git_ops.commit_log(&repo.path, &branch, limit).await?;
    Ok(Json(commits))
}

pub async fn diff(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(params): Query<DiffQuery>,
) -> Result<Json<DiffResult>, ApiError> {
    let repo = state
        .repos
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {id} not found")))?;
    let result = state
        .git_ops
        .diff(&repo.path, &params.from, &params.to)
        .await?;
    Ok(Json(result))
}

pub async fn create_mirror_repo(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateMirrorRequest>,
) -> Result<(StatusCode, Json<RepoResponse>), ApiError> {
    // Reject path traversal in project_id and name.
    for field in [req.project_id.as_str(), req.name.as_str()] {
        if field.contains("..") || field.contains('/') {
            return Err(ApiError::InvalidInput(
                "project_id and name must not contain '..' or '/'.".to_string(),
            ));
        }
    }
    // Only allow https:// URLs to prevent SSRF.
    if !req.url.starts_with("https://") {
        return Err(ApiError::InvalidInput(
            "mirror URL must use https:// scheme".to_string(),
        ));
    }
    let repos_root = std::env::var("GYRE_REPOS_PATH").unwrap_or_else(|_| "./repos".to_string());
    let repo_path = format!("{}/{}/{}.git", repos_root, req.project_id, req.name);

    let now = now_secs();
    let repo = Repository {
        id: new_id(),
        project_id: Id::new(req.project_id),
        name: req.name,
        path: repo_path.clone(),
        default_branch: "main".to_string(),
        created_at: now,
        is_mirror: true,
        mirror_url: Some(req.url.clone()),
        mirror_interval_secs: req.interval_secs,
        last_mirror_sync: None,
        workspace_id: None,
    };
    state.repos.create(&repo).await?;

    // Clone the remote as a bare mirror; log on failure but don't block the response.
    if let Err(e) = state.git_ops.clone_mirror(&req.url, &repo_path).await {
        tracing::warn!("clone_mirror failed for {repo_path}: {e}");
    }

    Ok((StatusCode::CREATED, Json(RepoResponse::from(repo))))
}

pub async fn sync_mirror(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<RepoResponse>, ApiError> {
    let mut repo = state
        .repos
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {id} not found")))?;

    if !repo.is_mirror {
        return Err(ApiError::InvalidInput("repo is not a mirror".to_string()));
    }

    state.git_ops.fetch_mirror(&repo.path).await?;
    repo.last_mirror_sync = Some(now_secs());
    state.repos.update(&repo).await?;

    Ok(Json(RepoResponse::from(repo)))
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

    #[tokio::test]
    async fn create_and_get_repo() {
        let app = app();
        let body = serde_json::json!({
            "project_id": "proj-1",
            "name": "gyre"
        });
        let create_resp = app
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
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let created = body_json(create_resp).await;
        let id = created["id"].as_str().unwrap().to_string();
        assert_eq!(created["default_branch"], "main");

        let get_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn create_repo_auto_generates_path() {
        let body = serde_json::json!({
            "project_id": "proj-99",
            "name": "my-svc"
        });
        let resp = app()
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
        // M-3: path is no longer exposed in API response
        assert!(json["path"].is_null(), "path should not be in response");
        assert!(json["name"].as_str().unwrap() == "my-svc");
    }

    #[tokio::test]
    async fn create_repo_ignores_user_supplied_path() {
        // C-4 security fix: user-supplied path is ignored; server computes path.
        let body = serde_json::json!({
            "project_id": "proj-1",
            "name": "gyre",
            "path": "/custom/path/gyre.git"
        });
        let resp = app()
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
        // M-3: path is no longer exposed in API response
        assert!(json["path"].is_null(), "path should not be in response");
        assert_eq!(json["name"], "gyre");
    }

    #[tokio::test]
    async fn list_repos_by_project() {
        let app = app();
        let body = serde_json::json!({
            "project_id": "proj-42",
            "name": "my-repo"
        });
        app.clone()
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

        let list_resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos?project_id=proj-42")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(list_resp.status(), StatusCode::OK);
        let json = body_json(list_resp).await;
        assert_eq!(json.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn get_repo_not_found() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/no-such-repo")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn list_branches_returns_empty_for_noop() {
        let app = app();
        // Create a repo first
        let body = serde_json::json!({"project_id": "proj-1", "name": "test"});
        let create_resp = app
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
        let created = body_json(create_resp).await;
        let id = created["id"].as_str().unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{id}/branches"))
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
    async fn list_branches_not_found() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/no-such/branches")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn commit_log_returns_empty_for_noop() {
        let app = app();
        let body = serde_json::json!({"project_id": "proj-1", "name": "test2"});
        let create_resp = app
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
        let created = body_json(create_resp).await;
        let id = created["id"].as_str().unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{id}/commits"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json.as_array().unwrap().is_empty());
    }
}
