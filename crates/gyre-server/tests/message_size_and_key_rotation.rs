//! Integration tests for the message-bus Edge Cases rows delivered by
//! task-254 (specs/system/message-bus.md §Edge Cases):
//!
//! - **Message too large** — the total POST request body on
//!   POST /api/v1/workspaces/:id/messages is capped at 64KB
//!   (configurable via `GYRE_MAX_MESSAGE_SIZE`) and rejected with 413,
//!   below the global `GYRE_MAX_BODY_SIZE` (default 10MB) router limit:
//!   nothing is stored or broadcast. Boundary bodies (exactly 64KB
//!   accepted, 64KB+1 rejected) pin the limit's inclusivity.
//! - **Signature key rotation** — after the server signing key rotates
//!   (operator provisioning via `GYRE_AGENT_SIGNING_KEY`), JWKS serves
//!   BOTH the old and the new key's kids, a message signed before the
//!   rotation verifies against the OLD key's served public key with a
//!   real Ed25519 `verify()` (and fails against the new key — a
//!   negative control proving the crypto is real), and new messages are
//!   signed with the new key and verify against its JWKS entry.
//!
//! Own test binary (own process): `build_state` and `api_router` read
//! `GYRE_DATABASE_URL`, `GYRE_AGENT_SIGNING_KEY`, and
//! `GYRE_MAX_MESSAGE_SIZE` from the process environment; other
//! integration binaries must not see these variables. Within this
//! binary the tests serialize on `ENV_LOCK` — an interleaved
//! `set_var`/`remove_var` from a sibling test would silently point
//! another test's `build_state` at the wrong database or key.

use base64::{
    engine::general_purpose::{STANDARD as B64, URL_SAFE_NO_PAD},
    Engine as _,
};
use gyre_common::Id;
use gyre_domain::Agent;
use gyre_server::{abac_middleware, build_router, build_state};
use http::{Method, Request, StatusCode};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tower::ServiceExt;

/// Env vars touched here are process-global; all tests in this binary
/// serialize on this lock (same pattern as message_persistence.rs).
static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const TOKEN: &str = "task-254-token";

/// A Directed-message request body for `agent_id` whose total serialized
/// length is `base + pad_len` bytes (the pad is inside the payload, so the
/// envelope — `to`, `kind`, `payload` — is what carries the size).
fn directed_body(agent_id: &str, pad_len: usize) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "to": {"agent": agent_id},
        "kind": "task_assignment",
        "payload": {"task_id": "T-254", "pad": "x".repeat(pad_len)}
    }))
    .unwrap()
}

/// Create the target agent plus its workspace record.
///
/// The send path resolves the target agent's tenant from its workspace
/// record (message-bus.md §Scoping Rules rule 1, delivered by task-248)
/// and REJECTS Agent-directed sends whose scope cannot be resolved
/// (fail-closed). This file predates that guard and originally created
/// only the agent row, so every Directed send here failed the rule-1
/// resolution with 403. Tenant "default" matches the global test token's
/// system principal tenant, so same-tenant sends succeed; assertion
/// strength elsewhere is unchanged.
async fn create_agent(state: &Arc<gyre_server::AppState>, agent_id: &str, workspace_id: &str) {
    let ws_id = Id::new(workspace_id);
    if state.workspaces.find_by_id(&ws_id).await.unwrap().is_none() {
        let ws = gyre_domain::Workspace::new(
            ws_id.clone(),
            Id::new("default"),
            format!("Workspace {workspace_id}"),
            workspace_id,
            0,
        );
        state.workspaces.create(&ws).await.unwrap();
    }
    let mut agent = Agent::new(Id::new(agent_id), agent_id, 0);
    agent.workspace_id = ws_id;
    state.agents.create(&agent).await.unwrap();
}

