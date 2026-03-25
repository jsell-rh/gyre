use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{BudgetConfig, UserRole, Workspace};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::auth::AuthenticatedAgent;
use crate::AppState;

use super::error::ApiError;
use super::{new_id, now_secs};

#[derive(Deserialize)]
pub struct CreateWorkspaceRequest {
    /// Optional override for system callers only. Non-system callers have
    /// tenant_id derived from their auth context and this field is ignored.
    pub tenant_id: Option<String>,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub budget: Option<BudgetConfig>,
    pub max_repos: Option<u32>,
    pub max_agents_per_repo: Option<u32>,
}

#[derive(Deserialize)]
pub struct UpdateWorkspaceRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub budget: Option<BudgetConfig>,
    pub max_repos: Option<u32>,
    pub max_agents_per_repo: Option<u32>,
}

#[derive(Deserialize)]
pub struct ListWorkspacesQuery {
    pub tenant_id: Option<String>,
}

#[derive(Serialize)]
pub struct WorkspaceResponse {
    pub id: String,
    pub tenant_id: String,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub budget: Option<BudgetConfig>,
    pub max_repos: Option<u32>,
    pub max_agents_per_repo: Option<u32>,
    pub created_at: u64,
}

impl From<Workspace> for WorkspaceResponse {
    fn from(ws: Workspace) -> Self {
        Self {
            id: ws.id.to_string(),
            tenant_id: ws.tenant_id.to_string(),
            name: ws.name,
            slug: ws.slug,
            description: ws.description,
            budget: ws.budget,
            max_repos: ws.max_repos,
            max_agents_per_repo: ws.max_agents_per_repo,
            created_at: ws.created_at,
        }
    }
}

#[derive(Deserialize)]
pub struct AddRepoRequest {
    pub repo_id: String,
}

#[derive(Serialize)]
pub struct WorkspaceRepoEntry {
    pub repo_id: String,
}

pub async fn create_workspace(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Json(req): Json<CreateWorkspaceRequest>,
) -> Result<(StatusCode, Json<WorkspaceResponse>), ApiError> {
    // Derive tenant_id from auth context. System callers may supply an
    // override in the request body; all other callers are bound to their
    // authenticated tenant scope.
    let tenant_id = if auth.agent_id == "system" {
        req.tenant_id.unwrap_or(auth.tenant_id)
    } else {
        auth.tenant_id
    };
    let now = now_secs();
    let mut ws = Workspace::new(new_id(), Id::new(&tenant_id), req.name, req.slug, now);
    ws.description = req.description;
    ws.budget = req.budget;
    ws.max_repos = req.max_repos;
    ws.max_agents_per_repo = req.max_agents_per_repo;
    state.workspaces.create(&ws).await?;
    Ok((StatusCode::CREATED, Json(WorkspaceResponse::from(ws))))
}

pub async fn list_workspaces(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Query(q): Query<ListWorkspacesQuery>,
) -> Result<Json<Vec<WorkspaceResponse>>, ApiError> {
    // Admin callers may filter by an explicit tenant_id query param.
    // All other callers are restricted to their own tenant scope.
    let tenant_id = if auth.roles.contains(&UserRole::Admin) {
        q.tenant_id.unwrap_or(auth.tenant_id)
    } else {
        auth.tenant_id
    };
    let workspaces = state
        .workspaces
        .list_by_tenant(&Id::new(&tenant_id))
        .await?;
    Ok(Json(
        workspaces
            .into_iter()
            .map(WorkspaceResponse::from)
            .collect(),
    ))
}

pub async fn get_workspace(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<WorkspaceResponse>, ApiError> {
    let ws = state
        .workspaces
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {id} not found")))?;
    Ok(Json(WorkspaceResponse::from(ws)))
}

pub async fn update_workspace(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateWorkspaceRequest>,
) -> Result<Json<WorkspaceResponse>, ApiError> {
    let mut ws = state
        .workspaces
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {id} not found")))?;
    if let Some(name) = req.name {
        ws.name = name;
    }
    if let Some(desc) = req.description {
        ws.description = Some(desc);
    }
    if let Some(budget) = req.budget {
        ws.budget = Some(budget);
    }
    if let Some(max_repos) = req.max_repos {
        ws.max_repos = Some(max_repos);
    }
    if let Some(max_agents) = req.max_agents_per_repo {
        ws.max_agents_per_repo = Some(max_agents);
    }
    state.workspaces.update(&ws).await?;
    Ok(Json(WorkspaceResponse::from(ws)))
}

pub async fn delete_workspace(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state
        .workspaces
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {id} not found")))?;
    state.workspaces.delete(&Id::new(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn add_repo_to_workspace(
    State(state): State<Arc<AppState>>,
    Path(ws_id): Path<String>,
    Json(req): Json<AddRepoRequest>,
) -> Result<StatusCode, ApiError> {
    state
        .workspaces
        .find_by_id(&Id::new(&ws_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {ws_id} not found")))?;
    let mut repo = state
        .repos
        .find_by_id(&Id::new(&req.repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {} not found", req.repo_id)))?;
    repo.workspace_id = Id::new(&ws_id);
    state.repos.update(&repo).await?;
    let mut repo_ids: Vec<String> = state
        .kv_store
        .kv_get("workspace_repos", &ws_id)
        .await
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    repo_ids.push(req.repo_id);
    let json = serde_json::to_string(&repo_ids).map_err(|e| ApiError::Internal(e.into()))?;
    state
        .kv_store
        .kv_set("workspace_repos", &ws_id, json)
        .await
        .map_err(ApiError::Internal)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_workspace_repos(
    State(state): State<Arc<AppState>>,
    Path(ws_id): Path<String>,
) -> Result<Json<Vec<WorkspaceRepoEntry>>, ApiError> {
    state
        .workspaces
        .find_by_id(&Id::new(&ws_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {ws_id} not found")))?;
    let repo_ids: Vec<String> = state
        .kv_store
        .kv_get("workspace_repos", &ws_id)
        .await
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    Ok(Json(
        repo_ids
            .into_iter()
            .map(|repo_id| WorkspaceRepoEntry { repo_id })
            .collect(),
    ))
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
    async fn create_and_list_workspaces() {
        let app = app();
        let body = serde_json::json!({
            "tenant_id": "t1",
            "name": "Platform Team",
            "slug": "platform-team"
        });
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let created = body_json(create_resp).await;
        assert_eq!(created["name"], "Platform Team");
        assert_eq!(created["slug"], "platform-team");
        assert_eq!(created["tenant_id"], "t1");

        let list_resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces?tenant_id=t1")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(list_resp.status(), StatusCode::OK);
        let list = body_json(list_resp).await;
        assert_eq!(list.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn get_workspace_not_found() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn update_workspace() {
        let app = app();
        let body = serde_json::json!({ "tenant_id": "t1", "name": "Old", "slug": "old" });
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
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
                    .uri(format!("/api/v1/workspaces/{id}"))
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
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
    async fn delete_workspace() {
        let app = app();
        let body = serde_json::json!({ "tenant_id": "t1", "name": "Del", "slug": "del" });
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
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
                    .uri(format!("/api/v1/workspaces/{id}"))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(del_resp.status(), StatusCode::NO_CONTENT);
    }
}
