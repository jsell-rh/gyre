//! Auth + security integration tests (M17.4).
//!
//! Tests all authentication mechanisms against a live gyre-server:
//!   1. Global auth token (admin-level, constant-time compare)
//!   2. Per-agent tokens (issued at agent registration, revoked on complete)
//!   3. API key auth (`gyre_<uuid>` prefix, stored as SHA-256 hash)
//!
//! Also validates:
//!   - 401 for missing / invalid tokens
//!   - 403 for insufficient role (agent token hitting AdminOnly endpoint)
//!   - Public `/api/v1/version` endpoint (no auth required)
//!   - Token revocation after `POST /api/v1/agents/{id}/complete`
//!   - Multiple API endpoints all enforce auth

use gyre_common::Id;
use gyre_domain::User;
use gyre_server::{abac_middleware, build_router, build_state};
use sha2::{Digest, Sha256};

// ── Constants ─────────────────────────────────────────────────────────────────

const GLOBAL_TOKEN: &str = "auth-int-test-global-token";

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Compute SHA-256 hex digest of a string (mirrors `auth::hash_api_key`).
fn sha256_hex(input: &str) -> String {
    let result = Sha256::digest(input.as_bytes());
    result.iter().map(|b| format!("{b:02x}")).collect()
}

/// Start a server with the global token. Returns (base_url, reqwest::Client).
async fn start_server() -> (String, reqwest::Client) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let base_url = format!("http://127.0.0.1:{port}");

    let state = build_state(GLOBAL_TOKEN, &base_url, None);
    abac_middleware::seed_builtin_policies(&state).await;
    let app = build_router(state);
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let client = reqwest::Client::new();
    (base_url, client)
}

/// Start a server with a pre-seeded user + API key.
/// Returns (base_url, client, raw_api_key) where raw_api_key has `gyre_` prefix.
async fn start_server_with_api_key() -> (String, reqwest::Client, String) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let base_url = format!("http://127.0.0.1:{port}");

    let state = build_state(GLOBAL_TOKEN, &base_url, None);
    abac_middleware::seed_builtin_policies(&state).await;

    // Pre-seed a user and API key before starting the server.
    let user = User::new(
        Id::new("auth-int-user-1"),
        "ext-auth-int-1",
        "api-key-tester",
        1000,
    );
    state.users.create(&user).await.unwrap();

    // API keys are stored as SHA-256 hashes of the raw key.
    let raw_key = "gyre_auth_integration_test_key_abc";
    let key_hash = sha256_hex(raw_key);
    state
        .api_keys
        .create(&key_hash, &user.id, "integration-test-key")
        .await
        .unwrap();

    let app = build_router(state);
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let client = reqwest::Client::new();
    (base_url, client, raw_key.to_string())
}

// ── Tests: missing / invalid authentication ───────────────────────────────────

#[tokio::test]
async fn unauthenticated_request_returns_401() {
    let (base, client) = start_server().await;
    let resp = client
        .get(format!("{base}/api/v1/repos"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "missing auth must return 401");
}

#[tokio::test]
async fn invalid_bearer_token_returns_401() {
    let (base, client) = start_server().await;
    let resp = client
        .get(format!("{base}/api/v1/repos"))
        .header("Authorization", "Bearer definitely-not-valid-token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "invalid token must return 401");
}

#[tokio::test]
async fn empty_bearer_token_returns_401() {
    let (base, client) = start_server().await;
    // "Bearer " with nothing after it — token is an empty string.
    let resp = client
        .get(format!("{base}/api/v1/repos"))
        .header("Authorization", "Bearer ")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "empty bearer token must return 401");
}

#[tokio::test]
async fn malformed_auth_header_returns_401() {
    let (base, client) = start_server().await;
    // Missing "Bearer " prefix.
    let resp = client
        .get(format!("{base}/api/v1/repos"))
        .header("Authorization", GLOBAL_TOKEN) // no "Bearer " prefix
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        401,
        "auth header without 'Bearer ' prefix must return 401"
    );
}

// ── Tests: global auth token ───────────────────────────────────────────────────

