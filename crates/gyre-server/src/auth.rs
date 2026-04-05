//! Authentication middleware for gyre-server.
//!
//! Auth chain (first match wins):
//! 1. Global `auth_token` -- for system/dev use, resolves as "system".
//! 2. Per-agent tokens -- issued at registration, resolves as the agent id.
//!    - UUID tokens: looked up in agent_tokens HashMap.
//!    - JWT tokens (start with "ey"): looked up in HashMap + cryptographically validated.
//! 3. API keys -- resolves as the owning user's name.
//! 4. JWT (OIDC/Keycloak) -- validates RS256 token; auto-creates user on first login.
//! 5. Federation JWT (G11) -- EdDSA JWT from a trusted remote Gyre instance.
//!    `iss` claim must match one of `GYRE_TRUSTED_ISSUERS`. JWKS fetched via OIDC
//!    discovery from the remote instance and cached for 5 minutes.
//!
//! If `jwt_config` is None the server runs without Keycloak (agent tokens only).
//! Gyre always mints its own agent JWTs via `AgentSigningKey` (EdDSA/Ed25519).

use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use gyre_common::Id;
use gyre_domain::{User, UserRole};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::{collections::HashMap, sync::Arc};
use subtle::ConstantTimeEq;

use crate::AppState;

// -- Federation JWKS cache (G11) ----------------------------------------------

/// Cached JWKS entry for a trusted remote Gyre instance.
pub struct RemoteJwksEntry {
    pub keys: HashMap<String, jsonwebtoken::DecodingKey>,
    pub fetched_at: std::time::Instant,
}

/// TTL for cached remote JWKS (5 minutes).
const REMOTE_JWKS_TTL_SECS: u64 = 300;

// -- Agent JWT signing (Gyre as OIDC provider) --------------------------------

/// Claims embedded in agent JWTs minted by Gyre's built-in OIDC provider.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentJwtClaims {
    /// Subject — the agent's UUID.
    pub sub: String,
    /// Issuer — the server's base URL.
    pub iss: String,
    /// Issued-at (Unix epoch seconds).
    pub iat: u64,
    /// Expiry (Unix epoch seconds).
    pub exp: u64,
    /// Always "agent" for agent tokens.
    pub scope: String,
    /// Task assigned to this agent at spawn time.
    pub task_id: String,
    /// Identity that called POST /api/v1/agents/spawn.
    pub spawned_by: String,

    // -- G10: Workload attestation claims -------------------------------------
    /// OS PID of the agent process (workload identity, G10).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wl_pid: Option<u32>,
    /// Hostname where the agent process is running (workload identity, G10).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wl_hostname: Option<String>,
    /// Compute target identifier: "local", docker container ID, SSH host (G10).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wl_compute_target: Option<String>,
    /// Stack fingerprint hash at spawn time (G10).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wl_stack_hash: Option<String>,
    // -- M19.4: Container workload claims -------------------------------------
    /// Container ID when the agent was spawned via ContainerTarget (M19.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wl_container_id: Option<String>,
    /// Image digest (sha256) when spawned via ContainerTarget (M19.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wl_image_hash: Option<String>,
}

/// Ed25519 key pair used to mint and verify agent JWTs.
///
/// Generated fresh on every server start. The public key is exposed via
/// `GET /.well-known/jwks.json` so external verifiers can validate tokens.
pub struct AgentSigningKey {
    pub encoding_key: jsonwebtoken::EncodingKey,
    pub decoding_key: jsonwebtoken::DecodingKey,
    /// Pre-serialised JWKS JSON response body.
    pub jwks_json: String,
    /// Key ID embedded in JWT headers.
    pub kid: String,
    /// Raw PKCS#8 bytes retained for signing arbitrary data (attestation bundles).
    pkcs8_bytes: Vec<u8>,
    /// Raw 32-byte public key for external verification.
    pub public_key_bytes: Vec<u8>,
}

impl AgentSigningKey {
    /// Generate a fresh Ed25519 key pair.
    pub fn generate() -> Self {
        use ring::rand::SystemRandom;
        use ring::signature::Ed25519KeyPair;

        let rng = SystemRandom::new();
        let pkcs8 =
            Ed25519KeyPair::generate_pkcs8(&rng).expect("Ed25519 key generation must succeed");
        let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref())
            .expect("Ed25519 key pair construction must succeed");
        use ring::signature::KeyPair as _;
        let pub_key_bytes = key_pair.public_key().as_ref();

        let kid = uuid::Uuid::new_v4().to_string();
        let x = URL_SAFE_NO_PAD.encode(pub_key_bytes);

        let jwks_json = serde_json::json!({
            "keys": [{
                "kty": "OKP",
                "crv": "Ed25519",
                "use": "sig",
                "alg": "EdDSA",
                "kid": kid,
                "x": x
            }]
        })
        .to_string();

        let encoding_key = jsonwebtoken::EncodingKey::from_ed_der(pkcs8.as_ref());
        let decoding_key = jsonwebtoken::DecodingKey::from_ed_der(pub_key_bytes);

