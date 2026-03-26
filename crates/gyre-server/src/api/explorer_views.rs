/// Explorer View CRUD + LLM Generation — S3.1 (TASK-263)
///
/// Implements:
///   GET  /api/v1/workspaces/:id/explorer-views           — list
///   POST /api/v1/workspaces/:id/explorer-views           — create
///   POST /api/v1/workspaces/:id/explorer-views/generate  — LLM generate (SSE)
///   GET  /api/v1/workspaces/:id/explorer-views/:view_id  — get
///   PUT  /api/v1/workspaces/:id/explorer-views/:view_id  — update
///   DELETE /api/v1/workspaces/:id/explorer-views/:view_id — delete
///
/// Storage: kv_store with namespace "explorer_view" (per-view) and
///          "workspace_explorer_views" (workspace→[view_id] index).
///
/// Spec: specs/system/ui-layout.md §4, specs/system/human-system-interface.md §3.
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::stream;
use gyre_common::{
    view_spec::{validate_view_spec, ViewSpec},
    Id,
};
use gyre_domain::CostEntry;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

use super::{error::ApiError, new_id, now_secs};
use crate::{auth::AuthenticatedAgent, AppState};

// ── Storage constants ─────────────────────────────────────────────────────────

const NS_VIEW: &str = "explorer_view";
const NS_INDEX: &str = "workspace_explorer_views";

// ── Domain types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplorerViewRecord {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub description: Option<String>,
    /// Raw ViewSpec as JSON value (round-tripped to preserve unknown fields).
    pub spec: serde_json::Value,
    pub created_by: String,
    pub is_builtin: bool,
    pub created_at: u64,
    pub updated_at: u64,
}

// ── Request / response shapes ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateViewRequest {
    pub name: String,
    pub description: Option<String>,
    pub spec: serde_json::Value,
}

#[derive(Deserialize)]
pub struct UpdateViewRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub spec: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct GenerateViewRequest {
    pub question: String,
    pub repo_id: Option<String>,
}

// ── KV helpers ────────────────────────────────────────────────────────────────

async fn load_view(state: &AppState, view_id: &str) -> Result<ExplorerViewRecord, ApiError> {
    let raw = state
        .kv_store
        .kv_get(NS_VIEW, view_id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("explorer view {view_id} not found")))?;
    serde_json::from_str(&raw)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("deserialize view: {e}")))
}

async fn save_view(state: &AppState, view: &ExplorerViewRecord) -> Result<(), ApiError> {
    let raw = serde_json::to_string(view)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("serialize view: {e}")))?;
    state
        .kv_store
        .kv_set(NS_VIEW, &view.id, raw)
        .await
        .map_err(ApiError::Internal)
}

async fn add_to_workspace_index(
    state: &AppState,
    workspace_id: &str,
    view_id: &str,
) -> Result<(), ApiError> {
    let mut ids = load_workspace_index(state, workspace_id).await?;
    if !ids.contains(&view_id.to_string()) {
        ids.push(view_id.to_string());
        let raw = serde_json::to_string(&ids)
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("serialize index: {e}")))?;
        state
            .kv_store
            .kv_set(NS_INDEX, workspace_id, raw)
            .await
            .map_err(ApiError::Internal)?;
    }
    Ok(())
}

async fn remove_from_workspace_index(
    state: &AppState,
    workspace_id: &str,
    view_id: &str,
) -> Result<(), ApiError> {
    let mut ids = load_workspace_index(state, workspace_id).await?;
    ids.retain(|id| id != view_id);
    let raw = serde_json::to_string(&ids)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("serialize index: {e}")))?;
    state
        .kv_store
        .kv_set(NS_INDEX, workspace_id, raw)
        .await
        .map_err(ApiError::Internal)
}

