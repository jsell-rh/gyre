//! OIDC discovery endpoints (M18).
//!
//! Gyre acts as its own OIDC provider for agent tokens, exposing:
//! - `GET /.well-known/openid-configuration` — standard discovery document
//! - `GET /.well-known/jwks.json` — public Ed25519 key for JWT verification

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde_json::json;
use std::sync::Arc;

use crate::AppState;

/// GET /.well-known/openid-configuration
///
/// Returns a standard OIDC discovery document. External verifiers can use this
/// to discover the JWKS URI and validate agent JWTs issued by Gyre.
pub async fn openid_configuration(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let issuer = &state.base_url;
    let doc = json!({
        "issuer": issuer,
        "jwks_uri": format!("{issuer}/.well-known/jwks.json"),
        "token_endpoint": format!("{issuer}/api/v1/auth/token"),
        "response_types_supported": ["token"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["EdDSA"],
        "token_endpoint_auth_methods_supported": ["none"],
        "scopes_supported": ["agent"],
        "claims_supported": ["sub", "iss", "iat", "exp", "scope", "task_id", "spawned_by"]
    });
    (StatusCode::OK, Json(doc))
}

/// GET /.well-known/jwks.json
///
/// Returns the public Ed25519 key in JWK Set format. Third-party verifiers
/// can use this key to validate agent JWTs without contacting Gyre.
pub async fn jwks(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let jwks: serde_json::Value = serde_json::from_str(&state.agent_signing_key.jwks_json)
        .unwrap_or_else(|_| json!({"keys": []}));
    (StatusCode::OK, Json(jwks))
}
