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
use std::time::Duration;

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
    /// Previous view definition the question refines (HSI §3 Generated
    /// Views: "follow-up input refines the current view"). Validated
    /// against the same grammar before the LLM sees it.
    pub previous_spec: Option<serde_json::Value>,
}

/// One prior conversation turn sent back by the client
/// (system-explorer.md §8 follow-up context). The server is stateless —
/// the client owns the conversation, like briefing-ask (HSI §1325).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AskHistoryEntry {
    /// "user" or "assistant".
    pub role: String,
    pub content: String,
}

#[derive(Deserialize)]
pub struct AskExplorerRequest {
    pub question: String,
    pub repo_id: Option<String>,
    /// Prior conversation turns (follow-up context, §8). Capped at 20 —
    /// the server rejects more, the client drops older turns.
    #[serde(default)]
    pub history: Option<Vec<AskHistoryEntry>>,
}

/// A validated entity reference extracted from an LLM answer
/// (system-explorer.md §8 grounding: every claim references a concrete
/// artifact). Only refs the server resolved against the real graph are
/// returned — the UI renders these as clickable links.
#[derive(Debug, Clone, Serialize)]
pub struct EntityRef {
    /// "node" | "spec" | "commit" | "agent".
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    /// Resolved display label (node name, spec path, short SHA, agent id).
    pub label: String,
}

/// Structural prediction triggered by a "what would happen if…" question
/// (§8 follow-up capabilities): the same prediction machinery as
/// `POST /repos/:id/graph/predict`, run over the resolved repo scope.
#[derive(Serialize)]
pub struct AskPrediction {
    pub repo_id: String,
    pub predictions: Vec<serde_json::Value>,
}

