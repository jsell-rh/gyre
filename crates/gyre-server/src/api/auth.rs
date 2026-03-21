//! API key management and token introspection endpoints.
//!
//! `POST /api/v1/auth/api-keys`  — create an API key (admin only).
//! `GET  /api/v1/auth/token-info` — introspect the caller's current token (M18).

use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    auth::{AdminOnly, AuthenticatedAgent},
    AppState,
};

#[derive(Debug, Deserialize)]
pub struct CreateApiKeyRequest {
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct CreateApiKeyResponse {
    pub key: String,
    pub name: String,
}

pub async fn create_api_key(
    AdminOnly { user_id, .. }: AdminOnly,
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateApiKeyRequest>,
) -> Result<(StatusCode, Json<CreateApiKeyResponse>), (StatusCode, String)> {
    let user_id = user_id.ok_or_else(|| {
        (
            StatusCode::FORBIDDEN,
            "API key creation requires user account".to_string(),
        )
    })?;

    let key = format!("gyre_{}", Uuid::new_v4().simple());
    // Store the SHA-256 hash so raw keys are never persisted to DB.
    let key_hash = crate::auth::hash_api_key(&key);

    state
        .api_keys
        .create(&key_hash, &user_id, &req.name)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok((
        StatusCode::CREATED,
        Json(CreateApiKeyResponse {
            key, // return raw key to caller once; hash is stored
            name: req.name,
        }),
    ))
}

// -- Token introspection ------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct TokenInfoResponse {
    /// Resolved agent/user identity.
    pub subject: String,
    /// Roles granted to this token.
    pub roles: Vec<String>,
    /// Token kind: "global", "agent_jwt", "agent_uuid", "api_key", "keycloak_jwt".
    pub token_kind: String,
    /// JWT-specific claims, present when token_kind is "agent_jwt".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwt_claims: Option<crate::auth::AgentJwtClaims>,
}

/// GET /api/v1/auth/token-info
///
/// Introspects the caller's current auth token, returning the resolved identity
/// and — for agent JWTs — the full decoded claims.
pub async fn token_info(
    auth: AuthenticatedAgent,
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> Result<Json<TokenInfoResponse>, (StatusCode, String)> {
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");

    let (token_kind, jwt_claims) = if token == state.auth_token {
        ("global".to_string(), None)
    } else if token.starts_with("ey") {
        match state.agent_signing_key.validate(token, &state.base_url) {
            Ok(claims) => ("agent_jwt".to_string(), Some(claims)),
            Err(_) => ("keycloak_jwt".to_string(), None),
        }
    } else if token.starts_with("gyre_") {
        ("api_key".to_string(), None)
    } else {
        ("agent_uuid".to_string(), None)
    };

    Ok(Json(TokenInfoResponse {
        subject: auth.agent_id,
        roles: auth.roles.iter().map(|r| r.as_str().to_string()).collect(),
        token_kind,
        jwt_claims,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
    use axum::{body::Body, routing::post, Router};
    use gyre_common::Id;
    use gyre_domain::{User, UserRole};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    #[allow(dead_code)]
    fn app_with_jwt() -> Router {
        let state = make_test_state_with_jwt();
        Router::new()
            .route("/api/v1/auth/api-keys", post(create_api_key))
            .with_state(state)
    }

    #[tokio::test]
    async fn admin_can_create_api_key() {
        let state = make_test_state_with_jwt();
        let now = 1000u64;
        let mut user = User::new(Id::new("u-admin"), "admin-sub-1", "admin-user", now);
        user.roles = vec![UserRole::Admin];
        state.users.create(&user).await.unwrap();

        let claims = serde_json::json!({
            "sub": "admin-sub-1",
            "preferred_username": "admin-user",
            "realm_access": { "roles": ["admin"] }
        });
        let token = sign_test_jwt(&claims, 3600);

        let app: Router = Router::new()
            .route("/api/v1/auth/api-keys", post(create_api_key))
            .with_state(state);

        let body = serde_json::json!({ "name": "ci-pipeline" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/api-keys")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(json["key"].as_str().unwrap().starts_with("gyre_"));
        assert_eq!(json["name"], "ci-pipeline");
    }

    #[tokio::test]
    async fn non_admin_cannot_create_api_key() {
        let state = make_test_state_with_jwt();
        let now = 1000u64;
        let user = User::new(Id::new("u-dev"), "dev-sub-1", "dev-user", now);
        state.users.create(&user).await.unwrap();

        let claims = serde_json::json!({
            "sub": "dev-sub-1",
            "preferred_username": "dev-user",
            "realm_access": { "roles": ["developer"] }
        });
        let token = sign_test_jwt(&claims, 3600);

        let app: Router = Router::new()
            .route("/api/v1/auth/api-keys", post(create_api_key))
            .with_state(state);

        let body = serde_json::json!({ "name": "ci-pipeline" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/api-keys")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn system_token_can_create_api_key() {
        // "system" resolves to agent_id="system" with no user_id, so this should fail
        // because the endpoint requires a user_id (you must have an account).
        let state = make_test_state_with_jwt();
        let app: Router = Router::new()
            .route("/api/v1/auth/api-keys", post(create_api_key))
            .with_state(state);

        let body = serde_json::json!({ "name": "system-key" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/api-keys")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        // system is admin so passes AdminOnly, but has no user_id → 403
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
