//! M18 integration tests: OIDC provider + JWT agent tokens.
//!
//! Verifies:
//! - `GET /.well-known/openid-configuration` returns valid OIDC discovery doc
//! - `GET /.well-known/jwks.json` returns Ed25519 public key in JWK format
//! - `POST /api/v1/agents/spawn` returns a JWT (starts with "ey")
//! - JWT authenticates on subsequent API calls
//! - Expired JWT is rejected after agent complete (token revocation)
//! - `GET /api/v1/auth/token-info` introspects JWT agent tokens correctly

use gyre_server::{abac_middleware, build_router, build_state};

const GLOBAL_TOKEN: &str = "m18-test-global-token";

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

// -- OIDC discovery -----------------------------------------------------------

#[tokio::test]
async fn oidc_discovery_document_is_valid() {
    let (base, client) = start_server().await;
    let resp = client
        .get(format!("{base}/.well-known/openid-configuration"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "OIDC discovery must return 200");

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["issuer"], base, "issuer must equal base_url");
    assert!(
        body["jwks_uri"]
            .as_str()
            .unwrap()
            .ends_with("/.well-known/jwks.json"),
        "jwks_uri must point to JWKS endpoint"
    );
    assert!(
        body["id_token_signing_alg_values_supported"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("EdDSA")),
        "must advertise EdDSA"
    );
}