#[derive(Serialize)]
pub struct AskExplorerResponse {
    /// Answer text with `[entity:<kind>:<id>]` tokens (all validated).
    pub answer: String,
    /// Validated entity references in order of first appearance.
    pub entity_refs: Vec<EntityRef>,
    /// View spec for "show me X" questions (grammar-validated), else null.
    pub view_spec: Option<serde_json::Value>,
    /// Structural prediction for "what would happen if…" questions, else null.
    pub prediction: Option<AskPrediction>,
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

/// Top-level fields of the ViewQuery grammar (view-query-grammar.md).
const VIEW_QUERY_FIELDS: &[&str] = &[
    "scope",
    "emphasis",
    "edges",
    "zoom",
    "annotation",
    "groups",
    "callouts",
    "narrative",
];

/// Top-level fields of the ViewSpec grammar (ui-layout.md §4).
const VIEW_SPEC_FIELDS: &[&str] = &[
    "name",
    "description",
    "data",
    "layout",
    "encoding",
    "annotations",
    "highlight",
    "explanation",
    "left",
    "right",
];

fn parse_and_validate(spec_json: &serde_json::Value) -> Result<(), ApiError> {
    let obj = spec_json
        .as_object()
        .ok_or_else(|| ApiError::BadRequest("view spec must be a JSON object".to_string()))?;
    // The two grammars have disjoint top-level field sets (view-query-grammar.md
    // supersedes ui-layout.md §4's grammar but saved views in either format are
    // accepted). serde ignores unknown fields by default, so "try ViewQuery
    // first, else ViewSpec" would let a hybrid payload ride the ViewQuery
    // branch and smuggle unvalidated ViewSpec fields (e.g. nested
    // side-by-side) into storage. Classify the document by its top-level keys
    // instead: a payload is one grammar or the other — never both, never
    // unknown fields.
    let view_query_fields = obj.keys().any(|k| VIEW_QUERY_FIELDS.contains(&k.as_str()));
    let view_spec_fields = obj.keys().any(|k| VIEW_SPEC_FIELDS.contains(&k.as_str()));
    if view_query_fields && view_spec_fields {
        return Err(ApiError::BadRequest(
            "view spec mixes ViewQuery and ViewSpec fields — submit exactly one grammar"
                .to_string(),
        ));
    }
    if view_query_fields {
        // ViewQuery (canonical grammar per view-query-grammar.md).
        let vq: gyre_common::view_query::ViewQuery = serde_json::from_value(spec_json.clone())
            .map_err(|e| ApiError::BadRequest(format!("invalid view query: {e}")))?;
        let errors = vq.validate();
        if errors.is_empty() {
            return Ok(());
        }
        return Err(ApiError::BadRequest(format!(
            "invalid view query: {}",
            errors.join("; ")
        )));
    }
    // ViewSpec (legacy grammar per ui-layout.md §4) — or an unrecognized
    // object, which fails ViewSpec parsing with a 400.
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

/// Validate a caller-supplied `repo_id` (request body scope field) against
/// the workspace the caller is authorized for: the repo must exist and
/// belong to that workspace. Returns Forbidden on cross-workspace repo_id,
/// NotFound on unknown repo_id.
async fn validate_repo_scope(
    state: &AppState,
    workspace_id: &Id,
    repo_id: Option<&str>,
) -> Result<(), ApiError> {
    let Some(rid) = repo_id else {
        return Ok(());
    };
    match state.repos.find_by_id(&Id::new(rid)).await {
        Ok(Some(r)) if r.workspace_id == *workspace_id => Ok(()),
        Ok(Some(_)) => Err(ApiError::Forbidden(
            "repo does not belong to this workspace".into(),
        )),
        Ok(None) => Err(ApiError::NotFound(format!("repo {rid} not found"))),
        Err(e) => Err(ApiError::Internal(e)),
    }
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
    // Per-user/workspace sliding-window rate limit (ui-layout.md §2):
    // 10 req/60 s, keyed on the auth-context user_id.
    {
        let mut limiter = state.llm_rate_limiter.lock().await;
        if let Err(retry_after) = check_rate_limit(
            &mut limiter,
            &crate::llm_rate_limit::rate_limit_principal(&caller),
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

    // Grounding (ui-layout.md §2 Available Data): the explorer-generate
    // prompt carries the question, the workspace's node types and counts,
    // and the view spec grammar schema.
    let ws = state
        .workspaces
        .find_by_id(&ws_id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {} not found", workspace_id)))?;
    let workspace_name = ws.name.clone();

    let repo_filter: Option<Id> = req.repo_id.as_ref().map(Id::new);
    if repo_filter.is_some() {
        // A repo_id from another workspace must not leak its graph into
        // this prompt — validate the caller-supplied repo scope against
        // the workspace the caller is authorized for.
        validate_repo_scope(&state, &ws_id, req.repo_id.as_deref()).await?;
    }
    let (node_type_summary, node_count) =
        crate::llm_helpers::workspace_graph_summary_parts(&state, &ws_id, repo_filter.as_ref())
            .await;

    // Prompt template (ui-layout.md §2 Prompt storage): DB override →
    // specs/prompts/explorer-generate.md in the repo git tree → hardcoded
    // fallback. `template.sha` carries git provenance for the cost entry.
    let template = crate::llm_helpers::resolve_prompt_template(
        &state,
        &ws_id,
        repo_filter.as_ref(),
        "explorer-generate",
        crate::llm_defaults::PROMPT_EXPLORER_GENERATE,
    )
    .await;

    // The user's question travels as the user prompt only — never into the
    // system template (prompt-injection containment). The committed
    // template's `{{question}}` placeholder is explicitly blanked so no
    // literal placeholder text leaks into the system prompt.
    let system_prompt = crate::llm_helpers::substitute_template(
        &template.content,
        &[
            ("workspace_name", workspace_name.as_str()),
            ("node_type_summary", node_type_summary.as_str()),
            ("node_count", &node_count.to_string()),
            ("view_spec_grammar", crate::llm_helpers::VIEW_SPEC_GRAMMAR),
            ("question", ""),
        ],
    );

    // Validate the previous view definition (refinement context) against the
    // same grammar before the LLM sees it — an invalid previous spec is a
    // client bug, not something to hand the model.
    if let Some(ref prev) = req.previous_spec {
        parse_and_validate(prev)?;
        validate_repo_ownership(&state, &workspace_id, prev).await?;
    }

    // MR ID discovery (HSI §3): inject recent MRs with trace data so the LLM
    // can pick a trace_source for flow-layout questions. Same lookup the
    // has_trace filter on the workspace MR list serves.
    let recent_traced_mrs = recent_mrs_with_traces(&state, &workspace_id, 10).await;
    let user_prompt = build_generate_user_prompt(
        &req.question,
        req.previous_spec.as_ref(),
        &recent_traced_mrs,
    );

    // Model + per-endpoint max output tokens (ui-layout.md §2).
    let (model, max_tokens) =
        crate::llm_helpers::resolve_llm_model(&state, &ws_id, "explorer-generate").await;
    let result = match factory
        .for_model(&model)
        .predict_json(&system_prompt, &user_prompt, max_tokens)
        .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(model = %model, workspace_id = %workspace_id, error = ?e, "LLM predict_json failed in generate_explorer_view");
            let error_data = serde_json::to_string(&json!({
                "error": format!("LLM request failed: {e}"),
            }))
            .unwrap_or_default();
            let events: Vec<Result<Event, std::convert::Infallible>> =
                vec![Ok(Event::default().event("error").data(error_data))];
            return Ok(Sse::new(stream::iter(events)).keep_alive(
                axum::response::sse::KeepAlive::new()
                    .interval(Duration::from_secs(15))
                    .text("keep-alive"),
            ));
        }
    };
    // Budget charging: `llm_query` cost entries (ui-layout.md §2). The
    // LlmPort does not report actual usage, so estimate from prompt size
    // (~4 chars/token) plus a response overhead floor; structured view
    // generation involves multi-step reasoning, so apply the existing 3x
    // multiplier. Split into input/output for the per-call budget record
    // (platform-model.md §Budget Tracking) so that input + output equals
    // the analytics estimate above: the prompt estimate is input, the
    // remainder (overhead + reasoning multiplier) lands on the output side.
    let estimated_input = (user_prompt.len() + system_prompt.len()) / 4;
    let base_estimate = (estimated_input as f64 + 500.0) * 3.0;
    let cost_entry = CostEntry::new(
        new_id(),
        Id::new(caller.agent_id.clone()),
        None,
        "llm_query",
        base_estimate,
        "tokens",
        now_secs(),
    );
    let _ = state.costs.record(&cost_entry).await;

    // Per-call `llm_query` budget record + workspace/tenant counter
    // increment (platform-model.md §Budget Tracking), carrying the
    // prompt-template git SHA for audit (ui-layout.md §2 "git SHA recorded
    // in cost entries"). User-initiated query: agent_id/task_id are None;
    // repo_id is the validated caller-supplied repo scope, if any.
    super::budget::record_llm_call_usage(
        &state,
        &super::budget::LlmCallUsage {
            tenant_id: ws.tenant_id.clone(),
            workspace_id: ws.id.clone(),
            repo_id: repo_filter.clone(),
            agent_id: None,
            task_id: None,
            usage_type: "llm_query".to_string(),
            input_tokens: estimated_input as u64,
            output_tokens: (base_estimate - estimated_input as f64) as u64,
            cost_usd: 0.0,
            model: model.clone(),
            prompt_template_sha: template.sha.clone(),
        },
    )
    .await;

    // Spec §2: the server validates the LLM's output against the view spec
    // grammar before sending `event: complete`. An invalid spec is reported
    // as `{view_spec: null, explanation, fallback}` (the same null-view-spec
    // pattern as unanswerable questions) — NOT a 500, and NOT forwarded raw
    // to the client.
    let grammar_ok = parse_and_validate(&result).is_ok();
    let mut complete = if grammar_ok {
        json!({
            "view_spec": result.clone(),
            "explanation": generated_view_explanation(&result, &req.question),
        })
    } else {
        tracing::warn!(workspace_id = %workspace_id, "LLM produced invalid view spec");
        invalid_llm_view_response()
    };
    // Validate repo_id ownership in the generated spec too — a hallucinated
    // repo_id must not leak cross-workspace data when the view renders.
    if grammar_ok {
        if let Err(e) = validate_repo_ownership(&state, &workspace_id, &result).await {
            tracing::warn!(workspace_id = %workspace_id, error = %e, "LLM view spec referenced foreign repo_id");
            complete = invalid_llm_view_response();
        }
    }
    // Flow-layout backing (HSI §3): a "flow" view must be backed by real
    // trace data on an MR in this workspace. The grammar already requires
    // trace_source.mr_id; this enforces that the MR exists, belongs to the
    // workspace, and currently has a captured trace. Anything else falls
    // back to "graph" with an explanatory note — the client never receives
    // a flow view that cannot animate.
    if complete["view_spec"].is_object() {
        let mut spec = complete["view_spec"].clone();
        if enforce_flow_trace_backing(&state, &workspace_id, &mut spec).await {
            let fallback_note =
                "No trace data is available for this concept — showing the structural graph instead.";
            let explanation = match complete["explanation"].as_str() {
                Some(e) if !e.is_empty() => format!("{e} {fallback_note}"),
                _ => fallback_note.to_string(),
            };
            complete["explanation"] = json!(explanation);
            complete["view_spec"] = spec;
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
    Ok(Sse::new(s).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    ))
}

// ── POST /api/v1/workspaces/:id/explorer-views/ask ──────────────────────

/// Grounding caps for the ask prompt (system-explorer.md §8 "knowledge
/// graph context"): the context carries real provenance — nodes with their
/// spec linkage and commit/agent attribution, edges, and timeline deltas —
/// but bounded so a large graph cannot blow the prompt budget. Nodes are
/// prioritized by recency of last modification.
const ASK_MAX_REPOS: usize = 10;
const ASK_MAX_NODES: usize = 400;
const ASK_MAX_EDGES: usize = 300;
const ASK_MAX_DELTAS: usize = 20;
const ASK_MAX_QUESTION_LEN: usize = 4000;
const ASK_MAX_HISTORY: usize = 20;

/// Resolvable-artifact index built from the same graph data that grounds
/// the prompt — the authority `validate_entity_refs` checks citations
/// against. A ref the LLM cites that is NOT in this index does not exist
/// from the workspace's perspective and must not render as a link.
#[derive(Default)]
struct GraphGrounding {
    /// node id → node name (for labels).
    node_names: std::collections::HashMap<String, String>,
    /// Spec paths linked to nodes or referenced by timeline deltas.
    spec_paths: std::collections::HashSet<String>,
    /// Commit SHAs seen in node provenance or timeline deltas.
    commit_shas: std::collections::HashSet<String>,
    /// Agent ids seen in node provenance or timeline deltas.
    agent_ids: std::collections::HashSet<String>,
}

/// Build the knowledge-graph grounding context (§8: "provenance records,
/// spec linkage, architectural timeline") and its validation index from
/// the workspace's live graph storage. Repo-scoped when `repo_filter` is
/// given (the repo was already scope-validated by the caller).
async fn build_ask_grounding(
    state: &AppState,
    workspace_id: &Id,
    repo_filter: Option<&Id>,
) -> (String, GraphGrounding) {
    let repos: Vec<gyre_domain::Repository> = match repo_filter {
        Some(rid) => state.repos.find_by_id(rid).await.ok().flatten().into_iter().collect(),
        None => state
            .repos
            .list_by_workspace(workspace_id)
            .await
            .unwrap_or_default(),
    };

    let mut grounding = GraphGrounding::default();
    let mut node_lines: Vec<serde_json::Value> = Vec::new();
    let mut edge_lines: Vec<serde_json::Value> = Vec::new();
    let mut delta_lines: Vec<(u64, serde_json::Value)> = Vec::new();

    for repo in repos.iter().take(ASK_MAX_REPOS) {
        let nodes = state
            .graph_store
            .list_nodes(&repo.id, None)
            .await
            .unwrap_or_default();
        let mut active: Vec<&gyre_common::graph::GraphNode> =
            nodes.iter().filter(|n| n.deleted_at.is_none()).collect();
        // Most recently modified first — the freshest provenance is the
        // most likely subject of a question.
        active.sort_by_key(|n| std::cmp::Reverse(n.last_modified_at));

        let name_by_id: std::collections::HashMap<String, String> = active
            .iter()
            .map(|n| (n.id.to_string(), n.name.clone()))
            .collect();

        let node_budget = ASK_MAX_NODES.saturating_sub(node_lines.len());
        for n in active.into_iter().take(node_budget) {
            let id = n.id.to_string();
            if let Some(sp) = &n.spec_path {
                grounding.spec_paths.insert(sp.clone());
            }
            grounding.commit_shas.insert(n.created_sha.clone());
            grounding.commit_shas.insert(n.last_modified_sha.clone());
            if let Some(agent) = &n.last_modified_by {
                grounding.agent_ids.insert(agent.to_string());
            }
            grounding
                .node_names
                .insert(id.clone(), n.name.clone());
            node_lines.push(serde_json::json!({
                "id": id,
                "name": n.name,
                "qualified_name": n.qualified_name,
                "type": format!("{:?}", n.node_type),
                "spec_path": n.spec_path,
                "created_sha": n.created_sha,
                "created_at": n.created_at,
                "last_modified_sha": n.last_modified_sha,
                "last_modified_by": n.last_modified_by.as_ref().map(|a| a.to_string()),
                "last_modified_at": n.last_modified_at,
            }));
        }

        let edges = state
            .graph_store
            .list_edges(&repo.id, None)
            .await
            .unwrap_or_default();
        let edge_budget = ASK_MAX_EDGES.saturating_sub(edge_lines.len());
        for e in edges
            .iter()
            .filter(|e| e.deleted_at.is_none())
            .take(edge_budget)
        {
            let src = name_by_id
                .get(&e.source_id.to_string())
                .cloned()
                .unwrap_or_else(|| "?".to_string());
            let tgt = name_by_id
                .get(&e.target_id.to_string())
                .cloned()
                .unwrap_or_else(|| "?".to_string());
            edge_lines.push(serde_json::json!({
                "source": src,
                "target": tgt,
                "type": format!("{:?}", e.edge_type).to_lowercase(),
            }));
        }

        let deltas = state
            .graph_store
            .list_deltas(&repo.id, None, None)
            .await
            .unwrap_or_default();
        for d in deltas {
            grounding.commit_shas.insert(d.commit_sha.clone());
            if let Some(agent) = &d.agent_id {
                grounding.agent_ids.insert(agent.to_string());
            }
            if let Some(spec) = &d.spec_ref {
                grounding.spec_paths.insert(spec.clone());
            }
            // Structural summary of the delta: names of added/removed nodes
            // (capped), parsed from the stored delta JSON.
            let added: Vec<String> = serde_json::from_str::<serde_json::Value>(&d.delta_json)
                .ok()
                .and_then(|v| {
                    v.get("nodes_added").and_then(|a| a.as_array()).map(|a| {
                        a.iter()
                            .filter_map(|n| {
                                n.get("name").and_then(|x| x.as_str()).map(str::to_string)
                            })
                            .take(6)
                            .collect()
                    })
                })
                .unwrap_or_default();
            delta_lines.push((
                d.timestamp,
                serde_json::json!({
                    "commit_sha": d.commit_sha,
                    "timestamp": d.timestamp,
                    "spec_ref": d.spec_ref,
                    "agent_id": d.agent_id.map(|a| a.to_string()),
                    "nodes_added": added,
                }),
            ));
        }
    }

    // Most recent timeline events first, capped.
    delta_lines.sort_by_key(|(ts, _)| std::cmp::Reverse(*ts));
    let delta_values: Vec<serde_json::Value> = delta_lines
        .into_iter()
        .take(ASK_MAX_DELTAS)
        .map(|(_, v)| v)
        .collect();

    let mut context = String::new();
    context.push_str("Nodes (id, name, qualified_name, type, spec_path, provenance — created/modified commits and agents):\n");
    for line in &node_lines {
        let _ = serde_json::to_string(line).map(|l| {
            context.push_str(&l);
            context.push('\n');
        });
    }
    context.push_str("\nEdges (source name → target name, type):\n");
    for line in &edge_lines {
        let _ = serde_json::to_string(line).map(|l| {
            context.push_str(&l);
            context.push('\n');
        });
    }
    context.push_str("\nArchitectural timeline (most recent structural changes):\n");
    for line in &delta_values {
        let _ = serde_json::to_string(line).map(|l| {
            context.push_str(&l);
            context.push('\n');
        });
    }
    if node_lines.is_empty() {
        context.push_str("(no graph nodes indexed for this scope)\n");
    }

    (context, grounding)
}

/// Short SHA label for a commit citation. Char-based (never byte slices at
/// fixed indexes — a SHA is ASCII, but the rule is mechanical).
fn short_sha(sha: &str) -> String {
    sha.chars().take(7).collect()
}

/// Validate the `[entity:<kind>:<id>]` tokens in an LLM answer against the
/// grounding index, returning the cleaned answer plus the validated refs
/// in order of first appearance.
///
/// Grounding requirement (§8): every claim must reference a concrete
/// artifact. A token whose id does not resolve against the real graph is a
/// hallucinated citation — it is reduced to its bare id text (readable
/// prose, no link) and excluded from `entity_refs`, so the UI can never
/// render a clickable reference to an artifact that does not exist.
/// Validated tokens stay in the text verbatim; the frontend parses the
/// same token syntax to render them as links.
fn validate_entity_refs(answer: &str, grounding: &GraphGrounding) -> (String, Vec<EntityRef>) {
    let mut cleaned = String::with_capacity(answer.len());
    let mut refs: Vec<EntityRef> = Vec::new();
    let mut seen: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    let mut rest = answer;
    const PREFIX: &str = "[entity:";
    while let Some(start) = rest.find(PREFIX) {
        cleaned.push_str(&rest[..start]);
        let after_prefix = &rest[start + PREFIX.len()..];
        match (after_prefix.find(':'), after_prefix.find(']')) {
            (Some(colon), Some(close)) if colon < close => {
                let kind = &after_prefix[..colon];
                let id = &after_prefix[colon + 1..close];
                let resolved = match kind {
                    "node" => grounding
                        .node_names
                        .get(id)
                        .map(|name| EntityRef {
                            kind: "node".to_string(),
                            id: id.to_string(),
                            label: name.clone(),
                        }),
                    "spec" => grounding.spec_paths.get(id).map(|_| EntityRef {
                        kind: "spec".to_string(),
                        id: id.to_string(),
                        label: id.to_string(),
                    }),
                    "commit" => grounding.commit_shas.get(id).map(|_| EntityRef {
                        kind: "commit".to_string(),
                        id: id.to_string(),
                        label: short_sha(id),
                    }),
                    "agent" => grounding.agent_ids.get(id).map(|_| EntityRef {
                        kind: "agent".to_string(),
                        id: id.to_string(),
                        label: id.to_string(),
                    }),
                    _ => None,
                };
                match resolved {
                    Some(r) => {
                        // Keep the token verbatim — the frontend renders it
                        // as a clickable link using the ref metadata.
                        cleaned.push_str(&rest[start..start + PREFIX.len() + close + 1]);
                        if seen.insert((r.kind.clone(), r.id.clone())) {
                            refs.push(r);
                        }
                    }
                    None => {
                        // Unresolvable citation: bare id as plain text.
                        cleaned.push_str(id);
                    }
                }
                rest = &after_prefix[close + 1..];
            }
            _ => {
                // Malformed token (no closing bracket / no kind colon) —
                // keep the literal text and continue scanning after it.
                cleaned.push_str(PREFIX);
                rest = after_prefix;
            }
        }
    }
    cleaned.push_str(rest);
    (cleaned, refs)
}

/// Resolve the repo a conversational prediction should run against: the
/// request's repo scope when given, else the workspace's single repo.
/// `None` when the workspace has zero or several repos and no explicit
/// scope — a prediction without a grounding repo is skipped (the text
/// answer still carries the analysis).
async fn resolve_prediction_repo(
    state: &AppState,
    workspace_id: &Id,
    repo_filter: Option<&Id>,
) -> Result<Option<Id>, ApiError> {
    if let Some(rid) = repo_filter {
        return Ok(Some(rid.clone()));
    }
    let repos = state
        .repos
        .list_by_workspace(workspace_id)
        .await
        .map_err(ApiError::Internal)?;
    Ok(if repos.len() == 1 {
        Some(repos[0].id.clone())
    } else {
        None
    })
}

/// Conversational exploration (system-explorer.md §8): natural-language
/// Q&A grounded in the knowledge graph. The LLM synthesizes an answer
/// from real provenance (nodes with spec linkage + commit/agent
/// attribution, edges, timeline deltas); the server validates every
/// `[entity:<kind>:<id>]` citation against the same graph before the
/// client sees it. Responses can carry canvas actions: a view spec for
/// "show me X" questions and a structural prediction for "what would
/// happen if…" questions.
pub async fn ask_explorer(
    State(state): State<Arc<AppState>>,
    Path(workspace_id): Path<String>,
    caller: AuthenticatedAgent,
    Json(req): Json<AskExplorerRequest>,
) -> Result<Json<AskExplorerResponse>, ApiError> {
    check_workspace_membership(&state, &workspace_id, &caller).await?;
    // Per-user/workspace sliding-window rate limit (ui-layout.md §2):
    // 10 req/60 s, keyed on the auth-context user_id — same limiter and
    // principal rule as the generate endpoint.
    {
        let mut limiter = state.llm_rate_limiter.lock().await;
        if let Err(retry_after) = check_rate_limit(
            &mut limiter,
            &crate::llm_rate_limit::rate_limit_principal(&caller),
            &workspace_id,
            LLM_RATE_LIMIT,
            LLM_WINDOW_SECS,
        ) {
            return Err(ApiError::RateLimited(retry_after));
        }
    }

    // ── Request validation (before any LLM budget is spent) ───────────
    let question = req.question.trim();
    if question.is_empty() {
        return Err(ApiError::BadRequest("question must not be empty".to_string()));
    }
    if question.len() > ASK_MAX_QUESTION_LEN {
        return Err(ApiError::BadRequest(format!(
            "question exceeds {ASK_MAX_QUESTION_LEN} character limit"
        )));
    }
    let history = req.history.unwrap_or_default();
    // §8 follow-up context / HSI §1325 rule: history is capped at 20
    // entries — the server REJECTS more (the client drops older turns).
    if history.len() > ASK_MAX_HISTORY {
        return Err(ApiError::InvalidInput(
            "history must contain at most 20 entries".to_string(),
        ));
    }
    for entry in &history {
        if entry.role != "user" && entry.role != "assistant" {
            return Err(ApiError::BadRequest(
                "history entries must have role \"user\" or \"assistant\"".to_string(),
            ));
        }
        if entry.content.trim().is_empty() {
            return Err(ApiError::BadRequest(
                "history entries must not be empty".to_string(),
            ));
        }
        if entry.content.len() > ASK_MAX_QUESTION_LEN {
            return Err(ApiError::BadRequest(
                "history entry exceeds character limit".to_string(),
            ));
        }
    }

    // Require LLM to be configured.
    let factory = state.llm.as_ref().ok_or(ApiError::LlmUnavailable)?;

    let ws_id = Id::new(&workspace_id);
    let ws = state
        .workspaces
        .find_by_id(&ws_id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {} not found", workspace_id)))?;

    let repo_filter: Option<Id> = req.repo_id.as_ref().map(Id::new);
    if repo_filter.is_some() {
        // A repo_id from another workspace must not leak its graph into
        // this prompt — validate the caller-supplied repo scope against
        // the workspace the caller is authorized for.
        validate_repo_scope(&state, &ws_id, req.repo_id.as_deref()).await?;
    }

    // ── Grounding (§8: provenance, spec linkage, timeline) ────────────
    let (node_type_summary, node_count) =
        crate::llm_helpers::workspace_graph_summary_parts(&state, &ws_id, repo_filter.as_ref())
            .await;
    let (graph_context, grounding) =
        build_ask_grounding(&state, &ws_id, repo_filter.as_ref()).await;

    // Prompt template (ui-layout.md §2 Prompt storage): DB override →
    // specs/prompts/explorer-ask.md in the repo git tree → hardcoded
    // fallback. `template.sha` carries git provenance for the cost entry.
    let template = crate::llm_helpers::resolve_prompt_template(
        &state,
        &ws_id,
        repo_filter.as_ref(),
        "explorer-ask",
        crate::llm_defaults::PROMPT_EXPLORER_ASK,
    )
    .await;

    // The user's question and the conversation history travel as the user
    // prompt only — never into the system template (prompt-injection
    // containment). The committed template's `{{question}}` placeholder is
    // explicitly blanked so no literal placeholder text leaks.
    let system_prompt = crate::llm_helpers::substitute_template(
        &template.content,
        &[
            ("workspace_name", ws.name.as_str()),
            ("node_type_summary", node_type_summary.as_str()),
            ("node_count", &node_count.to_string()),
            ("graph_context", graph_context.as_str()),
            ("view_spec_grammar", crate::llm_helpers::VIEW_SPEC_GRAMMAR),
            ("question", ""),
        ],
    );
    let mut user_prompt = String::new();
    if !history.is_empty() {
        user_prompt.push_str("Conversation so far (the user's follow-up builds on it):\n");
        for entry in &history {
            user_prompt.push_str(&entry.role);
            user_prompt.push_str(": ");
            user_prompt.push_str(entry.content.trim());
            user_prompt.push('\n');
        }
        user_prompt.push('\n');
    }
    user_prompt.push_str("Question: ");
    user_prompt.push_str(question);

    // Model + per-endpoint max output tokens (ui-layout.md §2).
    let (model, max_tokens) =
        crate::llm_helpers::resolve_llm_model(&state, &ws_id, "explorer-ask").await;
    let result = match factory
        .for_model(&model)
        .predict_json(&system_prompt, &user_prompt, max_tokens)
        .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(model = %model, workspace_id = %workspace_id, error = ?e, "LLM predict_json failed in ask_explorer");
            return Err(ApiError::Internal(anyhow::anyhow!(
                "LLM request failed: {e}"
            )));
        }
    };

    // Budget charging: `llm_query` cost entries (same estimation and 3x
    // reasoning multiplier as the generate endpoint — structured Q&A with
    // graph context is multi-step reasoning).
    let estimated_input = (user_prompt.len() + system_prompt.len()) / 4;
    let base_estimate = (estimated_input as f64 + 500.0) * 3.0;
    let cost_entry = CostEntry::new(
        new_id(),
        Id::new(caller.agent_id.clone()),
        None,
        "llm_query",
        base_estimate,
        "tokens",
        now_secs(),
    );
    let _ = state.costs.record(&cost_entry).await;
    super::budget::record_llm_call_usage(
        &state,
        &super::budget::LlmCallUsage {
            tenant_id: ws.tenant_id.clone(),
            workspace_id: ws.id.clone(),
            repo_id: repo_filter.clone(),
            agent_id: None,
            task_id: None,
            usage_type: "llm_query".to_string(),
            input_tokens: estimated_input as u64,
            output_tokens: (base_estimate - estimated_input as f64) as u64,
            cost_usd: 0.0,
            model: model.clone(),
            prompt_template_sha: template.sha.clone(),
        },
    )
    .await;

    // ── Answer extraction ─────────────────────────────────────────────
    // An LLM output without an `answer` (or `explanation`) string is the
    // same failure class as the generate endpoint's invalid spec: report
    // it in-band, never forward raw model output to the chat.
    let answer = result
        .get("answer")
        .and_then(|v| v.as_str())
        .or_else(|| result.get("explanation").and_then(|v| v.as_str()))
        .map(str::to_string)
        .filter(|a| !a.trim().is_empty())
        .unwrap_or_else(|| {
            tracing::warn!(workspace_id = %workspace_id, "LLM ask response missing answer field");
            "The assistant could not produce a grounded answer — try rephrasing the question.".to_string()
        });

    // Grounding enforcement: validate every citation against the graph the
    // prompt was built from.
    let (mut answer, entity_refs) = validate_entity_refs(&answer, &grounding);

    // ── View spec ("show me X" canvas action) ─────────────────────────
    // Same validation chain as the generate endpoint: grammar, repo
    // ownership, flow-trace backing. An invalid spec is dropped (null),
    // never forwarded to the canvas.
    let mut view_spec: Option<serde_json::Value> = None;
    if result.get("view_spec").is_some_and(|v| v.is_object()) {
        let mut spec = result["view_spec"].clone();
        if parse_and_validate(&spec).is_err()
            || validate_repo_ownership(&state, &workspace_id, &spec).await.is_err()
        {
            tracing::warn!(workspace_id = %workspace_id, "LLM ask view_spec failed validation — dropping");
        } else {
            // Same enforcement as the generate endpoint: an unbacked flow
            // view is rewritten to a graph layout, and the human is told
            // why (the note rides the answer text — this endpoint has no
            // separate explanation field).
            if enforce_flow_trace_backing(&state, &workspace_id, &mut spec).await {
                answer = format!(
                    "{answer} No trace data is available for this concept — showing the structural graph instead."
                );
            }
            view_spec = Some(spec);
        }
    }

    // ── Structural prediction ("what would happen if…") ──────────────
    // Runs the same prediction core as POST /repos/:id/graph/predict
    // over the resolved repo scope; best-effort — a prediction failure
    // degrades to the text answer, it does not fail the request.
    let mut prediction: Option<AskPrediction> = None;
    if result.get("predict").and_then(|v| v.as_bool()).unwrap_or(false) {
        match resolve_prediction_repo(&state, &ws_id, repo_filter.as_ref()).await? {
            Some(repo_id) => {
                let instruction = format!(
                    "Predict the structural impact of this hypothetical change:\n\n{question}"
                );
                match super::graph::run_structural_prediction(
                    &state, &ws_id, &repo_id, &instruction, None,
                )
                .await
                {
                    Ok(predictions) => {
                        prediction = Some(AskPrediction {
                            repo_id: repo_id.to_string(),
                            predictions,
                        });
                    }
                    Err(e) => {
                        tracing::warn!(workspace_id = %workspace_id, error = ?e, "conversational structural prediction failed");
                    }
                }
            }
            None => {
                tracing::warn!(
                    workspace_id = %workspace_id,
                    "conversational prediction requested but no single repo scope resolvable"
                );
            }
        }
    }

    Ok(Json(AskExplorerResponse {
        answer,
        entity_refs,
        view_spec,
        prediction,
    }))
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

/// One MR with trace data, injected into the generate prompt for flow-layout
/// `trace_source` selection (HSI §3 "MR ID discovery for trace_source").
#[derive(Serialize)]
struct MrWithTrace {
    mr_id: String,
    title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    spec_ref: Option<String>,
    status: String,
}

/// Recent MRs in the workspace that have captured gate traces, newest first.
/// This is the same `get_by_mr` lookup that serves `GET /merge-requests/:id/trace`
/// (and the `?has_trace=true` filter on the workspace MR list), so the list
/// the LLM sees cannot drift from what the trace endpoint would return.
async fn recent_mrs_with_traces(
    state: &AppState,
    workspace_id: &str,
    limit: usize,
) -> Vec<MrWithTrace> {
    let mrs = match state
        .merge_requests
        .list_by_workspace(&Id::new(workspace_id))
        .await
    {
        Ok(mrs) => mrs,
        Err(e) => {
            tracing::warn!(workspace_id = %workspace_id, error = %e, "MR discovery lookup failed");
            return Vec::new();
        }
    };
    let mut mrs = mrs;
    // "Recent MRs" ordering: newest updated first.
    mrs.sort_by_key(|m| std::cmp::Reverse(m.updated_at));
    let mut traced = Vec::new();
    for mr in mrs {
        let has_trace = state
            .traces
            .get_by_mr(&mr.id)
            .await
            .map(|t| t.is_some())
            .unwrap_or(false);
        if !has_trace {
            continue;
        }
        traced.push(MrWithTrace {
            mr_id: mr.id.to_string(),
            title: mr.title,
            spec_ref: mr.spec_ref,
            status: serde_json::to_value(mr.status)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default(),
        });
        if traced.len() >= limit {
            break;
        }
    }
    traced
}

/// Build the user prompt for view generation. The user's question is raw
/// input, so it is framed as data after the server-derived context blocks
/// (MR list, previous view definition) — never concatenated into the system
/// prompt.
fn build_generate_user_prompt(
    question: &str,
    previous_spec: Option<&serde_json::Value>,
    traced_mrs: &[MrWithTrace],
) -> String {
    let mut prompt = String::new();
    if traced_mrs.is_empty() {
        prompt.push_str(
            "Merge requests with trace data: none. Do not use layout \"flow\" — no trace data is available.\n\n",
        );
    } else {
        prompt.push_str(
            "Merge requests with trace data (the only valid mr_id values for a \"flow\" layout trace_source; match the user's question by title or spec_ref):\n",
        );
        for mr in traced_mrs {
            match serde_json::to_string(mr) {
                Ok(line) => {
                    prompt.push_str(&line);
                    prompt.push('\n');
                }
                Err(e) => {
                    tracing::warn!(mr_id = %mr.mr_id, error = %e, "MR prompt serialization failed")
                }
            }
        }
        prompt.push('\n');
    }
    if let Some(prev) = previous_spec {
        match serde_json::to_string_pretty(prev) {
            Ok(prev_json) => {
                prompt.push_str(
                    "Previous view definition (the user's follow-up question refines this view — keep its subject, adjust data/layout/filters to answer it):\n",
                );
                prompt.push_str(&prev_json);
                prompt.push_str("\n\n");
            }
            Err(e) => tracing::warn!(error = %e, "previous_spec serialization failed"),
        }
    }
    prompt.push_str("User question: ");
    prompt.push_str(question);
    prompt
}

/// The explanation sent with `event: complete` — the LLM's own prose from the
/// view spec's `explanation` (HSI §3: the view renders with the LLM's
/// explanation as a sidebar annotation), falling back to `description`, then
/// to a generic string so the field is always present.
fn generated_view_explanation(llm_output: &serde_json::Value, question: &str) -> String {
    for field in ["explanation", "description"] {
        if let Some(text) = llm_output.get(field).and_then(|v| v.as_str()) {
            if !text.trim().is_empty() {
                return text.to_string();
            }
        }
    }
    format!("Generated view for: {question}")
}

/// Server-side enforcement of the flow-layout trace backing (HSI §3): a
/// `"flow"` layout is only forwarded when `data.trace_source.mr_id` names an
/// MR that (a) exists, (b) belongs to this workspace, and (c) currently has
/// a captured trace. Otherwise the spec is rewritten in place to a `"graph"`
/// layout with `trace_source` removed — the "fall back to graph layout with
/// explanation" the spec mandates when no trace data is available. Returns
/// whether a fallback rewrite happened.
async fn enforce_flow_trace_backing(
    state: &AppState,
    workspace_id: &str,
    spec: &mut serde_json::Value,
) -> bool {
    if spec.get("layout").and_then(|l| l.as_str()) != Some("flow") {
        return false;
    }
    let mr_id = spec
        .pointer("/data/trace_source/mr_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let backed = match &mr_id {
        Some(mr_id) => {
            match state.merge_requests.find_by_id(&Id::new(mr_id)).await {
                Ok(Some(mr)) if mr.workspace_id.as_str() == workspace_id => state
                    .traces
                    .get_by_mr(&mr.id)
                    .await
                    .map(|t| t.is_some())
                    .unwrap_or(false),
                Ok(Some(_)) => {
                    // MR exists but belongs to another workspace — treat as
                    // unbacked; do not confirm cross-workspace existence.
                    tracing::warn!(
                        workspace_id = %workspace_id,
                        mr_id = %mr_id,
                        "flow trace_source referenced MR in another workspace"
                    );
                    false
                }
                Ok(None) => false,
                Err(e) => {
                    tracing::warn!(mr_id = %mr_id, error = %e, "flow trace_source MR lookup failed");
                    false
                }
            }
        }
        None => false,
    };
    if backed {
        return false;
    }
    tracing::warn!(
        workspace_id = %workspace_id,
        mr_id = ?mr_id,
        "flow view without trace backing — falling back to graph layout"
    );
    if let Some(obj) = spec.as_object_mut() {
        obj.insert("layout".to_string(), json!("graph"));
        if let Some(data) = obj.get_mut("data").and_then(|d| d.as_object_mut()) {
            data.remove("trace_source");
        }
    }
    true
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
    use super::{
        build_generate_user_prompt, generated_view_explanation, recent_mrs_with_traces,
        short_sha, validate_entity_refs, GraphGrounding, MrWithTrace,
    };
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
        // 7 system default views should be seeded.
        assert_eq!(arr.len(), 7);
        let names: Vec<&str> = arr.iter().map(|v| v["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"Architecture Overview"));
        assert!(names.contains(&"Test Coverage Gaps"));
        assert!(names.contains(&"High-Risk Untested Code"));
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

    /// App + state with a registered workspace in the system caller's
    /// tenant (`default`) so grounding's workspace lookup resolves.
    async fn app_with_workspace(ws: &str) -> axum::Router {
        let state = crate::mem::test_state();
        state
            .workspaces
            .create(&gyre_domain::Workspace::new(
                gyre_common::Id::new(ws),
                gyre_common::Id::new("default"),
                ws,
                ws,
                0,
            ))
            .await
            .unwrap();
        crate::build_router(state)
    }

    #[tokio::test]
    async fn generate_explorer_view_streams_sse() {
        let app = app_with_workspace("ws-4").await;
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

    /// App with a mock LLM returning a fixed response and a registered
    /// workspace, sharing state so budget calls are inspectable.
    async fn app_with_llm_and_ws(response: &str) -> (axum::Router, Arc<crate::AppState>, String) {
        let state = crate::mem::test_state();
        let ws_id = "ws-gen-tpl".to_string();
        state
            .workspaces
            .create(&gyre_domain::Workspace::new(
                gyre_common::Id::new(&ws_id),
                gyre_common::Id::new("default"),
                "gen-ws",
                "gen-ws",
                0,
            ))
            .await
            .unwrap();
        let mut s = (*state).clone();
        s.llm = Some(Arc::new(gyre_adapters::MockLlmPortFactory {
            inner: Arc::new(gyre_adapters::MockLlmAdapter::new(response)),
        }));
        let s = Arc::new(s);
        (crate::build_router(s.clone()), s, ws_id)
    }

    #[tokio::test]
    async fn generate_explorer_view_invalid_spec_emits_null_view_spec_and_fallback() {
        // ui-layout.md §2: an LLM view spec that fails grammar validation is
        // NOT an error event — complete carries view_spec: null plus a
        // list-layout fallback.
        let (app, _state, ws_id) =
            app_with_llm_and_ws(&serde_json::json!({"nonsense": true}).to_string()).await;

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/workspaces/{ws_id}/explorer-views/generate"
                    ))
                    .header("Authorization", auth())
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"question":"How does auth work?"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = String::from_utf8(bytes.to_vec()).unwrap();

        // Find the complete event and check its payload.
        let complete = body
            .lines()
            .find(|l| l.starts_with("data:") && l.contains("\"view_spec\""))
            .expect("complete event with view_spec");
        let payload: serde_json::Value =
            serde_json::from_str(complete.trim_start_matches("data:").trim()).unwrap();
        assert!(payload["view_spec"].is_null(), "invalid spec must null");
        assert_eq!(
            payload["explanation"],
            "Generated view was invalid — try rephrasing"
        );
        assert_eq!(payload["fallback"]["layout"], "list");
    }

    #[tokio::test]
    async fn generate_explorer_view_valid_spec_completes_with_view_spec() {
        // A grammar-valid spec must pass through as view_spec with no
        // fallback — kills the inverse bug of always nulling.
        let valid = serde_json::json!({
            "name": "Auth view",
            "description": "d",
            "data": {"node_types": ["Type"], "edge_types": [], "depth": 1},
            "layout": "graph"
        });
        let (app, _state, ws_id) = app_with_llm_and_ws(&valid.to_string()).await;

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/workspaces/{ws_id}/explorer-views/generate"
                    ))
                    .header("Authorization", auth())
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"question":"How does auth work?"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = String::from_utf8(bytes.to_vec()).unwrap();
        let complete = body
            .lines()
            .find(|l| l.starts_with("data:") && l.contains("\"view_spec\""))
            .expect("complete event");
        let payload: serde_json::Value =
            serde_json::from_str(complete.trim_start_matches("data:").trim()).unwrap();
        assert_eq!(payload["view_spec"]["name"], "Auth view");
        assert!(payload.get("fallback").map(|f| f.is_null()).unwrap_or(true));
    }

    /// Like `app_with_llm_and_ws`, but the LLM always fails — for testing
    /// the `event: error` branch of view generation (predict_json errors on
    /// connection failure; there is no mid-stream mode on this endpoint).
    async fn app_with_failing_llm_and_ws() -> (axum::Router, String) {
        let state = crate::mem::test_state();
        let ws_id = "ws-gen-connfail".to_string();
        state
            .workspaces
            .create(&gyre_domain::Workspace::new(
                gyre_common::Id::new(&ws_id),
                gyre_common::Id::new("default"),
                "gen-connfail-ws",
                "gen-connfail-ws",
                0,
            ))
            .await
            .unwrap();
        let mut s = (*state).clone();
        s.llm = Some(Arc::new(gyre_adapters::MockLlmPortFactory {
            inner: Arc::new(gyre_adapters::MockLlmAdapter::failing(
                "connection refused by mock",
            )),
        }));
        (crate::build_router(Arc::new(s)), ws_id)
    }

    #[tokio::test]
    async fn generate_explorer_view_llm_connection_failure_sends_error_event() {
        // ui-layout.md §2: an LLM connection failure is an SSE `error` event,
        // not an HTTP 500 — and never a `complete` event with a view_spec.
        let (app, ws_id) = app_with_failing_llm_and_ws().await;

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/workspaces/{ws_id}/explorer-views/generate"
                    ))
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

        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = String::from_utf8(bytes.to_vec()).unwrap();

        let mut error_msgs: Vec<String> = Vec::new();
        let mut complete_count = 0usize;
        let mut current_event = String::new();
        for line in body.lines() {
            if let Some(evt) = line.strip_prefix("event:") {
                current_event = evt.trim().to_string();
            } else if let Some(data) = line.strip_prefix("data:") {
                if current_event == "error" {
                    let v: serde_json::Value = serde_json::from_str(data.trim()).unwrap();
                    error_msgs.push(v["error"].as_str().unwrap_or("").to_string());
                } else if current_event == "complete" {
                    complete_count += 1;
                }
                current_event.clear();
            }
        }
        assert_eq!(error_msgs.len(), 1, "expected exactly one error event");
        assert_eq!(
            complete_count, 0,
            "a connection failure must never masquerade as a complete event"
        );
        assert!(
            error_msgs[0].contains("connection refused by mock"),
            "error event must carry the failure cause, got: {}",
            error_msgs[0]
        );
    }

    #[tokio::test]
    async fn generate_explorer_view_charges_llm_query_budget_record() {
        // ui-layout.md §2: every generate call appends an `llm_query`
        // BudgetCallRecord. With no git-tree template in the workspace, the
        // prompt_template_sha is None (hardcoded fallback).
        let valid = serde_json::json!({
            "name": "v", "description": "d",
            "data": {"node_types": [], "edge_types": [], "depth": 1},
            "layout": "list"
        });
        let (app, state, ws_id) = app_with_llm_and_ws(&valid.to_string()).await;

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/workspaces/{ws_id}/explorer-views/generate"
                    ))
                    .header("Authorization", auth())
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"question":"q?"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();

