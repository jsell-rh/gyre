//! Role-based access control extractors for gyre-server.
//!
//! Role hierarchy (highest to lowest):
//!   Admin > Developer > Agent > ReadOnly
//!
//! Each extractor first resolves the authenticated user via [`AuthenticatedAgent`],
//! then checks whether the user's roles satisfy the required permission level.
//! Returns 403 Forbidden with `{"error":"insufficient permissions"}` on failure.

use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use gyre_domain::UserRole;
use serde_json::json;
use std::sync::Arc;

use crate::{auth::AuthenticatedAgent, AppState};

#[allow(dead_code)]
fn forbidden() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(json!({"error": "insufficient permissions"})),
    )
        .into_response()
}

/// Requires Admin or Developer role.
#[allow(dead_code)]
pub struct RequireDeveloper(pub AuthenticatedAgent);

#[axum::async_trait]
impl FromRequestParts<Arc<AppState>> for RequireDeveloper {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let auth = AuthenticatedAgent::from_request_parts(parts, state).await?;
        let ok = auth.agent_id == "system"
            || auth.roles.contains(&UserRole::Admin)
            || auth.roles.contains(&UserRole::Developer);
        if ok {
            Ok(RequireDeveloper(auth))
        } else {
            Err(forbidden())
        }
    }
}

/// Requires Admin, Developer, or Agent role.
#[allow(dead_code)]
pub struct RequireAgent(pub AuthenticatedAgent);

#[axum::async_trait]
impl FromRequestParts<Arc<AppState>> for RequireAgent {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let auth = AuthenticatedAgent::from_request_parts(parts, state).await?;
        let ok = auth.agent_id == "system"
            || auth.roles.contains(&UserRole::Admin)
            || auth.roles.contains(&UserRole::Developer)
            || auth.roles.contains(&UserRole::Agent);
        if ok {
            Ok(RequireAgent(auth))
        } else {
            Err(forbidden())
        }
    }
}

/// Requires any authenticated user (ReadOnly and above).
#[allow(dead_code)]
pub struct RequireReadOnly(pub AuthenticatedAgent);

#[axum::async_trait]
impl FromRequestParts<Arc<AppState>> for RequireReadOnly {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let auth = AuthenticatedAgent::from_request_parts(parts, state).await?;
        Ok(RequireReadOnly(auth))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt},
        mem::test_state,
    };
    use axum::{body::Body, http::StatusCode, routing::get, Router};
    use http::Request;
    use tower::ServiceExt;

    use super::*;

    async fn dev_handler(_: RequireDeveloper) -> StatusCode {
        StatusCode::OK
    }

    async fn agent_handler(_: RequireAgent) -> StatusCode {
        StatusCode::OK
    }

    fn app_with_jwt() -> Router {
        let state = make_test_state_with_jwt();
        Router::new()
            .route("/dev", get(dev_handler))
            .route("/agent", get(agent_handler))
            .with_state(state)
    }

    fn admin_jwt() -> String {
        sign_test_jwt(
            &serde_json::json!({
                "sub": "admin-sub",
                "preferred_username": "admin-user",
                "realm_access": { "roles": ["admin"] }
            }),
            3600,
        )
    }

    fn developer_jwt() -> String {
        sign_test_jwt(
            &serde_json::json!({
                "sub": "dev-sub",
                "preferred_username": "dev-user",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        )
    }

    fn agent_jwt() -> String {
        sign_test_jwt(
            &serde_json::json!({
                "sub": "agent-sub",
                "preferred_username": "agent-user",
                "realm_access": { "roles": ["agent"] }
            }),
            3600,
        )
    }

    fn readonly_jwt() -> String {
        sign_test_jwt(
            &serde_json::json!({
                "sub": "ro-sub",
                "preferred_username": "readonly-user",
                "realm_access": { "roles": ["readonly"] }
            }),
            3600,
        )
    }

    #[tokio::test]
    async fn admin_can_access_developer_endpoint() {
        let resp = app_with_jwt()
            .oneshot(
                Request::builder()
                    .uri("/dev")
                    .header("Authorization", format!("Bearer {}", admin_jwt()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn developer_can_access_developer_endpoint() {
        let resp = app_with_jwt()
            .oneshot(
                Request::builder()
                    .uri("/dev")
                    .header("Authorization", format!("Bearer {}", developer_jwt()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn agent_cannot_access_developer_endpoint() {
        let resp = app_with_jwt()
            .oneshot(
                Request::builder()
                    .uri("/dev")
                    .header("Authorization", format!("Bearer {}", agent_jwt()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn readonly_cannot_access_developer_endpoint() {
        let resp = app_with_jwt()
            .oneshot(
                Request::builder()
                    .uri("/dev")
                    .header("Authorization", format!("Bearer {}", readonly_jwt()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn agent_can_access_agent_endpoint() {
        let resp = app_with_jwt()
            .oneshot(
                Request::builder()
                    .uri("/agent")
                    .header("Authorization", format!("Bearer {}", agent_jwt()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn readonly_cannot_access_agent_endpoint() {
        let resp = app_with_jwt()
            .oneshot(
                Request::builder()
                    .uri("/agent")
                    .header("Authorization", format!("Bearer {}", readonly_jwt()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn global_token_can_access_developer_endpoint() {
        let state = test_state();
        let app = Router::new()
            .route("/dev", get(dev_handler))
            .with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/dev")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn no_auth_returns_401_on_developer_endpoint() {
        let resp = app_with_jwt()
            .oneshot(Request::builder().uri("/dev").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
