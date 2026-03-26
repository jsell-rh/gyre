//! LLM function configuration API (LLM integration §4).
//!
//! GET    /api/v1/workspaces/:id/llm/config              — list workspace overrides
//! GET    /api/v1/workspaces/:id/llm/config/:function    — get effective config (resolved)
//! PUT    /api/v1/workspaces/:id/llm/config/:function    — upsert workspace override
//! DELETE /api/v1/workspaces/:id/llm/config/:function    — delete workspace override
//! GET    /api/v1/admin/llm/config                       — list tenant defaults (Admin only)
//! PUT    /api/v1/admin/llm/config/:function             — set tenant default (Admin only)

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{is_valid_function_key, UserRole};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{auth::AuthenticatedAgent, AppState};

use super::error::ApiError;

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct LlmFunctionConfigResponse {
    pub id: String,
    pub workspace_id: Option<String>,
    pub function_key: String,
    pub model_name: String,
    pub max_tokens: Option<u32>,
    pub updated_by: String,
    pub updated_at: u64,
    /// True when this config came from the tenant default (not a workspace override).
    pub is_default: bool,
}

impl From<gyre_domain::LlmFunctionConfig> for LlmFunctionConfigResponse {
    fn from(cfg: gyre_domain::LlmFunctionConfig) -> Self {
        let is_default = cfg.workspace_id.is_none();
        Self {
            id: cfg.id.to_string(),
            workspace_id: cfg.workspace_id.map(|id| id.to_string()),
            function_key: cfg.function_key,
            model_name: cfg.model_name,
            max_tokens: cfg.max_tokens,
            updated_by: cfg.updated_by.to_string(),
            updated_at: cfg.updated_at,
            is_default,
        }
    }
}