        Self {
            encoding_key,
            decoding_key,
            jwks_json,
            kid,
            pkcs8_bytes: pkcs8.as_ref().to_vec(),
            public_key_bytes: pub_key_bytes.to_vec(),
        }
    }

    /// Sign arbitrary bytes with the Ed25519 private key.
    ///
    /// Used by the attestation module to sign merge attestation bundles.
    pub fn sign_bytes(&self, data: &[u8]) -> Vec<u8> {
        use ring::signature::Ed25519KeyPair;
        let key_pair = Ed25519KeyPair::from_pkcs8(&self.pkcs8_bytes)
            .expect("stored pkcs8 bytes must remain valid");
        key_pair.sign(data).as_ref().to_vec()
    }

    /// Mint a signed EdDSA JWT for an agent.
    ///
    /// `workload` carries optional G10 workload attestation claims embedded
    /// directly in the JWT so external verifiers can reconstruct workload
    /// identity from the token alone.
    pub fn mint(
        &self,
        agent_id: &str,
        task_id: &str,
        spawned_by: &str,
        issuer: &str,
        ttl_secs: u64,
    ) -> Result<String, String> {
        self.mint_with_workload(
            agent_id, task_id, spawned_by, issuer, ttl_secs, None, None, None, None, None, None,
        )
    }

    /// Mint a signed EdDSA JWT with embedded G10 workload attestation claims.
    /// Optionally includes M19.4 container identity claims.
    #[allow(clippy::too_many_arguments)]
    pub fn mint_with_workload(
        &self,
        agent_id: &str,
        task_id: &str,
        spawned_by: &str,
        issuer: &str,
        ttl_secs: u64,
        wl_pid: Option<u32>,
        wl_hostname: Option<String>,
        wl_compute_target: Option<String>,
        wl_stack_hash: Option<String>,
        wl_container_id: Option<String>,
        wl_image_hash: Option<String>,
    ) -> Result<String, String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let claims = AgentJwtClaims {
            sub: agent_id.to_string(),
            iss: issuer.to_string(),
            iat: now,
            exp: now + ttl_secs,
            scope: "agent".to_string(),
            task_id: task_id.to_string(),
            spawned_by: spawned_by.to_string(),
            wl_pid,
            wl_hostname,
            wl_compute_target,
            wl_stack_hash,
            wl_container_id,
            wl_image_hash,
        };
        let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::EdDSA);
        header.kid = Some(self.kid.clone());
        jsonwebtoken::encode(&header, &claims, &self.encoding_key)
            .map_err(|e| format!("JWT mint error: {e}"))
    }

    /// Validate a self-issued agent JWT, returning its claims on success.
    pub fn validate(&self, token: &str, expected_issuer: &str) -> Result<AgentJwtClaims, String> {
        let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::EdDSA);
        validation.set_issuer(&[expected_issuer]);
        validation.validate_aud = false;
        jsonwebtoken::decode::<AgentJwtClaims>(token, &self.decoding_key, &validation)
            .map(|td| td.claims)
            .map_err(|e| format!("agent JWT validation: {e}"))
    }
}

// -- Security helpers ---------------------------------------------------------

/// Compare two tokens in constant time to prevent timing attacks.
pub(crate) fn tokens_equal(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.as_bytes().ct_eq(b.as_bytes()).into()
}

/// Hash an API key with SHA-256 before storage/lookup.
/// Prevents leaking raw keys if the database is compromised.
pub(crate) fn hash_api_key(key: &str) -> String {
    let result = sha2::Sha256::digest(key.as_bytes());
    result.iter().map(|b| format!("{b:02x}")).collect()
}

/// Resolved principal injected by the auth extractor.
#[derive(Clone)]
pub struct AuthenticatedAgent {
    pub agent_id: String,
    /// Present when auth was performed via JWT or API key.
    pub user_id: Option<Id>,
    pub roles: Vec<UserRole>,
    /// Tenant scope for this request.
    /// - JWT auth: extracted from `tenant_id` claim (defaults to "default").
    /// - All other auth methods: always "default".
    pub tenant_id: String,
    /// Raw JWT claims for ABAC evaluation (G6).
    /// - JWT auth (Keycloak or agent JWT): populated with the full claims object.
    /// - Global token or API key: `None` — ABAC checks are bypassed for these.
    pub jwt_claims: Option<serde_json::Value>,
}

// -- JWT claim types ----------------------------------------------------------

#[derive(Debug, Deserialize)]
struct JwtClaims {
    sub: String,
    #[serde(default)]
    preferred_username: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    realm_access: Option<RealmAccess>,
    /// Tenant scope claim. Absent = "default".
    #[serde(default)]
    tenant_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RealmAccess {
    #[serde(default)]
    roles: Vec<String>,
}

/// Validate a tenant_id value extracted from a JWT claim.
///
/// Rules:
/// - Must be 1-64 ASCII alphanumeric characters, hyphens, or underscores.
/// - The value "system" is reserved for Admin-role callers only.
fn validate_tenant_id(tenant_id: &str, roles: &[UserRole]) -> Result<(), String> {
    if tenant_id.is_empty() || tenant_id.len() > 64 {
        return Err(format!(
            "tenant_id length must be 1-64 chars, got {}",
            tenant_id.len()
        ));
    }
    if !tenant_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!(
            "tenant_id '{tenant_id}' contains invalid characters (allowed: a-z A-Z 0-9 - _)"
        ));
    }
    // "system" is a reserved tenant -- only Admin callers may use it.
    if tenant_id == "system" && !roles.contains(&UserRole::Admin) {
        return Err("tenant 'system' requires Admin role".to_string());
    }
    Ok(())
}

