//! Meta-spec reconciliation API endpoints (M32).
//!
//! GET  /api/v1/workspaces/{id}/meta-spec-set  — get workspace meta-spec set
//! PUT  /api/v1/workspaces/{id}/meta-spec-set  — update (Admin only)
//! GET  /api/v1/meta-specs/{path}/blast-radius — blast radius for a meta-spec
//! POST /api/v1/workspaces/{id}/meta-specs/preview — preview a draft meta-spec change
//! GET  /api/v1/workspaces/{id}/meta-specs/preview/{preview_id} — poll preview status

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::{auth::AuthenticatedAgent, AppState};

use super::error::ApiError;
use super::now_secs;

// ---------------------------------------------------------------------------
// Types — meta-spec set
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
#[derive(Clone, Serialize, Deserialize)]
pub struct AffectedRepo {
    pub id: String,
    pub workspace_id: String,
    pub reason: String,
}

/// An affected workspace entry in a blast radius response.
#[derive(Clone, Serialize, Deserialize)]
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
// Types — preview
// ---------------------------------------------------------------------------

/// A draft meta-spec (content not yet committed to the spec registry).
#[derive(Deserialize)]
pub struct DraftMetaSpec {
    pub kind: String,
    pub content: String,
}

/// Request body for POST /api/v1/workspaces/{id}/meta-specs/preview.
///
/// Accepts both the UI-style format (`persona_id` + `spec_paths`) and the
/// spec-defined format (`draft` + `targets`).
#[derive(Deserialize)]
pub struct PreviewMetaSpecRequest {
    /// Persona being previewed (UI flow: persona editor).
    #[serde(default)]
    pub persona_id: Option<String>,
    /// Draft content for the persona or meta-spec being tested.
    #[serde(default)]
    pub content: Option<String>,
    /// Spec paths (relative to workspace) to preview against (UI flow).
    #[serde(default)]
    pub spec_paths: Vec<String>,
    /// Draft meta-spec object (spec-defined format).
    #[serde(default)]
    pub draft: Option<DraftMetaSpec>,
}

/// Status of one spec in a preview run.
#[derive(Clone, Serialize, Deserialize)]
pub struct PreviewSpecItem {
    pub path: String,
    /// "running" while in progress; "complete" when done.
    pub status: String,
}

/// Summarised blast radius included in a preview response.
#[derive(Clone, Serialize, Deserialize)]
pub struct PreviewBlastRadius {
    pub affected_workspaces: Vec<AffectedWorkspace>,
    pub affected_repos: Vec<AffectedRepo>,
    /// Count of workspaces that currently bind meta-specs overlapping with the draft.
    pub affected_workspace_count: usize,
}

/// Structural impact summary included in a preview response.
#[derive(Clone, Serialize, Deserialize)]
pub struct StructuralImpact {
    /// Scope of the meta-spec ("workspace", "tenant", "repo").
    pub scope: String,
    /// Number of specs that would be affected by adopting this draft.
    pub affected_spec_count: usize,
}

/// Stored preview session (serialized to kv_store, namespace "meta_spec_previews").
#[derive(Serialize, Deserialize)]
struct PreviewRecord {
    pub preview_id: String,
    pub workspace_id: String,
    /// "running" → "complete" (currently set to "complete" immediately since
    /// agents are not spawned; designed for future async expansion).
    pub state: String,
    pub specs: Vec<PreviewSpecItem>,
    pub blast_radius: PreviewBlastRadius,
    pub structural_impact: StructuralImpact,
    pub created_at: u64,
}

/// Response for POST /api/v1/workspaces/{id}/meta-specs/preview (202 Accepted).
#[derive(Serialize)]
pub struct PreviewResponse {
    pub preview_id: String,
    pub state: String,
    pub specs: Vec<PreviewSpecItem>,
    pub blast_radius: PreviewBlastRadius,
    pub structural_impact: StructuralImpact,
}

/// Response for GET /api/v1/workspaces/{id}/meta-specs/preview/{preview_id}.
#[derive(Serialize)]
pub struct PreviewStatusResponse {
    pub preview_id: String,
    pub state: String,
    pub specs: Vec<PreviewSpecItem>,
    pub blast_radius: PreviewBlastRadius,
    pub structural_impact: StructuralImpact,
}

// ---------------------------------------------------------------------------
// GET /api/v1/workspaces/{id}/meta-spec-set
// ---------------------------------------------------------------------------

