//! Authentication middleware for gyre-server.
//!
//! Auth chain (first match wins):
//! 1. Global `auth_token` — for system/dev use, resolves as "system".
//! 2. Per-agent tokens — issued at registration, resolves as the agent id.
//! 3. API keys — resolves as the owning user's name.
//! 4. JWT (OIDC/Keycloak) — validates RS256 token; auto-creates user on first login.
//!
//! If `jwt_config` is None the server runs without Keycloak (agent tokens only).

use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
};
use gyre_common::Id;
use gyre_domain::{User, UserRole};
use serde::Deserialize;
use std::{collections::HashMap, sync::Arc};

use crate::AppState;

/// Resolved principal injected by the auth extractor.
pub struct AuthenticatedAgent {
    pub agent_id: String,
    /// Present when auth was performed via JWT or API key.
    pub user_id: Option<Id>,
    pub roles: Vec<UserRole>,
}

// ── JWT claim types ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct JwtClaims {
    sub: String,
    #[serde(default)]
    preferred_username: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    realm_access: Option<RealmAccess>,
}

#[derive(Debug, Deserialize)]
struct RealmAccess {
    #[serde(default)]
    roles: Vec<String>,
}

fn roles_from_claims(claims: &JwtClaims) -> Vec<UserRole> {
    let raw = claims
        .realm_access
        .as_ref()
        .map(|ra| ra.roles.as_slice())
        .unwrap_or(&[]);
    raw.iter().filter_map(|s| UserRole::from_str(s)).collect()
}

// ── JWKS refresh ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct OidcDiscovery {
    jwks_uri: String,
}

#[derive(Deserialize)]
struct JwkSet {
    keys: Vec<serde_json::Value>,
}

/// Fetch JWKS from the OIDC discovery endpoint and populate the key cache.
async fn refresh_jwks(
    issuer: &str,
    http_client: &reqwest::Client,
    cache: &std::sync::RwLock<HashMap<String, jsonwebtoken::DecodingKey>>,
) {
    let discovery_url = format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    );
    let discovery: OidcDiscovery = match http_client.get(&discovery_url).send().await {
        Ok(r) => match r.json().await {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("OIDC discovery parse error: {e}");
                return;
            }
        },
        Err(e) => {
            tracing::warn!("OIDC discovery fetch error: {e}");
            return;
        }
    };

    let jwk_set: JwkSet = match http_client.get(&discovery.jwks_uri).send().await {
        Ok(r) => match r.json().await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("JWKS parse error: {e}");
                return;
            }
        },
        Err(e) => {
            tracing::warn!("JWKS fetch error: {e}");
            return;
        }
    };

    let mut new_keys: HashMap<String, jsonwebtoken::DecodingKey> = HashMap::new();
    for jwk in &jwk_set.keys {
        let kid = match jwk.get("kid").and_then(|v| v.as_str()) {
            Some(k) => k.to_string(),
            None => continue,
        };
        let jwk_typed: jsonwebtoken::jwk::Jwk = match serde_json::from_value(jwk.clone()) {
            Ok(k) => k,
            Err(e) => {
                tracing::warn!("Failed to parse JWK {kid}: {e}");
                continue;
            }
        };
        match jsonwebtoken::DecodingKey::from_jwk(&jwk_typed) {
            Ok(dk) => {
                new_keys.insert(kid, dk);
            }
            Err(e) => tracing::warn!("Failed to build DecodingKey for {kid}: {e}"),
        }
    }

    if !new_keys.is_empty() {
        if let Ok(mut cache_guard) = cache.write() {
            cache_guard.extend(new_keys);
        }
    }
}