async fn post_message(
    app: axum::Router,
    workspace_id: &str,
    body: Vec<u8>,
) -> (StatusCode, serde_json::Value) {
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/v1/workspaces/{workspace_id}/messages"))
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {TOKEN}"))
                .body(axum::body::Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or_default();
    (status, json)
}

/// Reconstruct the message signing input exactly as the server builds it
/// (message-bus.md §Signing): id, from, workspace, to, kind, payload hash,
/// created_at, NUL-separated. The caller authenticated with the global
/// token (Admin role → Server origin), so `from` is `server` with no id.
fn reconstruct_sign_input(msg: &serde_json::Value) -> String {
    let id = msg["id"].as_str().expect("id");
    let (from_type, from_id) = ("server", "");
    let ws_id = msg["workspace_id"].as_str().expect("workspace_id");
    let to = msg["to"].as_object().expect("to object");
    let (to_type, to_id) = if let Some(agent) = to.get("agent").and_then(|v| v.as_str()) {
        ("agent", agent)
    } else {
        ("workspace", to["workspace"].as_str().expect("workspace id"))
    };
    let kind = msg["kind"].as_str().expect("kind");
    // Identical to the server's `serde_json::to_string(&payload)`:
    // `Value::to_string` is the same compact serialization.
    let payload_json = msg["payload"].to_string();
    let mut hasher = Sha256::new();
    hasher.update(payload_json.as_bytes());
    let payload_hash = format!("{:x}", hasher.finalize());
    let created_at = msg["created_at"].as_u64().expect("created_at");
    format!(
        "{id}\0{from_type}\0{from_id}\0{ws_id}\0{to_type}\0{to_id}\0{kind}\0{payload_hash}\0{created_at}"
    )
}

/// Fetch /.well-known/jwks.json through the full router (the route is
/// unauthenticated, like production).
async fn fetch_jwks(app: axum::Router) -> serde_json::Value {
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/.well-known/jwks.json")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

/// Find the JWK with the given kid in a JWKS document.
fn jwk_by_kid<'a>(jwks: &'a serde_json::Value, kid: &str) -> &'a serde_json::Value {
    jwks["keys"]
        .as_array()
        .expect("keys array")
        .iter()
        .find(|k| k["kid"].as_str() == Some(kid))
        .unwrap_or_else(|| panic!("kid {kid} must be present in JWKS: {jwks}"))
}

/// Verify an Ed25519 signature (standard-base64) over `sign_input` against
/// a JWK's `x` (base64url-unpadded public key) — real ring crypto.
fn verify_ed25519(jwk_x: &str, sign_input: &str, signature_b64: &str) -> Result<(), String> {
    let public_key = URL_SAFE_NO_PAD
        .decode(jwk_x)
        .map_err(|e| format!("JWK x is not valid base64: {e}"))?;
    let key = ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, public_key);
    let sig = B64
        .decode(signature_b64)
        .map_err(|e| format!("signature is not valid base64: {e}"))?;
    key.verify(sign_input.as_bytes(), &sig)
        .map_err(|e| format!("Ed25519 verify failed: {e:?}"))
}

// ── Message too large ─────────────────────────────────────────────────────────

/// 65KB-class bodies are rejected with 413 before anything is stored or
/// broadcast; a body at exactly the 64KB limit is accepted. Runs against
/// `build_router` (the global 10MB `GYRE_MAX_BODY_SIZE` layer included) so
/// the pass proves the message-specific limit takes precedence over the
/// global one.
#[tokio::test(flavor = "multi_thread")]
async fn oversized_message_body_rejected_413_no_store_no_broadcast() {
    let _env = ENV_LOCK.lock().await;
    unsafe {
        // Explicit in-memory opt-in (architecture.md §Storage): unset now
        // means durable SQLite at the platform default data path; this
        // stateless test opts into memory instead of removing the var
        // (which would fall through to the durable default).
        std::env::set_var("GYRE_DATABASE_URL", "memory");
        std::env::remove_var("GYRE_AGENT_SIGNING_KEY");
        std::env::remove_var("GYRE_MAX_MESSAGE_SIZE");
    }

    let state = build_state(TOKEN, "http://127.0.0.1:1", None);
    abac_middleware::seed_builtin_policies(&state)
        .await
        .expect("seed built-in ABAC policies");
    create_agent(&state, "agent-254-size", "ws-254-size").await;
    let app = build_router(state.clone());

    // Watch the broadcast channel: the 413 path must not emit anything.
    let mut broadcast_rx = state.message_broadcast_tx.subscribe();

    // A total body of 65KB+ (payload padding inside the full envelope).
    let oversized = directed_body(
        "agent-254-size",
        65_537 - directed_body("agent-254-size", 0).len(),
    );
    assert!(
        oversized.len() > 64 * 1024,
        "test body must exceed the 64KB limit, got {}",
        oversized.len()
    );
    let (status, body) = post_message(app.clone(), "ws-254-size", oversized).await;
    assert_eq!(
        status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "65KB body must be rejected with 413, got {status}: {body}"
    );

    // Nothing stored: the Directed inbox is still empty.
    let unacked = state
        .messages
        .count_unacked(&Id::new("agent-254-size"))
        .await
        .unwrap();
    assert_eq!(unacked, 0, "a 413-rejected message must not be stored");

    // Nothing broadcast: no WS frame arrives on the message channel.
    let leaked =
        tokio::time::timeout(std::time::Duration::from_millis(200), broadcast_rx.recv()).await;
    assert!(
        leaked.is_err(),
        "a 413-rejected message must not be broadcast"
    );

    // Boundary: a body of exactly 65536 bytes (the 64KB limit) is accepted.
    let at_limit = directed_body(
        "agent-254-size",
        65_536 - directed_body("agent-254-size", 0).len(),
    );
    assert_eq!(at_limit.len(), 65_536, "body must be exactly 64KB");
    let (status, body) = post_message(app, "ws-254-size", at_limit).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "a body at exactly the 64KB limit must be accepted, got {status}: {body}"
    );
    // And it WAS stored (one unacked Directed message) — proving the
    // earlier empty inbox was the 413, not a broken send path.
    let unacked = state
        .messages
        .count_unacked(&Id::new("agent-254-size"))
        .await
        .unwrap();
    assert_eq!(unacked, 1, "the accepted at-limit message must be stored");
}