#[tokio::test]
async fn valid_global_token_returns_200() {
    let (base, client) = start_server().await;
    let resp = client
        .get(format!("{base}/api/v1/repos"))
        .header("Authorization", format!("Bearer {GLOBAL_TOKEN}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "valid global token must return 200");
}

#[tokio::test]
async fn global_token_can_list_tasks() {
    let (base, client) = start_server().await;
    let resp = client
        .get(format!("{base}/api/v1/tasks"))
        .header("Authorization", format!("Bearer {GLOBAL_TOKEN}"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "global token must access tasks endpoint"
    );
}

#[tokio::test]
async fn global_token_can_create_repo() {
    let (base, client) = start_server().await;
    let body = serde_json::json!({"workspace_id": "ws-auth-test", "name": "auth-test-repo"});
    let resp = client
        .post(format!("{base}/api/v1/repos"))
        .header("Authorization", format!("Bearer {GLOBAL_TOKEN}"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "global token must be able to create repos, got {}",
        resp.status()
    );
}

#[tokio::test]
async fn global_token_can_access_admin_endpoint() {
    let (base, client) = start_server().await;
    // GET /api/v1/admin/health is AdminOnly.
    let resp = client
        .get(format!("{base}/api/v1/admin/health"))
        .header("Authorization", format!("Bearer {GLOBAL_TOKEN}"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "global token (admin) must access admin endpoints"
    );
}

// ── Tests: public endpoints require no auth ───────────────────────────────────

#[tokio::test]
async fn version_endpoint_is_public() {
    let (base, client) = start_server().await;
    // /api/v1/version is intentionally exempt from auth middleware.
    let resp = client
        .get(format!("{base}/api/v1/version"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "/api/v1/version must be accessible without auth"
    );
    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["name"], "gyre");
}

#[tokio::test]
async fn health_endpoint_is_public() {
    let (base, client) = start_server().await;
    let resp = client.get(format!("{base}/health")).send().await.unwrap();
    assert_eq!(
        resp.status(),
        200,
        "/health must be accessible without auth"
    );
}

// ── Tests: per-agent token ────────────────────────────────────────────────────

#[tokio::test]
async fn per_agent_token_allows_api_access() {
    let (base, client) = start_server().await;
    let auth = format!("Bearer {GLOBAL_TOKEN}");

    // Register a new agent to obtain a per-agent token.
    let reg_body = serde_json::json!({"name": "auth-test-agent"});
    let resp = client
        .post(format!("{base}/api/v1/agents"))
        .header("Authorization", &auth)
        .json(&reg_body)
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "agent registration must succeed"
    );
    let agent_json: serde_json::Value = resp.json().await.unwrap();
    let agent_id = agent_json["id"].as_str().unwrap().to_string();
    let agent_token = agent_json["auth_token"].as_str().unwrap().to_string();
    assert!(!agent_token.is_empty(), "agent token must be non-empty");

    // The per-agent token must work on an agent-scoped endpoint.
    let resp = client
        .get(format!("{base}/api/v1/agents/{agent_id}"))
        .header("Authorization", format!("Bearer {agent_token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "per-agent token must authenticate successfully"
    );
}

#[tokio::test]
async fn agent_token_rejected_on_admin_only_endpoint() {
    let (base, client) = start_server().await;
    let auth = format!("Bearer {GLOBAL_TOKEN}");

    // Register an agent.
    let reg_body = serde_json::json!({"name": "auth-rbac-agent"});
    let resp = client
        .post(format!("{base}/api/v1/agents"))
        .header("Authorization", &auth)
        .json(&reg_body)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let agent_json: serde_json::Value = resp.json().await.unwrap();
    let agent_token = agent_json["auth_token"].as_str().unwrap().to_string();

    // Agent token hitting AdminOnly endpoint must get 403.
    let resp = client
        .post(format!("{base}/api/v1/admin/seed"))
        .header("Authorization", format!("Bearer {agent_token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        403,
        "agent token must be forbidden from AdminOnly endpoints"
    );
}

#[tokio::test]
async fn token_revoked_after_agent_complete() {
    let (base, client) = start_server().await;
    let auth = format!("Bearer {GLOBAL_TOKEN}");

    // Create a repo and task for the spawn.
    let repo_resp: serde_json::Value = client
        .post(format!("{base}/api/v1/repos"))
        .header("Authorization", &auth)
        .json(&serde_json::json!({"workspace_id": "revoke-proj", "name": "revoke-repo"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let repo_id = repo_resp["id"].as_str().unwrap().to_string();

    let task_resp: serde_json::Value = client
        .post(format!("{base}/api/v1/tasks"))
        .header("Authorization", &auth)
        .json(&serde_json::json!({"title": "revoke-task", "task_type": "implementation"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let task_id = task_resp["id"].as_str().unwrap().to_string();

    // Spawn agent — gets a real per-agent token.
    let spawn_resp: serde_json::Value = client
        .post(format!("{base}/api/v1/agents/spawn"))
        .header("Authorization", &auth)
        .json(&serde_json::json!({
            "name": "revoke-agent",
            "repo_id": repo_id,
            "task_id": task_id,
            "branch": "feat/revoke-test",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let agent_id = spawn_resp["agent"]["id"].as_str().unwrap().to_string();
    let agent_token = spawn_resp["token"].as_str().unwrap().to_string();
    assert!(
        !agent_token.is_empty(),
        "spawn must return a non-empty token"
    );

    // Verify the agent token works before complete.
    let resp = client
        .get(format!("{base}/api/v1/agents/{agent_id}"))
        .header("Authorization", format!("Bearer {agent_token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "agent token must work before complete");

    // Complete the agent — this revokes the token.
    let complete_resp = client
        .post(format!("{base}/api/v1/agents/{agent_id}/complete"))
        .header("Authorization", &auth)
        .json(&serde_json::json!({
            "branch": "feat/revoke-test",
            "title": "Done",
            "target_branch": "main",
        }))
        .send()
        .await
        .unwrap();
    let complete_status = complete_resp.status();
    assert!(
        complete_status == 200 || complete_status == 201 || complete_status == 202,
        "complete must succeed (200/201/202), got {complete_status}"
    );

    // After complete, the agent token MUST be rejected.
    let resp = client
        .get(format!("{base}/api/v1/agents/{agent_id}"))
        .header("Authorization", format!("Bearer {agent_token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        401,
        "agent token must be revoked after complete — got {}",
        resp.status()
    );
}

// ── Tests: API key authentication ─────────────────────────────────────────────

#[tokio::test]
async fn api_key_auth_grants_access() {
    let (base, client, raw_key) = start_server_with_api_key().await;

    // Use the raw API key (gyre_... prefix) as the Bearer token.
    let resp = client
        .get(format!("{base}/api/v1/repos"))
        .header("Authorization", format!("Bearer {raw_key}"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "valid API key must return 200, got {}",
        resp.status()
    );
}

#[tokio::test]
async fn invalid_api_key_returns_401() {
    let (base, client, _) = start_server_with_api_key().await;

    // A key with the right prefix but wrong content must fail.
    let resp = client
        .get(format!("{base}/api/v1/repos"))
        .header(
            "Authorization",
            "Bearer gyre_this_key_does_not_exist_at_all",
        )
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "unknown API key must return 401");
}

// ── Tests: multiple endpoints all enforce auth ────────────────────────────────

#[tokio::test]
async fn multiple_endpoints_require_auth() {
    let (base, client) = start_server().await;

    let endpoints = vec![
        ("GET", format!("{base}/api/v1/repos")),
        ("GET", format!("{base}/api/v1/tasks")),
        ("GET", format!("{base}/api/v1/agents")),
        ("GET", format!("{base}/api/v1/merge-requests")),
        ("GET", format!("{base}/api/v1/merge-queue")),
    ];

    for (method, url) in endpoints {
        let req = match method {
            "GET" => client.get(&url),
            _ => unreachable!(),
        };
        let resp = req.send().await.unwrap();
        assert_eq!(
            resp.status(),
            401,
            "{method} {url} without auth should return 401, got {}",
            resp.status()
        );
    }
}

// ── Tests: constant-time token comparison (functional) ───────────────────────

#[tokio::test]
async fn constant_time_comparison_rejects_prefix_of_valid_token() {
    // A token that is a prefix of the valid token must be rejected.
    // This is a functional correctness test (not a timing test).
    let (base, client) = start_server().await;

    // Take the first 10 chars of the global token — should be rejected.
    let prefix_token = &GLOBAL_TOKEN[..10];
    let resp = client
        .get(format!("{base}/api/v1/repos"))
        .header("Authorization", format!("Bearer {prefix_token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        401,
        "prefix of valid token must be rejected (constant-time compare checks length)"
    );
}

#[tokio::test]
async fn constant_time_comparison_rejects_superstring_of_valid_token() {
    // A token that is the valid token with extra chars appended must be rejected.
    let (base, client) = start_server().await;

    let super_token = format!("{GLOBAL_TOKEN}-extra-suffix");
    let resp = client
        .get(format!("{base}/api/v1/repos"))
        .header("Authorization", format!("Bearer {super_token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        401,
        "superstring of valid token must be rejected"
    );
}

// ── Tests: token auth consistency across endpoints ────────────────────────────

#[tokio::test]
async fn valid_token_accepted_on_post_endpoint() {
    let (base, client) = start_server().await;
    let body = serde_json::json!({"title": "auth-test-task"});
    let resp = client
        .post(format!("{base}/api/v1/tasks"))
        .header("Authorization", format!("Bearer {GLOBAL_TOKEN}"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "valid token must work on POST endpoints, got {}",
        resp.status()
    );
}

#[tokio::test]
async fn invalid_token_rejected_on_post_endpoint() {
    let (base, client) = start_server().await;
    let body = serde_json::json!({"title": "should-fail-task"});
    let resp = client
        .post(format!("{base}/api/v1/tasks"))
        .header("Authorization", "Bearer completely-wrong-token")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        401,
        "invalid token on POST endpoint must return 401"
    );
}

#[tokio::test]
async fn admin_seed_endpoint_requires_admin_role() {
    let (base, client) = start_server().await;

    // No auth → 401.
    let resp = client
        .post(format!("{base}/api/v1/admin/seed"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        401,
        "admin/seed without auth must return 401"
    );

    // Global (admin) token → success.
    let resp = client
        .post(format!("{base}/api/v1/admin/seed"))
        .header("Authorization", format!("Bearer {GLOBAL_TOKEN}"))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "global token must be able to seed, got {}",
        resp.status()
    );
}