// ── Auth extractor ────────────────────────────────────────────────────────────

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

        // 1. Global auth token (dev / system usage).
        if token == state.auth_token {
            return Ok(AuthenticatedAgent {
                agent_id: "system".to_string(),
                user_id: None,
                roles: vec![UserRole::Admin],
            });
        }

        // 2. Per-agent tokens issued at registration.
        {
            let agent_tokens = state.agent_tokens.lock().await;
            if let Some(agent_id) = agent_tokens
                .iter()
                .find(|(_, t)| t.as_str() == token)
                .map(|(id, _)| id.clone())
            {
                return Ok(AuthenticatedAgent {
                    agent_id,
                    user_id: None,
                    roles: vec![UserRole::Agent],
                });
            }
        }

        // 3. API keys.
        if let Ok(Some(user_id)) = state.api_keys.find_user_id(token).await {
            if let Ok(Some(user)) = state.users.find_by_id(&user_id).await {
                return Ok(AuthenticatedAgent {
                    agent_id: user.name.clone(),
                    user_id: Some(user.id),
                    roles: user.roles,
                });
            }
        }

        // 4. JWT validation.
        if let Some(jwt_cfg) = &state.jwt_config {
            return validate_jwt(token, jwt_cfg, state).await.map_err(|e| {
                tracing::debug!("JWT validation failed: {e}");
                (StatusCode::UNAUTHORIZED, "Invalid or expired token").into_response()
            });
        }

        Err((StatusCode::UNAUTHORIZED, "Invalid token").into_response())
    }
}

/// Public wrapper for middleware JWT validation (checks token validity only).
pub async fn validate_jwt_middleware(
    token: &str,
    jwt_cfg: &crate::JwtConfig,
    state: &Arc<crate::AppState>,
) -> Result<(), String> {
    validate_jwt(token, jwt_cfg, state).await.map(|_| ())
}

async fn validate_jwt(
    token: &str,
    jwt_cfg: &crate::JwtConfig,
    state: &Arc<AppState>,
) -> Result<AuthenticatedAgent, String> {
    // Decode header to get kid.
    let header = jsonwebtoken::decode_header(token).map_err(|e| format!("decode header: {e}"))?;
    let kid = header.kid.unwrap_or_else(|| "default".to_string());

    // Try to find key; refresh JWKS once if not found.
    let decoding_key = {
        let cache = jwt_cfg.keys.read().unwrap();
        cache.get(&kid).cloned()
    };

    let decoding_key = if let Some(dk) = decoding_key {
        dk
    } else {
        refresh_jwks(&jwt_cfg.issuer, &state.http_client, &jwt_cfg.keys).await;
        let cache = jwt_cfg.keys.read().unwrap();
        cache
            .get(&kid)
            .cloned()
            .ok_or_else(|| format!("no JWK found for kid={kid}"))?
    };

    // Build validation.
    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::RS256);
    validation.set_issuer(&[&jwt_cfg.issuer]);
    if let Some(aud) = &jwt_cfg.audience {
        validation.set_audience(&[aud]);
    } else {
        validation.validate_aud = false;
    }

    let token_data = jsonwebtoken::decode::<JwtClaims>(token, &decoding_key, &validation)
        .map_err(|e| format!("decode: {e}"))?;

    let claims = token_data.claims;
    let roles = roles_from_claims(&claims);
    let username = claims
        .preferred_username
        .clone()
        .unwrap_or_else(|| claims.sub.clone());

    // Find or auto-create user.
    let user = find_or_create_user(
        state,
        &claims.sub,
        &username,
        claims.email.as_deref(),
        &roles,
    )
    .await
    .map_err(|e| format!("user resolution: {e}"))?;

    Ok(AuthenticatedAgent {
        agent_id: user.name.clone(),
        user_id: Some(user.id),
        roles: user.roles,
    })
}

