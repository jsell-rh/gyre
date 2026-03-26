//! GET /api/v1/conversations/:sha — Conversation provenance retrieval (HSI §5).
//!
//! This endpoint uses per-handler auth rather than the ABAC middleware because
//! `:sha` is not a UUID — workspace_id must be resolved from the conversation
//! metadata before ABAC can be evaluated.

use axum::{
    body::Body,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use gyre_common::Id;
use std::sync::Arc;

use crate::{auth::AuthenticatedAgent, AppState};

/// GET /api/v1/conversations/:sha
///
/// Returns the decompressed conversation blob (JSON bytes).
/// Auth: per-handler. Resolves workspace_id from conversation metadata,
/// then evaluates ABAC (caller must have `resource_type=workspace, action=read` access).
pub async fn get_conversation(
    State(state): State<Arc<AppState>>,
    Path(sha): Path<String>,
    auth: AuthenticatedAgent,
) -> Response {
    let tenant_id = Id::new(&auth.tenant_id);

    // Resolve workspace_id from conversation metadata (needed for ABAC).
    let meta = match state.conversations.get_metadata(&sha, &tenant_id).await {
        Ok(Some(m)) => m,
        Ok(None) => {
            return (StatusCode::NOT_FOUND, "conversation not found").into_response();
        }
        Err(e) => {
            tracing::error!(sha = %sha, error = %e, "failed to get conversation metadata");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response();
        }
    };
    let (_agent_id, workspace_id) = meta;

    // Per-handler ABAC: if caller has JWT claims, evaluate workspace read access.
    if let Some(claims) = &auth.jwt_claims {
        if let Err(reason) =
            crate::abac::check_workspace_abac_for_read(&state, workspace_id.as_str(), claims).await
        {
            return (StatusCode::FORBIDDEN, reason).into_response();
        }
    }
    // Global token / API key (no jwt_claims) → bypass ABAC.

    // Fetch and return decompressed bytes.
    match state.conversations.get(&sha, &tenant_id).await {
        Ok(Some(bytes)) => Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "application/octet-stream")
            .header("X-Gyre-Conversation-Sha", &sha)
            .body(Body::from(bytes))
            .unwrap(),
        Ok(None) => (StatusCode::NOT_FOUND, "conversation not found").into_response(),
        Err(e) => {
            tracing::error!(sha = %sha, error = %e, "failed to retrieve conversation");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}
