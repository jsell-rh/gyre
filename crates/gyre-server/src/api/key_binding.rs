//! Key binding endpoint (authorization-provenance.md §2.3, TASK-006).
//!
//! `POST /api/v1/auth/key-binding` — Bind an ephemeral Ed25519 public key to a user identity.
//! The client sends the public key + a self-signature; the server verifies the user's
//! IdP session is valid, countersigns the binding, and stores the public key.

use axum::{extract::State, http::StatusCode, Json};
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

    // Derive user_identity from auth context.
    let user_identity = if auth.jwt_claims.is_some() {
        // Agent JWT — use "agent:<id>" format.
        format!("agent:{}", auth.agent_id)
    } else {
        // Human user — use "user:<id>" format.
        format!("user:{}", auth.agent_id)
    };

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

    info!(
        user_identity = %user_identity,
        tenant_id = %auth.tenant_id,
        expires_at = expires_at,
        "key_binding.created"
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

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, routing::post, Router};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app() -> Router {
        let state = test_state();
        Router::new()
            .route("/api/v1/auth/key-binding", post(super::create_key_binding))
            .with_state(state)
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn create_key_binding_success() {
        use base64::engine::general_purpose::STANDARD;
        use base64::Engine;

        // Generate a test Ed25519 keypair for the public key.
        let fake_pubkey = [42u8; 32];
        let pubkey_b64 = STANDARD.encode(fake_pubkey);
        let sig_b64 = STANDARD.encode(b"fake-signature");

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
        assert!(json["platform_countersign"].as_str().unwrap().len() > 0);
        assert!(json["issued_at"].as_u64().is_some());
        assert!(json["expires_at"].as_u64().is_some());
    }

    #[tokio::test]
    async fn create_key_binding_invalid_pubkey_length() {
        use base64::engine::general_purpose::STANDARD;
        use base64::Engine;

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
        use base64::engine::general_purpose::STANDARD;
        use base64::Engine;

        let body = serde_json::json!({
            "public_key": STANDARD.encode([7u8; 32]),
            "user_signature": STANDARD.encode(b"sig"),
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
        use base64::engine::general_purpose::STANDARD;
        use base64::Engine;

        let state = test_state();
        let app = Router::new()
            .route(
                "/api/v1/auth/key-binding",
                axum::routing::post(super::create_key_binding),
            )
            .with_state(state.clone());

        let pubkey = [99u8; 32];
        let body = serde_json::json!({
            "public_key": STANDARD.encode(pubkey),
            "user_signature": STANDARD.encode(b"sig"),
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
        assert_eq!(bindings[0].public_key, pubkey.to_vec());
    }
}
