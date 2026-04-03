//! REST API for saved explorer views (per-repo).
//!
//! GET    /api/v1/repos/:id/views           — list saved views
//! POST   /api/v1/repos/:id/views           — create saved view
//! GET    /api/v1/repos/:id/views/:view_id  — get view
//! PUT    /api/v1/repos/:id/views/:view_id  — update view
//! DELETE /api/v1/repos/:id/views/:view_id  — delete view

use axum::{
    extract::{Path, State},
    Json,
};
use gyre_common::Id;
use gyre_ports::saved_view::SavedView;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{auth::AuthenticatedAgent, AppState};

use super::{new_id, now_secs};

#[derive(Debug, Deserialize)]
pub struct CreateViewRequest {
    pub name: String,
    pub description: Option<String>,
    pub query: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct UpdateViewRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub query: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct ViewResponse {
    pub id: String,
    pub repo_id: String,
    pub name: String,
    pub description: Option<String>,
    pub query: serde_json::Value,
    pub created_by: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub is_system: bool,
}

impl From<SavedView> for ViewResponse {
    fn from(v: SavedView) -> Self {
        let query =
            serde_json::from_str(&v.query_json).unwrap_or(serde_json::Value::Object(Default::default()));
        Self {
            id: v.id.to_string(),
            repo_id: v.repo_id.to_string(),
            name: v.name,
            description: v.description,
            query,
            created_by: v.created_by,
            created_at: v.created_at,
            updated_at: v.updated_at,
            is_system: v.is_system,
        }
    }
}

pub async fn list_views(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    _auth: AuthenticatedAgent,
) -> Result<Json<Vec<ViewResponse>>, (axum::http::StatusCode, String)> {
    let rid = Id::new(&repo_id);
    match state.saved_views.list_by_repo(&rid).await {
        Ok(views) => Ok(Json(views.into_iter().map(ViewResponse::from).collect())),
        Err(e) => Err((
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to list views: {e}"),
        )),
    }
}

pub async fn create_view(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    auth: AuthenticatedAgent,
    Json(req): Json<CreateViewRequest>,
) -> Result<Json<ViewResponse>, (axum::http::StatusCode, String)> {
    let now = now_secs();
    let query_json = serde_json::to_string(&req.query).map_err(|e| {
        (
            axum::http::StatusCode::BAD_REQUEST,
            format!("Invalid query JSON: {e}"),
        )
    })?;

    let view = SavedView {
        id: new_id(),
        repo_id: Id::new(&repo_id),
        workspace_id: Id::new(""), // Will be filled from repo lookup
        tenant_id: Id::new(&auth.tenant_id),
        name: req.name,
        description: req.description,
        query_json,
        created_by: auth.agent_id.clone(),
        created_at: now,
        updated_at: now,
        is_system: false,
    };

    match state.saved_views.create(view).await {
        Ok(v) => Ok(Json(ViewResponse::from(v))),
        Err(e) => Err((
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create view: {e}"),
        )),
    }
}

pub async fn get_view(
    State(state): State<Arc<AppState>>,
    Path((_repo_id, view_id)): Path<(String, String)>,
    _auth: AuthenticatedAgent,
) -> Result<Json<ViewResponse>, (axum::http::StatusCode, String)> {
    let vid = Id::new(&view_id);
    match state.saved_views.get(&vid).await {
        Ok(Some(v)) => Ok(Json(ViewResponse::from(v))),
        Ok(None) => Err((
            axum::http::StatusCode::NOT_FOUND,
            format!("View not found: {view_id}"),
        )),
        Err(e) => Err((
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to get view: {e}"),
        )),
    }
}

pub async fn update_view(
    State(state): State<Arc<AppState>>,
    Path((_repo_id, view_id)): Path<(String, String)>,
    _auth: AuthenticatedAgent,
    Json(req): Json<UpdateViewRequest>,
) -> Result<Json<ViewResponse>, (axum::http::StatusCode, String)> {
    let vid = Id::new(&view_id);
    let existing = match state.saved_views.get(&vid).await {
        Ok(Some(v)) => v,
        Ok(None) => {
            return Err((
                axum::http::StatusCode::NOT_FOUND,
                format!("View not found: {view_id}"),
            ))
        }
        Err(e) => {
            return Err((
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to get view: {e}"),
            ))
        }
    };

    let query_json = if let Some(q) = req.query {
        serde_json::to_string(&q).map_err(|e| {
            (
                axum::http::StatusCode::BAD_REQUEST,
                format!("Invalid query JSON: {e}"),
            )
        })?
    } else {
        existing.query_json.clone()
    };

    let updated = SavedView {
        name: req.name.unwrap_or(existing.name),
        description: req.description.or(existing.description),
        query_json,
        updated_at: now_secs(),
        ..existing
    };

    match state.saved_views.update(updated).await {
        Ok(v) => Ok(Json(ViewResponse::from(v))),
        Err(e) => Err((
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to update view: {e}"),
        )),
    }
}

pub async fn delete_view(
    State(state): State<Arc<AppState>>,
    Path((_repo_id, view_id)): Path<(String, String)>,
    _auth: AuthenticatedAgent,
) -> Result<axum::http::StatusCode, (axum::http::StatusCode, String)> {
    let vid = Id::new(&view_id);
    match state.saved_views.delete(&vid).await {
        Ok(()) => Ok(axum::http::StatusCode::NO_CONTENT),
        Err(e) => Err((
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to delete view: {e}"),
        )),
    }
}