/// GYRE_MAX_MESSAGE_SIZE overrides the 64KB default.
#[tokio::test(flavor = "multi_thread")]
async fn max_message_size_env_overrides_default() {
    let _env = ENV_LOCK.lock().await;
    unsafe {
        // Explicit in-memory opt-in: see the 413 test above.
        std::env::set_var("GYRE_DATABASE_URL", "memory");
        std::env::remove_var("GYRE_AGENT_SIGNING_KEY");
        std::env::set_var("GYRE_MAX_MESSAGE_SIZE", "2048");
    }

    let state = build_state(TOKEN, "http://127.0.0.1:1", None);
    abac_middleware::seed_builtin_policies(&state)
        .await
        .expect("seed built-in ABAC policies");
    create_agent(&state, "agent-254-env", "ws-254-env").await;
    let app = build_router(state.clone());

    // 3KB body: fine under the default 64KB, rejected under the override.
    let oversized = directed_body("agent-254-env", 3_000);
    let (status, body) = post_message(app.clone(), "ws-254-env", oversized).await;
    assert_eq!(
        status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "GYRE_MAX_MESSAGE_SIZE=2048 must reject a 3KB body with 413, got {status}: {body}"
    );
    let unacked = state
        .messages
        .count_unacked(&Id::new("agent-254-env"))
        .await
        .unwrap();
    assert_eq!(unacked, 0, "the rejected message must not be stored");

    // A small body still passes under the override.
    let small = directed_body("agent-254-env", 32);
    let (status, body) = post_message(app, "ws-254-env", small).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "small body must be accepted under the override, got {status}: {body}"
    );

    unsafe { std::env::remove_var("GYRE_MAX_MESSAGE_SIZE") };
}

// ── Signature key rotation ────────────────────────────────────────────────────

