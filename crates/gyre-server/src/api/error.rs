use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

pub enum ApiError {
    NotFound(String),
    InvalidInput(String),
    BadRequest(String),
    Forbidden(String),
    /// Resource conflict (HTTP 409).
    Conflict(String),
    TooManyRequests(String),
    /// Rate limit exceeded; carries `retry_after` seconds for the `Retry-After` header.
    RateLimited(u64),
    /// LLM features are disabled (GYRE_VERTEX_PROJECT not configured).
    LlmUnavailable,
    Internal(anyhow::Error),
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

impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        ApiError::Internal(err)
    }
}