pub async fn get_meta_spec_set(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedAgent,
    Path(workspace_id): Path<String>,
) -> Result<Json<MetaSpecSet>, ApiError> {
    // Verify workspace exists.
    state
        .workspaces
        .find_by_id(&Id::new(&workspace_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace '{workspace_id}' not found")))?;

    let set = match state.meta_spec_sets.get(&Id::new(&workspace_id)).await? {
        Some(json) => serde_json::from_str::<MetaSpecSet>(&json)
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("corrupt meta_spec_set: {e}")))?,
        None => MetaSpecSet {
            workspace_id: workspace_id.clone(),
            ..Default::default()
        },
    };
    Ok(Json(set))
}

// ---------------------------------------------------------------------------
// PUT /api/v1/workspaces/{id}/meta-spec-set  (Admin only)
// ---------------------------------------------------------------------------

pub async fn put_meta_spec_set(
    State(state): State<Arc<AppState>>,
    Path(workspace_id): Path<String>,
    auth: AuthenticatedAgent,
    Json(req): Json<UpdateMetaSpecSetRequest>,
) -> Result<(StatusCode, Json<MetaSpecSet>), ApiError> {
    // Admin-only: meta-spec-set bindings are governance controls that determine
    // which personas, principles, standards, and processes govern all agents in a
    // workspace. Allowing non-Admin callers (Developers, Agents) to modify these
    // bindings would let agents rewrite the rules they operate under (NEW-26).
    if !auth.roles.contains(&gyre_domain::UserRole::Admin) {
        return Err(ApiError::Forbidden(
            "only Admin role may update workspace meta-spec-set bindings".to_string(),
        ));
    }

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

    let json = serde_json::to_string(&set)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("serialize meta_spec_set: {e}")))?;
    state
        .meta_spec_sets
        .upsert(&Id::new(&workspace_id), &json)
        .await?;

    Ok((StatusCode::OK, Json(set)))
}

// ---------------------------------------------------------------------------
// GET /api/v1/meta-specs/{path}/blast-radius
// ---------------------------------------------------------------------------

pub async fn get_meta_spec_blast_radius(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedAgent,
    Path(spec_path): Path<String>,
) -> Json<BlastRadiusResponse> {
    let mut affected_workspaces: Vec<AffectedWorkspace> = Vec::new();
    let mut affected_repos: Vec<AffectedRepo> = Vec::new();

    // List all workspaces and check each one for a meta-spec-set referencing spec_path.
    let workspaces = state.workspaces.list().await.unwrap_or_default();
    for workspace in &workspaces {
        let ws_id = workspace.id.as_str();
        let set_opt = state
            .meta_spec_sets
            .get(&workspace.id)
            .await
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str::<MetaSpecSet>(&json).ok());

        if let Some(set) = set_opt {
            let references_spec = set.personas.values().any(|e| e.path == spec_path)
                || set.principles.iter().any(|e| e.path == spec_path)
                || set.standards.iter().any(|e| e.path == spec_path)
                || set.process.iter().any(|e| e.path == spec_path);

            if references_spec {
                affected_workspaces.push(AffectedWorkspace {
                    id: ws_id.to_string(),
                });

                // Collect repos bound to this workspace via the database.
                let repos = state
                    .repos
                    .list_by_workspace(&Id::new(ws_id))
                    .await
                    .unwrap_or_default();
                for repo in &repos {
                    affected_repos.push(AffectedRepo {
                        id: repo.id.to_string(),
                        workspace_id: ws_id.to_string(),
                        reason: "workspace_binding".to_string(),
                    });
                }
            }
        }
    }

    Json(BlastRadiusResponse {
        spec_path,
        affected_repos,
        affected_workspaces,
    })
}

// ---------------------------------------------------------------------------
// POST /api/v1/workspaces/{id}/meta-specs/preview  (Admin or Developer)
// ---------------------------------------------------------------------------