fn roles_from_claims(claims: &JwtClaims) -> Vec<UserRole> {
    let raw = claims
        .realm_access
        .as_ref()
        .map(|ra| ra.roles.as_slice())
        .unwrap_or(&[]);
    raw.iter().filter_map(|s| UserRole::from_str(s)).collect()
}

// -- JWKS refresh -------------------------------------------------------------

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

// -- Auth extractor -----------------------------------------------------------

#[axum::async_trait]
impl FromRequestParts<Arc<AppState>> for AuthenticatedAgent {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        // 0. Check for single-use WS ticket first (?ticket= query param).
        //    Tickets are short-lived (30s), single-use, and don't expose the
        //    real auth token in URLs/logs/browser history.
        if let Some(ticket) = parts.uri.query().and_then(|q| {
            q.split('&')
                .find(|p| p.starts_with("ticket="))
                .map(|p| &p[7..])
        }) {
            if let Some(auth) = state.ws_tickets.consume(ticket) {
                return Ok(auth);
            }
            return Err((StatusCode::UNAUTHORIZED, "Invalid or expired ticket").into_response());
        }

        // Check Authorization header first, then fall back to ?token= query param.
        // DEPRECATED: ?token= leaks credentials to logs/history. Use ?ticket= instead.
        let token = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .or_else(|| {
                parts.uri.query().and_then(|q| {
                    q.split('&')
                        .find(|p| p.starts_with("token="))
                        .map(|p| &p[6..])
                })
            })
            .ok_or_else(|| (StatusCode::UNAUTHORIZED, "Missing Bearer token").into_response())?;

        // 1. Global auth token (dev / system usage). Use constant-time compare.
        if tokens_equal(token, &state.auth_token) {
            return Ok(AuthenticatedAgent {
                agent_id: "system".to_string(),
                user_id: None,
                roles: vec![UserRole::Admin],
                tenant_id: "default".to_string(),
                jwt_claims: None, // Admin bypass — no ABAC evaluation.
            });
        }

        // 2. Per-agent tokens issued at spawn (UUID legacy or JWT).
        {
            let token_pairs = state
                .kv_store
                .kv_list("agent_tokens")
                .await
                .unwrap_or_default();
            if let Some(agent_id) = token_pairs
                .iter()
                .find(|(_, t)| t.as_str() == token)
                .map(|(id, _)| id.clone())
            {
                // JWT tokens: additionally validate signature and expiry.
                let jwt_claims = if token.starts_with("ey") {
                    match state.agent_signing_key.validate(token, &state.base_url) {
                        Ok(agent_claims) => serde_json::to_value(&agent_claims).ok(),
                        Err(e) => {
                            tracing::debug!("Agent JWT validation failed: {e}");
                            return Err((
                                StatusCode::UNAUTHORIZED,
                                "Invalid or expired agent token",
                            )
                                .into_response());
                        }
                    }
                } else {
                    None // Legacy UUID token — no JWT claims for ABAC.
                };
                return Ok(AuthenticatedAgent {
                    agent_id,
                    user_id: None,
                    roles: vec![UserRole::Agent],
                    tenant_id: "default".to_string(),
                    jwt_claims,
                });
            }
        }

        // 2.5. JWT token not found in agent_tokens — treat as revoked or unknown.
        //      Do not fall through to API-key or Keycloak for "ey" tokens,
        //      as that would allow a compromised agent JWT to escalate.
        if token.starts_with("ey") {
            // Attempt cryptographic validation to give a better error message.
            match state.agent_signing_key.validate(token, &state.base_url) {
                Ok(_) => {
                    // Valid signature but not in HashMap — token was revoked.
                    return Err(
                        (StatusCode::UNAUTHORIZED, "Agent token has been revoked").into_response()
                    );
                }
                Err(_) => {
                    // Invalid signature — fall through to Keycloak JWT path.
                    // (Could be a legitimate Keycloak JWT that starts with "ey".)
                }
            }
        }

        // 3. API keys -- look up by SHA-256 hash of the raw token.
        if let Ok(Some(user_id)) = state.api_keys.find_user_id(&hash_api_key(token)).await {
            if let Ok(Some(user)) = state.users.find_by_id(&user_id).await {
                return Ok(AuthenticatedAgent {
                    agent_id: user.display_name.clone(),
                    user_id: Some(user.id),
                    roles: user.roles,
                    tenant_id: "default".to_string(),
                    jwt_claims: None, // API key — no ABAC evaluation.
                });
            }
        }

        // 4. JWT validation.
        if let Some(jwt_cfg) = &state.jwt_config {
            if let Ok(auth) = validate_jwt(token, jwt_cfg, state).await {
                return Ok(auth);
            }
        }

        // 5. Federation JWT from a trusted remote Gyre instance (G11).
        if token.starts_with("ey") {
            if let Some(auth) = validate_federated_jwt(token, state).await {
                return Ok(auth);
            }
        }

        Err((StatusCode::UNAUTHORIZED, "Invalid token").into_response())
    }
}