/// Sign a Directed message under key K1, rotate the server signing key to
/// K2 (operator provisioning via GYRE_AGENT_SIGNING_KEY over the same
/// SQLite deployment), then require:
///
/// (a) GET /.well-known/jwks.json serves BOTH K1's and K2's kids;
/// (b) the K1-signed message's Ed25519 signature verifies against K1's
///     served public key (and NOT against K2's — a negative control);
/// (c) new messages are signed with K2 and verify against K2's entry.
#[tokio::test(flavor = "multi_thread")]
async fn jwks_serves_historical_keys_and_old_signatures_verify_after_rotation() {
    let _env = ENV_LOCK.lock().await;
    unsafe {
        std::env::remove_var("GYRE_MAX_MESSAGE_SIZE");
        std::env::remove_var("GYRE_AGENT_SIGNING_KEY");
    }

    let dir = tempfile::tempdir().unwrap();
    let db_url = format!("sqlite://{}", dir.path().join("gyre.db").display());

    // ── Boot 1 (key K1: generated and persisted by the server) ──────────
    unsafe { std::env::set_var("GYRE_DATABASE_URL", &db_url) };
    let state1 = build_state(TOKEN, "http://127.0.0.1:1", None);
    abac_middleware::seed_builtin_policies(&state1)
        .await
        .expect("seed built-in ABAC policies");
    create_agent(&state1, "agent-254-rot", "ws-254-rot").await;
    let kid1 = state1.agent_signing_key.kid.clone();

    let app1 = build_router(state1.clone());
    let (status, msg1) = post_message(app1, "ws-254-rot", directed_body("agent-254-rot", 64)).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "boot-1 send must succeed: {msg1}"
    );
    assert_eq!(msg1["key_id"].as_str(), Some(kid1.as_str()));
    let sig1_b64 = msg1["signature"].as_str().expect("signature").to_string();
    assert!(!sig1_b64.is_empty());
    drop(state1);

    // ── Rotate: provision key K2 via GYRE_AGENT_SIGNING_KEY ─────────────
    let (k2_pkcs8, k2_public) = {
        use ring::signature::{Ed25519KeyPair, KeyPair as _};
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        (pkcs8.as_ref().to_vec(), pair.public_key().as_ref().to_vec())
    };
    let kid2 = {
        // Same derivation the server uses (SHA-256 prefix of the public
        // key) so the test can locate K2's JWKS entry independently.
        let hash = Sha256::digest(&k2_public);
        hash[..16]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    assert_ne!(kid1, kid2, "K1 and K2 must be distinct keys");
    unsafe { std::env::set_var("GYRE_AGENT_SIGNING_KEY", B64.encode(&k2_pkcs8)) };

    // ── Boot 2 (key K2 active; K1 retained in durable history) ──────────
    let state2 = build_state(TOKEN, "http://127.0.0.1:1", None);
    assert_eq!(
        state2.agent_signing_key.kid, kid2,
        "the provisioned K2 must be the active signing key"
    );
    abac_middleware::seed_builtin_policies(&state2)
        .await
        .expect("seed built-in ABAC policies");
    let app2 = build_router(state2.clone());

    // (a) JWKS serves both kids.
    let jwks = fetch_jwks(app2.clone()).await;
    let k1_jwk = jwk_by_kid(&jwks, &kid1);
    let k2_jwk = jwk_by_kid(&jwks, &kid2);
    assert_eq!(
        k2_jwk["x"].as_str(),
        Some(URL_SAFE_NO_PAD.encode(&k2_public).as_str())
    );

    // (b) The K1-signed message verifies against K1's served public key —
    // real Ed25519 verify() of the reconstructed sign input.
    let sign_input = reconstruct_sign_input(&msg1);
    let k1_verified = verify_ed25519(k1_jwk["x"].as_str().unwrap(), &sign_input, &sig1_b64);
    assert!(
        k1_verified.is_ok(),
        "K1-signed message must verify against K1's served JWKS key after rotation: {k1_verified:?}"
    );
    // Negative control: the same signature must NOT verify under K2.
    let k1_under_k2 = verify_ed25519(k2_jwk["x"].as_str().unwrap(), &sign_input, &sig1_b64);
    assert!(
        k1_under_k2.is_err(),
        "K1 signature must not verify against K2's key (proves the verify is real)"
    );

    // (c) New messages are signed with K2 and verify against K2's entry.
    let (status, msg2) = post_message(app2, "ws-254-rot", directed_body("agent-254-rot", 64)).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "boot-2 send must succeed: {msg2}"
    );
    assert_eq!(
        msg2["key_id"].as_str(),
        Some(kid2.as_str()),
        "new messages after rotation must be signed with K2"
    );
    let sign_input2 = reconstruct_sign_input(&msg2);
    let sig2_b64 = msg2["signature"].as_str().expect("signature").to_string();
    let k2_verified = verify_ed25519(k2_jwk["x"].as_str().unwrap(), &sign_input2, &sig2_b64);
    assert!(
        k2_verified.is_ok(),
        "K2-signed message must verify against K2's served JWKS key: {k2_verified:?}"
    );
    // And the K2 signature must not verify under K1 (cross-check both ways).
    let k2_under_k1 = verify_ed25519(k1_jwk["x"].as_str().unwrap(), &sign_input2, &sig2_b64);
    assert!(
        k2_under_k1.is_err(),
        "K2 signature must not verify against K1's key"
    );

    unsafe {
        std::env::remove_var("GYRE_AGENT_SIGNING_KEY");
        std::env::remove_var("GYRE_DATABASE_URL");
    }
}