        let records = state
            .budget_calls
            .list_by_workspace(&ws_id, 0, 10)
            .await
            .unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].usage_type, "llm_query");
        assert!(records[0].input_tokens + records[0].output_tokens > 0);
    }

    #[tokio::test]
    async fn git_tree_template_injects_view_spec_grammar_into_system_prompt() {
        // ui-layout.md §2 Available Data / Output Format: "The server sends
        // the LLM: the question, the list of available node types and counts
        // in the workspace, and the view spec grammar schema." The git-tree
        // template (specs/prompts/explorer-generate.md — the spec's primary
        // prompt-storage path) must declare {{view_spec_grammar}} and the
        // handler must substitute it. Grounded end-to-end: the committed
        // template is committed into a real workspace repo's git tree, the
        // real handler runs, and a mock LLM that records its system prompt
        // proves what actually reached the model — no mirrored substitution
        // list. (Review 9fc567ff: the placeholder was previously absent from
        // the committed template, so the git-tree path never sent the
        // grammar.)
        //
        // include_str! so the test always exercises the exact committed
        // template bytes, never a copy.
        const COMMITTED_TEMPLATE: &str =
            include_str!("../../../../specs/prompts/explorer-generate.md");
        // Guards the template side: the placeholder must be declared.
        assert!(COMMITTED_TEMPLATE.contains("{{view_spec_grammar}}"));

        use gyre_ports::GitOpsPort as _;
        let dir = tempfile::TempDir::new().unwrap();
        let repo_path = dir.path().join("repo.git");
        let git = gyre_adapters::Git2OpsAdapter::new();
        git.init_bare(repo_path.to_str().unwrap()).await.unwrap();
        git.create_initial_commit(repo_path.to_str().unwrap(), "main")
            .await
            .unwrap();
        git.write_file(
            repo_path.to_str().unwrap(),
            "main",
            "specs/prompts/explorer-generate.md",
            COMMITTED_TEMPLATE.as_bytes(),
            "add prompt template",
        )
        .await
        .unwrap();

        let state = crate::mem::test_state();
        let ws_id = gyre_common::Id::new("ws-gen-grammar");
        state
            .workspaces
            .create(&gyre_domain::Workspace::new(
                ws_id.clone(),
                gyre_common::Id::new("default"),
                "grammar-ws",
                "grammar-ws",
                0,
            ))
            .await
            .unwrap();
        state
            .repos
            .create(&gyre_domain::Repository::new(
                gyre_common::Id::new("repo-gen-grammar"),
                ws_id.clone(),
                "grammar-repo",
                repo_path.to_str().unwrap().to_string(),
                0,
            ))
            .await
            .unwrap();

        let mock = Arc::new(gyre_adapters::MockLlmAdapter::json_response(
            serde_json::json!({"nonsense": true}),
        ));
        let mut s = (*state).clone();
        s.git_ops = Arc::new(git);
        s.llm = Some(Arc::new(gyre_adapters::MockLlmPortFactory {
            inner: mock.clone(),
        }));
        let app = crate::build_router(Arc::new(s));

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-gen-grammar/explorer-views/generate")
                    .header("Authorization", auth())
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"question":"q?"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        // Drain the SSE body so the handler runs to completion.
        axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();

        // The repo must stay on disk for the duration of the request; the
        // state holds only the path string.
        std::mem::forget(dir);

        let captured = mock.captured_system_prompts.lock().unwrap();
        assert_eq!(captured.len(), 1, "exactly one LLM call expected");
        let system_prompt = &captured[0];
        // Guards the handler side: the grammar schema actually reaches the
        // LLM through the git-tree template path.
        assert!(
            system_prompt.contains(crate::llm_helpers::VIEW_SPEC_GRAMMAR),
            "git-tree path must inject the view spec grammar schema into the \
             system prompt"
        );
        // No literal placeholder text may leak into the prompt.
        assert!(
            !system_prompt.contains("{{"),
            "unsubstituted template variables reached the LLM: {system_prompt}"
        );
    }

    #[tokio::test]
    async fn generate_explorer_view_rejects_foreign_repo_id() {
        // A repo_id belonging to another workspace must not leak its graph
        // into this prompt — 403, not a grounded generation.
        let valid = serde_json::json!({
            "name": "v", "description": "d",
            "data": {"node_types": [], "edge_types": [], "depth": 1},
            "layout": "list"
        });
        let (app, state, ws_id) = app_with_llm_and_ws(&valid.to_string()).await;

        // Repo in a DIFFERENT workspace.
        let other_ws = gyre_common::Id::new("ws-other");
        state
            .workspaces
            .create(&gyre_domain::Workspace::new(
                other_ws.clone(),
                gyre_common::Id::new("default"),
                "other-ws",
                "other-ws",
                0,
            ))
            .await
            .unwrap();
        let repo = gyre_domain::Repository::new(
            gyre_common::Id::new("repo-foreign"),
            other_ws,
            "foreign-repo",
            "/nonexistent",
            0,
        );
        state.repos.create(&repo).await.unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/workspaces/{ws_id}/explorer-views/generate"
                    ))
                    .header("Authorization", auth())
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"question":"q?","repo_id":"repo-foreign"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn generate_explorer_view_rate_limited_after_10_requests() {
        let app = app_with_workspace("ws-rl").await;
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

    async fn app_with_llm_response(response: serde_json::Value, ws_id: &str) -> axum::Router {
        let state = crate::mem::test_state();
        // The generate handler grounds the prompt in the real workspace
        // (name, graph summary), so the workspace must exist.
        state
            .workspaces
            .create(&gyre_domain::Workspace::new(
                gyre_common::Id::new(ws_id),
                gyre_common::Id::new("default"),
                ws_id,
                ws_id,
                0,
            ))
            .await
            .unwrap();
        let mut s = (*state).clone();
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
        let app = app_with_llm_response(
            serde_json::json!({
                "name": "LLM output",
                "data": {"node_types": [], "edge_types": [], "depth": 1},
                "layout": "flow"
            }),
            "ws-inv",
        )
        .await;
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
            payload["explanation"],
            "Generated view was invalid — try rephrasing"
        );
        assert_eq!(payload["fallback"]["layout"], "list");
    }

    #[tokio::test]
    async fn generate_forwards_valid_llm_view_spec() {
        let app = app_with_llm_response(
            serde_json::json!({
                "name": "LLM output",
                "data": {"node_types": ["Module"], "edge_types": [], "depth": 2},
                "layout": "hierarchical"
            }),
            "ws-ok",
        )
        .await;
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
        state
            .workspaces
            .create(&gyre_domain::Workspace::new(
                gyre_common::Id::new("ws-leak"),
                gyre_common::Id::new("default"),
                "ws-leak",
                "ws-leak",
                0,
            ))
            .await
            .unwrap();
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
    async fn create_view_rejects_hybrid_viewquery_viewspec_payload() {
        // A payload carrying BOTH a valid ViewQuery `scope` AND ViewSpec
        // fields (`left`/`right`). The two grammars have disjoint top-level
        // field sets; serde ignores unknown fields, so a "try ViewQuery
        // first" parse order would let this hybrid ride the ViewQuery branch
        // and smuggle an unvalidated nested side-by-side into storage
        // (the exact 201-instead-of-400 bypass found in review).
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
                    .uri("/api/v1/workspaces/ws-hybrid/explorer-views")
                    .header("Authorization", auth())
                    .header("Content-Type", "application/json")
                    .body(body)
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    // ── Generated views (HSI §3, task-089) ─────────────────────────────────

    /// Seed an MR (with optional captured trace) directly into the test
    /// state's repositories. Bypasses the HTTP create path so the workspace
    /// under test need not exist in the workspace repo (agent/system tokens
    /// pass membership checks for unknown workspaces).
    async fn seed_workspace(state: &crate::AppState, workspace_id: &str) {
        // The generate handler grounds the prompt in the real workspace
        // (name, graph summary), so the workspace must exist.
        if state
            .workspaces
            .find_by_id(&gyre_common::Id::new(workspace_id))
            .await
            .unwrap()
            .is_some()
        {
            return;
        }
        state
            .workspaces
            .create(&gyre_domain::Workspace::new(
                gyre_common::Id::new(workspace_id),
                gyre_common::Id::new("default"),
                workspace_id,
                workspace_id,
                0,
            ))
            .await
            .unwrap();
    }

    async fn seed_mr(
        state: &crate::AppState,
        mr_id: &str,
        workspace_id: &str,
        title: &str,
        with_trace: bool,
    ) {
        let mut mr = gyre_domain::MergeRequest::new(
            gyre_common::Id::new(mr_id),
            gyre_common::Id::new("repo-gen"),
            title,
            "feat/x",
            "main",
            1000,
        );
        mr.workspace_id = gyre_common::Id::new(workspace_id);
        mr.spec_ref = Some(
            "specs/system/identity-security.md@deadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
                .to_string(),
        );
        state.merge_requests.create(&mr).await.unwrap();
        if with_trace {
            let trace = gyre_common::GateTrace {
                id: gyre_common::Id::new(format!("trace-{mr_id}")),
                mr_id: gyre_common::Id::new(mr_id),
                gate_run_id: gyre_common::Id::new("gr-gen"),
                commit_sha: "deadbeef".to_string(),
                spans: vec![gyre_common::TraceSpan {
                    span_id: format!("{mr_id}-span-1"),
                    parent_span_id: None,
                    operation_name: "POST /payments/retry".to_string(),
                    service_name: "payment-api".to_string(),
                    kind: gyre_common::SpanKind::Server,
                    start_time: 1,
                    duration_us: 100,
                    attributes: Default::default(),
                    input_summary: None,
                    output_summary: None,
                    status: gyre_common::SpanStatus::Ok,
                    graph_node_id: None,
                }],
                captured_at: 1234,
            };
            state.traces.store(&trace).await.unwrap();
        }
    }

    async fn generate(
        app: axum::Router,
        workspace: &str,
        body: &str,
    ) -> (StatusCode, serde_json::Value) {
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/workspaces/{workspace}/explorer-views/generate"
                    ))
                    .header("Authorization", auth())
                    .header("Content-Type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        if status == StatusCode::OK {
            let sse = sse_body(resp).await;
            (status, complete_event_payload(&sse))
        } else {
            (status, body_json(resp).await)
        }
    }

    #[tokio::test]
    async fn generate_flow_layout_with_backed_trace_passes_through() {
        // HSI §3: a "flow" view whose trace_source.mr_id names an MR in this
        // workspace WITH a captured trace is forwarded as flow — the client
        // animates particles along the span tree.
        let state = crate::mem::test_state();
        seed_workspace(&state, "ws-flow").await;
        seed_mr(&state, "mr-traced", "ws-flow", "Retry payment flow", true).await;
        let mut s = (*state).clone();
        s.llm = Some(Arc::new(gyre_adapters::MockLlmPortFactory {
            inner: Arc::new(gyre_adapters::MockLlmAdapter::json_response(
                serde_json::json!({
                    "name": "Payment retry flow",
                    "data": {
                        "concept": "retry",
                        "node_types": ["Function"],
                        "edge_types": ["Calls"],
                        "depth": 3,
                        "trace_source": {"mr_id": "mr-traced"}
                    },
                    "layout": "flow",
                    "explanation": "Retry requests flow from the endpoint through the gateway."
                }),
            )),
        }));
        let app = crate::build_router(Arc::new(s));

        let (status, payload) = generate(
            app,
            "ws-flow",
            r#"{"question":"How does a payment retry flow through the system?"}"#,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(payload["view_spec"]["layout"], "flow", "payload: {payload}");
        assert_eq!(
            payload["view_spec"]["data"]["trace_source"]["mr_id"],
            "mr-traced"
        );
        // The LLM's own explanation is forwarded (HSI §3 sidebar annotation).
        assert_eq!(
            payload["explanation"],
            "Retry requests flow from the endpoint through the gateway."
        );
    }

    #[tokio::test]
    async fn generate_flow_layout_without_trace_falls_back_to_graph() {
        // HSI §3: "If no matching MR has trace data, the LLM responds with a
        // graph layout" — enforced server-side, because the LLM cannot be
        // trusted to know which MRs currently hold traces. The spec's
        // trace_source.mr_id here names an MR with NO captured trace.
        let state = crate::mem::test_state();
        seed_workspace(&state, "ws-flow2").await;
        seed_mr(
            &state,
            "mr-untraced",
            "ws-flow2",
            "Retry payment flow",
            false,
        )
        .await;
        let mut s = (*state).clone();
        s.llm = Some(Arc::new(gyre_adapters::MockLlmPortFactory {
            inner: Arc::new(gyre_adapters::MockLlmAdapter::json_response(
                serde_json::json!({
                    "name": "Payment retry flow",
                    "data": {
                        "concept": "retry",
                        "node_types": ["Function"],
                        "edge_types": [],
                        "depth": 3,
                        "trace_source": {"mr_id": "mr-untraced"}
                    },
                    "layout": "flow",
                    "explanation": "Retry path through the system."
                }),
            )),
        }));
        let app = crate::build_router(Arc::new(s));

        let (status, payload) = generate(
            app,
            "ws-flow2",
            r#"{"question":"How does a payment retry flow through the system?"}"#,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            payload["view_spec"]["layout"], "graph",
            "payload: {payload}"
        );
        assert!(
            payload["view_spec"]["data"].get("trace_source").is_none(),
            "fallback must strip trace_source: {payload}"
        );
        let explanation = payload["explanation"].as_str().unwrap();
        assert!(
            explanation.contains("No trace data is available"),
            "explanation must state the fallback: {explanation}"
        );
        assert!(
            explanation.starts_with("Retry path through the system."),
            "LLM explanation is preserved, not replaced: {explanation}"
        );
    }

    #[tokio::test]
    async fn generate_flow_layout_with_foreign_mr_falls_back_to_graph() {
        // A hallucinated (or cross-workspace) mr_id must not leak the other
        // workspace's trace data — the flow view falls back to graph without
        // confirming the MR's existence in this workspace.
        let state = crate::mem::test_state();
        seed_workspace(&state, "ws-flow3").await;
        // The traced MR belongs to a DIFFERENT workspace.
        seed_mr(
            &state,
            "mr-other-ws",
            "ws-elsewhere",
            "Other workspace MR",
            true,
        )
        .await;
        let mut s = (*state).clone();
        s.llm = Some(Arc::new(gyre_adapters::MockLlmPortFactory {
            inner: Arc::new(gyre_adapters::MockLlmAdapter::json_response(
                serde_json::json!({
                    "name": "Flow",
                    "data": {
                        "node_types": ["Function"],
                        "edge_types": [],
                        "depth": 1,
                        "trace_source": {"mr_id": "mr-other-ws"}
                    },
                    "layout": "flow"
                }),
            )),
        }));
        let app = crate::build_router(Arc::new(s));

        let (status, payload) = generate(
            app,
            "ws-flow3",
            r#"{"question":"Show me the request path"}"#,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            payload["view_spec"]["layout"], "graph",
            "payload: {payload}"
        );
        assert!(
            payload["view_spec"]["data"].get("trace_source").is_none(),
            "foreign trace_source must be stripped: {payload}"
        );
    }

    #[tokio::test]
    async fn generate_rejects_invalid_previous_spec() {
        // Refinement context is validated against the same grammar before
        // the LLM sees it — an invalid previous view is a client bug.
        let app = app_with_llm_response(
            serde_json::json!({
                "name": "ok",
                "data": {"node_types": [], "edge_types": [], "depth": 1},
                "layout": "graph"
            }),
            "ws-refine",
        )
        .await;
        let (status, _payload) = generate(
            app,
            "ws-refine",
            r#"{"question":"just the middleware","previous_spec":{"layout":"flow"}}"#,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn generate_forwards_previous_spec_to_llm() {
        // The user prompt must carry the previous view definition so the LLM
        // can refine it (asserted via the echo adapter: predict_json's echo
        // response embeds the user prompt, which grammar validation then
        // rejects — but the prompt text proves the injection happened).
        let prev = serde_json::json!({
            "name": "How auth works",
            "data": {"concept": "auth", "node_types": [], "edge_types": [], "depth": 2},
            "layout": "hierarchical"
        });
        let mrs = vec![MrWithTrace {
            mr_id: "mr-7".to_string(),
            title: "Auth middleware refactor".to_string(),
            spec_ref: Some("specs/system/identity-security.md@x".to_string()),
            status: "merged".to_string(),
        }];
        let prompt = build_generate_user_prompt("Just the middleware chain", Some(&prev), &mrs);
        assert!(
            prompt.contains("mr-7"),
            "MR list must be injected for trace_source discovery: {prompt}"
        );
        assert!(
            prompt.contains("Auth middleware refactor"),
            "MR titles must be injected for matching: {prompt}"
        );
        assert!(
            prompt.contains("specs/system/identity-security.md"),
            "MR spec_refs must be injected for matching: {prompt}"
        );
        assert!(
            prompt.contains("How auth works"),
            "previous view definition must be injected for refinement: {prompt}"
        );
        assert!(
            prompt.trim_end().ends_with("Just the middleware chain"),
            "the question must terminate the prompt: {prompt}"
        );

        // No traced MRs → the prompt says so explicitly.
        let empty = build_generate_user_prompt("How does auth work?", None, &[]);
        assert!(
            empty.contains("none"),
            "empty MR list must be explicit: {empty}"
        );
        assert!(
            empty.contains("Do not use layout \"flow\""),
            "empty MR list must forbid flow layout: {empty}"
        );
    }

    #[tokio::test]
    async fn generate_explanation_falls_back_to_description_then_generic() {
        // The complete event's explanation comes from the LLM's spec:
        // explanation → description → generic (in that order).
        assert_eq!(
            generated_view_explanation(
                &serde_json::json!({"explanation": "from explanation", "description": "from desc"}),
                "q"
            ),
            "from explanation"
        );
        assert_eq!(
            generated_view_explanation(&serde_json::json!({"description": "from desc"}), "q"),
            "from desc"
        );
        assert_eq!(
            generated_view_explanation(&serde_json::json!({}), "my question"),
            "Generated view for: my question"
        );
        // Whitespace-only explanations are treated as absent.
        assert_eq!(
            generated_view_explanation(&serde_json::json!({"explanation": "  "}), "q"),
            "Generated view for: q"
        );
    }

    #[tokio::test]
    async fn recent_mrs_with_traces_only_returns_traced_and_newest_first() {
        let state = crate::mem::test_state();
        // Three MRs: two traced (different updated_at), one untraced.
        let mut old = gyre_domain::MergeRequest::new(
            gyre_common::Id::new("mr-old"),
            gyre_common::Id::new("repo-gen"),
            "Older traced MR",
            "feat/a",
            "main",
            100,
        );
        old.workspace_id = gyre_common::Id::new("ws-mrs");
        old.updated_at = 100;
        state.merge_requests.create(&old).await.unwrap();

        let mut new = gyre_domain::MergeRequest::new(
            gyre_common::Id::new("mr-new"),
            gyre_common::Id::new("repo-gen"),
            "Newer traced MR",
            "feat/b",
            "main",
            200,
        );
        new.workspace_id = gyre_common::Id::new("ws-mrs");
        new.updated_at = 200;
        state.merge_requests.create(&new).await.unwrap();

        let mut untraced = gyre_domain::MergeRequest::new(
            gyre_common::Id::new("mr-untraced"),
            gyre_common::Id::new("repo-gen"),
            "Untraced MR",
            "feat/c",
            "main",
            300,
        );
        untraced.workspace_id = gyre_common::Id::new("ws-mrs");
        untraced.updated_at = 300;
        state.merge_requests.create(&untraced).await.unwrap();

        for mr_id in ["mr-old", "mr-new"] {
            let trace = gyre_common::GateTrace {
                id: gyre_common::Id::new(format!("trace-{mr_id}")),
                mr_id: gyre_common::Id::new(mr_id),
                gate_run_id: gyre_common::Id::new("gr-x"),
                commit_sha: "abc".to_string(),
                spans: vec![],
                captured_at: 1,
            };
            state.traces.store(&trace).await.unwrap();
        }

        let traced = recent_mrs_with_traces(&state, "ws-mrs", 10).await;
        let ids: Vec<&str> = traced.iter().map(|m| m.mr_id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["mr-new", "mr-old"],
            "newest first, untraced excluded"
        );

        // limit caps the result.
        let capped = recent_mrs_with_traces(&state, "ws-mrs", 1).await;
        assert_eq!(capped.len(), 1);
        assert_eq!(capped[0].mr_id, "mr-new");

        // Another workspace's MRs are not visible.
        let other = recent_mrs_with_traces(&state, "ws-other", 10).await;
        assert!(other.is_empty());
    }

    // ── Conversational exploration (system-explorer.md §8) ─────────────

    use gyre_common::graph::{ArchitecturalDelta, GraphNode, NodeType, SpecConfidence, Visibility};
    use gyre_common::Id;

    /// Seed one graph node with full provenance (spec linkage, creation
    /// commit, last-modify commit + agent) into `repo`.
    async fn seed_ask_node(
        state: &Arc<crate::AppState>,
        repo_id: &str,
        id: &str,
        name: &str,
    ) -> String {
        let node = GraphNode {
            id: Id::new(id),
            repo_id: Id::new(repo_id),
            node_type: NodeType::Type,
            name: name.to_string(),
            qualified_name: format!("app::{name}"),
            file_path: format!("src/{name}.rs"),
            line_start: 1,
            line_end: 20,
            visibility: Visibility::Public,
            doc_comment: None,
            spec_path: Some(format!("specs/system/{name}.md")),
            spec_paths: vec![],
            spec_confidence: SpecConfidence::High,
            last_modified_sha: "def456def456def456def456def456def456def45".to_string(),
            last_modified_by: Some(Id::new("agent-worker-12")),
            last_modified_at: 200,
            created_sha: "abc123abc123abc123abc123abc123abc123abc1".to_string(),
            created_at: 100,
            complexity: None,
            churn_count_30d: 0,
            test_coverage: None,
            first_seen_at: 100,
            last_seen_at: 200,
            deleted_at: None,
            test_node: false,
            spec_approved_at: None,
            milestone_completed_at: None,
        };
        state.graph_store.create_node(node).await.unwrap();
        id.to_string()
    }

    /// App + state with a workspace ("ws-ask"), one repo, one provenance-
    /// rich node, one timeline delta, and a mock LLM returning `response`.
    /// The workspace has exactly ONE repo so the conversational prediction
    /// repo resolution has a deterministic scope.
    async fn app_with_ask_graph(
        response: serde_json::Value,
    ) -> (axum::Router, Arc<crate::AppState>, String) {
        let state = crate::mem::test_state();
        let ws_id = "ws-ask".to_string();
        state
            .workspaces
            .create(&gyre_domain::Workspace::new(
                Id::new(&ws_id),
                Id::new("default"),
                "ask-ws",
                "ask-ws",
                0,
            ))
            .await
            .unwrap();
        state
            .repos
            .create(&gyre_domain::Repository::new(
                Id::new("repo-ask"),
                Id::new(&ws_id),
                "ask-repo",
                "/tmp/ask-repo",
                0,
            ))
            .await
            .unwrap();
        seed_ask_node(&state, "repo-ask", "node-svc", "SearchService").await;
        state
            .graph_store
            .record_delta(ArchitecturalDelta {
                id: Id::new("delta-1"),
                repo_id: Id::new("repo-ask"),
                commit_sha: "fedcba fedcba".replace(' ', ""),
                timestamp: 300,
                agent_id: Some(Id::new("agent-worker-7")),
                spec_ref: Some("specs/system/realized-model.md".to_string()),
                delta_json: serde_json::json!({
                    "nodes_added": [{"name": "SearchService", "node_type": "type"}]
                })
                .to_string(),
            })
            .await
            .unwrap();

        let mock = Arc::new(gyre_adapters::MockLlmAdapter::json_response(response));
        let mut s = (*state).clone();
        s.llm = Some(Arc::new(gyre_adapters::MockLlmPortFactory {
            inner: mock.clone(),
        }));
        let app = crate::build_router(Arc::new(s));
        (app, state, ws_id)
    }

    async fn post_ask(
        app: axum::Router,
        ws_id: &str,
        body: &str,
    ) -> (StatusCode, serde_json::Value) {
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/workspaces/{ws_id}/explorer-views/ask"))
                    .header("Authorization", auth())
                    .header("Content-Type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, body)
    }

    #[tokio::test]
    async fn ask_explorer_validates_entity_refs_against_the_graph() {
        // §8 grounding: only citations that resolve to real artifacts come
        // back as entity_refs; a hallucinated id is stripped to plain text.
        let llm = serde_json::json!({
            "answer": "SearchService [entity:node:node-svc] implements the port, created in \
                       [entity:commit:abc123abc123abc123abc123abc123abc123abc1] by \
                       [entity:agent:agent-worker-12], governed by \
                       [entity:spec:specs/system/SearchService.md]. It does NOT include \
                       [entity:node:ghost-node].",
            "view_spec": null,
            "predict": false
        });
        let (app, _state, ws_id) = app_with_ask_graph(llm).await;

        let (status, body) = post_ask(app, &ws_id, r#"{"question":"Why does SearchService exist?"}"#).await;
        assert_eq!(status, StatusCode::OK);

        // Valid tokens survive verbatim (the frontend parses the same syntax).
        let answer = body["answer"].as_str().unwrap();
        assert!(answer.contains("[entity:node:node-svc]"));
        assert!(answer.contains("[entity:commit:abc123abc123abc123abc123abc123abc123abc1]"));
        assert!(answer.contains("[entity:agent:agent-worker-12]"));
        assert!(answer.contains("[entity:spec:specs/system/SearchService.md]"));
        // The hallucinated citation is stripped — bare id text, no token.
        assert!(!answer.contains("[entity:node:ghost-node]"), "hallucinated ref must be stripped: {answer}");
        assert!(answer.contains("ghost-node"));

        let refs = body["entity_refs"].as_array().unwrap();
        assert_eq!(refs.len(), 4, "exactly the resolvable refs: {refs:?}");
        let by_kind: std::collections::HashMap<&str, &serde_json::Value> = refs
            .iter()
            .map(|r| (r["type"].as_str().unwrap(), r))
            .collect();
        assert_eq!(by_kind["node"]["id"], "node-svc");
        assert_eq!(by_kind["node"]["label"], "SearchService");
        assert_eq!(by_kind["spec"]["id"], "specs/system/SearchService.md");
        assert_eq!(by_kind["commit"]["label"], "abc123a");
        assert_eq!(by_kind["agent"]["id"], "agent-worker-12");
        // No canvas actions in this answer.
        assert!(body["view_spec"].is_null());
        assert!(body["prediction"].is_null());
    }

    #[tokio::test]
    async fn ask_explorer_passes_valid_view_spec_and_drops_invalid() {
        // "Show me X" answers carry a view spec; it must pass the same
        // grammar + ownership validation as the generate endpoint.
        let valid = serde_json::json!({
            "name": "Cache implementors",
            "description": "Types implementing CachePort",
            "data": {"node_types": ["Type"], "edge_types": [], "depth": 1},
            "layout": "list"
        });
        let (app, _state, ws_id) = app_with_ask_graph(serde_json::json!({
            "answer": "Here are the implementors.",
            "view_spec": valid,
            "predict": false
        }))
        .await;
        let (status, body) = post_ask(app, &ws_id, r#"{"question":"Show me services that implement CachePort"}"#).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["view_spec"]["name"], "Cache implementors");
        assert_eq!(body["view_spec"]["layout"], "list");

        // Invalid spec → null, answer still delivered.
        let (app, _state, ws_id) = app_with_ask_graph(serde_json::json!({
            "answer": "Here are the implementors.",
            "view_spec": {"nonsense": true},
            "predict": false
        }))
        .await;
        let (status, body) = post_ask(app, &ws_id, r#"{"question":"Show me everything"}"#).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["view_spec"].is_null(), "invalid spec must be dropped");
        assert_eq!(body["answer"], "Here are the implementors.");
    }

    #[tokio::test]
    async fn ask_explorer_predict_triggers_structural_prediction() {
        // "What would happen if…" → the same prediction core as
        // POST /repos/:id/graph/predict runs over the single-repo scope.
        let (app, _state, ws_id) = app_with_ask_graph(serde_json::json!({
            "answer": "Removing the port would break SearchService.",
            "view_spec": null,
            "predict": true
        }))
        .await;
        let (status, body) =
            post_ask(app, &ws_id, r#"{"question":"What would happen if I removed CachePort?"}"#).await;
        assert_eq!(status, StatusCode::OK);
        let prediction = body.get("prediction").expect("prediction present");
        assert_eq!(prediction["repo_id"], "repo-ask");
        assert!(
            prediction["predictions"].as_array().unwrap().len() >= 1,
            "prediction payload carries predictions"
        );
    }

    #[tokio::test]
    async fn ask_explorer_grounded_prompt_reaches_llm() {
        // The knowledge-graph context (provenance, spec linkage, timeline)
        // actually reaches the LLM, fully substituted.
        let mock = Arc::new(gyre_adapters::MockLlmAdapter::json_response(
            serde_json::json!({"answer": "ok", "view_spec": null, "predict": false}),
        ));
        let state = crate::mem::test_state();
        let ws_id = "ws-ask-prompt".to_string();
        state
            .workspaces
            .create(&gyre_domain::Workspace::new(
                Id::new(&ws_id),
                Id::new("default"),
                "ask-prompt-ws",
                "ask-prompt-ws",
                0,
            ))
            .await
            .unwrap();
        state
            .repos
            .create(&gyre_domain::Repository::new(
                Id::new("repo-ask-prompt"),
                Id::new(&ws_id),
                "ask-prompt-repo",
                "/tmp/ask-prompt-repo",
                0,
            ))
            .await
            .unwrap();
        seed_ask_node(&state, "repo-ask-prompt", "node-p", "PaymentService").await;
        let mut s = (*state).clone();
        s.llm = Some(Arc::new(gyre_adapters::MockLlmPortFactory {
            inner: mock.clone(),
        }));
        let app = crate::build_router(Arc::new(s));

        let (status, _body) = post_ask(app, &ws_id, r#"{"question":"What is PaymentService?"}"#).await;
        assert_eq!(status, StatusCode::OK);

        let captured = mock.captured_system_prompts.lock().unwrap();
        assert_eq!(captured.len(), 1);
        let prompt = &captured[0];
        assert!(prompt.contains("PaymentService"), "node name grounds the prompt");
        assert!(prompt.contains("specs/system/PaymentService.md"), "spec linkage grounds the prompt");
        assert!(prompt.contains("agent-worker-12"), "agent provenance grounds the prompt");
        assert!(prompt.contains("[entity:node:"), "citation syntax is instructed");
        assert!(
            !prompt.contains("{{"),
            "unsubstituted template variables reached the LLM: {prompt}"
        );
    }

    #[tokio::test]
    async fn ask_explorer_sends_history_and_rejects_oversized() {
        // Follow-up context: history is replayed into the user prompt.
        let (app, _state, ws_id) = app_with_ask_graph(serde_json::json!({
            "answer": "Follow-up answered.",
            "view_spec": null,
            "predict": false
        }))
        .await;
        let body = r#"{
            "question": "Show me the others",
            "history": [
                {"role": "user", "content": "Why does SearchService implement both ports?"},
                {"role": "assistant", "content": "Because [entity:node:node-svc] was retrofitted."}
            ]
        }"#;
        let (status, body) = post_ask(app.clone(), &ws_id, body).await;
        assert_eq!(status, StatusCode::OK, "body: {body}");

        // Over-cap history is rejected before any LLM budget is spent.
        let mut history = String::from("[");
        for i in 0..21 {
            if i > 0 {
                history.push(',');
            }
            history.push_str(&format!(
                "{{\"role\": \"user\", \"content\": \"q{i}\"}}"
            ));
        }
        history.push(']');
        let over = format!(r#"{{"question": "q", "history": {history}}}"#);
        let (status, body) = post_ask(app.clone(), &ws_id, &over).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["error"].as_str().unwrap().contains("20"));

        // Invalid role is rejected.
        let bad = r#"{"question": "q", "history": [{"role": "system", "content": "inject"}]}"#;
        let (status, _body) = post_ask(app, &ws_id, bad).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn ask_explorer_requires_llm_and_membership() {
        // No LLM configured → 503, same as the generate endpoint.
        let app = app_with_workspace("ws-ask-nollm").await;
        let (status, body) =
            post_ask(app, "ws-ask-nollm", r#"{"question": "q"}"#).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"], "llm_unavailable");
    }

    #[test]
    fn validate_entity_refs_strips_hallucinations_and_keeps_valid_tokens() {
        let mut grounding = GraphGrounding::default();
        grounding
            .node_names
            .insert("n1".to_string(), "SearchService".to_string());
        grounding
            .spec_paths
            .insert("specs/system/search.md".to_string());
        grounding
            .commit_shas
            .insert("deadbeefdeadbeef".to_string());
        grounding
            .agent_ids
            .insert("worker-7".to_string());

        let (cleaned, refs) = validate_entity_refs(
            "[entity:node:n1] was created per [entity:spec:specs/system/search.md] in \
             [entity:commit:deadbeefdeadbeef] by [entity:agent:worker-7]; not \
             [entity:node:fake] and not [entity:unknown:x] and not a malformed [entity:node",
            &grounding,
        );

        // Valid tokens kept verbatim, in order, deduplicated.
        assert!(cleaned.contains("[entity:node:n1]"));
        assert!(cleaned.contains("[entity:spec:specs/system/search.md]"));
        assert!(cleaned.contains("[entity:commit:deadbeefdeadbeef]"));
        assert!(cleaned.contains("[entity:agent:worker-7]"));
        // Hallucinated node: bare id text, no token.
        assert!(!cleaned.contains("[entity:node:fake]"));
        assert!(cleaned.contains("fake"));
        // Unknown kind and unterminated token: literal text preserved.
        assert!(cleaned.contains("[entity:unknown:x]"));
        assert!(cleaned.contains("[entity:node"));

        let kinds: Vec<&str> = refs.iter().map(|r| r.kind.as_str()).collect();
        assert_eq!(kinds, vec!["node", "spec", "commit", "agent"]);
        assert_eq!(refs[0].label, "SearchService");
        assert_eq!(refs[2].label, "deadbee");
    }

    #[test]
    fn short_sha_never_panics_on_short_or_unicode_input() {
        assert_eq!(short_sha("deadbeefdeadbeef"), "deadbee");
        assert_eq!(short_sha("abc"), "abc");
        assert_eq!(short_sha(""), "");
        // Multi-byte chars must not panic (fixed byte slicing would).
        assert_eq!(short_sha("déadbeef"), "déadbe");
    }
}