/// Validate a raw token string and return an `AuthenticatedAgent`.
///
/// Used by the WebSocket handler which receives the token as a message payload
/// rather than an HTTP Authorization header.
pub async fn authenticate_token(
    token: &str,
    state: &Arc<AppState>,
) -> Result<AuthenticatedAgent, &'static str> {
    // 1. Global auth token.
    if tokens_equal(token, &state.auth_token) {
        return Ok(AuthenticatedAgent {
            agent_id: "system".to_string(),
            user_id: None,
            roles: vec![UserRole::Admin],
            tenant_id: "default".to_string(),
            jwt_claims: None,
        });
    }

    // 2. Per-agent tokens.
    {
        let token_pairs = state
            .kv_store
            .kv_list("agent_tokens")
            .await
            .unwrap_or_default();
        if let Some(agent_id) = token_pairs
            .iter()
            .find(|(_, t)| t.as_str() == token)
            .map(|(id, _)| id.clone())
        {
            let jwt_claims = if token.starts_with("ey") {
                match state.agent_signing_key.validate(token, &state.base_url) {
                    Ok(agent_claims) => serde_json::to_value(&agent_claims).ok(),
                    Err(_) => return Err("Invalid or expired agent token"),
                }
            } else {
                None
            };
            return Ok(AuthenticatedAgent {
                agent_id,
                user_id: None,
                roles: vec![UserRole::Agent],
                tenant_id: "default".to_string(),
                jwt_claims,
            });
        }
    }

    // 2.5. JWT token not in agent_tokens — check if revoked.
    if token.starts_with("ey")
        && state
            .agent_signing_key
            .validate(token, &state.base_url)
            .is_ok()
    {
        return Err("Agent token has been revoked");
    }

    // 3. API keys.
    if let Ok(Some(user_id)) = state.api_keys.find_user_id(&hash_api_key(token)).await {
        if let Ok(Some(user)) = state.users.find_by_id(&user_id).await {
            return Ok(AuthenticatedAgent {
                agent_id: user.display_name.clone(),
                user_id: Some(user.id),
                roles: user.roles,
                tenant_id: "default".to_string(),
                jwt_claims: None,
            });
        }
    }

    // 4. Keycloak JWT.
    if let Some(jwt_cfg) = &state.jwt_config {
        if let Ok(auth) = validate_jwt(token, jwt_cfg, state).await {
            return Ok(auth);
        }
    }

    // 5. Federated JWT.
    if token.starts_with("ey") {
        if let Some(auth) = validate_federated_jwt(token, state).await {
            return Ok(auth);
        }
    }

    Err("Invalid token")
}

/// Public wrapper for middleware JWT validation (checks token validity only).
pub async fn validate_jwt_middleware(
    token: &str,
    jwt_cfg: &crate::JwtConfig,
    state: &Arc<crate::AppState>,
) -> Result<(), String> {
    validate_jwt(token, jwt_cfg, state).await.map(|_| ())
}

/// Public wrapper for federation JWT validation used by the global auth middleware.
pub async fn validate_federated_jwt_middleware(
    token: &str,
    state: &Arc<crate::AppState>,
) -> Result<(), String> {
    validate_federated_jwt(token, state)
        .await
        .map(|_| ())
        .ok_or_else(|| "federated JWT validation failed".to_string())
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

    // Also capture the raw claims as JSON for ABAC evaluation (G6).
    let raw_claims = jsonwebtoken::decode::<serde_json::Value>(token, &decoding_key, &validation)
        .map(|td| td.claims)
        .ok();

    let claims = token_data.claims;
    let roles = roles_from_claims(&claims);
    let username = claims
        .preferred_username
        .clone()
        .unwrap_or_else(|| claims.sub.clone());

    // Extract and validate tenant_id from JWT claim.
    let tenant_id = claims.tenant_id.as_deref().unwrap_or("default").to_string();
    validate_tenant_id(&tenant_id, &roles).map_err(|e| format!("invalid tenant_id in JWT: {e}"))?;

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
        agent_id: user.display_name.clone(),
        user_id: Some(user.id),
        roles: user.roles,
        tenant_id,
        jwt_claims: raw_claims,
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

// -- Federation JWT validation (G11) ------------------------------------------

/// Extract the `iss` claim from a JWT payload without verifying the signature.
/// Used to route federation tokens to the correct remote JWKS endpoint.
fn extract_iss_from_jwt_payload(token: &str) -> Option<String> {
    let payload_b64 = token.split('.').nth(1)?;
    let payload_bytes = URL_SAFE_NO_PAD.decode(payload_b64).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&payload_bytes).ok()?;
    value.get("iss")?.as_str().map(|s| s.to_string())
}

/// Return true iff `jwks_uri` shares the same scheme and host as `issuer`.
///
/// Prevents SSRF: a compromised remote Gyre's discovery document could set
/// `jwks_uri` to an internal metadata endpoint (e.g. `http://169.254.169.254/`)
/// unless we verify it stays on the same origin as the trusted issuer.
fn is_same_origin(issuer: &str, jwks_uri: &str) -> bool {
    let iss = url::Url::parse(issuer).ok();
    let jwks = url::Url::parse(jwks_uri).ok();
    match (iss, jwks) {
        (Some(a), Some(b)) => {
            a.scheme() == b.scheme()
                && a.host() == b.host()
                && a.port_or_known_default() == b.port_or_known_default()
        }
        _ => false,
    }
}

