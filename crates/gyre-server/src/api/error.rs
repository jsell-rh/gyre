use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

#[derive(Debug)]
pub enum ApiError {
    NotFound(String),
    InvalidInput(String),
    BadRequest(String),
    Forbidden(String),
    /// Resource conflict (HTTP 409).
    Conflict(String),
    TooManyRequests(String),
    /// Unsupported media type (HTTP 415).
    UnsupportedMediaType(String),
    /// Rate limit exceeded; carries `retry_after` seconds for the `Retry-After` header.
    RateLimited(u64),
    /// LLM features are disabled (GYRE_VERTEX_PROJECT not configured).
    LlmUnavailable,
    /// Keyless commit signing refused: the caller has no presentable OIDC
    /// identity token (identity-security.md §Layer 3, task-264). HTTP 403.
    CommitSigningRefused(String),
    /// The configured commit-signing stack failed; no signature was
    /// produced and no silent local-key fallback ran (task-264). The
    /// message names the active signing mode. HTTP 502.
    CommitSigningFailed(String),
    Internal(anyhow::Error),
}

impl From<crate::commit_signatures::CommitSigningError> for ApiError {
    fn from(e: crate::commit_signatures::CommitSigningError) -> Self {
        match e {
            crate::commit_signatures::CommitSigningError::NoOidcIdentity(reason) => {
                ApiError::CommitSigningRefused(reason)
            }
            crate::commit_signatures::CommitSigningError::SigningStackFailed { .. } => {
                ApiError::CommitSigningFailed(e.describe())
            }
        }
    }
}

impl ApiError {
    pub fn forbidden(msg: impl Into<String>) -> Self {
        ApiError::Forbidden(msg.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if let ApiError::RateLimited(retry_after) = self {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                [("Retry-After", retry_after.to_string())],
                Json(json!({
                    "error": "rate limit exceeded",
                    "retry_after": retry_after
                })),
            )
                .into_response();
        }
        if let ApiError::LlmUnavailable = self {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "error": "llm_unavailable",
                    "message": "LLM features are not configured on this server. Contact your administrator.",
                    "hint": "Set GYRE_VERTEX_PROJECT and GOOGLE_APPLICATION_CREDENTIALS environment variables."
                })),
            )
                .into_response();
        }
        let (status, message) = match self {
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            ApiError::InvalidInput(msg) | ApiError::BadRequest(msg) => {
                (StatusCode::BAD_REQUEST, msg)
            }
            ApiError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg),
            ApiError::Conflict(msg) => (StatusCode::CONFLICT, msg),
            ApiError::UnsupportedMediaType(msg) => (StatusCode::UNSUPPORTED_MEDIA_TYPE, msg),
            ApiError::CommitSigningRefused(msg) => (StatusCode::FORBIDDEN, msg),
            ApiError::CommitSigningFailed(msg) => (StatusCode::BAD_GATEWAY, msg),
            ApiError::TooManyRequests(msg) => (StatusCode::TOO_MANY_REQUESTS, msg),
            ApiError::Internal(err) => {
                tracing::error!("internal error: {err:#}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal server error".to_string(),
                )
            }
            ApiError::RateLimited(_) | ApiError::LlmUnavailable => unreachable!(),
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::NotFound(m)
            | ApiError::InvalidInput(m)
            | ApiError::BadRequest(m)
            | ApiError::Forbidden(m)
            | ApiError::Conflict(m)
            | ApiError::TooManyRequests(m)
            | ApiError::UnsupportedMediaType(m)
            | ApiError::CommitSigningRefused(m)
            | ApiError::CommitSigningFailed(m) => f.write_str(m),
            ApiError::RateLimited(secs) => write!(f, "rate limit exceeded, retry after {secs}s"),
            ApiError::LlmUnavailable => f.write_str("llm unavailable"),
            ApiError::Internal(err) => write!(f, "internal error: {err:#}"),
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        ApiError::Internal(err)
    }
}