async fn load_workspace_index(
    state: &AppState,
    workspace_id: &str,
) -> Result<Vec<String>, ApiError> {
    match state
        .kv_store
        .kv_get(NS_INDEX, workspace_id)
        .await
        .map_err(ApiError::Internal)?
    {
        None => Ok(vec![]),
        Some(raw) => serde_json::from_str(&raw)
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("deserialize index: {e}"))),
    }
}

// ── Built-in view seeding ─────────────────────────────────────────────────────

/// Seed the 4 built-in views for a workspace if they haven't been seeded yet.
///
/// Called lazily on the first list request for a workspace (idempotent: checks
/// `workspace_explorer_views_seeded:<workspace_id>` sentinel key).
async fn seed_builtin_views_if_needed(
    state: &AppState,
    workspace_id: &str,
) -> Result<(), ApiError> {
    let sentinel_ns = "workspace_explorer_views_seeded";
    if state
        .kv_store
        .kv_get(sentinel_ns, workspace_id)
        .await
        .map_err(ApiError::Internal)?
        .is_some()
    {
        return Ok(());
    }

    let builtins: &[(&str, Option<&str>, serde_json::Value)] = &[
        (
            "API Surface",
            Some("Public endpoints and functions"),
            json!({
                "name": "API Surface",
                "description": "Public endpoints and functions",
                "data": {
                    "node_types": ["Endpoint", "Function"],
                    "edge_types": [],
                    "depth": 2
                },
                "layout": "hierarchical"
            }),
        ),
        (
            "Domain Model",
            Some("Types and interfaces across all repos"),
            json!({
                "name": "Domain Model",
                "description": "Types and interfaces across all repos",
                "data": {
                    "node_types": ["Type", "Interface"],
                    "edge_types": [],
                    "depth": 3
                },
                "layout": "graph"
            }),
        ),
        (
            "Security Boundary",
            Some("Auth-related code and dependencies"),
            json!({
                "name": "Security Boundary",
                "description": "Auth-related code and dependencies",
                "data": {
                    "concept": "auth",
                    "node_types": [],
                    "edge_types": [],
                    "depth": 2
                },
                "layout": "hierarchical"
            }),
        ),
        (
            "Test Coverage",
            Some("Functions and their test nodes"),
            json!({
                "name": "Test Coverage",
                "description": "Functions and their test nodes",
                "data": {
                    "node_types": ["Function", "Test"],
                    "edge_types": [],
                    "depth": 1
                },
                "layout": "list"
            }),
        ),
    ];

    let now = now_secs();
    for (name, desc, spec_json) in builtins {
        let id = new_id().to_string();
        let view = ExplorerViewRecord {
            id: id.clone(),
            workspace_id: workspace_id.to_string(),
            name: name.to_string(),
            description: desc.map(|s| s.to_string()),
            spec: spec_json.clone(),
            created_by: "system".to_string(),
            is_builtin: true,
            created_at: now,
            updated_at: now,
        };
        save_view(state, &view).await?;
        add_to_workspace_index(state, workspace_id, &id).await?;
    }

    // Mark as seeded.
    state
        .kv_store
        .kv_set(sentinel_ns, workspace_id, "1".to_string())
        .await
        .map_err(ApiError::Internal)?;

    Ok(())
}

// ── ViewSpec validation helper ────────────────────────────────────────────────

fn parse_and_validate(spec_json: &serde_json::Value) -> Result<(), ApiError> {
    let spec: ViewSpec = serde_json::from_value(spec_json.clone())
        .map_err(|e| ApiError::BadRequest(format!("invalid view spec: {e}")))?;
    validate_view_spec(&spec).map_err(ApiError::BadRequest)
}

// ── GET /api/v1/workspaces/:id/explorer-views ─────────────────────────────────

