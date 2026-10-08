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
/// Storage: delegates to SavedViewRepository port (shared with the repo-scoped
///          /api/v1/repos/:id/views endpoints). Views are scoped by workspace_id.
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
use gyre_ports::saved_view::SavedView;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

use super::{error::ApiError, new_id, now_secs, saved_views::system_default_views};
use crate::{
    auth::AuthenticatedAgent,
    llm_rate_limit::{check_rate_limit, LLM_RATE_LIMIT, LLM_WINDOW_SECS},
    AppState,
};
use gyre_domain::UserRole;

// ── Domain types ──────────────────────────────────────────────────────────────

/// Response shape for explorer-views endpoints (workspace-scoped).
///
/// Maps from the canonical `SavedView` port model, translating field names
/// for backward compatibility with existing frontend consumers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplorerViewRecord {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub description: Option<String>,
    /// Raw ViewSpec as JSON value (maps from SavedView.query_json).
    pub spec: serde_json::Value,
    pub created_by: String,
    pub is_builtin: bool,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<SavedView> for ExplorerViewRecord {
    fn from(v: SavedView) -> Self {
        let spec = match serde_json::from_str(&v.query_json) {
            Ok(parsed) => parsed,
            Err(e) => {
                tracing::warn!(
                    view_id = %v.id,
                    "Corrupt query_json in saved view — returning empty object: {e}"
                );
                serde_json::Value::Object(Default::default())
            }
        };
        Self {
            id: v.id.to_string(),
            workspace_id: v.workspace_id.to_string(),
            name: v.name,
            description: v.description,
            spec,
            created_by: v.created_by,
            is_builtin: v.is_system,
            created_at: v.created_at,
            updated_at: v.updated_at,
        }
    }
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

// ── Built-in view seeding (uses SavedViewRepository) ─────────────────────────

/// Seed system default views for a workspace if they haven't been seeded.
///
/// Uses the same canonical defaults as the repo-scoped saved_views API.
/// The check-then-insert race is handled by INSERT OR IGNORE at the adapter level.
async fn seed_system_views_if_needed(
    state: &AppState,
    workspace_id: &str,
    tenant_id: &str,
) -> Result<(), ApiError> {
    // caller-scope:ok — iterates static system defaults, all scoped to the same workspace
    let wid = Id::new(workspace_id);
    let views = state
        .saved_views
        .list_by_workspace(&wid)
        .await
        .map_err(ApiError::Internal)?;

    if views.iter().any(|v| v.is_system) {
        return Ok(());
    }

    let now = now_secs();
    for default in system_default_views() {
        let query_json = default.2.to_string();
        let view = SavedView {
            id: new_id(),
            // Workspace-scoped views use a sentinel repo_id.
            repo_id: Id::new("__workspace__"),
            workspace_id: Id::new(workspace_id),
            tenant_id: Id::new(tenant_id),
            name: default.0.to_string(),
            description: Some(default.1.to_string()),
            query_json,
            created_by: "system".to_string(),
            created_at: now,
            updated_at: now,
            is_system: true,
        };
        // INSERT OR IGNORE handles races — duplicates are silently dropped.
        let _ = state.saved_views.create(view).await;
    }

    Ok(())
}

// system_default_views() is imported from super::saved_views.

// ── ViewSpec validation helper ────────────────────────────────────────────────

fn parse_and_validate(spec_json: &serde_json::Value) -> Result<(), ApiError> {
    // Accept both ViewQuery (canonical per spec) and ViewSpec (legacy) formats.
    // Try ViewQuery first since it's the primary grammar defined in view-query-grammar.md.
    if let Ok(vq) = serde_json::from_value::<gyre_common::view_query::ViewQuery>(spec_json.clone())
    {
        let errors = vq.validate();
        if errors.is_empty() {
            return Ok(());
        }
        // Fall through to try ViewSpec if ViewQuery validation fails
    }
    let spec: ViewSpec = serde_json::from_value(spec_json.clone())
        .map_err(|e| ApiError::BadRequest(format!("invalid view spec: {e}")))?;
    validate_view_spec(&spec).map_err(ApiError::BadRequest)
}

/// Verify the caller has workspace membership (any role is sufficient for views).
/// Agent/system tokens (user_id = None) skip the membership check but still
/// verify workspace-tenant ownership when the workspace exists.
async fn check_workspace_membership(
    state: &AppState,
    workspace_id: &str,
    caller: &AuthenticatedAgent,
) -> Result<(), ApiError> {
    let wid = Id::new(workspace_id);
    match state.workspaces.find_by_id(&wid).await {
        Ok(Some(ws)) => {
            // Workspace exists: verify it belongs to caller's tenant
            if ws.tenant_id.as_str() != caller.tenant_id {
                return Err(ApiError::Forbidden(
                    "Access denied: workspace not in your tenant".to_string(),
                ));
            }
        }
        Ok(None) => {
            // Workspace not found in DB. Non-user tokens (agents/system) are allowed
            // through — they're already tenant-scoped by the auth layer. User tokens
            // must have a real workspace.
            if caller.user_id.is_some() {
                return Err(ApiError::NotFound(format!(
                    "Workspace not found: {workspace_id}"
                )));
            }
            // Agent/system token: tenant-scoped auth is sufficient
        }
        Err(e) => {
            return Err(ApiError::Internal(e));
        }
    }
    // Check workspace membership for user tokens (agent tokens skip this)
    if let Some(ref user_id) = caller.user_id {
        match state
            .workspace_memberships
            .find_by_user_and_workspace(user_id, &wid)
            .await
        {
            Ok(Some(_)) => {}
            Ok(None) => {
                return Err(ApiError::Forbidden(
                    "Access denied: not a member of this workspace".to_string(),
                ));
            }
            Err(e) => {
                return Err(ApiError::Internal(e));
            }
        }
    }
    Ok(())
}

// ── GET /api/v1/workspaces/:id/explorer-views ─────────────────────────────────

pub async fn list_explorer_views(
    State(state): State<Arc<AppState>>,
    Path(workspace_id): Path<String>,
    caller: AuthenticatedAgent,
) -> Result<Json<Vec<ExplorerViewRecord>>, ApiError> {
    check_workspace_membership(&state, &workspace_id, &caller).await?;
    seed_system_views_if_needed(&state, &workspace_id, &caller.tenant_id).await?;

    let wid = Id::new(&workspace_id);
    let views = state
        .saved_views
        .list_by_workspace(&wid)
        .await
        .map_err(ApiError::Internal)?;

    let records: Vec<ExplorerViewRecord> = views
        .into_iter()
        .filter(|v| v.tenant_id.as_str() == caller.tenant_id)
        .map(ExplorerViewRecord::from)
        .collect();

    Ok(Json(records))
}

// ── POST /api/v1/workspaces/:id/explorer-views ────────────────────────────────

pub async fn create_explorer_view(
    State(state): State<Arc<AppState>>,
    Path(workspace_id): Path<String>,
    caller: AuthenticatedAgent,
    Json(req): Json<CreateViewRequest>,
) -> Result<(StatusCode, Json<ExplorerViewRecord>), ApiError> {
    check_workspace_membership(&state, &workspace_id, &caller).await?;

    // Validate length limits on user-supplied strings
    if req.name.len() > 200 {
        return Err(ApiError::BadRequest(
            "View name exceeds 200 character limit".to_string(),
        ));
    }
    if let Some(ref desc) = req.description {
        if desc.len() > 2000 {
            return Err(ApiError::BadRequest(
                "View description exceeds 2000 character limit".to_string(),
            ));
        }
    }

    parse_and_validate(&req.spec)?;

    // Validate repo_id belongs to this workspace if provided.
    validate_repo_ownership(&state, &workspace_id, &req.spec).await?;

    let query_json = serde_json::to_string(&req.spec)
        .map_err(|e| ApiError::BadRequest(format!("invalid spec JSON: {e}")))?;

    let now = now_secs();
    let view = SavedView {
        id: new_id(),
        repo_id: Id::new("__workspace__"),
        workspace_id: Id::new(&workspace_id),
        tenant_id: Id::new(&caller.tenant_id),
        name: req.name,
        description: req.description,
        query_json,
        created_by: caller.agent_id.to_string(),
        created_at: now,
        updated_at: now,
        is_system: false,
    };

    let created = state
        .saved_views
        .create(view)
        .await
        .map_err(ApiError::Internal)?;

    Ok((StatusCode::CREATED, Json(ExplorerViewRecord::from(created))))
}

// ── GET /api/v1/workspaces/:id/explorer-views/:view_id ───────────────────────

pub async fn get_explorer_view(
    State(state): State<Arc<AppState>>,
    Path((workspace_id, view_id)): Path<(String, String)>,
    caller: AuthenticatedAgent,
) -> Result<Json<ExplorerViewRecord>, ApiError> {
    check_workspace_membership(&state, &workspace_id, &caller).await?;
    let vid = Id::new(&view_id);
    let view = state
        .saved_views
        .get(&vid)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("explorer view {view_id} not found")))?;

    if view.workspace_id.as_str() != workspace_id {
        return Err(ApiError::NotFound(format!(
            "explorer view {view_id} not found in workspace {workspace_id}"
        )));
    }
    if view.tenant_id.as_str() != caller.tenant_id {
        return Err(ApiError::Forbidden("access denied".to_string()));
    }

    Ok(Json(ExplorerViewRecord::from(view)))
}

