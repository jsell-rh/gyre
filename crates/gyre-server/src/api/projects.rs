use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::Project;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

use super::error::ApiError;
use super::{new_id, now_secs};

#[derive(Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateProjectRequest {
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Serialize)]
pub struct ProjectResponse {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<Project> for ProjectResponse {
    fn from(p: Project) -> Self {
        Self {
            id: p.id.to_string(),
            name: p.name,
            description: p.description,
            created_at: p.created_at,
            updated_at: p.updated_at,
        }
    }
}

pub async fn create_project(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateProjectRequest>,
) -> Result<(StatusCode, Json<ProjectResponse>), ApiError> {
    let now = now_secs();
    let mut project = Project::new(new_id(), req.name, now);
    project.description = req.description;
    state.projects.create(&project).await?;
    Ok((StatusCode::CREATED, Json(ProjectResponse::from(project))))
}

#[derive(serde::Deserialize)]
pub struct ListProjectsQuery {
    pub workspace_id: Option<String>,
}

pub async fn list_projects(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListProjectsQuery>,
) -> Result<Json<Vec<ProjectResponse>>, ApiError> {
    let projects = if let Some(ws_id) = params.workspace_id {
        state.projects.list_by_workspace(&Id::new(ws_id)).await?
    } else {
        state.projects.list().await?
    };
    Ok(Json(
        projects.into_iter().map(ProjectResponse::from).collect(),
    ))
}

pub async fn get_project(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<ProjectResponse>, ApiError> {
    let project = state
        .projects
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("project {id} not found")))?;
    Ok(Json(ProjectResponse::from(project)))
}

pub async fn update_project(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateProjectRequest>,
) -> Result<Json<ProjectResponse>, ApiError> {
    let mut project = state
        .projects
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("project {id} not found")))?;
    if let Some(name) = req.name {
        project.name = name;
    }
    if let Some(desc) = req.description {
        project.description = Some(desc);
    }
    project.updated_at = now_secs();
    state.projects.update(&project).await?;
    Ok(Json(ProjectResponse::from(project)))
}

pub async fn delete_project(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state
        .projects
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("project {id} not found")))?;
    state.projects.delete(&Id::new(id)).await?;
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

    #[tokio::test]
    async fn list_projects_empty() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/projects")
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
    async fn create_and_get_project() {
        let app = app();
        let body = serde_json::json!({ "name": "Gyre", "description": "Autonomous dev platform" });
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/projects")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let created = body_json(create_resp).await;
        let id = created["id"].as_str().unwrap().to_string();

        let get_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/projects/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_resp.status(), StatusCode::OK);
        let got = body_json(get_resp).await;
        assert_eq!(got["name"], "Gyre");
        assert_eq!(got["description"], "Autonomous dev platform");
    }

    #[tokio::test]
    async fn get_project_not_found() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/projects/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn update_project() {
        let app = app();
        let create_body = serde_json::json!({ "name": "Old Name" });
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/projects")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&create_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let created = body_json(create_resp).await;
        let id = created["id"].as_str().unwrap().to_string();

        let update_body = serde_json::json!({ "name": "New Name" });
        let update_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/projects/{id}"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&update_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(update_resp.status(), StatusCode::OK);
        let updated = body_json(update_resp).await;
        assert_eq!(updated["name"], "New Name");
    }

    #[tokio::test]
    async fn delete_project() {
        let app = app();
        let create_body = serde_json::json!({ "name": "ToDelete" });
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/projects")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&create_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let created = body_json(create_resp).await;
        let id = created["id"].as_str().unwrap().to_string();

        let del_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/projects/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(del_resp.status(), StatusCode::NO_CONTENT);

        let get_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/projects/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_resp.status(), StatusCode::NOT_FOUND);
    }
}