pub async fn post_meta_spec_preview(
    State(state): State<Arc<AppState>>,
    Path(workspace_id): Path<String>,
    auth: AuthenticatedAgent,
    Json(req): Json<PreviewMetaSpecRequest>,
) -> Result<(StatusCode, Json<PreviewResponse>), ApiError> {
    // Require Admin or Developer role: preview runs agents against real specs, so
    // read-only and agent callers must not trigger preview runs.
    let allowed = auth.roles.contains(&gyre_domain::UserRole::Admin)
        || auth.roles.contains(&gyre_domain::UserRole::Developer);
    if !allowed {
        return Err(ApiError::Forbidden(
            "Admin or Developer role required to run meta-spec preview".to_string(),
        ));
    }

    // Verify workspace exists.
    state
        .workspaces
        .find_by_id(&Id::new(&workspace_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace '{workspace_id}' not found")))?;

    // Collect spec paths from either field format.
    let spec_paths: Vec<String> = if !req.spec_paths.is_empty() {
        req.spec_paths
    } else if let Some(ref draft) = req.draft {
        // spec-format: extract path from draft kind for a synthetic target
        vec![format!("meta/{}.md", draft.kind.replace(':', "/"))]
    } else {
        vec![]
    };

    if spec_paths.is_empty() {
        return Err(ApiError::BadRequest(
            "at least one spec path must be provided in spec_paths or targets".to_string(),
        ));
    }

    // Compute blast radius from the workspace's current meta-spec-set.
    // This tells the caller which workspaces/repos currently bind meta-specs that
    // the draft would replace (structural impact prediction without LLM).
    let blast_radius = compute_preview_blast_radius(&state, &workspace_id).await;

    let affected_spec_count = spec_paths.len();

    let structural_impact = StructuralImpact {
        scope: "workspace".to_string(),
        affected_spec_count,
    };

    // Build spec items. Preview is non-blocking: specs are marked "complete"
    // immediately because no agents are spawned in this endpoint. The endpoint
    // provides structural impact analysis; actual agent-based preview runs are
    // a future extension (spawn agents on preview branches per §5).
    let specs: Vec<PreviewSpecItem> = spec_paths
        .iter()
        .map(|p| PreviewSpecItem {
            path: p.clone(),
            status: "complete".to_string(),
        })
        .collect();

    // Generate a stable preview ID and persist the record so the status endpoint
    // can look it up.
    let preview_id = uuid::Uuid::new_v4().to_string();

    let record = PreviewRecord {
        preview_id: preview_id.clone(),
        workspace_id: workspace_id.clone(),
        state: "complete".to_string(),
        specs: specs.clone(),
        blast_radius: blast_radius.clone(),
        structural_impact: structural_impact.clone(),
        created_at: now_secs(),
    };

    let record_json = serde_json::to_string(&record)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("serialize preview record: {e}")))?;

    state
        .kv_store
        .kv_set("meta_spec_previews", &preview_id, record_json)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("store preview record: {e}")))?;

    Ok((
        StatusCode::ACCEPTED,
        Json(PreviewResponse {
            preview_id,
            state: "complete".to_string(),
            specs,
            blast_radius,
            structural_impact,
        }),
    ))
}

// ---------------------------------------------------------------------------
// GET /api/v1/workspaces/{id}/meta-specs/preview/{preview_id}
// ---------------------------------------------------------------------------

pub async fn get_meta_spec_preview_status(
    State(state): State<Arc<AppState>>,
    Path((workspace_id, preview_id)): Path<(String, String)>,
    _auth: AuthenticatedAgent,
) -> Result<Json<PreviewStatusResponse>, ApiError> {
    // Verify workspace exists.
    state
        .workspaces
        .find_by_id(&Id::new(&workspace_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace '{workspace_id}' not found")))?;

    let record_json = state
        .kv_store
        .kv_get("meta_spec_previews", &preview_id)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("kv lookup: {e}")))?
        .ok_or_else(|| ApiError::NotFound(format!("preview '{preview_id}' not found")))?;

    let record: PreviewRecord = serde_json::from_str(&record_json)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("corrupt preview record: {e}")))?;

    // Validate that this preview belongs to the requested workspace.
    if record.workspace_id != workspace_id {
        return Err(ApiError::NotFound(format!(
            "preview '{preview_id}' not found"
        )));
    }

    Ok(Json(PreviewStatusResponse {
        preview_id: record.preview_id,
        state: record.state,
        specs: record.specs,
        blast_radius: record.blast_radius,
        structural_impact: record.structural_impact,
    }))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Compute a blast-radius summary for a workspace's current meta-spec-set.