/// Effective config response — always includes is_default and falls back to env/hardcoded.
#[derive(Serialize)]
pub struct EffectiveLlmConfigResponse {
    pub function_key: String,
    pub model_name: String,
    pub max_tokens: Option<u32>,
    pub is_default: bool,
}

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct UpsertLlmConfigRequest {
    pub model_name: String,
    pub max_tokens: Option<u32>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/v1/workspaces/:id/llm/config
/// List all workspace-level model config overrides.
pub async fn list_workspace_llm_configs(
    _auth: AuthenticatedAgent,
    Path(workspace_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<LlmFunctionConfigResponse>>, ApiError> {
    let ws_id = Id::new(workspace_id);
    let cfgs = state
        .llm_configs
        .list_by_workspace(&ws_id)
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(cfgs.into_iter().map(Into::into).collect()))
}

/// GET /api/v1/workspaces/:id/llm/config/:function
/// Get effective resolved config for a function (workspace override → tenant default → env var → hardcoded).
pub async fn get_effective_llm_config(
    _auth: AuthenticatedAgent,
    Path((workspace_id, function_key)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<EffectiveLlmConfigResponse>, ApiError> {
    if !is_valid_function_key(&function_key) {
        return Err(ApiError::BadRequest(format!(
            "unknown function key '{}'; valid keys: graph-predict, briefing-ask, specs-assist, explorer-generate",
            function_key
        )));
    }
    let ws_id = Id::new(workspace_id);
    let (model_name, max_tokens, is_default) = match state
        .llm_configs
        .get_effective(&ws_id, &function_key)
        .await
        .map_err(ApiError::Internal)?
    {
        Some(cfg) => {
            let is_default = cfg.workspace_id.is_none();
            (cfg.model_name, cfg.max_tokens, is_default)
        }
        None => {
            let model = std::env::var("GYRE_LLM_MODEL")
                .unwrap_or_else(|_| crate::llm_helpers::DEFAULT_LLM_MODEL.to_string());
            (model, None, true)
        }
    };
    Ok(Json(EffectiveLlmConfigResponse {
        function_key,
        model_name,
        max_tokens,
        is_default,
    }))
}

/// PUT /api/v1/workspaces/:id/llm/config/:function
/// Upsert a workspace-level model override.
pub async fn put_workspace_llm_config(
    auth: AuthenticatedAgent,
    Path((workspace_id, function_key)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
    Json(body): Json<UpsertLlmConfigRequest>,
) -> Result<Json<LlmFunctionConfigResponse>, ApiError> {
    if !is_valid_function_key(&function_key) {
        return Err(ApiError::BadRequest(format!(
            "unknown function key '{}'; valid keys: graph-predict, briefing-ask, specs-assist, explorer-generate",
            function_key
        )));
    }
    if body.model_name.is_empty() {
        return Err(ApiError::BadRequest(
            "model_name must not be empty".to_string(),
        ));
    }
    let ws_id = Id::new(workspace_id);
    let updated_by = Id::new(auth.agent_id.clone());
    let cfg = state
        .llm_configs
        .upsert_workspace(
            &ws_id,
            &function_key,
            &body.model_name,
            body.max_tokens,
            &updated_by,
        )
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(cfg.into()))
}

/// DELETE /api/v1/workspaces/:id/llm/config/:function
/// Delete workspace override (204 No Content).
pub async fn delete_workspace_llm_config(
    _auth: AuthenticatedAgent,
    Path((workspace_id, function_key)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, ApiError> {
    if !is_valid_function_key(&function_key) {
        return Err(ApiError::BadRequest(format!(
            "unknown function key '{}'; valid keys: graph-predict, briefing-ask, specs-assist, explorer-generate",
            function_key
        )));
    }
    let ws_id = Id::new(workspace_id);
    state
        .llm_configs
        .delete_workspace_override(&ws_id, &function_key)
        .await
        .map_err(ApiError::Internal)?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/admin/llm/config
/// List tenant-level defaults (Admin role required).
pub async fn list_tenant_llm_defaults(
    auth: AuthenticatedAgent,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<LlmFunctionConfigResponse>>, ApiError> {
    if !auth.roles.contains(&UserRole::Admin) {
        return Err(ApiError::forbidden("admin role required"));
    }
    let cfgs = state
        .llm_configs
        .list_tenant_defaults()
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(cfgs.into_iter().map(Into::into).collect()))
}

/// PUT /api/v1/admin/llm/config/:function
/// Set tenant-level default for a function (Admin role required).
pub async fn put_tenant_llm_default(
    auth: AuthenticatedAgent,
    Path(function_key): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(body): Json<UpsertLlmConfigRequest>,
) -> Result<Json<LlmFunctionConfigResponse>, ApiError> {
    if !auth.roles.contains(&UserRole::Admin) {
        return Err(ApiError::forbidden("admin role required"));
    }
    if !is_valid_function_key(&function_key) {
        return Err(ApiError::BadRequest(format!(
            "unknown function key '{}'; valid keys: graph-predict, briefing-ask, specs-assist, explorer-generate",
            function_key
        )));
    }
    if body.model_name.is_empty() {
        return Err(ApiError::BadRequest(
            "model_name must not be empty".to_string(),
        ));
    }
    let updated_by = Id::new(auth.agent_id.clone());
    let cfg = state
        .llm_configs
        .upsert_tenant_default(
            &function_key,
            &body.model_name,
            body.max_tokens,
            &updated_by,
        )
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(cfg.into()))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{Method, Request, StatusCode},
    };
    use tower::ServiceExt;

    use crate::mem::test_state;

    fn app() -> axum::Router {
        let state = test_state();
        crate::build_router(state)
    }

    fn auth_header() -> (&'static str, &'static str) {
        ("authorization", "Bearer test-token")
    }

    #[tokio::test]
    async fn list_workspace_llm_configs_empty() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/workspaces/ws-1/llm/config")
                    .header(auth_header().0, auth_header().1)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json.as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn put_and_get_effective_config() {
        let state = test_state();
        let router = crate::build_router(state.clone());

        // PUT workspace override.
        let put_resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri("/api/v1/workspaces/ws-1/llm/config/briefing-ask")
                    .header(auth_header().0, auth_header().1)
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"model_name":"gemini-1.5-pro-002","max_tokens":4096}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(put_resp.status(), StatusCode::OK);

        // GET effective should return the override.
        let get_resp = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/workspaces/ws-1/llm/config/briefing-ask")
                    .header(auth_header().0, auth_header().1)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(get_resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["model_name"], "gemini-1.5-pro-002");
        assert_eq!(json["is_default"], false);
    }

    #[tokio::test]
    async fn get_effective_returns_default_when_no_config() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/workspaces/ws-1/llm/config/graph-predict")
                    .header(auth_header().0, auth_header().1)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        // No config set — should return hardcoded default or GYRE_LLM_MODEL.
        assert!(json["model_name"].is_string());
        assert_eq!(json["is_default"], true);
    }

    #[tokio::test]
    async fn delete_reverts_to_default() {
        let state = test_state();
        let router = crate::build_router(state.clone());

        // Set override.
        router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri("/api/v1/workspaces/ws-1/llm/config/specs-assist")
                    .header(auth_header().0, auth_header().1)
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"model_name":"gemini-1.5-pro-002"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        // DELETE override.
        let del_resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri("/api/v1/workspaces/ws-1/llm/config/specs-assist")
                    .header(auth_header().0, auth_header().1)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(del_resp.status(), StatusCode::NO_CONTENT);

        // GET effective should return default now.
        let get_resp = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/workspaces/ws-1/llm/config/specs-assist")
                    .header(auth_header().0, auth_header().1)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(get_resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["is_default"], true);
    }

    #[tokio::test]
    async fn invalid_function_key_returns_400() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/workspaces/ws-1/llm/config/not-a-real-function")
                    .header(auth_header().0, auth_header().1)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