pub async fn list_explorer_views(
    State(state): State<Arc<AppState>>,
    Path(workspace_id): Path<String>,
    _caller: AuthenticatedAgent,
) -> Result<Json<Vec<ExplorerViewRecord>>, ApiError> {
    seed_builtin_views_if_needed(&state, &workspace_id).await?;

    let ids = load_workspace_index(&state, &workspace_id).await?;
    let mut views = Vec::with_capacity(ids.len());
    for id in &ids {
        match load_view(&state, id).await {
            Ok(v) => views.push(v),
            Err(ApiError::NotFound(_)) => {
                // Stale index entry — skip.
            }
            Err(e) => return Err(e),
        }
    }
    Ok(Json(views))
}

// ── POST /api/v1/workspaces/:id/explorer-views ────────────────────────────────

pub async fn create_explorer_view(
    State(state): State<Arc<AppState>>,
    Path(workspace_id): Path<String>,
    caller: AuthenticatedAgent,
    Json(req): Json<CreateViewRequest>,
) -> Result<(StatusCode, Json<ExplorerViewRecord>), ApiError> {
    parse_and_validate(&req.spec)?;

    // Validate repo_id belongs to this workspace if provided.
    validate_repo_ownership(&state, &workspace_id, &req.spec).await?;

    let now = now_secs();
    let id = new_id().to_string();
    let view = ExplorerViewRecord {
        id: id.clone(),
        workspace_id: workspace_id.clone(),
        name: req.name,
        description: req.description,
        spec: req.spec,
        created_by: caller.agent_id.to_string(),
        is_builtin: false,
        created_at: now,
        updated_at: now,
    };

    save_view(&state, &view).await?;
    add_to_workspace_index(&state, &workspace_id, &id).await?;

    Ok((StatusCode::CREATED, Json(view)))
}

// ── GET /api/v1/workspaces/:id/explorer-views/:view_id ───────────────────────

pub async fn get_explorer_view(
    State(state): State<Arc<AppState>>,
    Path((workspace_id, view_id)): Path<(String, String)>,
    _caller: AuthenticatedAgent,
) -> Result<Json<ExplorerViewRecord>, ApiError> {
    let view = load_view(&state, &view_id).await?;
    if view.workspace_id != workspace_id {
        return Err(ApiError::NotFound(format!(
            "explorer view {view_id} not found in workspace {workspace_id}"
        )));
    }
    Ok(Json(view))
}

// ── PUT /api/v1/workspaces/:id/explorer-views/:view_id ───────────────────────

pub async fn update_explorer_view(
    State(state): State<Arc<AppState>>,
    Path((workspace_id, view_id)): Path<(String, String)>,
    caller: AuthenticatedAgent,
    Json(req): Json<UpdateViewRequest>,
) -> Result<Json<ExplorerViewRecord>, ApiError> {
    let mut view = load_view(&state, &view_id).await?;
    if view.workspace_id != workspace_id {
        return Err(ApiError::NotFound(format!(
            "explorer view {view_id} not found in workspace {workspace_id}"
        )));
    }

    // Ownership check: only creator or Admin may update.
    check_ownership(&view, &caller)?;

    if let Some(name) = req.name {
        view.name = name;
    }
    if let Some(desc) = req.description {
        view.description = Some(desc);
    }
    if let Some(spec) = req.spec {
        parse_and_validate(&spec)?;
        validate_repo_ownership(&state, &workspace_id, &spec).await?;
        view.spec = spec;
    }
    view.updated_at = now_secs();

    save_view(&state, &view).await?;
    Ok(Json(view))
}

// ── DELETE /api/v1/workspaces/:id/explorer-views/:view_id ────────────────────

pub async fn delete_explorer_view(
    State(state): State<Arc<AppState>>,
    Path((workspace_id, view_id)): Path<(String, String)>,
    caller: AuthenticatedAgent,
) -> Result<StatusCode, ApiError> {
    let view = load_view(&state, &view_id).await?;
    if view.workspace_id != workspace_id {
        return Err(ApiError::NotFound(format!(
            "explorer view {view_id} not found in workspace {workspace_id}"
        )));
    }

    check_ownership(&view, &caller)?;

    state
        .kv_store
        .kv_remove(NS_VIEW, &view_id)
        .await
        .map_err(ApiError::Internal)?;
    remove_from_workspace_index(&state, &workspace_id, &view_id).await?;

    Ok(StatusCode::NO_CONTENT)
}