///
/// Returns the set of workspaces and repos that reference any of the meta-specs
/// currently bound to `workspace_id`. This gives a structural impact estimate:
/// if the draft replaces any of those specs, all of these workspaces/repos are
/// affected.
async fn compute_preview_blast_radius(
    state: &Arc<AppState>,
    workspace_id: &str,
) -> PreviewBlastRadius {
    // caller-scope:ok — correctly uses ws_id (each workspace's own ID) inside the loop, not the parameter
    // Collect the spec paths currently bound in this workspace's meta-spec-set.
    let bound_paths: Vec<String> = state
        .meta_spec_sets
        .get(&Id::new(workspace_id))
        .await
        .ok()
        .flatten()
        .and_then(|json| serde_json::from_str::<MetaSpecSet>(&json).ok())
        .map(|set| {
            let mut paths: Vec<String> = set.personas.values().map(|e| e.path.clone()).collect();
            paths.extend(set.principles.iter().map(|e| e.path.clone()));
            paths.extend(set.standards.iter().map(|e| e.path.clone()));
            paths.extend(set.process.iter().map(|e| e.path.clone()));
            paths
        })
        .unwrap_or_default();

    let mut affected_workspaces: Vec<AffectedWorkspace> = Vec::new();
    let mut affected_repos: Vec<AffectedRepo> = Vec::new();

    let workspaces = state.workspaces.list().await.unwrap_or_default();
    for workspace in &workspaces {
        let ws_id = workspace.id.as_str();
        let set_opt = state
            .meta_spec_sets
            .get(&workspace.id)
            .await
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str::<MetaSpecSet>(&json).ok());

        if let Some(set) = set_opt {
            let references_any_bound = bound_paths.iter().any(|bp| {
                set.personas.values().any(|e| &e.path == bp)
                    || set.principles.iter().any(|e| &e.path == bp)
                    || set.standards.iter().any(|e| &e.path == bp)
                    || set.process.iter().any(|e| &e.path == bp)
            });

            if references_any_bound {
                affected_workspaces.push(AffectedWorkspace {
                    id: ws_id.to_string(),
                });

                let repos = state
                    .repos
                    .list_by_workspace(&Id::new(ws_id))
                    .await
                    .unwrap_or_default();

                for repo in &repos {
                    affected_repos.push(AffectedRepo {
                        id: repo.id.to_string(),
                        workspace_id: ws_id.to_string(),
                        reason: "workspace_binding".to_string(),
                    });
                }
            }
        }
    }

    let affected_workspace_count = affected_workspaces.len();

    PreviewBlastRadius {
        affected_workspaces,
        affected_repos,
        affected_workspace_count,
    }
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
    async fn put_meta_spec_set_requires_admin() {
        // A Developer-role JWT should be rejected with 403 (NEW-26 fix).
        use crate::abac_middleware::seed_builtin_policies;
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
        let state = make_test_state_with_jwt();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(seed_builtin_policies(&state))
        });

        // Create a workspace using the admin static token.
        let ws_resp = crate::api::api_router()
            .with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"ws26","slug":"ws26"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ws_resp.status(), StatusCode::CREATED);
        let ws_json = body_json(ws_resp).await;
        let ws_id = ws_json["id"].as_str().unwrap().to_string();

        // Developer-role OIDC JWT.
        let dev_token = sign_test_jwt(
            &serde_json::json!({
                "sub": "dev-sub",
                "preferred_username": "developer-user",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );

        let resp = crate::api::api_router()
            .with_state(state)
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/workspaces/{ws_id}/meta-spec-set"))
                    .header("authorization", format!("Bearer {dev_token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"personas":{},"principles":[],"standards":[],"process":[]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
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

    // -----------------------------------------------------------------------
    // Preview endpoint tests
    // -----------------------------------------------------------------------

    /// Helper: create a workspace and return its id string.
    async fn create_workspace(app: &Router, name: &str) -> String {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"name":"{name}","slug":"{name}"}}"#
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::CREATED,
            "workspace creation failed"
        );
        body_json(resp).await["id"].as_str().unwrap().to_string()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn preview_returns_202_with_preview_id() {
        let app = app();
        let ws_id = create_workspace(&app, "preview-ws-1").await;

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/workspaces/{ws_id}/meta-specs/preview"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"spec_paths":["specs/system/search.md","specs/system/identity.md"]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::ACCEPTED);
        let json = body_json(resp).await;
        assert!(
            json["preview_id"].as_str().is_some(),
            "preview_id must be present"
        );
        assert_eq!(json["state"].as_str().unwrap(), "complete");
        let specs = json["specs"].as_array().unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0]["status"].as_str().unwrap(), "complete");
        assert_eq!(specs[1]["status"].as_str().unwrap(), "complete");
        assert!(json["blast_radius"].is_object());
        assert!(json["structural_impact"].is_object());
        assert_eq!(
            json["structural_impact"]["affected_spec_count"]
                .as_u64()
                .unwrap(),
            2
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn preview_status_endpoint_returns_stored_preview() {
        let app = app();
        let ws_id = create_workspace(&app, "preview-ws-2").await;

        // Create preview.
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/workspaces/{ws_id}/meta-specs/preview"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"spec_paths":["specs/system/auth.md"]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_resp.status(), StatusCode::ACCEPTED);
        let create_json = body_json(create_resp).await;
        let preview_id = create_json["preview_id"].as_str().unwrap().to_string();

        // Poll status.
        let status_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/workspaces/{ws_id}/meta-specs/preview/{preview_id}"
                    ))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(status_resp.status(), StatusCode::OK);
        let status_json = body_json(status_resp).await;
        assert_eq!(status_json["preview_id"].as_str().unwrap(), preview_id);
        assert_eq!(status_json["state"].as_str().unwrap(), "complete");
        let specs = status_json["specs"].as_array().unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0]["path"].as_str().unwrap(), "specs/system/auth.md");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn preview_requires_at_least_one_spec() {
        let app = app();
        let ws_id = create_workspace(&app, "preview-ws-3").await;

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/workspaces/{ws_id}/meta-specs/preview"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"spec_paths":[]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn preview_returns_404_for_unknown_workspace() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/00000000-0000-0000-0000-000000000000/meta-specs/preview")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"spec_paths":["specs/system/auth.md"]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn preview_status_returns_404_for_unknown_preview_id() {
        let app = app();
        let ws_id = create_workspace(&app, "preview-ws-4").await;

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/workspaces/{ws_id}/meta-specs/preview/00000000-0000-0000-0000-000000000000"
                    ))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn preview_readonly_role_is_forbidden() {
        use crate::abac_middleware::seed_builtin_policies;
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
        let state = make_test_state_with_jwt();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(seed_builtin_policies(&state))
        });

        // Create workspace as admin.
        let ws_resp = crate::api::api_router()
            .with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"name":"preview-ws-ro","slug":"preview-ws-ro"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ws_resp.status(), StatusCode::CREATED);
        let ws_json = body_json(ws_resp).await;
        let ws_id = ws_json["id"].as_str().unwrap().to_string();

        // ReadOnly OIDC JWT.
        let ro_token = sign_test_jwt(
            &serde_json::json!({
                "sub": "ro-sub",
                "preferred_username": "readonly-user",
                "realm_access": { "roles": ["readonly"] }
            }),
            3600,
        );

        let resp = crate::api::api_router()
            .with_state(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/workspaces/{ws_id}/meta-specs/preview"))
                    .header("authorization", format!("Bearer {ro_token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"spec_paths":["specs/system/auth.md"]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}

// ===========================================================================
// Meta-spec registry CRUD API (agent-runtime spec §2)
//
// GET    /api/v1/meta-specs-registry           — list (query: scope, scope_id, kind, required)
// POST   /api/v1/meta-specs-registry           — create
// GET    /api/v1/meta-specs-registry/:id       — get by id
// PUT    /api/v1/meta-specs-registry/:id       — update (new version, bumps version)
// DELETE /api/v1/meta-specs-registry/:id       — delete (409 if bindings)
// GET    /api/v1/meta-specs-registry/:id/versions       — list versions
// GET    /api/v1/meta-specs-registry/:id/versions/:ver  — get specific version
// ===========================================================================

use axum::extract::Query;
use gyre_domain::meta_spec::{MetaSpec, MetaSpecApprovalStatus, MetaSpecKind, MetaSpecScope};
use gyre_ports::MetaSpecFilter;
use sha2::{Digest, Sha256};

fn sha256_hex(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    hex::encode(h.finalize())
}

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ListMetaSpecsQuery {
    pub scope: Option<String>,
    pub scope_id: Option<String>,
    pub kind: Option<String>,
    pub required: Option<bool>,
}

#[derive(Deserialize)]
pub struct CreateMetaSpecRequest {
    pub kind: String,
    pub name: String,
    pub scope: String,
    pub scope_id: Option<String>,
    pub prompt: Option<String>,
    pub required: Option<bool>,
}

#[derive(Deserialize)]
pub struct UpdateMetaSpecRequest {
    pub name: Option<String>,
    pub prompt: Option<String>,
    pub required: Option<bool>,
    pub approval_status: Option<String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_kind(s: &str) -> Result<MetaSpecKind, ApiError> {
    MetaSpecKind::parse(s).ok_or_else(|| {
        ApiError::BadRequest(format!(
            "invalid kind '{s}'; must be one of: meta:persona, meta:principle, meta:standard, meta:process"
        ))
    })
}

fn parse_scope(s: &str) -> Result<MetaSpecScope, ApiError> {
    MetaSpecScope::parse(s).ok_or_else(|| {
        ApiError::BadRequest(format!(
            "invalid scope '{s}'; must be one of: Global, Workspace"
        ))
    })
}

fn parse_approval_status(s: &str) -> Result<MetaSpecApprovalStatus, ApiError> {
    MetaSpecApprovalStatus::parse(s).ok_or_else(|| {
        ApiError::BadRequest(format!(
            "invalid approval_status '{s}'; must be: Pending, Approved, or Rejected"
        ))
    })
}

// ---------------------------------------------------------------------------
// GET /api/v1/meta-specs-registry
// ---------------------------------------------------------------------------

pub async fn list_meta_specs_registry(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedAgent,
    Query(q): Query<ListMetaSpecsQuery>,
) -> Result<Json<Vec<MetaSpec>>, ApiError> {
    let scope = match q.scope.as_deref() {
        None => None,
        Some(s) => Some(parse_scope(s)?),
    };
    let kind = match q.kind.as_deref() {
        None => None,
        Some(k) => Some(parse_kind(k)?),
    };
    let filter = MetaSpecFilter {
        scope,
        scope_id: q.scope_id,
        kind,
        required: q.required,
    };
    let results = state
        .meta_specs
        .list(&filter)
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(results))
}

// ---------------------------------------------------------------------------
// POST /api/v1/meta-specs-registry
// ---------------------------------------------------------------------------

pub async fn create_meta_spec_registry(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Json(req): Json<CreateMetaSpecRequest>,
) -> Result<(StatusCode, Json<MetaSpec>), ApiError> {
    let kind = parse_kind(&req.kind)?;
    let scope = parse_scope(&req.scope)?;
    let prompt = req.prompt.unwrap_or_default();
    let content_hash = sha256_hex(&prompt);
    let now = now_secs();

    let ms = MetaSpec {
        id: Id::new(uuid::Uuid::new_v4().to_string()),
        kind,
        name: req.name,
        scope,
        scope_id: req.scope_id,
        prompt,
        version: 1,
        content_hash,
        required: req.required.unwrap_or(false),
        approval_status: MetaSpecApprovalStatus::Pending,
        approved_by: None,
        approved_at: None,
        created_by: auth.agent_id.as_str().to_string(),
        created_at: now,
        updated_at: now,
    };

    state
        .meta_specs
        .create(&ms)
        .await
        .map_err(ApiError::Internal)?;
    Ok((StatusCode::CREATED, Json(ms)))
}

// ---------------------------------------------------------------------------
// GET /api/v1/meta-specs-registry/:id
// ---------------------------------------------------------------------------

pub async fn get_meta_spec_registry(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedAgent,
    Path(id): Path<String>,
) -> Result<Json<MetaSpec>, ApiError> {
    let ms = state
        .meta_specs
        .get_by_id(&Id::new(&id))
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("meta-spec '{id}' not found")))?;
    Ok(Json(ms))
}

// ---------------------------------------------------------------------------
// PUT /api/v1/meta-specs-registry/:id
// ---------------------------------------------------------------------------

pub async fn update_meta_spec_registry(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(id): Path<String>,
    Json(req): Json<UpdateMetaSpecRequest>,
) -> Result<Json<MetaSpec>, ApiError> {
    let mut ms = state
        .meta_specs
        .get_by_id(&Id::new(&id))
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("meta-spec '{id}' not found")))?;

    let now = now_secs();

    if let Some(name) = req.name {
        ms.name = name;
    }
    if let Some(prompt) = req.prompt {
        ms.content_hash = sha256_hex(&prompt);
        ms.prompt = prompt;
        // Spec §2: editing content resets approval to Pending until re-reviewed.
        ms.approval_status = MetaSpecApprovalStatus::Pending;
        ms.approved_by = None;
        ms.approved_at = None;
    }
    if let Some(required) = req.required {
        ms.required = required;
    }
    if let Some(ref status_str) = req.approval_status {
        let status = parse_approval_status(status_str)?;
        if status == MetaSpecApprovalStatus::Approved {
            ms.approved_by = Some(auth.agent_id.as_str().to_string());
            ms.approved_at = Some(now);
        }
        ms.approval_status = status;
    }

    ms.version += 1;
    ms.updated_at = now;

    state
        .meta_specs
        .update(&ms)
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(ms))
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/meta-specs-registry/:id
// ---------------------------------------------------------------------------

