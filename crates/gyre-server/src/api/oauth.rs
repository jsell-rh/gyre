//! Claude Max account enrollment via OAuth 2.0 + PKCE
//! (admin-panel.md §Credential Management: "Claude Max accounts supported
//! via OAuth + token refresh"; TASK-273).
//!
//! Flow:
//!
//! 1. `POST /api/v1/admin/oauth/claude-max/enroll` (Admin only): generates
//!    a random `state` + PKCE `code_verifier`, persists them via the
//!    `OAuthStateRepository` port with the initiating admin and tenant
//!    derived SERVER-SIDE from the authenticated caller (never
//!    client-supplied), and redirects (302) to the provider's
//!    authorization endpoint.
//! 2. The provider redirects to
//!    `GET /api/v1/oauth/claude-max/callback?code=…&state=…` (public: the
//!    state row is the capability; per-handler authorization = state
//!    validation — exists, unexpired, unconsumed, single-use).
//! 3. The callback exchanges the code at the provider token endpoint with
//!    the PKCE verifier (latency-bounded HTTP) and stores the resulting
//!    refresh token as the scoped `claude_max_refresh_token` secret — an
//!    OAuth credential document the `secret_rotation_check` job's
//!    RFC 6749 handler then keeps fresh.
//!
//! Provider configuration (env): `GYRE_CLAUDE_MAX_CLIENT_ID` (required for
//! enrollment), `GYRE_CLAUDE_MAX_AUTH_ENDPOINT`,
//! `GYRE_CLAUDE_MAX_TOKEN_ENDPOINT`, `GYRE_CLAUDE_MAX_REDIRECT_URI`,
//! `GYRE_CLAUDE_MAX_SCOPE`, optional `GYRE_CLAUDE_MAX_CLIENT_SECRET`.

use axum::extract::{Query, State};
use axum::response::Redirect;
use axum::Json;
use gyre_common::{Id, Secret, SecretScope, SecretType};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::auth::AuthenticatedAgent;
use crate::AppState;

use super::error::ApiError;
use super::now_secs;

/// Lifetime of an authorization-request state row (seconds). The provider
/// round-trip must complete within it; afterwards the row is expired and
/// only eligible for `oauth_state_cleanup`.
const STATE_TTL_SECS: u64 = 600;

/// The stored secret's name (the refresh job's OAuth credential document).
pub const CLAUDE_MAX_SECRET_NAME: &str = "claude_max_refresh_token";

/// Provider endpoints and client config captured at initiation so the
/// callback uses the same provider config the admin was redirected to.
#[derive(Serialize, Deserialize, Clone)]
pub struct ProviderConfig {
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    pub redirect_uri: String,
    pub scope: String,
}

/// Load the provider config from the environment. Fails (fail-closed) when
/// the client id is not configured — enrollment without it cannot complete.
fn provider_config_from_env() -> Result<ProviderConfig, ApiError> {
    let client_id = std::env::var("GYRE_CLAUDE_MAX_CLIENT_ID")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| {
            ApiError::InvalidInput(
                "Claude Max enrollment is not configured: set GYRE_CLAUDE_MAX_CLIENT_ID \
                 (plus GYRE_CLAUDE_MAX_AUTH_ENDPOINT / GYRE_CLAUDE_MAX_TOKEN_ENDPOINT / \
                 GYRE_CLAUDE_MAX_REDIRECT_URI as needed)"
                    .to_string(),
            )
        })?;
    Ok(ProviderConfig {
        authorization_endpoint: std::env::var("GYRE_CLAUDE_MAX_AUTH_ENDPOINT")
            .unwrap_or_else(|_| "https://claude.ai/oauth/authorize".to_string()),
        token_endpoint: std::env::var("GYRE_CLAUDE_MAX_TOKEN_ENDPOINT")
            .unwrap_or_else(|_| "https://console.anthropic.com/v1/oauth/token".to_string()),
        client_secret: std::env::var("GYRE_CLAUDE_MAX_CLIENT_SECRET")
            .ok()
            .filter(|v| !v.trim().is_empty()),
        redirect_uri: std::env::var("GYRE_CLAUDE_MAX_REDIRECT_URI")
            .unwrap_or_else(|_| "/api/v1/oauth/claude-max/callback".to_string()),
        scope: std::env::var("GYRE_CLAUDE_MAX_SCOPE")
            .unwrap_or_else(|_| "user:profile user:inference".to_string()),
        client_id,
    })
}

