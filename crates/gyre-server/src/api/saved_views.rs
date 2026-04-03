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
        let query = serde_json::from_str(&v.query_json)
            .unwrap_or(serde_json::Value::Object(Default::default()));
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
    auth: AuthenticatedAgent,
) -> Result<Json<Vec<ViewResponse>>, (axum::http::StatusCode, String)> {
    let rid = Id::new(&repo_id);
    let views = state.saved_views.list_by_repo(&rid).await.map_err(|e| {
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to list views: {e}"),
        )
    })?;

    // Seed system default views on first access (lazy initialization).
    if !views.iter().any(|v| v.is_system) {
        let now = now_secs();
        for default in system_default_views(&repo_id, &auth.tenant_id) {
            let view = SavedView {
                id: new_id(),
                repo_id: Id::new(&repo_id),
                workspace_id: Id::new(""),
                tenant_id: Id::new(&auth.tenant_id),
                name: default.0.to_string(),
                description: Some(default.1.to_string()),
                query_json: default.2.to_string(),
                created_by: "system".to_string(),
                created_at: now,
                updated_at: now,
                is_system: true,
            };
            let _ = state.saved_views.create(view).await;
        }
        // Re-fetch after seeding.
        let refreshed = state.saved_views.list_by_repo(&rid).await.map_err(|e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to list views: {e}"),
            )
        })?;
        return Ok(Json(
            refreshed.into_iter().map(ViewResponse::from).collect(),
        ));
    }

    Ok(Json(views.into_iter().map(ViewResponse::from).collect()))
}

/// System default views per the explorer-implementation.md spec.
fn system_default_views(
    _repo_id: &str,
    _tenant_id: &str,
) -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        (
            "Architecture Overview",
            "Full codebase structure",
            r##"{"scope":{"type":"all"},"zoom":"fit"}"##,
        ),
        (
            "Test Coverage Gaps",
            "Functions not reachable from any test",
            r##"{"scope":{"type":"test_gaps"},"emphasis":{"highlight":{"matched":{"color":"#ef4444","label":"Untested"}},"dim_unmatched":0.3},"annotation":{"title":"Test coverage gaps","description":"{{count}} functions not reachable from any test"}}"##,
        ),
        (
            "Hot Paths",
            "Most-called functions",
            r##"{"scope":{"type":"all"},"emphasis":{"heat":{"metric":"incoming_calls","palette":"blue-red"}},"annotation":{"title":"Hot paths"}}"##,
        ),
        (
            "Blast Radius (click)",
            "Click any node to see what it impacts",
            r##"{"scope":{"type":"focus","node":"$clicked","edges":["calls","implements","field_of","depends_on"],"direction":"incoming","depth":10},"emphasis":{"tiered_colors":["#ef4444","#f97316","#eab308","#94a3b8"],"dim_unmatched":0.12},"edges":{"filter":["calls","implements","field_of","depends_on"]},"zoom":"fit","annotation":{"title":"Blast radius: $name","description":"{{count}} transitive callers/implementors"}}"##,
        ),
    ]
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