// ── PUT /api/v1/workspaces/:id/explorer-views/:view_id ───────────────────────

pub async fn update_explorer_view(
    State(state): State<Arc<AppState>>,
    Path((workspace_id, view_id)): Path<(String, String)>,
    caller: AuthenticatedAgent,
    Json(req): Json<UpdateViewRequest>,
) -> Result<Json<ExplorerViewRecord>, ApiError> {
    check_workspace_membership(&state, &workspace_id, &caller).await?;

    // Validate length limits on user-supplied strings
    if let Some(ref name) = req.name {
        if name.len() > 200 {
            return Err(ApiError::BadRequest(
                "View name exceeds 200 character limit".to_string(),
            ));
        }
    }
    if let Some(ref desc) = req.description {
        if desc.len() > 2000 {
            return Err(ApiError::BadRequest(
                "View description exceeds 2000 character limit".to_string(),
            ));
        }
    }
    let vid = Id::new(&view_id);
    let existing = state
        .saved_views
        .get(&vid)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("explorer view {view_id} not found")))?;

    if existing.workspace_id.as_str() != workspace_id {
        return Err(ApiError::NotFound(format!(
            "explorer view {view_id} not found in workspace {workspace_id}"
        )));
    }
    if existing.tenant_id.as_str() != caller.tenant_id {
        return Err(ApiError::Forbidden("access denied".to_string()));
    }

    // Ownership check: only creator or Admin may update. System views reject edits.
    check_ownership(&existing, &caller)?;

    let query_json = if let Some(spec) = &req.spec {
        parse_and_validate(spec)?;
        validate_repo_ownership(&state, &workspace_id, spec).await?;
        serde_json::to_string(spec)
            .map_err(|e| ApiError::BadRequest(format!("invalid spec JSON: {e}")))?
    } else {
        existing.query_json.clone()
    };

    let updated = SavedView {
        name: req.name.unwrap_or(existing.name),
        description: match req.description {
            Some(d) if d.is_empty() => None,
            Some(d) => Some(d),
            None => existing.description,
        },
        query_json,
        updated_at: now_secs(),
        ..existing
    };

    let saved = state
        .saved_views
        .update(updated)
        .await
        .map_err(ApiError::Internal)?;

    Ok(Json(ExplorerViewRecord::from(saved)))
}