// ── POST /api/v1/workspaces/:id/explorer-views/generate (SSE) ────────────────

pub async fn generate_explorer_view(
    State(state): State<Arc<AppState>>,
    Path(_workspace_id): Path<String>,
    caller: AuthenticatedAgent,
    Json(req): Json<GenerateViewRequest>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>>, ApiError>
{
    // TODO: per-user rate limiting (10 req/min per user per workspace) via
    //       in-memory sliding window counter on AppState.llm_rate_limits.

    // Build a stub view spec as the generated result.
    // Real implementation would: read prompt template from specs/prompts/explorer-generate.md,
    // get graph summary, call LLM via GYRE_LLM_MODEL, stream partial/complete events.
    let generated_spec = json!({
        "name": req.question,
        "description": format!("Generated view for: {}", req.question),
        "data": {
            "node_types": [],
            "edge_types": [],
            "depth": 2,
            "repo_id": req.repo_id
        },
        "layout": "graph",
        "explanation": format!(
            "Stub response: LLM integration not yet wired. Question: \"{}\"",
            req.question
        )
    });

    // Charge budget: record as llm_query cost entry.
    let cost_entry = CostEntry::new(
        new_id(),
        Id::new(caller.agent_id.clone()),
        None,
        "llm_query",
        1.0,
        "tokens",
        now_secs(),
    );
    let _ = state.costs.record(&cost_entry).await;

    let partial_event = json!({
        "explanation": "Generating view..."
    });
    let complete_event = json!({
        "view_spec": generated_spec,
        "explanation": format!(
            "Stub response: LLM integration not yet wired. Question: \"{}\"",
            req.question
        )
    });

    let partial_data = serde_json::to_string(&partial_event).unwrap_or_default();
    let complete_data = serde_json::to_string(&complete_event).unwrap_or_default();

    let events: Vec<Result<Event, std::convert::Infallible>> = vec![
        Ok(Event::default().event("partial").data(partial_data)),
        Ok(Event::default().event("complete").data(complete_data)),
    ];

    let s = stream::iter(events);
    Ok(Sse::new(s))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Check that the caller is the view creator (builtin views reject all edits
/// since created_by is "system").
fn check_ownership(view: &ExplorerViewRecord, caller: &AuthenticatedAgent) -> Result<(), ApiError> {
    if view.is_builtin {
        return Err(ApiError::Forbidden(
            "built-in views cannot be modified".to_string(),
        ));
    }
    // Admin role bypass: caller.role is checked here if available.
    // For now enforce created_by == caller.
    if view.created_by != caller.agent_id {
        return Err(ApiError::Forbidden(
            "only the creator may modify this view".to_string(),
        ));
    }
    Ok(())
}

/// Validate that `repo_id` in the spec belongs to `workspace_id`.
/// Prevents cross-workspace data leakage (spec requirement).
async fn validate_repo_ownership(
    state: &AppState,
    workspace_id: &str,
    spec_json: &serde_json::Value,
) -> Result<(), ApiError> {
    if let Some(repo_id) = spec_json
        .get("data")
        .and_then(|d| d.get("repo_id"))
        .and_then(|r| r.as_str())
    {
        // Load workspace repos from kv_store.
        let workspace_repos_ns = "workspace_repos";
        let raw = state
            .kv_store
            .kv_get(workspace_repos_ns, workspace_id)
            .await
            .map_err(ApiError::Internal)?;

        if let Some(raw) = raw {
            let repo_ids: Vec<String> = serde_json::from_str(&raw).unwrap_or_default();
            if !repo_ids.contains(&repo_id.to_string()) {
                return Err(ApiError::BadRequest(format!(
                    "repo {repo_id} does not belong to workspace {workspace_id}"
                )));
            }
        }
        // If no workspace_repos entry exists, allow (permissive until repos are registered).
    }
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request};
    use http::StatusCode;
    use tower::ServiceExt;

    fn app() -> axum::Router {
        crate::build_router(crate::mem::test_state())
    }

    fn auth() -> &'static str {
        "Bearer test-token"
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn view_body(name: &str) -> Body {
        Body::from(format!(
            r#"{{
                "name": "{name}",
                "description": "test view",
                "spec": {{
                    "name": "{name}",
                    "data": {{"node_types": ["Type"], "edge_types": [], "depth": 1}},
                    "layout": "graph"
                }}
            }}"#
        ))
    }

    #[tokio::test]
    async fn list_explorer_views_returns_builtins() {
        let app = app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces/ws-1/explorer-views")
                    .header("Authorization", auth())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp).await;
        let arr = body.as_array().unwrap();
        // 4 built-in views should be seeded.
        assert_eq!(arr.len(), 4);
        let names: Vec<&str> = arr.iter().map(|v| v["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"API Surface"));
        assert!(names.contains(&"Domain Model"));
        assert!(names.contains(&"Security Boundary"));
        assert!(names.contains(&"Test Coverage"));
    }

    #[tokio::test]
    async fn create_and_get_explorer_view() {
        let app = app();
        // Create
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-2/explorer-views")
                    .header("Authorization", auth())
                    .header("Content-Type", "application/json")
                    .body(view_body("My Auth View"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = body_json(resp).await;
        let view_id = body["id"].as_str().unwrap().to_string();
        assert_eq!(body["name"], "My Auth View");
        assert_eq!(body["is_builtin"], false);

        // Get
        let resp2 = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/workspaces/ws-2/explorer-views/{view_id}"))
                    .header("Authorization", auth())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp2.status(), StatusCode::OK);
        let body2 = body_json(resp2).await;
        assert_eq!(body2["id"], view_id.as_str());
    }

    #[tokio::test]
    async fn create_view_validates_spec() {
        let app = app();
        // flow layout without trace_source should fail.
        let bad_body = Body::from(
            r#"{
                "name": "Bad Flow",
                "spec": {
                    "name": "Bad Flow",
                    "data": {"node_types": [], "edge_types": [], "depth": 1},
                    "layout": "flow"
                }
            }"#,
        );
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-3/explorer-views")
                    .header("Authorization", auth())
                    .header("Content-Type", "application/json")
                    .body(bad_body)
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn generate_explorer_view_streams_sse() {
        let app = app();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-4/explorer-views/generate")
                    .header("Authorization", auth())
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"question":"How does auth work?"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp.headers().get("content-type").unwrap();
        assert!(ct.to_str().unwrap().contains("text/event-stream"));
    }

    #[tokio::test]
    async fn delete_builtin_view_forbidden() {
        let app = app();
        // First list to seed builtins.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces/ws-5/explorer-views")
                    .header("Authorization", auth())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = body_json(resp).await;
        let builtin_id = body.as_array().unwrap()[0]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let resp2 = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!(
                        "/api/v1/workspaces/ws-5/explorer-views/{builtin_id}"
                    ))
                    .header("Authorization", auth())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp2.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn list_is_idempotent_for_builtins() {
        let app = app();
        // Call list twice — builtins should only appear once.
        let resp1 = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces/ws-6/explorer-views")
                    .header("Authorization", auth())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body1 = body_json(resp1).await;

        let resp2 = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces/ws-6/explorer-views")
                    .header("Authorization", auth())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body2 = body_json(resp2).await;

        assert_eq!(
            body1.as_array().unwrap().len(),
            body2.as_array().unwrap().len()
        );
    }
}
