//! Federation API (G11) — cross-instance JWT verification via OIDC discovery.
//!
//! Exposes the list of trusted remote Gyre instances configured via
//! `GYRE_TRUSTED_ISSUERS`. Callers can use this to discover which external
//! Gyre instances' agent JWTs are accepted by this server.

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde_json::json;
use std::sync::Arc;

use crate::AppState;

/// GET /api/v1/federation/trusted-issuers
///
/// Returns the list of trusted remote Gyre base URLs whose agent JWTs are
/// accepted by this server's auth middleware.
///
/// Response: `{"trusted_issuers": ["https://gyre-2.example.com", ...]}`
pub async fn list_trusted_issuers(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({
            "trusted_issuers": state.trusted_issuers,
        })),
    )
}