/// Cryptographically random hex string of `bytes` random bytes.
fn random_hex(bytes: usize) -> Result<String, ApiError> {
    use ring::rand::SecureRandom;
    let mut buf = vec![0u8; bytes];
    ring::rand::SystemRandom::new()
        .fill(&mut buf)
        .map_err(|_| ApiError::Internal(anyhow::anyhow!("system RNG failure").into()))?;
    Ok(hex::encode(buf))
}

/// PKCE code verifier: 64 random bytes, base64url (no padding) — RFC 7636
/// §4.1 allows 43..128 unreserved characters.
fn pkce_verifier() -> Result<String, ApiError> {
    use base64::Engine as _;
    let raw = random_hex(32)?; // 64 hex chars ≈ 43+ unreserved chars
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw.as_bytes()))
}

/// S256 code challenge for a verifier (RFC 7636 §4.2).
fn pkce_challenge(verifier: &str) -> String {
    use base64::Engine as _;
    use sha2::Digest;
    let digest = sha2::Sha256::digest(verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

#[derive(Deserialize)]
pub struct EnrollRequest {
    /// Workspace to scope the enrollment to. Optional: absent (or `null`)
    /// scopes the resulting secret at the caller's tenant level. Validated
    /// server-side against the caller's tenant.
    pub workspace_id: Option<String>,
}

/// POST /api/v1/admin/oauth/claude-max/enroll — Admin-only initiation of
/// the Claude Max OAuth enrollment. Persists the authorization-request
/// state (server-derived admin + tenant, PKCE verifier, expiry) and
/// redirects (302) to the provider's authorization endpoint.
pub async fn enroll_claude_max(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    body: Option<Json<EnrollRequest>>,
) -> Result<Redirect, ApiError> {
    let endpoint = "/api/v1/admin/oauth/claude-max/enroll";
    if !auth.roles.contains(&gyre_domain::UserRole::Admin) {
        crate::rbac::record_rbac_denied(&state, &auth, "Admin", endpoint).await;
        return Err(ApiError::Forbidden(
            "Claude Max enrollment requires Admin role".to_string(),
        ));
    }

    let provider = provider_config_from_env()?;

    // Server-derived scoping: the tenant ALWAYS comes from the authenticated
    // caller; the optional workspace is validated against it. The initiating
    // admin is likewise server-derived — scope fields are never taken from
    // the request body (anti-forgery: task-273 cross-cutting invariants).
    let tenant_id = auth.tenant_id.clone();
    let workspace_id = match body.and_then(|Json(r)| r.workspace_id) {
        None => None,
        Some(ws_id) => {
            let ws = state
                .workspaces
                .find_by_id(&Id::new(&ws_id))
                .await
                .map_err(ApiError::Internal)?
                .ok_or_else(|| ApiError::NotFound(format!("workspace '{ws_id}' not found")))?;
            if ws.tenant_id.to_string() != tenant_id {
                return Err(ApiError::Forbidden(
                    "workspace belongs to a different tenant".to_string(),
                ));
            }
            Some(ws_id)
        }
    };
    let initiated_by = auth
        .user_id
        .map(|u| u.to_string())
        .unwrap_or_else(|| auth.agent_id.clone());

    let state_value = random_hex(32)?;
    let verifier = pkce_verifier()?;
    let now = now_secs();
    let record = gyre_ports::OAuthStateRecord {
        state: state_value.clone(),
        tenant_id: tenant_id.clone(),
        workspace_id: workspace_id.clone(),
        initiated_by: initiated_by.clone(),
        pkce_verifier: verifier.clone(),
        created_at: now,
        expires_at: now + STATE_TTL_SECS,
        consumed: false,
        provider_config: serde_json::to_string(&provider).map_err(|e| {
            ApiError::Internal(anyhow::anyhow!("serializing provider config: {e}").into())
        })?,
    };
    state
        .oauth_states
        .create(&record)
        .await
        .map_err(ApiError::Internal)?;

    // Authorization request (OAuth 2.0 + PKCE S256), query-encoded by
    // the url crate (reqwest dependency) rather than string interpolation.
    let authorization_url = reqwest::Url::parse_with_params(
        &provider.authorization_endpoint,
        &[
            ("response_type", "code"),
            ("client_id", provider.client_id.as_str()),
            ("redirect_uri", provider.redirect_uri.as_str()),
            ("scope", provider.scope.as_str()),
            ("state", state_value.as_str()),
            ("code_challenge", pkce_challenge(&verifier).as_str()),
            ("code_challenge_method", "S256"),
        ],
    )
    .map_err(|e| ApiError::Internal(anyhow::anyhow!("building authorization URL: {e}").into()))?;

    tracing::info!(
        event = "oauth.enrollment_started",
        tenant_id = %tenant_id,
        workspace_id = ?workspace_id,
        initiated_by = %initiated_by,
        "Claude Max OAuth enrollment initiated"
    );

    Ok(Redirect::temporary(authorization_url.as_str()))
}

#[derive(Deserialize)]
pub struct CallbackParams {
    pub code: Option<String>,
    pub state: String,
    /// Providers report failures back on the redirect (RFC 6749 §4.1.2.1).
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct CallbackOutcome {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    pub secret_name: String,
}

/// GET /api/v1/oauth/claude-max/callback — provider redirect target.
///
/// Per-handler authorization (resolver-exempt route): the single-use state
/// row IS the capability — the callback rejects unknown, expired, or
/// already-consumed states. On success the code is exchanged with the PKCE
/// verifier at the token endpoint (latency-bounded HTTP) and the resulting
/// refresh token is stored as the scoped `claude_max_refresh_token` secret.
pub async fn claude_max_callback(
    State(state): State<Arc<AppState>>,
    Query(params): Query<CallbackParams>,
) -> Result<Json<CallbackOutcome>, ApiError> {
    if let Some(err) = params.error {
        return Err(ApiError::InvalidInput(format!(
            "provider denied the authorization request: {err}"
        )));
    }
    let code = params.code.ok_or_else(|| {
        ApiError::InvalidInput("callback is missing the authorization code".to_string())
    })?;

    // State validation: exists, unexpired, single-use.
    let record = state
        .oauth_states
        .find_by_state(&params.state)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| {
            ApiError::InvalidInput("unknown OAuth state (expired or forged)".to_string())
        })?;
    let now = now_secs();
    if record.consumed {
        return Err(ApiError::InvalidInput(
            "OAuth state was already used (replay rejected)".to_string(),
        ));
    }
    if record.expires_at <= now {
        return Err(ApiError::InvalidInput("OAuth state expired".to_string()));
    }
    // Consume BEFORE the network exchange: a replayed callback must fail
    // even if the first exchange errored.
    state
        .oauth_states
        .mark_consumed(&params.state)
        .await
        .map_err(ApiError::Internal)?;

    let provider: ProviderConfig = serde_json::from_str(&record.provider_config).map_err(|e| {
        ApiError::Internal(anyhow::anyhow!("stored provider config is unreadable: {e}").into())
    })?;

    // Authorization-code + PKCE exchange at the provider token endpoint.
    // state.http_client is built with a 30 s total timeout, so a hung
    // provider cannot stall the callback.
    let mut form: Vec<(&str, &str)> = vec![
        ("grant_type", "authorization_code"),
        ("code", code.as_str()),
        ("redirect_uri", provider.redirect_uri.as_str()),
        ("client_id", provider.client_id.as_str()),
        ("code_verifier", record.pkce_verifier.as_str()),
    ];
    if let Some(cs) = provider.client_secret.as_deref() {
        form.push(("client_secret", cs));
    }
    let resp = state
        .http_client
        .post(&provider.token_endpoint)
        .form(&form)
        .send()
        .await
        .map_err(|e| ApiError::BadGateway(format!("token endpoint request failed: {e}")))?;
    let status = resp.status();
    let body = resp
        .bytes()
        .await
        .map_err(|e| ApiError::BadGateway(format!("reading token response: {e}")))?;
    if !status.is_success() {
        return Err(ApiError::BadGateway(format!(
            "token endpoint returned HTTP {status}"
        )));
    }

    let tokens: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|e| ApiError::BadGateway(format!("token response is not JSON: {e}")))?;
    let refresh_token = tokens
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::BadGateway("token response missing refresh_token".to_string()))?
        .to_string();
    let access_token = tokens
        .get("access_token")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let expires_in = tokens
        .get("expires_in")
        .and_then(|v| v.as_u64())
        .unwrap_or(3600);

    // Store the OAuth credential document (the refresh job's RFC 6749
    // handler keeps it fresh) as the scoped secret. Scope and tenant come
    // from the validated state row — server-derived at initiation.
    let (scope, scope_id) = match &record.workspace_id {
        Some(ws_id) => (SecretScope::Workspace, ws_id.clone()),
        None => (SecretScope::Tenant, record.tenant_id.clone()),
    };
    let doc = serde_json::json!({
        "token_endpoint": provider.token_endpoint,
        "client_id": provider.client_id,
        "client_secret": provider.client_secret,
        "refresh_token": refresh_token,
        "access_token": access_token,
    });
    let value = serde_json::to_vec(&doc)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("serializing credential: {e}").into()))?;

    let secret = Secret {
        id: Id::new(uuid::Uuid::new_v4().to_string()),
        name: CLAUDE_MAX_SECRET_NAME.to_string(),
        scope,
        scope_id: scope_id.clone(),
        secret_type: SecretType::Rotated,
        created_by: format!("oauth:claude-max:{}", record.initiated_by),
        created_at: now,
        expires_at: Some(now + expires_in),
        last_rotated_at: None,
        tenant_id: record.tenant_id.clone(),
    };

    // Idempotent re-enrollment: an existing secret at the same scope is
    // rotated (value replaced) instead of failing on the port's
    // duplicate-(scope, name) constraint.
    let existing = state
        .secrets
        .list_by_scope(scope, &scope_id, &record.tenant_id)
        .await
        .map_err(ApiError::Internal)?
        .into_iter()
        .find(|s| s.name == CLAUDE_MAX_SECRET_NAME);
    match existing {
        Some(existing) => {
            state
                .secrets
                .rotate(&existing.id, &value, &record.tenant_id)
                .await
                .map_err(ApiError::Internal)?;
        }
        None => {
            state
                .secrets
                .create(&secret, &value)
                .await
                .map_err(ApiError::Internal)?;
        }
    }

    tracing::info!(
        event = "oauth.enrollment_completed",
        tenant_id = %record.tenant_id,
        workspace_id = ?record.workspace_id,
        initiated_by = %record.initiated_by,
        "Claude Max enrollment completed; refresh token stored as a scoped secret"
    );

    Ok(Json(CallbackOutcome {
        status: "enrolled".to_string(),
        workspace_id: record.workspace_id,
        secret_name: CLAUDE_MAX_SECRET_NAME.to_string(),
    }))
}

