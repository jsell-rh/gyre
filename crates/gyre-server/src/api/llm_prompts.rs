//! LLM prompt template CRUD API.
//!
//! GET    /api/v1/workspaces/:id/llm/prompts              — list workspace overrides
//! GET    /api/v1/workspaces/:id/llm/prompts/:function    — get effective prompt (resolved)
//! PUT    /api/v1/workspaces/:id/llm/prompts/:function    — upsert workspace override
//! DELETE /api/v1/workspaces/:id/llm/prompts/:function    — delete override (revert to tenant/default)
//! GET    /api/v1/admin/llm/prompts                       — list tenant defaults (admin only)
//! PUT    /api/v1/admin/llm/prompts/:function             — upsert tenant default (admin only)

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{UserRole, LLM_FUNCTION_KEYS};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{auth::AuthenticatedAgent, AppState};

use super::error::ApiError;

// ---------------------------------------------------------------------------
// Shared types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct PromptTemplateResponse {
    pub id: String,
    pub workspace_id: Option<String>,
    pub function_key: String,
    pub content: String,
    pub is_default: bool,
    pub created_by: String,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<gyre_domain::PromptTemplate> for PromptTemplateResponse {
    fn from(t: gyre_domain::PromptTemplate) -> Self {
        let is_default = t.workspace_id.is_none();
        Self {
            id: t.id.as_str().to_string(),
            workspace_id: t.workspace_id.map(|id| id.as_str().to_string()),
            function_key: t.function_key,
            content: t.content,
            is_default,
            created_by: t.created_by.as_str().to_string(),
            created_at: t.created_at,
            updated_at: t.updated_at,
        }
    }
}

#[derive(Deserialize)]
pub struct UpsertPromptBody {
    pub content: String,
}

// ---------------------------------------------------------------------------
// GET /api/v1/workspaces/:id/llm/prompts
// ---------------------------------------------------------------------------

pub async fn list_workspace_prompts(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedAgent,
    Path(workspace_id): Path<String>,
) -> Result<Json<Vec<PromptTemplateResponse>>, ApiError> {
    let ws_id = Id::new(&workspace_id);
    let templates = state
        .prompt_templates
        .list_by_workspace(&ws_id)
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(templates.into_iter().map(Into::into).collect()))
}

// ---------------------------------------------------------------------------
// GET /api/v1/workspaces/:id/llm/prompts/:function
// ---------------------------------------------------------------------------

pub async fn get_effective_prompt(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedAgent,
    Path((workspace_id, function_key)): Path<(String, String)>,
) -> Result<Json<PromptTemplateResponse>, ApiError> {
    if !LLM_FUNCTION_KEYS.contains(&function_key.as_str()) {
        return Err(ApiError::NotFound(format!(
            "unknown function key '{}'; valid keys: {}",
            function_key,
            LLM_FUNCTION_KEYS.join(", ")
        )));
    }

    let ws_id = Id::new(&workspace_id);
    let template = state
        .prompt_templates
        .get_effective(&ws_id, &function_key)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| {
            ApiError::NotFound(format!(
                "no prompt template configured for function '{function_key}'"
            ))
        })?;

    Ok(Json(template.into()))
}

// ---------------------------------------------------------------------------
// PUT /api/v1/workspaces/:id/llm/prompts/:function
// ---------------------------------------------------------------------------