// ── DELETE /api/v1/workspaces/:id/explorer-views/:view_id ────────────────────

pub async fn delete_explorer_view(
    State(state): State<Arc<AppState>>,
    Path((workspace_id, view_id)): Path<(String, String)>,
    caller: AuthenticatedAgent,
) -> Result<StatusCode, ApiError> {
    check_workspace_membership(&state, &workspace_id, &caller).await?;
    let vid = Id::new(&view_id);
    let view = state
        .saved_views
        .get(&vid)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("explorer view {view_id} not found")))?;

    if view.workspace_id.as_str() != workspace_id {
        return Err(ApiError::NotFound(format!(
            "explorer view {view_id} not found in workspace {workspace_id}"
        )));
    }
    if view.tenant_id.as_str() != caller.tenant_id {
        return Err(ApiError::Forbidden("access denied".to_string()));
    }

    check_ownership(&view, &caller)?;

    let tid = Id::new(&caller.tenant_id);
    state
        .saved_views
        .delete(&vid, &tid)
        .await
        .map_err(ApiError::Internal)?;

    Ok(StatusCode::NO_CONTENT)
}

// ── POST /api/v1/workspaces/:id/explorer-views/generate (SSE) ────────────────

pub async fn generate_explorer_view(
    State(state): State<Arc<AppState>>,
    Path(workspace_id): Path<String>,
    caller: AuthenticatedAgent,
    Json(req): Json<GenerateViewRequest>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>>, ApiError>
{
    check_workspace_membership(&state, &workspace_id, &caller).await?;
    // Per-user/workspace sliding-window rate limit (HSI §6): 10 req/60 s.
    {
        let mut limiter = state.llm_rate_limiter.lock().await;
        if let Err(retry_after) = check_rate_limit(
            &mut limiter,
            &caller.agent_id,
            &workspace_id,
            LLM_RATE_LIMIT,
            LLM_WINDOW_SECS,
        ) {
            return Err(ApiError::RateLimited(retry_after));
        }
    }

    // Require LLM to be configured.
    let factory = state.llm.as_ref().ok_or(ApiError::LlmUnavailable)?;

    let ws_id = Id::new(&workspace_id);

    // Load effective prompt; fall back to hardcoded default.
    let template_content = state
        .prompt_templates
        .get_effective(&ws_id, "explorer-generate")
        .await
        .map_err(ApiError::Internal)?
        .map(|t| t.content)
        .unwrap_or_else(|| crate::llm_defaults::PROMPT_EXPLORER_GENERATE.to_string());

    // Do NOT inject user input into the system prompt — that enables prompt injection.
    // The template may contain {{question}} for backward compatibility, but we strip it
    // and pass the user's question solely as the user prompt.
    let system_prompt = template_content.replace("{{question}}", "");
    let user_prompt = req.question.clone();

    // Resolve model and call LLM for structured JSON output.
    let (model, _) =
        crate::llm_helpers::resolve_llm_model(&state, &ws_id, "explorer-generate").await;
    let llm_output = factory
        .for_model(&model)
        .predict_json(&system_prompt, &user_prompt)
        .await
        .map_err(|e| {
            tracing::error!(model = %model, workspace_id = %workspace_id, error = ?e, "LLM predict_json failed in generate_explorer_view");
            ApiError::Internal(e)
        })?;

    // Charge budget: estimate token cost from prompt size.
    // The LlmPort doesn't return actual usage, so we estimate:
    // ~4 chars per token for English, plus response overhead (~500 tokens).
    // Explorer view generation involves structured JSON output and multi-step
    // reasoning, so we apply a 3x multiplier to the base estimate.
    let estimated_input = (user_prompt.len() + system_prompt.len()) / 4;
    let base_estimate = (estimated_input + 500) as f64;
    let estimated_tokens = base_estimate * 3.0;
    let cost_entry = CostEntry::new(
        new_id(),
        Id::new(caller.agent_id.clone()),
        None,
        "llm_query",
        estimated_tokens,
        "tokens",
        now_secs(),
    );
    let _ = state.costs.record(&cost_entry).await;

    // Spec §2: the server validates the LLM's output against the view spec
    // grammar before sending `event: complete`. An invalid spec is reported
    // as `{view_spec: null, explanation, fallback}` (the same null-view-spec
    // pattern as unanswerable questions) — NOT a 500, and NOT forwarded raw
    // to the client.
    let grammar_ok = parse_and_validate(&llm_output).is_ok();
    let mut complete = if grammar_ok {
        json!({
            "view_spec": llm_output.clone(),
            "explanation": format!("Generated view for: {}", req.question),
        })
    } else {
        tracing::warn!(workspace_id = %workspace_id, "LLM produced invalid view spec");
        invalid_llm_view_response()
    };
    // Validate repo_id ownership in the generated spec too — a hallucinated
    // repo_id must not leak cross-workspace data when the view renders.
    if grammar_ok {
        if let Err(e) = validate_repo_ownership(&state, &workspace_id, &llm_output).await {
            tracing::warn!(workspace_id = %workspace_id, error = %e, "LLM view spec referenced foreign repo_id");
            complete = invalid_llm_view_response();
        }
    }

    let partial_data =
        serde_json::to_string(&json!({"explanation": "Generating view..."})).unwrap_or_default();
    let complete_data = serde_json::to_string(&complete).unwrap_or_default();

    let events: Vec<Result<Event, std::convert::Infallible>> = vec![
        Ok(Event::default().event("partial").data(partial_data)),
        Ok(Event::default().event("complete").data(complete_data)),
    ];

    let s = stream::iter(events);
    Ok(Sse::new(s))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Check that the caller is the view creator or an Admin.
/// System (built-in) views reject all edits regardless of role.
fn check_ownership(view: &SavedView, caller: &AuthenticatedAgent) -> Result<(), ApiError> {
    if view.is_system {
        return Err(ApiError::Forbidden(
            "built-in views cannot be modified".to_string(),
        ));
    }
    // Admin bypass: admins may modify any user-created view.
    if caller.roles.contains(&UserRole::Admin) {
        return Ok(());
    }
    if view.created_by != caller.agent_id {
        return Err(ApiError::Forbidden(
            "only the creator or an admin may modify this view".to_string(),
        ));
    }
    Ok(())
}

/// The `event: complete` payload for an LLM output that failed grammar or
/// ownership validation (ui-layout.md §2): null `view_spec` + explanation +
/// a fallback list view the client renders instead of crashing.
fn invalid_llm_view_response() -> serde_json::Value {
    json!({
        "view_spec": null,
        "explanation": "Generated view was invalid — try rephrasing",
        "fallback": {
            "name": "Fallback list view",
            "data": {"node_types": [], "edge_types": [], "depth": 1},
            "layout": "list"
        },
    })
}

/// Validate that every `repo_id` referenced by the spec — the top-level data
/// layer AND each `side-by-side` sub-view's own data layer — belongs to
/// `workspace_id`. Prevents cross-workspace data leakage (spec requirement:
/// sub-views do not inherit the parent's scope, so each must be checked).
async fn validate_repo_ownership(
    state: &AppState,
    workspace_id: &str,
    spec_json: &serde_json::Value,
) -> Result<(), ApiError> {
    // The top-level data layer plus left/right sub-view data layers.
    let mut data_layers = vec![spec_json.get("data")];
    for side in ["left", "right"] {
        if let Some(sub) = spec_json.get(side) {
            data_layers.push(sub.get("data"));
        }
    }
    for data in data_layers.into_iter().flatten() {
        if let Some(repo_id) = data.get("repo_id").and_then(|r| r.as_str()) {
            // Verify ownership via the database.
            let repo = state
                .repos
                .find_by_id(&gyre_common::Id::new(repo_id))
                .await
                .map_err(ApiError::Internal)?;
            match repo {
                Some(r) if r.workspace_id.to_string() == workspace_id => {}
                _ => {
                    return Err(ApiError::BadRequest(format!(
                        "repo {repo_id} does not belong to workspace {workspace_id}"
                    )));
                }
            }
        }
    }
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request};
    use http::StatusCode;
    use std::sync::Arc;
    use tower::ServiceExt;

    fn app() -> axum::Router {
        crate::build_router(crate::mem::test_state())
    }

    fn app_no_llm() -> axum::Router {
        let mut s = (*crate::mem::test_state()).clone();
        s.llm = None;
        crate::build_router(Arc::new(s))
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
        // 6 system default views should be seeded.
        assert_eq!(arr.len(), 6);
        let names: Vec<&str> = arr.iter().map(|v| v["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"Architecture Overview"));
        assert!(names.contains(&"Test Coverage Gaps"));
        assert!(names.contains(&"Hot Paths"));
        assert!(names.contains(&"Blast Radius (click)"));
        assert!(names.contains(&"Spec Coverage"));
        assert!(names.contains(&"Ungoverned Risk"));
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

    #[tokio::test]
    async fn generate_explorer_view_returns_503_when_llm_unavailable() {
        let app = app_no_llm();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-503/explorer-views/generate")
                    .header("Authorization", auth())
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"question":"What are the main components?"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"], "llm_unavailable");
    }

    #[tokio::test]
    async fn generate_explorer_view_rate_limited_after_10_requests() {
        let app = app();
        let generate_body = r#"{"question":"What uses the database?"}"#;

        // First 10 requests must succeed (SSE 200).
        for i in 0..10 {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/workspaces/ws-rl/explorer-views/generate")
                        .header("Authorization", auth())
                        .header("Content-Type", "application/json")
                        .body(Body::from(generate_body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK, "request {i} should succeed");
        }

        // 11th request must be rate-limited.
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-rl/explorer-views/generate")
                    .header("Authorization", auth())
                    .header("Content-Type", "application/json")
                    .body(Body::from(generate_body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        let retry_after = resp
            .headers()
            .get("Retry-After")
            .expect("Retry-After header");
        let secs: u64 = retry_after.to_str().unwrap().parse().unwrap();
        assert!(secs >= 1, "Retry-After must be at least 1 second");
    }

    fn app_with_llm_response(response: serde_json::Value) -> axum::Router {
        let mut s = (*crate::mem::test_state()).clone();
        s.llm = Some(Arc::new(gyre_adapters::MockLlmPortFactory {
            inner: Arc::new(gyre_adapters::MockLlmAdapter::json_response(response)),
        }));
        crate::build_router(Arc::new(s))
    }

    async fn sse_body(resp: axum::response::Response) -> String {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    fn complete_event_payload(sse: &str) -> serde_json::Value {
        // SSE frames look like "event: complete\ndata: {...}\n\n".
        let frame = sse
            .split("event: complete")
            .nth(1)
            .expect("complete event present");
        let data_line = frame
            .lines()
            .find(|l| l.starts_with("data: "))
            .expect("data line in complete event");
        serde_json::from_str(data_line.trim_start_matches("data: ")).unwrap()
    }

    #[tokio::test]
    async fn create_view_rejects_side_by_side_sub_view_nesting() {
        let app = app();
        let body = Body::from(
            r#"{
                "name": "Nested",
                "spec": {
                    "name": "Nested",
                    "data": {"node_types": [], "edge_types": [], "depth": 1},
                    "layout": "side-by-side",
                    "left": {"data": {}, "layout": "side-by-side"},
                    "right": {"data": {}, "layout": "list"}
                }
            }"#,
        );
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-nest/explorer-views")
                    .header("Authorization", auth())
                    .header("Content-Type", "application/json")
                    .body(body)
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn create_view_rejects_spec_path_filter_without_repo_id() {
        let app = app();
        let body = Body::from(
            r#"{
                "name": "Spec filter",
                "spec": {
                    "name": "Spec filter",
                    "data": {
                        "node_types": [], "edge_types": [], "depth": 1,
                        "filter": {"spec_path": "system/payment-retry.md"}
                    },
                    "layout": "graph"
                }
            }"#,
        );
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-sp/explorer-views")
                    .header("Authorization", auth())
                    .header("Content-Type", "application/json")
                    .body(body)
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn create_view_rejects_unknown_repo_id() {
        let app = app();
        let body = Body::from(
            r#"{
                "name": "Foreign repo",
                "spec": {
                    "name": "Foreign repo",
                    "data": {"node_types": [], "edge_types": [], "depth": 1, "repo_id": "repo-other"},
                    "layout": "graph"
                }
            }"#,
        );
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-own/explorer-views")
                    .header("Authorization", auth())
                    .header("Content-Type", "application/json")
                    .body(body)
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn create_view_rejects_foreign_repo_id_in_sub_view() {
        // Sub-views don't inherit the parent's repo scope — each must be
        // validated. A foreign repo_id smuggled into `left.data` must 400.
        let state = crate::mem::test_state();
        // Seed a repo that belongs to a *different* workspace.
        let foreign = gyre_domain::Repository::new(
            gyre_common::Id::new("repo-foreign"),
            gyre_common::Id::new("ws-other"),
            "foreign",
            "repos/foreign.git",
            0,
        );
        state.repos.create(&foreign).await.unwrap();
        let app = crate::build_router(state);

        let body = Body::from(
            r#"{
                "name": "Smuggled",
                "spec": {
                    "name": "Smuggled",
                    "data": {"node_types": [], "edge_types": [], "depth": 1},
                    "layout": "side-by-side",
                    "left": {"data": {"repo_id": "repo-foreign"}, "layout": "list"},
                    "right": {"data": {}, "layout": "list"}
                }
            }"#,
        );
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-sub/explorer-views")
                    .header("Authorization", auth())
                    .header("Content-Type", "application/json")
                    .body(body)
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn generate_rejects_invalid_llm_view_spec_with_fallback() {
        // The mock LLM returns a structurally-plausible but grammatically
        // invalid spec (flow layout, no trace_source). The server must NOT
        // 500 or forward it — the complete event carries view_spec: null
        // plus a fallback list view (ui-layout.md §2).
        let app = app_with_llm_response(serde_json::json!({
            "name": "LLM output",
            "data": {"node_types": [], "edge_types": [], "depth": 1},
            "layout": "flow"
        }));
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-inv/explorer-views/generate")
                    .header("Authorization", auth())
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"question":"How does auth work?"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let sse = sse_body(resp).await;
        let payload = complete_event_payload(&sse);
        assert!(payload["view_spec"].is_null(), "payload: {payload}");
        assert_eq!(
            payload["explanation"], "Generated view was invalid — try rephrasing"
        );
        assert_eq!(payload["fallback"]["layout"], "list");
    }

    #[tokio::test]
    async fn generate_forwards_valid_llm_view_spec() {
        let app = app_with_llm_response(serde_json::json!({
            "name": "LLM output",
            "data": {"node_types": ["Module"], "edge_types": [], "depth": 2},
            "layout": "hierarchical"
        }));
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-ok/explorer-views/generate")
                    .header("Authorization", auth())
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"question":"Module structure?"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let sse = sse_body(resp).await;
        let payload = complete_event_payload(&sse);
        assert_eq!(payload["view_spec"]["layout"], "hierarchical");
        assert_eq!(payload["view_spec"]["name"], "LLM output");
    }

    #[tokio::test]
    async fn generate_rejects_llm_spec_with_foreign_repo_id() {
        // A hallucinated repo_id pointing at another workspace's repo must
        // not be forwarded — the client would render cross-workspace data.
        let state = crate::mem::test_state();
        let foreign = gyre_domain::Repository::new(
            gyre_common::Id::new("repo-hallucinated"),
            gyre_common::Id::new("ws-other"),
            "other-ws-repo",
            "repos/other-ws-repo.git",
            0,
        );
        state.repos.create(&foreign).await.unwrap();
        let mut s = (*state).clone();
        s.llm = Some(Arc::new(gyre_adapters::MockLlmPortFactory {
            inner: Arc::new(gyre_adapters::MockLlmAdapter::json_response(
                serde_json::json!({
                    "name": "Leaky",
                    "data": {"node_types": [], "edge_types": [], "depth": 1, "repo_id": "repo-hallucinated"},
                    "layout": "graph"
                }),
            )),
        }));
        let app = crate::build_router(Arc::new(s));

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-leak/explorer-views/generate")
                    .header("Authorization", auth())
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"question":"Show me everything"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let sse = sse_body(resp).await;
        let payload = complete_event_payload(&sse);
        assert!(payload["view_spec"].is_null(), "payload: {payload}");
        assert_eq!(payload["fallback"]["layout"], "list");
    }

    #[tokio::test]
    async fn PROBE_create_view_rejects_viewquery_smuggled_nested_side_by_side() {
        // PROBE: a payload carrying BOTH a valid ViewQuery `scope` AND an
        // invalid nested side-by-side. parse_and_validate tries ViewQuery
        // first and ignores unknown ViewSpec fields — does the nesting
        // rule get bypassed?
        let app = app();
        let body = Body::from(
            r#"{
                "name": "Smuggle",
                "spec": {
                    "scope": {"type": "all"},
                    "layout": "side-by-side",
                    "left": {"data": {}, "layout": "side-by-side"},
                    "right": {"data": {}, "layout": "list"}
                }
            }"#,
        );
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-probe/explorer-views")
                    .header("Authorization", auth())
                    .header("Content-Type", "application/json")
                    .body(body)
                    .unwrap(),
            )
            .await
            .unwrap();
        eprintln!("PROBE status: {}", resp.status());
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