pub async fn delete_meta_spec_registry(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedAgent,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let rid = Id::new(&id);
    let has_bindings = state
        .meta_spec_bindings
        .has_bindings_for(&rid)
        .await
        .map_err(ApiError::Internal)?;
    if has_bindings {
        return Err(ApiError::Conflict(format!(
            "cannot delete meta-spec '{id}': active bindings reference it"
        )));
    }
    state
        .meta_specs
        .delete(&rid)
        .await
        .map_err(ApiError::Internal)?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// GET /api/v1/meta-specs-registry/:id/versions
// ---------------------------------------------------------------------------

pub async fn list_meta_spec_versions(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedAgent,
    Path(id): Path<String>,
) -> Result<Json<Vec<gyre_domain::MetaSpecVersion>>, ApiError> {
    // Ensure meta-spec exists.
    state
        .meta_specs
        .get_by_id(&Id::new(&id))
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("meta-spec '{id}' not found")))?;

    let versions = state
        .meta_specs
        .list_versions(&Id::new(&id))
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(versions))
}

// ---------------------------------------------------------------------------
// GET /api/v1/meta-specs-registry/:id/versions/:version
// ---------------------------------------------------------------------------

pub async fn get_meta_spec_version(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedAgent,
    Path((id, version)): Path<(String, u32)>,
) -> Result<Json<gyre_domain::MetaSpecVersion>, ApiError> {
    let ver = state
        .meta_specs
        .get_version(&Id::new(&id), version)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| {
            ApiError::NotFound(format!("version {version} of meta-spec '{id}' not found"))
        })?;
    Ok(Json(ver))
}