pub async fn upsert_workspace_prompt(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path((workspace_id, function_key)): Path<(String, String)>,
    Json(body): Json<UpsertPromptBody>,
) -> Result<(StatusCode, Json<PromptTemplateResponse>), ApiError> {
    if !LLM_FUNCTION_KEYS.contains(&function_key.as_str()) {
        return Err(ApiError::BadRequest(format!(
            "unknown function key '{}'; valid keys: {}",
            function_key,
            LLM_FUNCTION_KEYS.join(", ")
        )));
    }
    if body.content.trim().is_empty() {
        return Err(ApiError::BadRequest(
            "content must not be empty".to_string(),
        ));
    }

    let ws_id = Id::new(&workspace_id);
    let caller_id = Id::new(&auth.agent_id);
    let template = state
        .prompt_templates
        .upsert_workspace(&ws_id, &function_key, &body.content, &caller_id)
        .await
        .map_err(ApiError::Internal)?;

    Ok((StatusCode::OK, Json(template.into())))
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/workspaces/:id/llm/prompts/:function
// ---------------------------------------------------------------------------

pub async fn delete_workspace_prompt(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedAgent,
    Path((workspace_id, function_key)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    if !LLM_FUNCTION_KEYS.contains(&function_key.as_str()) {
        return Err(ApiError::NotFound(format!(
            "unknown function key '{}'; valid keys: {}",
            function_key,
            LLM_FUNCTION_KEYS.join(", ")
        )));
    }

    let ws_id = Id::new(&workspace_id);

    // Check override exists before deleting.
    let templates = state
        .prompt_templates
        .list_by_workspace(&ws_id)
        .await
        .map_err(ApiError::Internal)?;
    let exists = templates.iter().any(|t| t.function_key == function_key);
    if !exists {
        return Err(ApiError::NotFound(format!(
            "no workspace override for function '{function_key}'"
        )));
    }

    state
        .prompt_templates
        .delete_workspace_override(&ws_id, &function_key)
        .await
        .map_err(ApiError::Internal)?;

    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// GET /api/v1/admin/llm/prompts  (admin only)
// ---------------------------------------------------------------------------

pub async fn list_tenant_defaults(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
) -> Result<Json<Vec<PromptTemplateResponse>>, ApiError> {
    if !auth.roles.contains(&UserRole::Admin) {
        return Err(ApiError::Forbidden("admin role required".to_string()));
    }

    // Use a sentinel workspace ID that will never match a workspace override;
    // we query all templates with workspace_id IS NULL via list_by_workspace workaround.
    // Instead, get all known function keys' effective tenant defaults.
    let sentinel = Id::new("__tenant__");
    let mut results = Vec::new();
    for fkey in LLM_FUNCTION_KEYS {
        if let Some(tmpl) = state
            .prompt_templates
            .get_effective(&sentinel, fkey)
            .await
            .map_err(ApiError::Internal)?
        {
            // Only include if it's actually a tenant default (workspace_id is None).
            if tmpl.workspace_id.is_none() {
                results.push(tmpl.into());
            }
        }
    }
    Ok(Json(results))
}

// ---------------------------------------------------------------------------
// PUT /api/v1/admin/llm/prompts/:function  (admin only)
// ---------------------------------------------------------------------------

pub async fn upsert_tenant_default(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(function_key): Path<String>,
    Json(body): Json<UpsertPromptBody>,
) -> Result<(StatusCode, Json<PromptTemplateResponse>), ApiError> {
    if !auth.roles.contains(&UserRole::Admin) {
        return Err(ApiError::Forbidden("admin role required".to_string()));
    }
    if !LLM_FUNCTION_KEYS.contains(&function_key.as_str()) {
        return Err(ApiError::BadRequest(format!(
            "unknown function key '{}'; valid keys: {}",
            function_key,
            LLM_FUNCTION_KEYS.join(", ")
        )));
    }
    if body.content.trim().is_empty() {
        return Err(ApiError::BadRequest(
            "content must not be empty".to_string(),
        ));
    }

    let caller_id = Id::new(&auth.agent_id);
    let template = state
        .prompt_templates
        .upsert_tenant_default(&function_key, &body.content, &caller_id)
        .await
        .map_err(ApiError::Internal)?;

    Ok((StatusCode::OK, Json(template.into())))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{header, Method, Request},
    };
    use serde_json::{json, Value};
    use tower::ServiceExt;

    use crate::api::api_router;
    use crate::mem::test_state;

    async fn app() -> axum::Router {
        let state = test_state();
        api_router().with_state(state)
    }

    fn auth_header() -> (header::HeaderName, header::HeaderValue) {
        (
            header::AUTHORIZATION,
            header::HeaderValue::from_static("Bearer test-token"),
        )
    }

    #[tokio::test]
    async fn list_workspace_prompts_empty() {
        let app = app().await;
        let ws_id = Id::new(uuid::Uuid::new_v4().to_string());
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("/api/v1/workspaces/{}/llm/prompts", ws_id.as_str()))
                    .header(auth_header().0, auth_header().1)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(body.as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn get_effective_returns_404_for_unknown_function() {
        let app = app().await;
        let ws_id = Id::new(uuid::Uuid::new_v4().to_string());
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!(
                        "/api/v1/workspaces/{}/llm/prompts/unknown-fn",
                        ws_id.as_str()
                    ))
                    .header(auth_header().0, auth_header().1)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn put_creates_override_get_returns_it() {
        let state = test_state();
        let app = api_router().with_state(Arc::clone(&state));
        let ws_id = Id::new(uuid::Uuid::new_v4().to_string());

        // PUT workspace override.
        let put_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!(
                        "/api/v1/workspaces/{}/llm/prompts/briefing-ask",
                        ws_id.as_str()
                    ))
                    .header(auth_header().0, auth_header().1)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&json!({"content": "custom prompt"})).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(put_resp.status(), StatusCode::OK);

        // GET effective — should return workspace override.
        let get_resp = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!(
                        "/api/v1/workspaces/{}/llm/prompts/briefing-ask",
                        ws_id.as_str()
                    ))
                    .header(auth_header().0, auth_header().1)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_resp.status(), StatusCode::OK);
        let body: Value = serde_json::from_slice(
            &axum::body::to_bytes(get_resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["content"], "custom prompt");
        assert_eq!(body["is_default"], false);
    }

    #[tokio::test]
    async fn delete_removes_override() {
        let state = test_state();
        let app = api_router().with_state(Arc::clone(&state));
        let ws_id = Id::new(uuid::Uuid::new_v4().to_string());

        // Create override.
        app.clone()
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!(
                        "/api/v1/workspaces/{}/llm/prompts/graph-predict",
                        ws_id.as_str()
                    ))
                    .header(auth_header().0, auth_header().1)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&json!({"content": "ws-override"})).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Delete override.
        let del_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!(
                        "/api/v1/workspaces/{}/llm/prompts/graph-predict",
                        ws_id.as_str()
                    ))
                    .header(auth_header().0, auth_header().1)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(del_resp.status(), StatusCode::NO_CONTENT);

        // GET effective — should be 404 (no tenant default either).
        let get_resp = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!(
                        "/api/v1/workspaces/{}/llm/prompts/graph-predict",
                        ws_id.as_str()
                    ))
                    .header(auth_header().0, auth_header().1)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn put_invalid_content_returns_400() {
        let app = app().await;
        let ws_id = Id::new(uuid::Uuid::new_v4().to_string());
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!(
                        "/api/v1/workspaces/{}/llm/prompts/briefing-ask",
                        ws_id.as_str()
                    ))
                    .header(auth_header().0, auth_header().1)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&json!({"content": "   "})).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
