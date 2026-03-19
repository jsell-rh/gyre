use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::Repository;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

use super::error::ApiError;
use super::{new_id, now_secs};

#[derive(Deserialize)]
pub struct CreateRepoRequest {
    pub project_id: String,
    pub name: String,
    pub path: String,
    pub default_branch: Option<String>,
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
    pub path: String,
    pub default_branch: String,
    pub created_at: u64,
}

impl From<Repository> for RepoResponse {
    fn from(r: Repository) -> Self {
        Self {
            id: r.id.to_string(),
            project_id: r.project_id.to_string(),
            name: r.name,
            path: r.path,
            default_branch: r.default_branch,
            created_at: r.created_at,
        }
    }
}

pub async fn create_repo(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateRepoRequest>,
) -> Result<(StatusCode, Json<RepoResponse>), ApiError> {
    let now = now_secs();
    let mut repo = Repository::new(new_id(), Id::new(req.project_id), req.name, req.path, now);
    if let Some(branch) = req.default_branch {
        repo.default_branch = branch;
    }
    state.repos.create(&repo).await?;
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
            "name": "gyre",
            "path": "/code/gyre"
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
    async fn list_repos_by_project() {
        let app = app();
        let body = serde_json::json!({
            "project_id": "proj-42",
            "name": "my-repo",
            "path": "/repos/my-repo"
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
}
