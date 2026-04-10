//! Key binding endpoints (authorization-provenance.md §2.3, TASK-006, TASK-047).
//!
//! `POST /api/v1/auth/key-binding` — Bind an ephemeral Ed25519 public key to a user identity.
//! `DELETE /api/v1/auth/key-binding/:id` — Revoke a specific key binding by hex-encoded public key.
//! `DELETE /api/v1/auth/key-bindings` — Revoke all active key bindings for the authenticated user.
//!
//! The client sends the public key + a self-signature; the server verifies the user's
//! IdP session is valid, countersigns the binding, and stores the public key.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_domain::UserRole;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

use crate::{api::error::ApiError, auth::AuthenticatedAgent, AppState};

// ── Request / Response types ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateKeyBindingRequest {
    /// Base64-encoded Ed25519 public key bytes.
    pub public_key: String,
    /// User's self-signature over the binding document (base64-encoded).
    pub user_signature: String,
    /// Requested TTL in seconds (server may cap this).
    #[serde(default = "default_ttl")]
    pub ttl_secs: u64,
}

fn default_ttl() -> u64 {
    3600
}

#[derive(Debug, Serialize)]
pub struct KeyBindingResponse {
    pub public_key: String,
    pub user_identity: String,
    pub issuer: String,
    pub trust_anchor_id: String,
    pub issued_at: u64,
    pub expires_at: u64,
    pub platform_countersign: String,
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Derive user_identity string from auth context.
/// Agent JWTs produce "agent:<id>"; all other auth methods produce "user:<id>".
fn derive_user_identity(auth: &AuthenticatedAgent) -> String {
    if auth.jwt_claims.is_some() {
        format!("agent:{}", auth.agent_id)
    } else {
        format!("user:{}", auth.agent_id)
    }
}

// ── Handler ─────────────────────────────────────────────────────────────────

/// POST /api/v1/auth/key-binding
///
/// Binds an ephemeral Ed25519 public key to the caller's authenticated identity.
/// Requires a valid user session (JWT or API key — not agent JWT).
/// The server countersigns the binding as a timestamp witness.
pub async fn create_key_binding(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Json(req): Json<CreateKeyBindingRequest>,
) -> Result<(StatusCode, Json<KeyBindingResponse>), ApiError> {
    // Decode base64 public key.
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;

    let public_key_bytes = STANDARD.decode(&req.public_key).map_err(|_| {
        ApiError::InvalidInput("public_key must be valid base64-encoded bytes".to_string())
    })?;

    // Ed25519 public keys are exactly 32 bytes.
    if public_key_bytes.len() != 32 {
        return Err(ApiError::InvalidInput(
            "public_key must be exactly 32 bytes (Ed25519)".to_string(),
        ));
    }

    let user_signature_bytes = STANDARD.decode(&req.user_signature).map_err(|_| {
        ApiError::InvalidInput("user_signature must be valid base64-encoded bytes".to_string())
    })?;

    // Verify the user's self-signature proves ownership of the private key (§2.3 step 3).
    // The user signs the public key bytes as a proof-of-possession.
    {
        use ring::signature::{self, UnparsedPublicKey};
        let peer_public_key = UnparsedPublicKey::new(&signature::ED25519, &public_key_bytes);
        peer_public_key
            .verify(&public_key_bytes, &user_signature_bytes)
            .map_err(|_| {
                ApiError::InvalidInput(
                    "user_signature is not a valid Ed25519 signature over the public key — \
                     proof of private key ownership failed"
                        .to_string(),
                )
            })?;
    }

    // Derive user_identity from auth context.
    let user_identity = derive_user_identity(&auth);

    // Determine issuer from auth context.
    let issuer = if let Some(ref jwt_config) = state.jwt_config {
        jwt_config.issuer.clone()
    } else {
        state.base_url.clone()
    };

    let now = crate::api::now_secs();
    // Cap TTL to 24 hours maximum.
    let max_ttl = 86_400u64;
    let ttl = req.ttl_secs.min(max_ttl);
    let expires_at = now + ttl;

    // Platform countersigns the binding as a timestamp witness.
    // Construct the canonical binding data to countersign.
    let binding_data = serde_json::json!({
        "public_key": req.public_key,
        "user_identity": user_identity,
        "issuer": issuer,
        "issued_at": now,
        "expires_at": expires_at,
    });
    let binding_bytes = serde_json::to_vec(&binding_data).unwrap_or_default();
    let platform_countersign = state.agent_signing_key.sign_bytes(&binding_bytes);

    // Build the KeyBinding.
    let trust_anchor_id = if auth.jwt_claims.is_some() {
        "gyre-oidc".to_string()
    } else {
        "tenant-idp".to_string()
    };

    let binding = gyre_common::KeyBinding {
        public_key: public_key_bytes,
        user_identity: user_identity.clone(),
        issuer: issuer.clone(),
        trust_anchor_id: trust_anchor_id.clone(),
        issued_at: now,
        expires_at,
        user_signature: user_signature_bytes,
        platform_countersign: platform_countersign.clone(),
    };

    // Store the binding.
    state
        .key_bindings
        .store(&auth.tenant_id, &binding)
        .await
        .map_err(|e| ApiError::Internal(e))?;

    // §7.7: key_binding.created audit event.
    info!(
        user_identity = %user_identity,
        tenant_id = %auth.tenant_id,
        expires_at = expires_at,
        category = "Identity",
        event = "key_binding.created",
        "key_binding.created: key binding registered for {user_identity}"
    );

    Ok((
        StatusCode::CREATED,
        Json(KeyBindingResponse {
            public_key: req.public_key,
            user_identity,
            issuer,
            trust_anchor_id,
            issued_at: now,
            expires_at,
            platform_countersign: STANDARD.encode(&platform_countersign),
        }),
    ))
}

// ── Revocation handlers (TASK-047) ──────────────────────────────────────────

/// DELETE /api/v1/auth/key-binding/:id
///
/// Revoke a specific key binding. The `:id` path parameter is the hex-encoded
/// Ed25519 public key (64 hex characters for a 32-byte key).
/// Only the binding owner or a tenant admin can revoke.
pub async fn revoke_key_binding(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    // Decode hex public key from path parameter.
    let public_key_bytes = hex::decode(&id).map_err(|_| {
        ApiError::InvalidInput("id must be a hex-encoded Ed25519 public key".to_string())
    })?;

    if public_key_bytes.len() != 32 {
        return Err(ApiError::InvalidInput(
            "id must be exactly 64 hex characters (32-byte Ed25519 public key)".to_string(),
        ));
    }

    // Look up the binding (returns None if already revoked or not found).
    let binding = state
        .key_bindings
        .find_by_public_key(&auth.tenant_id, &public_key_bytes)
        .await
        .map_err(ApiError::Internal)?;

    let binding = match binding {
        Some(b) => b,
        None => return Err(ApiError::NotFound("key binding not found".to_string())),
    };

    // Authorization: caller must own the binding or be a tenant admin.
    let caller_identity = derive_user_identity(&auth);
    let is_owner = binding.user_identity == caller_identity;
    let is_admin = auth.roles.contains(&UserRole::Admin);
    if !is_owner && !is_admin {
        return Err(ApiError::Forbidden(
            "only the binding owner or a tenant admin can revoke".to_string(),
        ));
    }

    // Revoke the binding.
    state
        .key_bindings
        .invalidate(&auth.tenant_id, &public_key_bytes)
        .await
        .map_err(ApiError::Internal)?;

    // §7.7: key_binding.revoked audit event.
    info!(
        user_identity = %binding.user_identity,
        tenant_id = %auth.tenant_id,
        binding_id = %id,
        revoked_by = %caller_identity,
        category = "Identity",
        event = "key_binding.revoked",
        "key_binding.revoked: key binding revoked for {}", binding.user_identity,
    );

    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/v1/auth/key-bindings
///
/// Revoke all active key bindings for the authenticated user.
/// Useful for logout flow ("invalidate all my keys").
pub async fn revoke_all_key_bindings(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
) -> Result<StatusCode, ApiError> {
    let user_identity = derive_user_identity(&auth);

    // Retrieve active bindings before revoking, so we can emit per-binding audit events.
    let active_bindings = state
        .key_bindings
        .find_active_by_identity(&auth.tenant_id, &user_identity)
        .await
        .map_err(ApiError::Internal)?;

    // Revoke all bindings for this identity.
    state
        .key_bindings
        .invalidate_all_for_identity(&auth.tenant_id, &user_identity)
        .await
        .map_err(ApiError::Internal)?;

    // §7.7: key_binding.revoked audit event for each revoked binding.
    for binding in &active_bindings {
        let binding_id = hex::encode(&binding.public_key);
        info!(
            user_identity = %user_identity,
            tenant_id = %auth.tenant_id,
            binding_id = %binding_id,
            revoked_by = %user_identity,
            category = "Identity",
            event = "key_binding.revoked",
            "key_binding.revoked: key binding revoked for {user_identity}",
        );
    }

    Ok(StatusCode::NO_CONTENT)
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{
        body::Body,
        routing::{delete, post},
        Router,
    };
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app() -> Router {
        let state = test_state();
        Router::new()
            .route("/api/v1/auth/key-binding", post(super::create_key_binding))
            .with_state(state)
    }

    /// Generate a real Ed25519 keypair and return (public_key_bytes, signature_over_pubkey).
    fn generate_test_keypair() -> (Vec<u8>, Vec<u8>) {
        use ring::rand::SystemRandom;
        use ring::signature::{Ed25519KeyPair, KeyPair};
        let rng = SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let pub_key = key_pair.public_key().as_ref().to_vec();
        // Sign the public key bytes as proof-of-possession.
        let sig = key_pair.sign(&pub_key).as_ref().to_vec();
        (pub_key, sig)
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn create_key_binding_success() {
        let (pub_key, sig) = generate_test_keypair();
        let pubkey_b64 = STANDARD.encode(&pub_key);
        let sig_b64 = STANDARD.encode(&sig);

        let body = serde_json::json!({
            "public_key": pubkey_b64,
            "user_signature": sig_b64,
            "ttl_secs": 1800
        });

        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/key-binding")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["public_key"], pubkey_b64);
        assert!(json["user_identity"].as_str().unwrap().starts_with("user:"));
        assert!(!json["platform_countersign"].as_str().unwrap().is_empty());
        assert!(json["issued_at"].as_u64().is_some());
        assert!(json["expires_at"].as_u64().is_some());
    }

    #[tokio::test]
    async fn create_key_binding_invalid_signature_rejected() {
        // Valid 32-byte public key but wrong signature — must be rejected.
        let (pub_key, _sig) = generate_test_keypair();
        let body = serde_json::json!({
            "public_key": STANDARD.encode(&pub_key),
            "user_signature": STANDARD.encode(b"this-is-not-a-valid-ed25519-signature-at-all!padding"),
        });

        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/key-binding")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn create_key_binding_mismatched_key_signature_rejected() {
        // Generate two different keypairs — sign with one, submit the other's public key.
        let (pub_key_a, _sig_a) = generate_test_keypair();
        let (_pub_key_b, sig_b) = generate_test_keypair();

        let body = serde_json::json!({
            "public_key": STANDARD.encode(&pub_key_a),
            "user_signature": STANDARD.encode(&sig_b),
        });

        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/key-binding")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn create_key_binding_invalid_pubkey_length() {
        let short_key = [1u8; 16]; // Not 32 bytes
        let body = serde_json::json!({
            "public_key": STANDARD.encode(short_key),
            "user_signature": STANDARD.encode(b"sig"),
        });

        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/key-binding")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn create_key_binding_invalid_base64() {
        let body = serde_json::json!({
            "public_key": "not-valid-base64!!!",
            "user_signature": "also-invalid!!!",
        });

        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/key-binding")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn create_key_binding_ttl_capped_at_24h() {
        let (pub_key, sig) = generate_test_keypair();
        let body = serde_json::json!({
            "public_key": STANDARD.encode(&pub_key),
            "user_signature": STANDARD.encode(&sig),
            "ttl_secs": 999_999  // way more than 24h
        });

        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/key-binding")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        let issued = json["issued_at"].as_u64().unwrap();
        let expires = json["expires_at"].as_u64().unwrap();
        assert!(expires - issued <= 86_400);
    }

    #[tokio::test]
    async fn create_key_binding_stored_and_retrievable() {
        let state = test_state();
        let app = Router::new()
            .route(
                "/api/v1/auth/key-binding",
                axum::routing::post(super::create_key_binding),
            )
            .with_state(state.clone());

        let (pub_key, sig) = generate_test_keypair();
        let body = serde_json::json!({
            "public_key": STANDARD.encode(&pub_key),
            "user_signature": STANDARD.encode(&sig),
        });

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/key-binding")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // Verify it's stored — find by identity.
        let bindings = state
            .key_bindings
            .find_active_by_identity("default", "user:system")
            .await
            .unwrap();
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].public_key, pub_key);
    }

    // ── Revocation tests (TASK-047) ────────────────────────────────────────

    /// Helper: build a DELETE router with shared state and store a binding directly.
    fn revoke_app(state: std::sync::Arc<crate::AppState>) -> Router {
        Router::new()
            .route(
                "/api/v1/auth/key-binding/:id",
                delete(super::revoke_key_binding),
            )
            .route(
                "/api/v1/auth/key-bindings",
                delete(super::revoke_all_key_bindings),
            )
            .with_state(state)
    }

    /// Helper: store a test binding directly in state and return the hex-encoded public key.
    async fn store_test_binding(
        state: &std::sync::Arc<crate::AppState>,
        public_key: Vec<u8>,
        user_identity: &str,
    ) -> String {
        let binding = gyre_common::KeyBinding {
            public_key: public_key.clone(),
            user_identity: user_identity.to_string(),
            issuer: "https://keycloak.example.com".to_string(),
            trust_anchor_id: "tenant-keycloak".to_string(),
            issued_at: 1_700_000_000,
            expires_at: 4_102_444_800, // far future
            user_signature: vec![10, 20, 30, 40],
            platform_countersign: vec![50, 60, 70, 80],
        };
        state.key_bindings.store("default", &binding).await.unwrap();
        hex::encode(&public_key)
    }

    #[tokio::test]
    async fn revoke_own_key_binding_returns_204() {
        let state = test_state();
        // Store a binding owned by "user:system" (matches test-token auth).
        let pub_key = vec![1u8; 32];
        let hex_id = store_test_binding(&state, pub_key.clone(), "user:system").await;

        // Verify binding exists before revocation.
        let found = state
            .key_bindings
            .find_by_public_key("default", &pub_key)
            .await
            .unwrap();
        assert!(found.is_some());

        // Revoke via DELETE endpoint.
        let resp = revoke_app(state.clone())
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/auth/key-binding/{hex_id}"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // Verify binding is no longer findable (revoked_at set).
        let found = state
            .key_bindings
            .find_by_public_key("default", &pub_key)
            .await
            .unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn revoke_nonexistent_binding_returns_404() {
        let state = test_state();
        // Use a valid hex-encoded 32-byte key that was never registered.
        let fake_id = hex::encode([42u8; 32]);

        let resp = revoke_app(state)
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/auth/key-binding/{fake_id}"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn revoke_invalid_hex_id_returns_400() {
        let state = test_state();

        let resp = revoke_app(state)
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/v1/auth/key-binding/not-valid-hex!!!")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn revoke_wrong_length_hex_id_returns_400() {
        let state = test_state();
        // 16 bytes (32 hex chars) instead of 32 bytes (64 hex chars).
        let short_id = hex::encode([1u8; 16]);

        let resp = revoke_app(state)
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/auth/key-binding/{short_id}"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn revoke_another_users_binding_as_admin_succeeds() {
        let state = test_state();
        // Store a binding owned by a different user.
        let pub_key = vec![99u8; 32];
        let hex_id = store_test_binding(&state, pub_key.clone(), "user:other-person").await;

        // test-token auth is Admin, so admin can revoke another user's binding.
        let resp = revoke_app(state.clone())
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/auth/key-binding/{hex_id}"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // Verify it's revoked.
        let found = state
            .key_bindings
            .find_by_public_key("default", &pub_key)
            .await
            .unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn revoke_non_owner_non_admin_returns_403() {
        // Test authorization logic directly since test-token always grants Admin.
        // Construct a non-admin AuthenticatedAgent manually.
        use crate::auth::AuthenticatedAgent;
        use gyre_domain::UserRole;

        let state = test_state();

        // Store a binding owned by "user:alice".
        let pub_key = vec![77u8; 32];
        store_test_binding(&state, pub_key.clone(), "user:alice").await;

        // Caller is "user:bob" with Developer role (not Admin, not owner).
        let auth = AuthenticatedAgent {
            agent_id: "bob".to_string(),
            user_id: None,
            roles: vec![UserRole::Developer],
            tenant_id: "default".to_string(),
            jwt_claims: None,
            deprecated_token_auth: false,
        };

        let hex_id = hex::encode(&pub_key);
        let result = super::revoke_key_binding(
            axum::extract::State(state.clone()),
            auth,
            axum::extract::Path(hex_id),
        )
        .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        // ApiError::Forbidden maps to 403.
        let resp = axum::response::IntoResponse::into_response(err);
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Verify binding is NOT revoked (authorization was denied).
        let found = state
            .key_bindings
            .find_by_public_key("default", &pub_key)
            .await
            .unwrap();
        assert!(found.is_some(), "binding must not be revoked after 403");
    }

    #[tokio::test]
    async fn revoke_all_revokes_all_active_bindings() {
        let state = test_state();

        // Store two bindings for user:system directly.
        store_test_binding(&state, vec![11u8; 32], "user:system").await;
        store_test_binding(&state, vec![22u8; 32], "user:system").await;

        // Verify both exist.
        let active = state
            .key_bindings
            .find_active_by_identity("default", "user:system")
            .await
            .unwrap();
        assert_eq!(active.len(), 2);

        // Revoke all.
        let resp = revoke_app(state.clone())
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/v1/auth/key-bindings")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // Verify all are revoked.
        let active = state
            .key_bindings
            .find_active_by_identity("default", "user:system")
            .await
            .unwrap();
        assert_eq!(active.len(), 0);
    }

    #[tokio::test]
    async fn revoke_all_with_no_bindings_returns_204() {
        let state = test_state();

        // Revoke all when there are no bindings — should still succeed.
        let resp = revoke_app(state)
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/v1/auth/key-bindings")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn revoked_binding_rejected_at_lookup() {
        let state = test_state();
        let pub_key = vec![55u8; 32];
        let hex_id = store_test_binding(&state, pub_key.clone(), "user:system").await;

        // Revoke the binding.
        let resp = revoke_app(state.clone())
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/auth/key-binding/{hex_id}"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // Push-time chain verification uses find_by_public_key, which filters revoked.
        // Verify revoked binding is not returned.
        let found = state
            .key_bindings
            .find_by_public_key("default", &pub_key)
            .await
            .unwrap();
        assert!(found.is_none(), "revoked binding must not be returned");

        // Also verify it's excluded from find_active_by_identity.
        let active = state
            .key_bindings
            .find_active_by_identity("default", "user:system")
            .await
            .unwrap();
        assert!(
            active.iter().all(|b| b.public_key != pub_key),
            "revoked binding must not appear in active list"
        );
    }

    #[tokio::test]
    async fn revoke_all_does_not_affect_other_users() {
        let state = test_state();

        // Store bindings for two different users.
        store_test_binding(&state, vec![11u8; 32], "user:system").await;
        store_test_binding(&state, vec![22u8; 32], "user:bob").await;

        // Revoke all for the test-token user (user:system).
        let resp = revoke_app(state.clone())
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/v1/auth/key-bindings")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // user:system's binding should be gone.
        let found_system = state
            .key_bindings
            .find_by_public_key("default", &vec![11u8; 32])
            .await
            .unwrap();
        assert!(found_system.is_none());

        // user:bob's binding should still exist.
        let found_bob = state
            .key_bindings
            .find_by_public_key("default", &vec![22u8; 32])
            .await
            .unwrap();
        assert!(found_bob.is_some());
    }
}
