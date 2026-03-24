//! Meta-spec reconciliation API endpoints (M32).
//!
//! GET  /api/v1/workspaces/{id}/meta-spec-set  — get workspace meta-spec set
//! PUT  /api/v1/workspaces/{id}/meta-spec-set  — update (Admin only)
//! GET  /api/v1/meta-specs/{path}/blast-radius — blast radius for a meta-spec

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::auth::AdminOnly;
use crate::AppState;

use super::error::ApiError;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A pinned meta-spec entry (path + SHA).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetaSpecPinnedEntry {
    pub path: String,
    pub sha: String,
}

/// The bound collection of meta-specs active in a workspace.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct MetaSpecSet {
    pub workspace_id: String,
    /// Named persona bindings: role -> pinned entry (e.g. "backend" -> path@sha).
    #[serde(default)]
    pub personas: HashMap<String, MetaSpecPinnedEntry>,
    /// Ordered principle specs.
    #[serde(default)]
    pub principles: Vec<MetaSpecPinnedEntry>,
    /// Ordered coding standard specs.
    #[serde(default)]
    pub standards: Vec<MetaSpecPinnedEntry>,
    /// Ordered process specs.
    #[serde(default)]
    pub process: Vec<MetaSpecPinnedEntry>,
}

/// Request body for PUT /api/v1/workspaces/{id}/meta-spec-set.
#[derive(Deserialize)]
pub struct UpdateMetaSpecSetRequest {
    #[serde(default)]
    pub personas: HashMap<String, MetaSpecPinnedEntry>,
    #[serde(default)]
    pub principles: Vec<MetaSpecPinnedEntry>,
    #[serde(default)]
    pub standards: Vec<MetaSpecPinnedEntry>,
    #[serde(default)]
    pub process: Vec<MetaSpecPinnedEntry>,
}

/// An affected repo entry in a blast radius response.
#[derive(Serialize)]
pub struct AffectedRepo {
    pub id: String,
    pub workspace_id: String,
    pub reason: String,
}

/// An affected workspace entry in a blast radius response.
#[derive(Serialize)]
pub struct AffectedWorkspace {
    pub id: String,
}

/// Blast radius response for a meta-spec change.
#[derive(Serialize)]
pub struct BlastRadiusResponse {
    pub spec_path: String,
    pub affected_repos: Vec<AffectedRepo>,
    pub affected_workspaces: Vec<AffectedWorkspace>,
}

// ---------------------------------------------------------------------------
// GET /api/v1/workspaces/{id}/meta-spec-set
// ---------------------------------------------------------------------------

pub async fn get_meta_spec_set(
    State(state): State<Arc<AppState>>,
    Path(workspace_id): Path<String>,
) -> Result<Json<MetaSpecSet>, ApiError> {
    // Verify workspace exists.
    state
        .workspaces
        .find_by_id(&Id::new(&workspace_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace '{workspace_id}' not found")))?;

    let sets = state.meta_spec_sets.lock().await;
    let set = sets
        .get(&workspace_id)
        .cloned()
        .unwrap_or_else(|| MetaSpecSet {
            workspace_id: workspace_id.clone(),
            ..Default::default()
        });
    Ok(Json(set))
}

// ---------------------------------------------------------------------------
// PUT /api/v1/workspaces/{id}/meta-spec-set  (Admin only)
// ---------------------------------------------------------------------------

pub async fn put_meta_spec_set(
    State(state): State<Arc<AppState>>,
    Path(workspace_id): Path<String>,
    _admin: AdminOnly,
    Json(req): Json<UpdateMetaSpecSetRequest>,
) -> Result<(StatusCode, Json<MetaSpecSet>), ApiError> {
    // Verify workspace exists.
    state
        .workspaces
        .find_by_id(&Id::new(&workspace_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace '{workspace_id}' not found")))?;

    let set = MetaSpecSet {
        workspace_id: workspace_id.clone(),
        personas: req.personas,
        principles: req.principles,
        standards: req.standards,
        process: req.process,
    };

    {
        let mut sets = state.meta_spec_sets.lock().await;
        sets.insert(workspace_id, set.clone());
    }

    Ok((StatusCode::OK, Json(set)))
}

// ---------------------------------------------------------------------------
// GET /api/v1/meta-specs/{path}/blast-radius
// ---------------------------------------------------------------------------

pub async fn get_meta_spec_blast_radius(
    State(state): State<Arc<AppState>>,
    Path(spec_path): Path<String>,
) -> Json<BlastRadiusResponse> {
    // Collect matching workspace IDs while holding the lock, then drop it before async calls.
    let matching_workspace_ids: Vec<String> = {
        let sets = state.meta_spec_sets.lock().await;
        sets.iter()
            .filter(|(_, set)| {
                set.personas.values().any(|e| e.path == spec_path)
                    || set.principles.iter().any(|e| e.path == spec_path)
                    || set.standards.iter().any(|e| e.path == spec_path)
                    || set.process.iter().any(|e| e.path == spec_path)
            })
            .map(|(ws_id, _)| ws_id.clone())
            .collect()
    };

    let mut affected_workspaces: Vec<AffectedWorkspace> = Vec::new();
    let mut affected_repos: Vec<AffectedRepo> = Vec::new();

    for workspace_id in &matching_workspace_ids {
        affected_workspaces.push(AffectedWorkspace {
            id: workspace_id.clone(),
        });

        // Collect repos bound to this workspace via kv_store.
        let repo_ids: Vec<String> = state
            .kv_store
            .kv_get("workspace_repos", workspace_id)
            .await
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
            .unwrap_or_default();
        for repo_id in &repo_ids {
            affected_repos.push(AffectedRepo {
                id: repo_id.clone(),
                workspace_id: workspace_id.clone(),
                reason: "workspace_binding".to_string(),
            });
        }
    }

    Json(BlastRadiusResponse {
        spec_path,
        affected_repos,
        affected_workspaces,
    })
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app() -> Router {
        let state = test_state();
        crate::api::api_router().with_state(state)
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn blast_radius_empty() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/meta-specs/meta%2Fpersonas%2Fbackend.md/blast-radius")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json["affected_repos"].as_array().unwrap().is_empty());
        assert!(json["affected_workspaces"].as_array().unwrap().is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn meta_spec_set_not_found_for_unknown_workspace() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces/00000000-0000-0000-0000-000000000000/meta-spec-set")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