// ── Tests ───────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app(state: Arc<crate::AppState>) -> axum::Router {
        axum::Router::new()
            .route(
                "/api/v1/admin/oauth/claude-max/enroll",
                axum::routing::post(super::enroll_claude_max),
            )
            .route(
                "/api/v1/oauth/claude-max/callback",
                axum::routing::get(super::claude_max_callback),
            )
            .with_state(state)
    }

    fn enroll_request(body: &str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri("/api/v1/admin/oauth/claude-max/enroll")
            .header("Authorization", "Bearer test-token")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    /// Stub provider: authorization at /auth (records the query), token
    /// endpoint at /token returning the given JSON.
    async fn spawn_provider(
        token_response: serde_json::Value,
        token_status: StatusCode,
    ) -> (String, String, Arc<tokio::sync::Mutex<Vec<String>>>) {
        let seen = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let seen_token = Arc::clone(&seen);
        let app = axum::Router::new()
            .route("/auth", axum::routing::get(|| async { "authorize-ok" }))
            .route(
                "/token",
                axum::routing::post(move |body: String| {
                    let seen = Arc::clone(&seen_token);
                    async move {
                        seen.lock().await.push(body);
                        (token_status, axum::Json(token_response))
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let base = format!("http://127.0.0.1:{port}");
        (format!("{base}/auth"), format!("{base}/token"), seen)
    }

    static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[tokio::test]
    async fn enroll_generates_state_pkce_and_redirects() {
        let _guard = ENV_LOCK.lock().await;
        let (auth_url, token_url, _seen) =
            spawn_provider(serde_json::json!({}), StatusCode::OK).await;
        std::env::set_var("GYRE_CLAUDE_MAX_CLIENT_ID", "test-client");
        std::env::set_var("GYRE_CLAUDE_MAX_AUTH_ENDPOINT", &auth_url);
        std::env::set_var("GYRE_CLAUDE_MAX_TOKEN_ENDPOINT", &token_url);
        std::env::set_var("GYRE_CLAUDE_MAX_REDIRECT_URI", "http://localhost:3000/cb");

        let state = crate::mem::test_state();
        let app = app(state.clone());
        let response = app.oneshot(enroll_request("{}")).await.unwrap();
        assert_eq!(response.status(), StatusCode::FOUND);
        let location = response
            .headers()
            .get("location")
            .and_then(|l| l.to_str().ok())
            .unwrap()
            .to_string();
        assert!(location.starts_with(&auth_url), "redirects to the provider");
        assert!(
            location.contains("code_challenge_method=S256"),
            "PKCE S256: {location}"
        );
        assert!(location.contains("state="), "carries state: {location}");

        // The state row is persisted server-side with the server-derived
        // admin + tenant.
        let state_param = location
            .split("state=")
            .nth(1)
            .unwrap()
            .split('&')
            .next()
            .unwrap();
        let record = state
            .oauth_states
            .find_by_state(state_param)
            .await
            .unwrap()
            .expect("state row persisted");
        assert_eq!(record.tenant_id, "default");
        assert_eq!(record.initiated_by, "user-admin");
        assert!(!record.pkce_verifier.is_empty());

        std::env::remove_var("GYRE_CLAUDE_MAX_CLIENT_ID");
        std::env::remove_var("GYRE_CLAUDE_MAX_AUTH_ENDPOINT");
        std::env::remove_var("GYRE_CLAUDE_MAX_TOKEN_ENDPOINT");
        std::env::remove_var("GYRE_CLAUDE_MAX_REDIRECT_URI");
    }

    #[tokio::test]
    async fn enroll_requires_admin_role() {
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
        let state = make_test_state_with_jwt();
        let app = app(state);
        let developer_jwt = sign_test_jwt(
            &serde_json::json!({
                "sub": "dev-sub",
                "preferred_username": "dev-user",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/oauth/claude-max/enroll")
                    .header("Authorization", format!("Bearer {developer_jwt}"))
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn enroll_rejects_workspace_of_another_tenant() {
        let _guard = ENV_LOCK.lock().await;
        let (auth_url, token_url, _seen) =
            spawn_provider(serde_json::json!({}), StatusCode::OK).await;
        std::env::set_var("GYRE_CLAUDE_MAX_CLIENT_ID", "test-client");
        std::env::set_var("GYRE_CLAUDE_MAX_AUTH_ENDPOINT", &auth_url);
        std::env::set_var("GYRE_CLAUDE_MAX_TOKEN_ENDPOINT", &token_url);

        let state = crate::mem::test_state();
        // Workspace belonging to another tenant.
        let now = now_secs();
        let ws = gyre_domain::Workspace::new(
            Id::new("ws-other"),
            Id::new("tenant-other"),
            "Other",
            "other",
            now,
        );
        state.workspaces.create(&ws).await.unwrap();

        let app = app(state);
        let response = app
            .oneshot(enroll_request(r#"{"workspace_id":"ws-other"}"#))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        std::env::remove_var("GYRE_CLAUDE_MAX_CLIENT_ID");
        std::env::remove_var("GYRE_CLAUDE_MAX_AUTH_ENDPOINT");
        std::env::remove_var("GYRE_CLAUDE_MAX_TOKEN_ENDPOINT");
    }

    #[tokio::test]
    async fn callback_exchanges_code_and_stores_scoped_secret() {
        let _guard = ENV_LOCK.lock().await;
        let (auth_url, token_url, seen) = spawn_provider(
            serde_json::json!({
                "access_token": "at-new",
                "refresh_token": "rt-new",
                "expires_in": 7200
            }),
            StatusCode::OK,
        )
        .await;
        std::env::set_var("GYRE_CLAUDE_MAX_CLIENT_ID", "test-client");
        std::env::set_var("GYRE_CLAUDE_MAX_AUTH_ENDPOINT", &auth_url);
        std::env::set_var("GYRE_CLAUDE_MAX_TOKEN_ENDPOINT", &token_url);
        std::env::set_var("GYRE_CLAUDE_MAX_REDIRECT_URI", "http://localhost:3000/cb");

        let state = crate::mem::test_state();
        let now = now_secs();
        let provider = provider_config_from_env().unwrap();
        let record = gyre_ports::OAuthStateRecord {
            state: "test-state-abc".to_string(),
            tenant_id: "default".to_string(),
            workspace_id: Some("ws-enroll".to_string()),
            initiated_by: "user-admin".to_string(),
            pkce_verifier: "test-verifier".to_string(),
            created_at: now,
            expires_at: now + 600,
            consumed: false,
            provider_config: serde_json::to_string(&provider).unwrap(),
        };
        state.oauth_states.create(&record).await.unwrap();

        let callback_app = app(state.clone());
        let response = callback_app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/oauth/claude-max/callback?code=the-code&state=test-state-abc")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "enrolled");

        // The exchange posted the code + PKCE verifier.
        let posted = seen.lock().await.join("\n");
        assert!(posted.contains("grant_type=authorization_code"), "{posted}");
        assert!(posted.contains("code=the-code"), "{posted}");
        assert!(posted.contains("code_verifier=test-verifier"), "{posted}");

        // The refresh token landed as the scoped workspace secret, as an
        // OAuth credential document the refresh job can keep fresh.
        let secrets = state
            .secrets
            .list_by_scope(SecretScope::Workspace, "ws-enroll", "default")
            .await
            .unwrap();
        let secret = secrets
            .iter()
            .find(|s| s.name == CLAUDE_MAX_SECRET_NAME)
            .expect("enrollment secret stored");
        assert_eq!(secret.secret_type, SecretType::Rotated);
        assert!(secret.expires_at.unwrap() > now);
        let value = state
            .secrets
            .get_value(&secret.id, "default")
            .await
            .unwrap()
            .unwrap();
        let doc: serde_json::Value = serde_json::from_slice(&value).unwrap();
        assert_eq!(doc["refresh_token"], "rt-new");
        assert_eq!(doc["access_token"], "at-new");

        // The state row is consumed: a replay is rejected.
        let replay_app = app(state.clone());
        let response = replay_app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/oauth/claude-max/callback?code=the-code&state=test-state-abc")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        std::env::remove_var("GYRE_CLAUDE_MAX_CLIENT_ID");
        std::env::remove_var("GYRE_CLAUDE_MAX_AUTH_ENDPOINT");
        std::env::remove_var("GYRE_CLAUDE_MAX_TOKEN_ENDPOINT");
        std::env::remove_var("GYRE_CLAUDE_MAX_REDIRECT_URI");
    }

    #[tokio::test]
    async fn callback_rejects_unknown_and_expired_state() {
        let state = crate::mem::test_state();
        let app = app(state.clone());

        // Unknown state.
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/oauth/claude-max/callback?code=c&state=nope")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        // Expired state.
        let now = now_secs();
        let provider = ProviderConfig {
            authorization_endpoint: "http://x/auth".to_string(),
            token_endpoint: "http://x/token".to_string(),
            client_id: "c".to_string(),
            client_secret: None,
            redirect_uri: "http://x/cb".to_string(),
            scope: "s".to_string(),
        };
        state
            .oauth_states
            .create(&gyre_ports::OAuthStateRecord {
                state: "expired-state".to_string(),
                tenant_id: "default".to_string(),
                workspace_id: None,
                initiated_by: "user-admin".to_string(),
                pkce_verifier: "v".to_string(),
                created_at: now - 1000,
                expires_at: now - 100,
                consumed: false,
                provider_config: serde_json::to_string(&provider).unwrap(),
            })
            .await
            .unwrap();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/oauth/claude-max/callback?code=c&state=expired-state")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