#[tokio::test]
async fn oidc_discovery_requires_no_auth() {
    let (base, client) = start_server().await;
    // No Authorization header — must still return 200.
    let resp = client
        .get(format!("{base}/.well-known/openid-configuration"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn jwks_endpoint_returns_ed25519_key() {
    let (base, client) = start_server().await;
    let resp = client
        .get(format!("{base}/.well-known/jwks.json"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "JWKS endpoint must return 200");

    let body: serde_json::Value = resp.json().await.unwrap();
    let keys = body["keys"].as_array().expect("must have 'keys' array");
    assert_eq!(keys.len(), 1, "must have exactly one key");

    let key = &keys[0];
    assert_eq!(key["kty"], "OKP", "kty must be OKP");
    assert_eq!(key["crv"], "Ed25519", "crv must be Ed25519");
    assert_eq!(key["alg"], "EdDSA", "alg must be EdDSA");
    assert_eq!(key["use"], "sig", "use must be sig");
    assert!(key["kid"].is_string(), "kid must be present");
    assert!(key["x"].is_string(), "x (public key) must be present");
}

// -- JWT agent token issuance -------------------------------------------------

#[tokio::test]
async fn spawn_returns_jwt_token() {
    let (base, client) = start_server().await;

    // Create repo and task.
    let repo: serde_json::Value = client
        .post(format!("{base}/api/v1/repos"))
        .header("Authorization", format!("Bearer {GLOBAL_TOKEN}"))
        .json(&serde_json::json!({"name": "m18-repo", "workspace_id": "ws-m18"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let repo_id = repo["id"].as_str().unwrap();

    let task: serde_json::Value = client
        .post(format!("{base}/api/v1/tasks"))
        .header("Authorization", format!("Bearer {GLOBAL_TOKEN}"))
        .json(&serde_json::json!({"title": "M18 task", "description": "", "priority": "medium", "status": "todo", "task_type": "implementation"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let task_id = task["id"].as_str().unwrap();

    // Spawn agent.
    let spawn_resp = client
        .post(format!("{base}/api/v1/agents/spawn"))
        .header("Authorization", format!("Bearer {GLOBAL_TOKEN}"))
        .json(&serde_json::json!({
            "name": "m18-agent",
            "repo_id": repo_id,
            "task_id": task_id,
            "branch": "feat/m18-test"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(spawn_resp.status(), 201, "spawn must return 201");

    let spawn_json: serde_json::Value = spawn_resp.json().await.unwrap();
    let token = spawn_json["token"].as_str().unwrap();

    // JWT tokens start with "ey" (base64url-encoded header).
    assert!(
        token.starts_with("ey"),
        "spawn token must be a JWT (starts with 'ey'), got: {token}"
    );

    // JWT must have 3 dot-separated parts.
    let parts: Vec<&str> = token.split('.').collect();
    assert_eq!(parts.len(), 3, "JWT must have 3 parts separated by '.'");
}

#[tokio::test]
async fn jwt_token_authenticates_api_calls() {
    let (base, client) = start_server().await;

    // Setup repo/task.
    let repo: serde_json::Value = client
        .post(format!("{base}/api/v1/repos"))
        .header("Authorization", format!("Bearer {GLOBAL_TOKEN}"))
        .json(&serde_json::json!({"name": "jwt-auth-repo", "workspace_id": "ws-jwt-auth"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let repo_id = repo["id"].as_str().unwrap();

    let task: serde_json::Value = client
        .post(format!("{base}/api/v1/tasks"))
        .header("Authorization", format!("Bearer {GLOBAL_TOKEN}"))
        .json(&serde_json::json!({"title": "jwt auth task", "description": "", "priority": "medium", "status": "todo", "task_type": "implementation"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let task_id = task["id"].as_str().unwrap();

    let spawn_json: serde_json::Value = client
        .post(format!("{base}/api/v1/agents/spawn"))
        .header("Authorization", format!("Bearer {GLOBAL_TOKEN}"))
        .json(&serde_json::json!({
            "name": "jwt-auth-agent",
            "repo_id": repo_id,
            "task_id": task_id,
            "branch": "feat/jwt-auth-test"
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let jwt_token = spawn_json["token"].as_str().unwrap();
    assert!(jwt_token.starts_with("ey"), "token must be JWT");

    // Use the JWT to list projects (should succeed).
    let list_resp = client
        .get(format!("{base}/api/v1/repos"))
        .header("Authorization", format!("Bearer {jwt_token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        list_resp.status(),
        200,
        "JWT token must authenticate API calls"
    );
}

// -- Token introspection ------------------------------------------------------

#[tokio::test]
async fn token_info_for_jwt_agent_token() {
    let (base, client) = start_server().await;

    // Setup and spawn.
    let repo: serde_json::Value = client
        .post(format!("{base}/api/v1/repos"))
        .header("Authorization", format!("Bearer {GLOBAL_TOKEN}"))
        .json(&serde_json::json!({"name": "info-repo", "workspace_id": "ws-info"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let repo_id = repo["id"].as_str().unwrap();

    let task: serde_json::Value = client
        .post(format!("{base}/api/v1/tasks"))
        .header("Authorization", format!("Bearer {GLOBAL_TOKEN}"))
        .json(&serde_json::json!({"title": "info task", "description": "", "priority": "low", "status": "todo", "task_type": "implementation"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let task_id = task["id"].as_str().unwrap();

    let spawn_json: serde_json::Value = client
        .post(format!("{base}/api/v1/agents/spawn"))
        .header("Authorization", format!("Bearer {GLOBAL_TOKEN}"))
        .json(&serde_json::json!({
            "name": "info-agent",
            "repo_id": repo_id,
            "task_id": task_id,
            "branch": "feat/info-test"
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let jwt_token = spawn_json["token"].as_str().unwrap();
    let agent_id = spawn_json["agent"]["id"].as_str().unwrap();

    // Call token-info with the agent JWT.
    let info_resp = client
        .get(format!("{base}/api/v1/auth/token-info"))
        .header("Authorization", format!("Bearer {jwt_token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(info_resp.status(), 200, "token-info must return 200");

    let info: serde_json::Value = info_resp.json().await.unwrap();
    assert_eq!(
        info["token_kind"], "agent_jwt",
        "token_kind must be agent_jwt"
    );
    assert_eq!(
        info["subject"].as_str().unwrap(),
        agent_id,
        "subject must be agent id"
    );
    // JWT claims should be present.
    let claims = &info["jwt_claims"];
    assert!(
        claims.is_object(),
        "jwt_claims must be present for JWT tokens"
    );
    assert_eq!(claims["sub"].as_str().unwrap(), agent_id);
    assert_eq!(claims["scope"].as_str().unwrap(), "agent");
    assert_eq!(claims["task_id"].as_str().unwrap(), task_id);
}

#[tokio::test]
async fn token_info_for_global_token() {
    let (base, client) = start_server().await;
    let resp = client
        .get(format!("{base}/api/v1/auth/token-info"))
        .header("Authorization", format!("Bearer {GLOBAL_TOKEN}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let info: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(info["token_kind"], "global");
    assert_eq!(info["subject"], "system");
}

// -- Revocation after complete ------------------------------------------------

#[tokio::test]
async fn jwt_revoked_after_agent_complete() {
    let (base, client) = start_server().await;

    let repo: serde_json::Value = client
        .post(format!("{base}/api/v1/repos"))
        .header("Authorization", format!("Bearer {GLOBAL_TOKEN}"))
        .json(&serde_json::json!({"name": "revoke-repo", "workspace_id": "ws-revoke"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let repo_id = repo["id"].as_str().unwrap();

    let task: serde_json::Value = client
        .post(format!("{base}/api/v1/tasks"))
        .header("Authorization", format!("Bearer {GLOBAL_TOKEN}"))
        .json(&serde_json::json!({"title": "revoke task", "description": "", "priority": "medium", "status": "todo", "task_type": "implementation"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let task_id = task["id"].as_str().unwrap();

    let spawn_json: serde_json::Value = client
        .post(format!("{base}/api/v1/agents/spawn"))
        .header("Authorization", format!("Bearer {GLOBAL_TOKEN}"))
        .json(&serde_json::json!({
            "name": "revoke-agent",
            "repo_id": repo_id,
            "task_id": task_id,
            "branch": "feat/revoke-test"
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let jwt_token = spawn_json["token"].as_str().unwrap().to_string();
    let agent_id = spawn_json["agent"]["id"].as_str().unwrap().to_string();

    // Verify JWT works before complete.
    let pre = client
        .get(format!("{base}/api/v1/repos"))
        .header("Authorization", format!("Bearer {jwt_token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(pre.status(), 200, "JWT must work before complete");

    // Complete the agent.
    let complete_resp = client
        .post(format!("{base}/api/v1/agents/{agent_id}/complete"))
        .header("Authorization", format!("Bearer {GLOBAL_TOKEN}"))
        .json(&serde_json::json!({
            "branch": "feat/revoke-test",
            "title": "Revoke test MR",
            "target_branch": "main"
        }))
        .send()
        .await
        .unwrap();
    assert!(
        complete_resp.status().is_success(),
        "complete must return 2xx, got {}",
        complete_resp.status()
    );

    // JWT must now be rejected.
    let post = client
        .get(format!("{base}/api/v1/repos"))
        .header("Authorization", format!("Bearer {jwt_token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        post.status(),
        401,
        "JWT must be rejected after agent complete"
    );
}
