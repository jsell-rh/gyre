use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{BranchInfo, CommitInfo, DiffResult, RepoStatus, Repository};
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
    pub workspace_id: String,
    pub name: String,
    pub description: Option<String>,
    /// Ignored — path is always computed server-side (C-4 security fix).
    #[serde(default)]
    pub _path: Option<String>,
    pub default_branch: Option<String>,
    #[serde(default)]
    pub initialize: Option<bool>,
}

#[derive(Deserialize)]
pub struct UpdateRepoRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub default_branch: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateMirrorRequest {
    pub workspace_id: String,
    pub name: String,
    pub url: String,
    pub interval_secs: Option<u64>,
}

#[derive(Deserialize)]
pub struct ListReposQuery {
    pub workspace_id: Option<String>,
}

#[derive(Serialize)]
pub struct RepoResponse {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub description: Option<String>,
    // path is intentionally omitted — it is a server-internal filesystem path.
    pub default_branch: String,
    pub status: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub is_mirror: bool,
    pub mirror_url: Option<String>,
    pub mirror_interval_secs: Option<u64>,
    pub last_mirror_sync: Option<u64>,
}

impl From<Repository> for RepoResponse {
    fn from(r: Repository) -> Self {
        Self {
            id: r.id.to_string(),
            workspace_id: r.workspace_id.to_string(),
            name: r.name,
            description: r.description,
            default_branch: r.default_branch,
            status: r.status.to_string(),
            created_at: r.created_at,
            updated_at: r.updated_at,
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
    // Reject path traversal in workspace_id and name.
    for field in [req.workspace_id.as_str(), req.name.as_str()] {
        if field.contains("..") || field.contains('/') {
            return Err(ApiError::InvalidInput(
                "workspace_id and name must not contain '..' or '/'".to_string(),
            ));
        }
    }
    // C-4 fix: always compute path server-side, never from user input.
    let repo_path = format!("{}/{}/{}.git", state.repos_root, req.workspace_id, req.name);

    let now = now_secs();
    let mut repo = Repository::new(
        new_id(),
        Id::new(req.workspace_id),
        req.name,
        repo_path.clone(),
        now,
    );
    if let Some(branch) = req.default_branch {
        repo.default_branch = branch;
    }
    if let Some(desc) = req.description {
        repo.description = Some(desc);
    }
    state.repos.create(&repo).await?;

    // Initialize the bare git repository; log on failure but don't block the response.
    if let Err(e) = state.git_ops.init_bare(&repo_path).await {
        tracing::warn!("init_bare failed for {repo_path}: {e}");
    } else {
        // Create an initial empty commit so HEAD is valid. Without this, `git worktree add -b`
        // fails with "fatal: invalid reference: HEAD" on freshly-created repos.
        let branch = repo.default_branch.clone();
        if let Err(e) = state
            .git_ops
            .create_initial_commit(&repo_path, &branch)
            .await
        {
            tracing::warn!("create_initial_commit failed for {repo_path}: {e}");
        }
    }

    Ok((StatusCode::CREATED, Json(RepoResponse::from(repo))))
}

pub async fn list_repos(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListReposQuery>,
) -> Result<Json<Vec<RepoResponse>>, ApiError> {
    let repos = if let Some(ws_id) = params.workspace_id {
        state.repos.list_by_workspace(&Id::new(ws_id)).await?
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

pub async fn update_repo(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateRepoRequest>,
) -> Result<Json<RepoResponse>, ApiError> {
    let mut repo = state
        .repos
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {id} not found")))?;

    if let Some(name) = req.name {
        if name.contains("..") || name.contains('/') {
            return Err(ApiError::InvalidInput(
                "name must not contain '..' or '/'".to_string(),
            ));
        }
        repo.name = name;
    }
    if let Some(desc) = req.description {
        repo.description = Some(desc);
    }
    if let Some(branch) = req.default_branch {
        repo.default_branch = branch;
    }
    repo.updated_at = now_secs();

    state.repos.update(&repo).await?;
    Ok(Json(RepoResponse::from(repo)))
}

pub async fn archive_repo(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<RepoResponse>, ApiError> {
    let mut repo = state
        .repos
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {id} not found")))?;

    if repo.is_archived() {
        return Err(ApiError::InvalidInput(
            "repo is already archived".to_string(),
        ));
    }

    repo.archive();
    repo.updated_at = now_secs();
    state.repos.update(&repo).await?;
    Ok(Json(RepoResponse::from(repo)))
}

pub async fn unarchive_repo(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<RepoResponse>, ApiError> {
    let mut repo = state
        .repos
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {id} not found")))?;

    if !repo.is_archived() {
        return Err(ApiError::InvalidInput("repo is not archived".to_string()));
    }

    repo.unarchive();
    repo.updated_at = now_secs();
    state.repos.update(&repo).await?;
    Ok(Json(RepoResponse::from(repo)))
}

pub async fn delete_repo(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let repo = state
        .repos
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {id} not found")))?;

    if !repo.is_archived() {
        return Err(ApiError::InvalidInput(
            "repo must be archived before deletion".to_string(),
        ));
    }

    state.repos.delete(&repo.id).await?;
    Ok(StatusCode::NO_CONTENT)
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
    let branch = params.branch.unwrap_or(repo.default_branch);
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
    // Reject path traversal in workspace_id and name.
    for field in [req.workspace_id.as_str(), req.name.as_str()] {
        if field.contains("..") || field.contains('/') {
            return Err(ApiError::InvalidInput(
                "workspace_id and name must not contain '..' or '/'.".to_string(),
            ));
        }
    }
    // Only allow https:// URLs to prevent SSRF.
    if !req.url.starts_with("https://") {
        return Err(ApiError::InvalidInput(
            "mirror URL must use https:// scheme".to_string(),
        ));
    }
    let repo_path = format!("{}/{}/{}.git", state.repos_root, req.workspace_id, req.name);

    let now = now_secs();
    let url = req.url;
    let repo = Repository {
        id: new_id(),
        workspace_id: Id::new(req.workspace_id),
        name: req.name,
        path: repo_path.clone(),
        default_branch: "main".to_string(),
        created_at: now,
        is_mirror: true,
        mirror_url: Some(url.clone()),
        mirror_interval_secs: req.interval_secs,
        last_mirror_sync: None,
        description: None,
        status: RepoStatus::Active,
        updated_at: now,
    };
    state.repos.create(&repo).await?;

    // Clone the remote as a bare mirror; log on failure but don't block the response.
    if let Err(e) = state.git_ops.clone_mirror(&url, &repo_path).await {
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
    let now = now_secs();
    repo.last_mirror_sync = Some(now);
    repo.updated_at = now;
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
            "workspace_id": "ws-1",
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
        assert_eq!(created["status"], "Active");

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
            "workspace_id": "ws-99",
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
            "workspace_id": "ws-1",
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
    async fn list_repos_by_workspace() {
        let app = app();
        let body = serde_json::json!({
            "workspace_id": "ws-42",
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
                    .uri("/api/v1/repos?workspace_id=ws-42")
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
        let body = serde_json::json!({"workspace_id": "ws-1", "name": "test"});
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
        let body = serde_json::json!({"workspace_id": "ws-1", "name": "test2"});
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

    #[tokio::test]
    async fn update_repo_settings() {
        let app = app();
        let body = serde_json::json!({
            "workspace_id": "ws-1",
            "name": "my-repo",
            "description": "original"
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

        let update_body = serde_json::json!({
            "description": "updated description",
            "default_branch": "develop"
        });
        let update_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/repos/{id}"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&update_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(update_resp.status(), StatusCode::OK);
        let updated = body_json(update_resp).await;
        assert_eq!(updated["description"], "updated description");
        assert_eq!(updated["default_branch"], "develop");
    }

    #[tokio::test]
    async fn archive_and_unarchive_repo() {
        let app = app();
        let body = serde_json::json!({
            "workspace_id": "ws-1",
            "name": "archive-me"
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
        let created = body_json(create_resp).await;
        let id = created["id"].as_str().unwrap().to_string();

        // Archive
        let archive_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{id}/archive"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(archive_resp.status(), StatusCode::OK);
        let archived = body_json(archive_resp).await;
        assert_eq!(archived["status"], "Archived");

        // Unarchive
        let unarchive_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{id}/unarchive"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unarchive_resp.status(), StatusCode::OK);
        let unarchived = body_json(unarchive_resp).await;
        assert_eq!(unarchived["status"], "Active");
    }

    #[tokio::test]
    async fn delete_repo_requires_archived() {
        let app = app();
        let body = serde_json::json!({
            "workspace_id": "ws-1",
            "name": "delete-me"
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
        let created = body_json(create_resp).await;
        let id = created["id"].as_str().unwrap().to_string();

        // Delete without archiving should fail
        let del_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/repos/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(del_resp.status(), StatusCode::BAD_REQUEST);

        // Archive first, then delete succeeds
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{id}/archive"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let del_resp2 = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/repos/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(del_resp2.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn create_mirror_repo_returns_201() {
        let body = serde_json::json!({
            "workspace_id": "ws-mirror-test",
            "name": "my-mirror",
            "url": "https://github.com/org/repo.git",
            "interval_secs": 300
        });
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos/mirror")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["name"], "my-mirror");
        assert_eq!(json["is_mirror"], true);
    }
}