/// Fetch JWKS keys for a remote Gyre issuer via OIDC discovery.
async fn fetch_remote_jwks_for_issuer(
    issuer: &str,
    http_client: &reqwest::Client,
) -> Option<HashMap<String, jsonwebtoken::DecodingKey>> {
    let discovery_url = format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    );
    let discovery: OidcDiscovery = http_client
        .get(&discovery_url)
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;

    // G11-A: Reject jwks_uri that redirects to a different origin (SSRF guard).
    if !is_same_origin(issuer, &discovery.jwks_uri) {
        tracing::warn!(
            issuer = %issuer,
            jwks_uri = %discovery.jwks_uri,
            "Federation SSRF guard: jwks_uri is not same-origin as issuer — rejecting"
        );
        return None;
    }

    let jwk_set: JwkSet = http_client
        .get(&discovery.jwks_uri)
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;

    let mut keys = HashMap::new();
    for jwk in &jwk_set.keys {
        let kid = match jwk.get("kid").and_then(|v| v.as_str()) {
            Some(k) => k.to_string(),
            None => continue,
        };
        let jwk_typed: jsonwebtoken::jwk::Jwk = match serde_json::from_value(jwk.clone()) {
            Ok(k) => k,
            Err(e) => {
                tracing::warn!("Federation: failed to parse JWK {kid} from {issuer}: {e}");
                continue;
            }
        };
        match jsonwebtoken::DecodingKey::from_jwk(&jwk_typed) {
            Ok(dk) => {
                keys.insert(kid, dk);
            }
            Err(e) => tracing::warn!(
                "Federation: failed to build DecodingKey for {kid} from {issuer}: {e}"
            ),
        }
    }

    if keys.is_empty() {
        tracing::warn!("Federation: no valid keys found in JWKS for {issuer}");
        None
    } else {
        Some(keys)
    }
}

/// Validate a JWT from a trusted remote Gyre instance (EdDSA/Ed25519).
///
/// Returns `Some(AuthenticatedAgent)` if the token is valid and its `iss`
/// matches a configured trusted issuer. Returns `None` if not applicable
/// (unknown issuer, invalid signature, or expired).
async fn validate_federated_jwt(token: &str, state: &Arc<AppState>) -> Option<AuthenticatedAgent> {
    if state.trusted_issuers.is_empty() {
        return None;
    }

    // Extract issuer without verifying signature.
    let iss = extract_iss_from_jwt_payload(token)?;

    // Check if the issuer is trusted.
    let normalized_iss = iss.trim_end_matches('/').to_string();
    if !state
        .trusted_issuers
        .iter()
        .any(|t| t.trim_end_matches('/') == normalized_iss)
    {
        return None;
    }

    let header = jsonwebtoken::decode_header(token).ok()?;
    let kid = header.kid.unwrap_or_else(|| "default".to_string());

    // Check cache for a fresh entry.
    let decoding_key = {
        let cache = state.remote_jwks_cache.read().await;
        cache.get(&normalized_iss).and_then(|entry| {
            if entry.fetched_at.elapsed().as_secs() < REMOTE_JWKS_TTL_SECS {
                entry.keys.get(&kid).cloned()
            } else {
                None
            }
        })
    };

    let decoding_key = if let Some(dk) = decoding_key {
        dk
    } else {
        // Fetch fresh JWKS.
        let keys = fetch_remote_jwks_for_issuer(&normalized_iss, &state.http_client).await?;
        let dk = keys.get(&kid).cloned()?;
        let mut cache = state.remote_jwks_cache.write().await;
        cache.insert(
            normalized_iss.clone(),
            RemoteJwksEntry {
                keys,
                fetched_at: std::time::Instant::now(),
            },
        );
        dk
    };

    // Validate signature and standard claims (exp, iss).
    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::EdDSA);
    validation.set_issuer(&[&normalized_iss]);
    validation.validate_aud = false;

    let token_data =
        jsonwebtoken::decode::<AgentJwtClaims>(token, &decoding_key, &validation).ok()?;
    let claims = token_data.claims;

    tracing::debug!(
        issuer = %normalized_iss,
        sub = %claims.sub,
        "Federated JWT accepted"
    );

    // Federated agents receive Agent role; agent_id is prefixed with the
    // remote host to avoid collisions with local agent IDs.
    let remote_host = normalized_iss
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let fed_claims_json = serde_json::to_value(&claims).ok();
    Some(AuthenticatedAgent {
        agent_id: format!("{remote_host}/{}", claims.sub),
        user_id: None,
        roles: vec![UserRole::Agent],
        tenant_id: "default".to_string(),
        jwt_claims: fed_claims_json,
    })
}

// AdminOnly extractor removed (M34 Slice 4). ABAC middleware is the sole
// authorization layer. Handlers that previously used AdminOnly now use
// AuthenticatedAgent directly; ABAC middleware enforces admin-only access
// via the admin-all-operations built-in policy.