// ---------------------------------------------------------------------------
// Registry-level tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod registry_tests {
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
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn create_and_get_meta_spec() {
        let app = app();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs-registry")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"kind":"meta:persona","name":"test-worker","scope":"Global","prompt":"You are a worker."}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        let id = json["id"].as_str().unwrap().to_string();
        assert_eq!(json["name"].as_str().unwrap(), "test-worker");
        assert_eq!(json["version"].as_u64().unwrap(), 1);
        assert!(json["content_hash"].as_str().is_some());

        // GET by id — reuse same app instance so the in-memory store is shared
        let get_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/meta-specs-registry/{id}"))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_resp.status(), StatusCode::OK);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn list_meta_specs_registry_empty() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/meta-specs-registry")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json.as_array().unwrap().is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn update_bumps_version() {
        let app = app();
        // Create
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs-registry")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"kind":"meta:principle","name":"conventional-commits","scope":"Global","prompt":"Use CC."}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let create_json = body_json(create_resp).await;
        let id = create_json["id"].as_str().unwrap().to_string();

        // Update
        let update_resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/meta-specs-registry/{id}"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"prompt":"Updated CC prompt."}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(update_resp.status(), StatusCode::OK);
        let update_json = body_json(update_resp).await;
        assert_eq!(update_json["version"].as_u64().unwrap(), 2);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_not_found_returns_404() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/meta-specs-registry/00000000-0000-0000-0000-000000000000")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn delete_meta_spec() {
        let app = app();
        // Create
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/meta-specs-registry")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"kind":"meta:standard","name":"test-coverage","scope":"Global"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let id = body_json(create_resp).await["id"]
            .as_str()
            .unwrap()
            .to_string();

        // Delete
        let del_resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/meta-specs-registry/{id}"))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(del_resp.status(), StatusCode::NO_CONTENT);
    }
}
