//! Integration tests for conversation provenance endpoint (HSI §5).
//!
//! Coverage:
//!   - GET /api/v1/conversations/:sha — success (200 with decompressed blob)
//!   - GET /api/v1/conversations/:sha — not found (404)
//!   - GET /api/v1/conversations/:sha — unauthenticated (401)

use gyre_common::Id;
use gyre_server::{abac_middleware, build_router, build_state};
use std::sync::Arc;

const TOKEN: &str = "conv-integration-token";

struct Ctx {
    client: reqwest::Client,
    base: String,
    state: Arc<gyre_server::AppState>,
}

impl Ctx {
    async fn new() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{port}");

        let state = build_state(TOKEN, &base_url, None);
        abac_middleware::seed_builtin_policies(&state).await;
        let app = build_router(Arc::clone(&state));
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        Self {
            client: reqwest::Client::new(),
            base: base_url,
            state,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }
}

/// Store a zstd-compressed conversation blob via the repository port.
/// Returns the SHA-256 hex digest assigned by the repository.
async fn seed_conversation(state: &gyre_server::AppState, raw_json: &[u8]) -> String {
    let compressed = zstd::encode_all(raw_json, 3).expect("zstd compress");
    let agent_id = Id::new("conv-test-agent");
    let workspace_id = Id::new("conv-test-ws");
    let tenant_id = Id::new("default"); // global token auth uses tenant_id="default"
    state
        .conversations
        .store(&agent_id, &workspace_id, &tenant_id, &compressed)
        .await
        .expect("seed conversation")
}

// ── 1. Success path: GET returns 200 with decompressed blob ──────────────────

#[tokio::test]
async fn get_conversation_returns_decompressed_blob() {
    let ctx = Ctx::new().await;

    let raw_json = br#"{"turns":[{"role":"user","content":"hello"}]}"#;
    let sha = seed_conversation(&ctx.state, raw_json).await;

    let resp = ctx
        .client
        .get(ctx.url(&format!("/api/v1/conversations/{sha}")))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);

    // Verify Content-Type header.
    let content_type = resp
        .headers()
        .get("Content-Type")
        .expect("Content-Type header present")
        .to_str()
        .unwrap();
    assert_eq!(content_type, "application/octet-stream");

    // Verify X-Gyre-Conversation-Sha header echoes the requested SHA.
    let sha_header = resp
        .headers()
        .get("X-Gyre-Conversation-Sha")
        .expect("X-Gyre-Conversation-Sha header present")
        .to_str()
        .unwrap();
    assert_eq!(sha_header, sha);

    // Body should be the original (decompressed) JSON bytes.
    let body = resp.bytes().await.unwrap();
    assert_eq!(body.as_ref(), raw_json);
}

// ── 2. Not-found path: nonexistent SHA returns 404 ───────────────────────────

#[tokio::test]
async fn get_conversation_nonexistent_returns_404() {
    let ctx = Ctx::new().await;

    let resp = ctx
        .client
        .get(ctx.url("/api/v1/conversations/deadbeefcafebabe0000000000000000000000000000000000000000deadbeef"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);
}

// ── 3. ABAC enforcement: unauthenticated request returns 401 ─────────────────

#[tokio::test]
async fn get_conversation_unauthenticated_returns_401() {
    let ctx = Ctx::new().await;

    // Seed a conversation so the 401 isn't just masking a 404.
    let raw_json = br#"{"turns":[]}"#;
    let sha = seed_conversation(&ctx.state, raw_json).await;

    let resp = ctx
        .client
        .get(ctx.url(&format!("/api/v1/conversations/{sha}")))
        // No Authorization header.
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 401);
}
