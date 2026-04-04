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

/// Resolve the workspace_id for a repo. Returns empty string if not found.
async fn resolve_workspace_id(state: &AppState, repo_id: &str) -> String {
    let rid = Id::new(repo_id);
    match state.repos.find_by_id(&rid).await {
        Ok(Some(r)) => r.workspace_id.to_string(),
        _ => String::new(),
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
        let workspace_id = resolve_workspace_id(&state, &repo_id).await;
        for default in system_default_views() {
            let view = SavedView {
                id: new_id(),
                repo_id: Id::new(&repo_id),
                workspace_id: Id::new(&workspace_id),
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
            refreshed
                .into_iter()
                .filter(|v| v.tenant_id.as_str() == auth.tenant_id)
                .map(ViewResponse::from)
                .collect(),
        ));
    }

    Ok(Json(
        views
            .into_iter()
            .filter(|v| v.tenant_id.as_str() == auth.tenant_id)
            .map(ViewResponse::from)
            .collect(),
    ))
}

/// System default views per the explorer-implementation.md spec.
///
/// Each tuple is `(name, description, query_json)`. Shared by both the
/// repo-scoped (`/repos/:id/views`) and workspace-scoped
/// (`/workspaces/:id/explorer-views`) saved-view APIs.
pub fn system_default_views() -> Vec<(&'static str, &'static str, &'static str)> {
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

    let workspace_id = resolve_workspace_id(&state, &repo_id).await;

    let view = SavedView {
        id: new_id(),
        repo_id: Id::new(&repo_id),
        workspace_id: Id::new(&workspace_id),
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
    Path((repo_id, view_id)): Path<(String, String)>,
    auth: AuthenticatedAgent,
) -> Result<Json<ViewResponse>, (axum::http::StatusCode, String)> {
    let vid = Id::new(&view_id);
    match state.saved_views.get(&vid).await {
        Ok(Some(v)) => {
            // Verify repo_id matches and tenant access.
            if v.repo_id.as_str() != repo_id {
                return Err((
                    axum::http::StatusCode::NOT_FOUND,
                    format!("View not found: {view_id}"),
                ));
            }
            if v.tenant_id.as_str() != auth.tenant_id {
                return Err((
                    axum::http::StatusCode::FORBIDDEN,
                    "Access denied".to_string(),
                ));
            }
            Ok(Json(ViewResponse::from(v)))
        }
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
    Path((repo_id, view_id)): Path<(String, String)>,
    auth: AuthenticatedAgent,
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

    // Verify repo_id matches and tenant access.
    if existing.repo_id.as_str() != repo_id {
        return Err((
            axum::http::StatusCode::NOT_FOUND,
            format!("View not found: {view_id}"),
        ));
    }
    if existing.tenant_id.as_str() != auth.tenant_id {
        return Err((
            axum::http::StatusCode::FORBIDDEN,
            "Access denied".to_string(),
        ));
    }

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
    Path((repo_id, view_id)): Path<(String, String)>,
    auth: AuthenticatedAgent,
) -> Result<axum::http::StatusCode, (axum::http::StatusCode, String)> {
    let vid = Id::new(&view_id);
    // Verify ownership before deleting.
    match state.saved_views.get(&vid).await {
        Ok(Some(v)) => {
            if v.repo_id.as_str() != repo_id {
                return Err((
                    axum::http::StatusCode::NOT_FOUND,
                    format!("View not found: {view_id}"),
                ));
            }
            if v.tenant_id.as_str() != auth.tenant_id {
                return Err((
                    axum::http::StatusCode::FORBIDDEN,
                    "Access denied".to_string(),
                ));
            }
        }
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
    }

    match state.saved_views.delete(&vid).await {
        Ok(()) => Ok(axum::http::StatusCode::NO_CONTENT),
        Err(e) => Err((
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to delete view: {e}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── System default views ───────────────────────────────────────────────

    #[test]
    fn test_system_default_views_count() {
        let defaults = system_default_views();
        assert_eq!(
            defaults.len(),
            4,
            "Should have exactly 4 system default views"
        );
    }

    #[test]
    fn test_system_default_views_names() {
        let defaults = system_default_views();
        let names: Vec<&str> = defaults.iter().map(|(n, _, _)| *n).collect();
        assert!(names.contains(&"Architecture Overview"));
        assert!(names.contains(&"Test Coverage Gaps"));
        assert!(names.contains(&"Hot Paths"));
        assert!(names.contains(&"Blast Radius (click)"));
    }

    #[test]
    fn test_system_default_views_have_valid_json() {
        let defaults = system_default_views();
        for (name, _desc, query_json) in &defaults {
            let parsed: Result<serde_json::Value, _> = serde_json::from_str(query_json);
            assert!(
                parsed.is_ok(),
                "System view '{}' has invalid JSON: {}",
                name,
                parsed.err().unwrap()
            );
        }
    }

    #[test]
    fn test_system_default_views_have_scope() {
        let defaults = system_default_views();
        for (name, _desc, query_json) in &defaults {
            let parsed: serde_json::Value = serde_json::from_str(query_json).unwrap();
            assert!(
                parsed.get("scope").is_some(),
                "System view '{}' should have a scope field",
                name
            );
            assert!(
                parsed["scope"].get("type").is_some(),
                "System view '{}' scope should have a type field",
                name
            );
        }
    }

    #[test]
    fn test_architecture_overview_uses_all_scope() {
        let defaults = system_default_views();
        let overview = defaults
            .iter()
            .find(|(n, _, _)| *n == "Architecture Overview")
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(overview.2).unwrap();
        assert_eq!(parsed["scope"]["type"], "all");
    }

    #[test]
    fn test_test_coverage_gaps_uses_test_gaps_scope() {
        let defaults = system_default_views();
        let gaps = defaults
            .iter()
            .find(|(n, _, _)| *n == "Test Coverage Gaps")
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(gaps.2).unwrap();
        assert_eq!(parsed["scope"]["type"], "test_gaps");
        // Should have emphasis highlighting
        assert!(
            parsed.get("emphasis").is_some(),
            "Test Coverage Gaps should have emphasis"
        );
    }

    #[test]
    fn test_hot_paths_uses_heat_emphasis() {
        let defaults = system_default_views();
        let hot = defaults.iter().find(|(n, _, _)| *n == "Hot Paths").unwrap();
        let parsed: serde_json::Value = serde_json::from_str(hot.2).unwrap();
        assert!(
            parsed["emphasis"]["heat"].is_object(),
            "Hot Paths should use heat emphasis"
        );
        assert_eq!(
            parsed["emphasis"]["heat"]["metric"], "incoming_calls",
            "Hot Paths should use incoming_calls metric"
        );
    }

    #[test]
    fn test_blast_radius_uses_clicked_focus() {
        let defaults = system_default_views();
        let blast = defaults
            .iter()
            .find(|(n, _, _)| *n == "Blast Radius (click)")
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(blast.2).unwrap();
        assert_eq!(parsed["scope"]["type"], "focus");
        assert_eq!(
            parsed["scope"]["node"], "$clicked",
            "Blast Radius should use $clicked for interactive exploration"
        );
        // Should have tiered_colors
        assert!(
            parsed["emphasis"]["tiered_colors"].is_array(),
            "Blast Radius should use tiered_colors emphasis"
        );
    }

    // ── ViewResponse conversion ────────────────────────────────────────────

    #[test]
    fn test_view_response_from_saved_view() {
        let view = SavedView {
            id: Id::new("v1"),
            repo_id: Id::new("repo1"),
            workspace_id: Id::new("ws1"),
            tenant_id: Id::new("t1"),
            name: "Test View".to_string(),
            description: Some("A test".to_string()),
            query_json: r#"{"scope":{"type":"all"}}"#.to_string(),
            created_by: "user1".to_string(),
            created_at: 1000,
            updated_at: 2000,
            is_system: false,
        };
        let response = ViewResponse::from(view);
        assert_eq!(response.id, "v1");
        assert_eq!(response.repo_id, "repo1");
        assert_eq!(response.name, "Test View");
        assert_eq!(response.description.as_deref(), Some("A test"));
        assert_eq!(response.query["scope"]["type"], "all");
        assert_eq!(response.created_by, "user1");
        assert_eq!(response.created_at, 1000);
        assert_eq!(response.updated_at, 2000);
        assert!(!response.is_system);
    }

    #[test]
    fn test_view_response_with_invalid_query_json() {
        let view = SavedView {
            id: Id::new("v1"),
            repo_id: Id::new("repo1"),
            workspace_id: Id::new("ws1"),
            tenant_id: Id::new("t1"),
            name: "Bad View".to_string(),
            description: None,
            query_json: "not valid json".to_string(),
            created_by: "user1".to_string(),
            created_at: 1000,
            updated_at: 2000,
            is_system: false,
        };
        let response = ViewResponse::from(view);
        // Should fall back to empty object, not panic
        assert!(response.query.is_object());
    }

    #[test]
    fn test_view_response_system_flag() {
        let view = SavedView {
            id: Id::new("v1"),
            repo_id: Id::new("repo1"),
            workspace_id: Id::new("ws1"),
            tenant_id: Id::new("t1"),
            name: "System View".to_string(),
            description: None,
            query_json: r#"{"scope":{"type":"all"}}"#.to_string(),
            created_by: "system".to_string(),
            created_at: 1000,
            updated_at: 1000,
            is_system: true,
        };
        let response = ViewResponse::from(view);
        assert!(response.is_system, "System flag should be preserved");
    }
}
