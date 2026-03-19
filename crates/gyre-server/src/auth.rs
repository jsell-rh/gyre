//! Token-based auth extractor for gyre-server.
//!
//! Validates `Authorization: Bearer <token>` against:
//! 1. The global `auth_token` (for system/dev use).
//! 2. Per-agent tokens stored in `agent_tokens` on registration.

use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
};
use std::sync::Arc;

use crate::AppState;

/// Injected by the auth extractor; contains the resolved agent_id (or "system").
pub struct AuthenticatedAgent {
    pub agent_id: String,
}

#[axum::async_trait]
impl FromRequestParts<Arc<AppState>> for AuthenticatedAgent {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(|| (StatusCode::UNAUTHORIZED, "Missing Bearer token").into_response())?;

        // Accept the global auth token (dev / system usage).
        if token == state.auth_token {
            return Ok(AuthenticatedAgent {
                agent_id: "system".to_string(),
            });
        }

        // Check per-agent tokens issued at registration.
        let agent_tokens = state.agent_tokens.lock().await;
        let agent_id = agent_tokens
            .iter()
            .find(|(_, t)| t.as_str() == token)
            .map(|(id, _)| id.clone())
            .ok_or_else(|| (StatusCode::UNAUTHORIZED, "Invalid token").into_response())?;

        Ok(AuthenticatedAgent { agent_id })
    }
}

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, routing::get, Router};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    use super::AuthenticatedAgent;

    async fn authenticated_handler(AuthenticatedAgent { agent_id }: AuthenticatedAgent) -> String {
        agent_id
    }

    fn app() -> Router {
        let state = test_state();
        Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state)
    }

    #[tokio::test]
    async fn missing_auth_header_returns_401() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn invalid_token_returns_401() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", "Bearer bad-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn global_auth_token_accepted() {
        let body = app()
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(body.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn agent_token_accepted() {
        let state = test_state();
        // Pre-populate an agent token.
        state
            .agent_tokens
            .lock()
            .await
            .insert("agent-42".to_string(), "agent-secret".to_string());

        let app = Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", "Bearer agent-secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(bytes.as_ref(), b"agent-42");
    }
}