// -- WebSocket ticket-based auth -----------------------------------------------
//
// Browser WebSocket API does not support custom headers. Instead of passing
// the real auth token in a query parameter (which leaks to logs, browser
// history, and proxy caches), we issue short-lived, single-use tickets.
//
// Flow:
//   1. Client calls POST /api/v1/ws-ticket with Bearer token in header
//   2. Server returns { "ticket": "<random>" } (valid 30 s, single use)
//   3. Client connects to WS with ?ticket=<random>
//   4. Server validates and consumes the ticket atomically

/// TTL for WebSocket tickets (30 seconds).
const WS_TICKET_TTL_SECS: u64 = 30;
/// Maximum tickets in the store (prevents DoS via ticket flooding).
const WS_TICKET_MAX: usize = 10_000;

/// A single-use WebSocket auth ticket.
#[derive(Clone)]
pub struct WsTicketEntry {
    /// The resolved identity for this ticket.
    pub auth: AuthenticatedAgent,
    /// When the ticket was created (monotonic).
    pub created: std::time::Instant,
}

/// Thread-safe ticket store with automatic GC.
#[derive(Clone, Default)]
pub struct WsTicketStore {
    tickets: Arc<std::sync::Mutex<HashMap<String, WsTicketEntry>>>,
}

impl WsTicketStore {
    pub fn new() -> Self {
        Self {
            tickets: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Issue a new ticket for the given authenticated identity.
    /// Returns the opaque ticket string.
    pub fn issue(&self, auth: AuthenticatedAgent) -> String {
        let ticket = uuid::Uuid::new_v4().to_string();
        let mut store = self.tickets.lock().unwrap_or_else(|e| e.into_inner());
        // GC expired tickets and enforce cap
        let now = std::time::Instant::now();
        store.retain(|_, v| now.duration_since(v.created).as_secs() < WS_TICKET_TTL_SECS);
        if store.len() >= WS_TICKET_MAX {
            // Drop oldest entries
            let mut entries: Vec<_> = store.drain().collect();
            entries.sort_by_key(|(_, v)| v.created);
            entries.truncate(WS_TICKET_MAX / 2);
            store.extend(entries);
        }
        store.insert(ticket.clone(), WsTicketEntry { auth, created: now });
        ticket
    }

    /// Consume a ticket: returns the auth identity if valid, removes it atomically.
    /// Returns None if expired, already used, or not found.
    pub fn consume(&self, ticket: &str) -> Option<AuthenticatedAgent> {
        let mut store = self.tickets.lock().unwrap_or_else(|e| e.into_inner());
        let entry = store.remove(ticket)?;
        let age = std::time::Instant::now().duration_since(entry.created);
        if age.as_secs() >= WS_TICKET_TTL_SECS {
            return None; // Expired
        }
        Some(entry.auth)
    }
}

// -- Tests --------------------------------------------------------------------

#[cfg(test)]
pub mod test_helpers {
    use super::*;
    use crate::JwtConfig;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
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
    /// Does NOT use jsonwebtoken's PEM feature -- no simple_asn1/time MSRV issue.
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
    use std::sync::Arc;
    use tower::ServiceExt;

    use super::{test_helpers::*, AuthenticatedAgent, WsTicketStore};
    use gyre_domain::UserRole;

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
            .kv_store
            .kv_set("agent_tokens", "agent-42", "agent-secret".to_string())
            .await
            .unwrap();

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
        assert_eq!(user.display_name, "dave");
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
        // Store the SHA-256 hash of the API key (matching auth extractor behaviour).
        let raw_key = "gyre_test_api_key";
        state
            .api_keys
            .create(&super::hash_api_key(raw_key), &user.id, "test-key")
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
            .kv_store
            .kv_set("agent_tokens", "agt-1", "agt-secret-xyz".to_string())
            .await
            .unwrap();

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

    // -- Tenant resolution tests ----------------------------------------------

    #[tokio::test]
    async fn jwt_tenant_id_extracted_from_claim() {
        use super::AuthenticatedAgent;

        let state = make_test_state_with_jwt();

        async fn tenant_handler(auth: AuthenticatedAgent) -> String {
            auth.tenant_id
        }

        let claims = serde_json::json!({
            "sub": "tenant-test-sub",
            "preferred_username": "grace",
            "tenant_id": "acme-corp"
        });
        let token = sign_test_jwt(&claims, 3600);

        let app: Router = Router::new()
            .route("/tenant", get(tenant_handler))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/tenant")
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
        assert_eq!(body.as_ref(), b"acme-corp");
    }

    #[tokio::test]
    async fn jwt_missing_tenant_id_defaults_to_default() {
        use super::AuthenticatedAgent;

        let state = make_test_state_with_jwt();

        async fn tenant_handler(auth: AuthenticatedAgent) -> String {
            auth.tenant_id
        }

        let claims = serde_json::json!({
            "sub": "no-tenant-sub",
            "preferred_username": "henry"
        });
        let token = sign_test_jwt(&claims, 3600);

        let app: Router = Router::new()
            .route("/tenant", get(tenant_handler))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/tenant")
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
        assert_eq!(body.as_ref(), b"default");
    }

    #[tokio::test]
    async fn jwt_system_tenant_rejected_for_non_admin() {
        let claims = serde_json::json!({
            "sub": "attacker-sub",
            "preferred_username": "attacker",
            "realm_access": { "roles": ["developer"] },
            "tenant_id": "system"
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
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn jwt_system_tenant_allowed_for_admin() {
        use super::AuthenticatedAgent;

        let state = make_test_state_with_jwt();

        async fn tenant_handler(auth: AuthenticatedAgent) -> String {
            auth.tenant_id
        }

        let claims = serde_json::json!({
            "sub": "admin-sub",
            "preferred_username": "alice-admin",
            "realm_access": { "roles": ["admin"] },
            "tenant_id": "system"
        });
        let token = sign_test_jwt(&claims, 3600);

        let app: Router = Router::new()
            .route("/tenant", get(tenant_handler))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/tenant")
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
        assert_eq!(body.as_ref(), b"system");
    }

    // -- Federation JWT tests (G11) -------------------------------------------

    /// Build a minimal AppState that trusts a given issuer URL.
    fn make_state_with_trusted_issuer(issuer_url: &str) -> Arc<crate::AppState> {
        let base = crate::mem::test_state();
        let mut state = (*base).clone();
        state.trusted_issuers = vec![issuer_url.to_string()];
        Arc::new(state)
    }

    /// Mint an EdDSA JWT using a fresh AgentSigningKey, simulating a remote Gyre.
    fn mint_remote_jwt(
        signing_key: &crate::auth::AgentSigningKey,
        issuer: &str,
        sub: &str,
        ttl_secs: u64,
    ) -> String {
        signing_key
            .mint(sub, "task-1", "system", issuer, ttl_secs)
            .expect("mint must succeed")
    }

    /// Inject a remote JWKS entry into the state's cache so we don't need an
    /// actual HTTP server for most tests.
    async fn seed_remote_jwks(
        state: &Arc<crate::AppState>,
        issuer: &str,
        remote_key: &crate::auth::AgentSigningKey,
    ) {
        use jsonwebtoken::DecodingKey;
        let jwks: serde_json::Value =
            serde_json::from_str(&remote_key.jwks_json).expect("valid JWKS");
        let keys_arr = jwks["keys"].as_array().expect("keys array");
        let mut map = std::collections::HashMap::new();
        for jwk in keys_arr {
            let jwk_typed: jsonwebtoken::jwk::Jwk =
                serde_json::from_value(jwk.clone()).expect("valid JWK");
            let dk = DecodingKey::from_jwk(&jwk_typed).expect("valid DecodingKey");
            let kid = jwk["kid"].as_str().unwrap_or("default").to_string();
            map.insert(kid, dk);
        }
        state.remote_jwks_cache.write().await.insert(
            issuer.trim_end_matches('/').to_string(),
            crate::auth::RemoteJwksEntry {
                keys: map,
                fetched_at: std::time::Instant::now(),
            },
        );
    }

    #[tokio::test]
    async fn federated_jwt_from_trusted_issuer_accepted() {
        let remote_url = "http://remote-gyre.example.com";
        let state = make_state_with_trusted_issuer(remote_url);
        let remote_key = crate::auth::AgentSigningKey::generate();
        seed_remote_jwks(&state, remote_url, &remote_key).await;

        let token = mint_remote_jwt(&remote_key, remote_url, "agent-abc", 3600);

        let app: Router = Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state);

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
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = std::str::from_utf8(&bytes).unwrap();
        // agent_id should be prefixed with the remote host
        assert!(
            body.contains("agent-abc"),
            "expected agent-abc in agent_id, got: {body}"
        );
    }

    #[tokio::test]
    async fn federated_jwt_from_untrusted_issuer_rejected() {
        // Trust "http://trusted.example.com" but present a token from "http://evil.example.com"
        let state = make_state_with_trusted_issuer("http://trusted.example.com");
        let evil_key = crate::auth::AgentSigningKey::generate();
        // Do NOT seed the trusted cache — JWKS fetch would fail for the evil issuer anyway,
        // but the issuer check should reject before attempting JWKS fetch.

        let token = mint_remote_jwt(&evil_key, "http://evil.example.com", "evil-agent", 3600);

        let app: Router = Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state);

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
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn federated_jwt_expired_rejected() {
        let remote_url = "http://remote-gyre.example.com";
        let state = make_state_with_trusted_issuer(remote_url);
        let remote_key = crate::auth::AgentSigningKey::generate();
        seed_remote_jwks(&state, remote_url, &remote_key).await;

        // Negative TTL — token is already expired.
        // We use jsonwebtoken directly to mint an expired token.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let claims = crate::auth::AgentJwtClaims {
            sub: "agent-xyz".to_string(),
            iss: remote_url.to_string(),
            iat: now - 7200,
            exp: now - 3600,
            scope: "agent".to_string(),
            task_id: "task-1".to_string(),
            spawned_by: "system".to_string(),
            wl_pid: None,
            wl_hostname: None,
            wl_compute_target: None,
            wl_stack_hash: None,
            wl_container_id: None,
            wl_image_hash: None,
        };
        let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::EdDSA);
        header.kid = Some(remote_key.kid.clone());
        let token = jsonwebtoken::encode(&header, &claims, &remote_key.encoding_key).expect("mint");

        let app: Router = Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state);

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
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn federated_jwt_wrong_signature_rejected() {
        let remote_url = "http://remote-gyre.example.com";
        let state = make_state_with_trusted_issuer(remote_url);
        let trusted_key = crate::auth::AgentSigningKey::generate();
        seed_remote_jwks(&state, remote_url, &trusted_key).await;

        // Sign with a DIFFERENT key — same issuer but wrong signature.
        let attacker_key = crate::auth::AgentSigningKey::generate();
        let token = mint_remote_jwt(&attacker_key, remote_url, "agent-evil", 3600);

        let app: Router = Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state);

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
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn federated_jwt_no_trusted_issuers_rejected() {
        // State with no trusted issuers — federation step skipped entirely.
        let state = test_state(); // default test_state has trusted_issuers: vec![]
        let remote_key = crate::auth::AgentSigningKey::generate();
        let token = mint_remote_jwt(&remote_key, "http://some-gyre.example.com", "agent-1", 3600);

        let app: Router = Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state);

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
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn jwt_invalid_tenant_id_chars_rejected() {
        let claims = serde_json::json!({
            "sub": "bad-tenant-sub",
            "preferred_username": "badguy",
            "tenant_id": "acme; DROP TABLE tenants"
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
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // -- G11-A SSRF guard tests -----------------------------------------------

    #[test]
    fn is_same_origin_same_host_accepted() {
        assert!(super::is_same_origin(
            "https://remote-gyre.example.com",
            "https://remote-gyre.example.com/jwks"
        ));
    }

    #[test]
    fn is_same_origin_different_host_rejected() {
        assert!(!super::is_same_origin(
            "https://remote-gyre.example.com",
            "http://169.254.169.254/latest/meta-data/iam/security-credentials/"
        ));
    }

    #[test]
    fn is_same_origin_different_scheme_rejected() {
        assert!(!super::is_same_origin(
            "https://remote-gyre.example.com",
            "http://remote-gyre.example.com/jwks"
        ));
    }

    #[test]
    fn is_same_origin_malformed_uri_rejected() {
        assert!(!super::is_same_origin(
            "https://remote-gyre.example.com",
            "not a url"
        ));
    }

    #[test]
    fn is_same_origin_different_port_rejected() {
        assert!(!super::is_same_origin(
            "https://remote-gyre.example.com",
            "https://remote-gyre.example.com:9999/jwks"
        ));
    }

    #[tokio::test]
    async fn cross_origin_jwks_uri_rejected_by_ssrf_guard() {
        use axum::routing::get;
        use axum::Router;
        use http::{Request, StatusCode};
        use tower::ServiceExt;

        // Build a state that trusts our "remote" issuer.
        let remote_url = "http://remote-gyre.example.com";
        let state = make_state_with_trusted_issuer(remote_url);
        let remote_key = crate::auth::AgentSigningKey::generate();

        // Instead of injecting via seed_remote_jwks (which bypasses the HTTP
        // fetch), we inject a *stale* cache entry so the code re-fetches.
        // But since we can't start an HTTP server here, we rely on the unit
        // test for is_same_origin above to cover the guard logic directly.
        //
        // This integration-level test verifies that when the cache is empty
        // and the issuer is trusted, but no real JWKS can be fetched (network
        // unavailable in test), the token is ultimately rejected.
        let token = mint_remote_jwt(&remote_key, remote_url, "agent-ssrf", 3600);

        let app: Router = Router::new()
            .route("/protected", get(authenticated_handler))
            .with_state(state);

        // No JWKS seeded → fetch_remote_jwks_for_issuer will fail (no real
        // server) → token rejected.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn non_jwt_auth_uses_default_tenant() {
        use super::AuthenticatedAgent;

        let state = test_state();

        async fn tenant_handler(auth: AuthenticatedAgent) -> String {
            auth.tenant_id
        }

        let app = Router::new()
            .route("/tenant", get(tenant_handler))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/tenant")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(body.as_ref(), b"default");
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

    #[test]
    fn test_ws_ticket_issue_and_consume() {
        let store = WsTicketStore::new();
        let auth = AuthenticatedAgent {
            agent_id: "test-user".to_string(),
            user_id: None,
            roles: vec![UserRole::Admin],
            tenant_id: "default".to_string(),
            jwt_claims: None,
        };
        let ticket = store.issue(auth);
        assert!(!ticket.is_empty());

        // First consume succeeds
        let result = store.consume(&ticket);
        assert!(result.is_some());
        assert_eq!(result.unwrap().agent_id, "test-user");

        // Second consume fails (single-use)
        assert!(store.consume(&ticket).is_none());
    }

    #[test]
    fn test_ws_ticket_invalid_ticket() {
        let store = WsTicketStore::new();
        assert!(store.consume("nonexistent-ticket").is_none());
    }

    #[test]
    fn test_ws_ticket_does_not_expose_real_token() {
        let store = WsTicketStore::new();
        let auth = AuthenticatedAgent {
            agent_id: "test-user".to_string(),
            user_id: None,
            roles: vec![UserRole::Admin],
            tenant_id: "default".to_string(),
            jwt_claims: None,
        };
        let ticket = store.issue(auth);
        // Ticket is a UUID, not a Bearer token or API key
        assert!(ticket.len() == 36, "Ticket should be a UUID format");
        assert!(!ticket.starts_with("ey"), "Ticket must not be a JWT");
    }
}