async fn find_or_create_user(
    state: &Arc<AppState>,
    external_id: &str,
    name: &str,
    email: Option<&str>,
    roles: &[UserRole],
) -> anyhow::Result<User> {
    if let Some(existing) = state.users.find_by_external_id(external_id).await? {
        return Ok(existing);
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let id = Id::new(uuid::Uuid::new_v4().to_string());
    let mut user = User::new(id, external_id, name, now);
    user.email = email.map(|e| e.to_string());
    if !roles.is_empty() {
        user.roles = roles.to_vec();
    }

    state.users.create(&user).await?;
    Ok(user)
}

// ── AdminOnly extractor ───────────────────────────────────────────────────────

/// Extractor that requires the caller to have Admin role (or be "system").
pub struct AdminOnly {
    pub agent_id: String,
    pub user_id: Option<Id>,
}

#[axum::async_trait]
impl FromRequestParts<Arc<AppState>> for AdminOnly {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let auth = AuthenticatedAgent::from_request_parts(parts, state).await?;
        if auth.agent_id == "system" || auth.roles.contains(&UserRole::Admin) {
            Ok(AdminOnly {
                agent_id: auth.agent_id,
                user_id: auth.user_id,
            })
        } else {
            Err((StatusCode::FORBIDDEN, "Admin role required").into_response())
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
pub mod test_helpers {
    use super::*;
    use crate::JwtConfig;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use jsonwebtoken::DecodingKey;
    use std::time::{SystemTime, UNIX_EPOCH};

    pub const TEST_KID: &str = "test-key-1";

    /// Pre-generated RSA-2048 private key (PKCS8 DER, base64-encoded).
    /// Used by ring to sign test JWTs.
    pub const TEST_RSA_PRIVATE_DER_B64: &str =
        "MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQC+pxi5pumJYejs\
O48NpzBcwW7tu/Ukwo8CeOTkfcHwf4nnpTUHFds/OynA3OevJFt25U/tnS2nplU+\
3zpZ/w0eSwDzBbA7uvukR8hjOPp8726Jim8slmKVLmnlAFxMNgkaPOWuHQjNCvAe\
GbLm6hMigAAmpo1HyrtjsAViq7hezKAdegsbmSbcOShG9Yy1eNBRcoNr+BTut2bF\
RF5dGf24oHUXhVn1A89eYUDcYFLqstrZ/uyvyrYuSKKaJQmQ3O7J2PyfPoX9skqx\
ACOlyN1qJSkpSI1olqaXMhD/chqoyM5rvYerDts5iVp7kY9tBEyd5GnazD2qtsm0\
58vGdAdZAgMBAAECggEAClgm5mjU5kY8lPe4z+70t+2rYwUGHK3dRE8yrSaZI59J\
6FqNhWJFVHmVbwBVZkp413tC8lOrRtvGp4wFXp1w9OYxKZ5zBNSlA3F9IQwWBDoa\
A2ayWEcbF3jOmFUkDOMILKcl96Pd5sErjNiMXUzRR7eN+oriIsVwVCz4Z9WFIgiM\
XNB2WGrtAdpYRuhLcXXxKF4MyCNe5xwiTtdTxOPNYZCPmkBuGOH/cXW2nRZOX17t\
HzCYQApplay6Deav7tRFLFy9i0/oTKmOd+9MjaRG7AGkpojFTERQU1QC7wNKGM/G\
jWyTcLqCNz7Eeiw8xccu8K/4YWTIb1guxSXVR8tRkQKBgQD3kvgZkBxrwQ/LxPk9\
KwnqREjyU6TklAkMFi3hH8ztG6/CARf5Z/PAtG+LRFjjdA6ZUk0NQB3vIuVR+l8k\
X7/yGxR+AZFP+fC0WiOt3nK6dUQZjM1EVeXN8SF89AaLmVqYeVfOuFZASVNdOywV\
64Fs4vngyx1yRe6L4gmuJRTpsQKBgQDFJDC9x/pv3uwxiVUwQAuOmhxarm3zFIMA\
hdgElKarF//VSf2ClNlkBjOdOG1qygP/uDiKgEji74o/OC8W+7KJEc0GCC9+Re26\
VjGoAeW9tpdPcFG4pGy3O8TgUev4Kd7M5yXrP+t7Tk7BzFYbolyH2nStVnoGzmEV\
ovLRrVC6KQKBgQCHKNqAAjgx77Wy9ndnmI6SMfNtMJMOI2+JBLbJd/YF4J6kmLpA\
9CqUIy3eZ1Lr4llyBQeB22Y8Pt3ZUooMrqV5BKW8YRi3A+oQXEGsU4xpFUo3dLI/\
w3FxbFpvAD5uiit2oJB6kBKuiBmWBY787JS9IHFw8xLBZ0JjZeUJCL+REQKBgGyH\
aunfoMhbke4Ha8V/TJ15y+hEJLVwclokVPLYnxSLxsS10gFETpVZMKr5+IqUojTy\
RvZAf1+nfZUmKg5eDECXcyZB9VtvxdsSPo3+VWrmjVAnRmX+5vbMyaqwjtZ0vnpR\
OjfTXaDpUEQyaKkX7hbANgGDHPSIQzw1E7ZI0qtZAoGBAOYcsRdNUwGPYqLKzfua\
W89V2Jogg217+3HIxjgfmGn9nb8RqIwhGSoSNGBa2gKWJWUzPNzF4H3TIQw1MGyp\
l4UzPHH0YLfuMsgbYIGpoe9l0zTcucflm8gMLUHysu5WSxRDtHa7Kv8BVafmD7zL\
HKWsbrW0tHUPuMuz8Xgvs0yV";

    /// JWK (JSON) for the corresponding RSA-2048 public key.
    /// n = base64url(modulus), e = base64url(65537).
    const TEST_JWK_JSON: &str = r#"{"kty":"RSA","alg":"RS256","use":"sig","kid":"test-key-1","n":"vqcYuabpiWHo7DuPDacwXMFu7bv1JMKPAnjk5H3B8H-J56U1BxXbPzspwNznryRbduVP7Z0tp6ZVPt86Wf8NHksA8wWwO7r7pEfIYzj6fO9uiYpvLJZilS5p5QBcTDYJGjzlrh0IzQrwHhmy5uoTIoAAJqaNR8q7Y7AFYqu4XsygHXoLG5km3DkoRvWMtXjQUXKDa_gU7rdmxUReXRn9uKB1F4VZ9QPPXmFA3GBS6rLa2f7sr8q2LkiimiUJkNzuydj8nz6F_bJKsQAjpcjdaiUpKUiNaJamlzIQ_3IaqMjOa72Hqw7bOYlae5GPbQRMneRp2sw9qrbJtOfLxnQHWQ","e":"AQAB"}"#;

    pub fn make_jwt_config() -> Arc<JwtConfig> {
        let cfg = JwtConfig::new("http://localhost:8080/realms/gyre", None);

        let jwk: jsonwebtoken::jwk::Jwk =
            serde_json::from_str(TEST_JWK_JSON).expect("test JWK must be valid JSON");
        let dk = DecodingKey::from_jwk(&jwk).expect("test JWK must produce a valid DecodingKey");
        cfg.insert_key(TEST_KID, dk);
        Arc::new(cfg)
    }

    pub fn make_test_state_with_jwt() -> Arc<crate::AppState> {
        let base = crate::mem::test_state();
        let mut state = (*base).clone();
        state.jwt_config = Some(make_jwt_config());
        Arc::new(state)
    }

    /// Sign a test JWT using ring's RSA-PKCS1-SHA256 (RS256).
    /// Does NOT use jsonwebtoken's PEM feature — no simple_asn1/time MSRV issue.
    pub fn sign_test_jwt(claims: &serde_json::Value, exp_delta_secs: i64) -> String {
        use base64::engine::general_purpose::STANDARD;
        use ring::rand::SystemRandom;
        use ring::signature::{RsaKeyPair, RSA_PKCS1_SHA256};

        // Decode private key DER (standard base64, not URL-safe).
        let private_der = STANDARD
            .decode(TEST_RSA_PRIVATE_DER_B64)
            .expect("private key base64 must be valid");

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let mut payload = claims.clone();
        let obj = payload.as_object_mut().unwrap();
        obj.insert(
            "iss".to_string(),
            serde_json::json!("http://localhost:8080/realms/gyre"),
        );
        obj.insert("iat".to_string(), serde_json::json!(now));
        obj.insert("exp".to_string(), serde_json::json!(now + exp_delta_secs));

        // Build header JSON.
        let header = serde_json::json!({"alg":"RS256","typ":"JWT","kid": TEST_KID});
        let header_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_string(&header).unwrap());
        let payload_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_string(&payload).unwrap());
        let signing_input = format!("{}.{}", header_b64, payload_b64);

        let key_pair =
            RsaKeyPair::from_pkcs8(&private_der).expect("private key DER must be valid PKCS8");
        let rng = SystemRandom::new();
        let mut sig = vec![0u8; key_pair.public().modulus_len()];
        key_pair
            .sign(&RSA_PKCS1_SHA256, &rng, signing_input.as_bytes(), &mut sig)
            .expect("RSA signing must succeed");

        let sig_b64 = URL_SAFE_NO_PAD.encode(&sig);
        format!("{}.{}", signing_input, sig_b64)
    }
}

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, routing::get, Router};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    use super::{test_helpers::*, AuthenticatedAgent};

    async fn authenticated_handler(
        AuthenticatedAgent { agent_id, .. }: AuthenticatedAgent,
    ) -> String {
        agent_id
    }

    fn app_no_jwt() -> Router {
        let state = test_state();
        Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state)
    }

    #[allow(dead_code)]
    fn app_with_jwt() -> Router {
        let state = make_test_state_with_jwt();
        Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state)
    }

    #[tokio::test]
    async fn missing_auth_header_returns_401() {
        let resp = app_no_jwt()
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
        let resp = app_no_jwt()
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
        let resp = app_no_jwt()
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn agent_token_accepted() {
        let state = test_state();
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

    #[tokio::test]
    async fn valid_jwt_accepted() {
        let claims = serde_json::json!({
            "sub": "user-sub-001",
            "preferred_username": "alice",
            "email": "alice@example.com",
            "realm_access": { "roles": ["developer"] }
        });
        let token = sign_test_jwt(&claims, 3600);

        let resp = app_with_jwt()
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(bytes.as_ref(), b"alice");
    }

    #[tokio::test]
    async fn expired_jwt_rejected() {
        let claims = serde_json::json!({
            "sub": "user-sub-002",
            "preferred_username": "bob"
        });
        // Negative delta = already expired.
        let token = sign_test_jwt(&claims, -3600);

        let resp = app_with_jwt()
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn invalid_signature_rejected() {
        // Tamper with the token by altering a byte in the signature.
        let claims = serde_json::json!({ "sub": "user-sub-003", "preferred_username": "carol" });
        let mut token = sign_test_jwt(&claims, 3600);
        // Replace last char to corrupt signature.
        let last = token.pop().unwrap();
        token.push(if last == 'A' { 'B' } else { 'A' });

        let resp = app_with_jwt()
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn jwt_auto_creates_user_on_first_login() {
        let state = make_test_state_with_jwt();

        let claims = serde_json::json!({
            "sub": "new-user-sub-999",
            "preferred_username": "dave",
            "email": "dave@example.com",
            "realm_access": { "roles": ["developer"] }
        });
        let token = sign_test_jwt(&claims, 3600);

        // No user exists yet.
        let before = state
            .users
            .find_by_external_id("new-user-sub-999")
            .await
            .unwrap();
        assert!(before.is_none());

        let app: Router = Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state.clone());

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // User should now exist.
        let after = state
            .users
            .find_by_external_id("new-user-sub-999")
            .await
            .unwrap();
        assert!(after.is_some());
        let user = after.unwrap();
        assert_eq!(user.name, "dave");
        assert_eq!(user.email.as_deref(), Some("dave@example.com"));
    }

    #[tokio::test]
    async fn role_extraction_from_jwt_claims() {
        use super::AuthenticatedAgent;

        let state = make_test_state_with_jwt();

        async fn role_handler(auth: AuthenticatedAgent) -> String {
            auth.roles
                .iter()
                .map(|r| r.as_str())
                .collect::<Vec<_>>()
                .join(",")
        }

        let claims = serde_json::json!({
            "sub": "role-test-sub",
            "preferred_username": "eve",
            "realm_access": { "roles": ["admin", "developer"] }
        });
        let token = sign_test_jwt(&claims, 3600);

        let app: Router = Router::new()
            .route("/roles", get(role_handler))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/roles")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let roles_str = std::str::from_utf8(&body).unwrap();
        assert!(roles_str.contains("Admin"), "expected Admin in {roles_str}");
        assert!(
            roles_str.contains("Developer"),
            "expected Developer in {roles_str}"
        );
    }

    #[tokio::test]
    async fn api_key_auth_works() {
        use gyre_common::Id;
        use gyre_domain::User;

        let state = test_state();
        let now = 1000u64;
        let user = User::new(Id::new("u1"), "ext-1", "frank", now);
        state.users.create(&user).await.unwrap();
        state
            .api_keys
            .create("gyre_test_api_key", &user.id, "test-key")
            .await
            .unwrap();

        let app = Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", "Bearer gyre_test_api_key")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(bytes.as_ref(), b"frank");
    }

    #[tokio::test]
    async fn backward_compat_global_token_still_works_with_jwt_enabled() {
        let resp = app_with_jwt()
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(bytes.as_ref(), b"system");
    }

    #[tokio::test]
    async fn backward_compat_agent_token_with_jwt_enabled() {
        let state = make_test_state_with_jwt();
        state
            .agent_tokens
            .lock()
            .await
            .insert("agt-1".to_string(), "agt-secret-xyz".to_string());

        let app = Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", "Bearer agt-secret-xyz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(bytes.as_ref(), b"agt-1");
    }

    #[tokio::test]
    async fn jwt_wrong_kid_rejected() {
        // Sign a JWT with a kid that doesn't exist in the cache.
        // We build it manually using ring.
        use base64::{
            engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
            Engine as _,
        };
        use ring::rand::SystemRandom;
        use ring::signature::{RsaKeyPair, RSA_PKCS1_SHA256};

        use crate::auth::test_helpers;
        let private_der = STANDARD
            .decode(test_helpers::TEST_RSA_PRIVATE_DER_B64)
            .unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let header = serde_json::json!({"alg":"RS256","typ":"JWT","kid":"unknown-kid"});
        let payload = serde_json::json!({
            "sub": "x",
            "iss": "http://localhost:8080/realms/gyre",
            "iat": now,
            "exp": now + 3600,
        });

        let h = URL_SAFE_NO_PAD.encode(serde_json::to_string(&header).unwrap());
        let p = URL_SAFE_NO_PAD.encode(serde_json::to_string(&payload).unwrap());
        let signing_input = format!("{h}.{p}");

        let key_pair = RsaKeyPair::from_pkcs8(&private_der).unwrap();
        let rng = SystemRandom::new();
        let mut sig = vec![0u8; key_pair.public().modulus_len()];
        key_pair
            .sign(&RSA_PKCS1_SHA256, &rng, signing_input.as_bytes(), &mut sig)
            .unwrap();

        let token = format!("{}.{}", signing_input, URL_SAFE_NO_PAD.encode(&sig));

        let resp = app_with_jwt()
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
