//! Explorer WebSocket integration tests.
//!
//! Tests the explorer WS handler: session lifecycle, message protocol,
//! saved view CRUD, rate limiting, and session limits.

use futures_util::{SinkExt, StreamExt};
use gyre_server::{abac_middleware, build_router, build_state};
use serde_json::json;
use std::sync::Arc;
use tokio_tungstenite::tungstenite::Message;

const TOKEN: &str = "explorer-ws-test-token";

struct WsCtx {
    base: String,
    _port: u16,
}

impl WsCtx {
    async fn new() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{port}");

        let state = build_state(TOKEN, &base_url, None);
        abac_middleware::seed_builtin_policies(&state).await;
        let app = build_router(Arc::clone(&state));
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        // Create a workspace and repo for explorer tests.
        let client = reqwest::Client::new();
        let ws_resp = client
            .post(format!("{base_url}/api/v1/workspaces"))
            .header("Authorization", format!("Bearer {TOKEN}"))
            .json(&json!({ "name": "ws-test", "tenant_id": "default" }))
            .send()
            .await
            .unwrap();
        let ws_body: serde_json::Value = ws_resp.json().await.unwrap();
        let ws_id = ws_body["id"].as_str().unwrap();

        let repo_resp = client
            .post(format!("{base_url}/api/v1/repos"))
            .header("Authorization", format!("Bearer {TOKEN}"))
            .json(&json!({ "name": "explorer-test-repo", "workspace_id": ws_id }))
            .send()
            .await
            .unwrap();
        let repo_body: serde_json::Value = repo_resp.json().await.unwrap();
        let _repo_id = repo_body["id"].as_str().unwrap();

        Self {
            base: base_url,
            _port: port,
        }
    }

    /// Get the repo ID by listing repos.
    async fn repo_id(&self) -> String {
        let client = reqwest::Client::new();
        let resp = client
            .get(format!("{}/api/v1/repos", self.base))
            .header("Authorization", format!("Bearer {TOKEN}"))
            .send()
            .await
            .unwrap();
        let body: serde_json::Value = resp.json().await.unwrap();
        body.as_array()
            .and_then(|a| a.first())
            .and_then(|r| r["id"].as_str())
            .unwrap()
            .to_string()
    }

    /// Connect a WebSocket to the explorer endpoint.
    async fn connect(
        &self,
        repo_id: &str,
    ) -> (
        futures_util::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
            Message,
        >,
        futures_util::stream::SplitStream<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
        >,
    ) {
        let ws_url = format!(
            "ws://127.0.0.1:{}/api/v1/repos/{}/explorer?token={}",
            self._port, repo_id, TOKEN
        );
        let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
        ws_stream.split()
    }

    /// Read next text message with timeout, skipping non-text frames (Ping/Pong).
    async fn read_msg(
        reader: &mut futures_util::stream::SplitStream<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
        >,
    ) -> Option<serde_json::Value> {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return None;
            }
            match tokio::time::timeout(remaining, reader.next()).await {
                Ok(Some(Ok(Message::Text(text)))) => {
                    return serde_json::from_str(&text).ok();
                }
                Ok(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => {
                    continue; // Skip control frames
                }
                _ => return None,
            }
        }
    }

    /// Read next text message, skipping any "warning" messages (e.g. deprecation notices).
    /// Tests use ?token= auth which triggers a deprecation warning; this helper
    /// skips those so tests can focus on the actual protocol messages.
    async fn read_msg_skip_warnings(
        reader: &mut futures_util::stream::SplitStream<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
        >,
    ) -> Option<serde_json::Value> {
        for _ in 0..10 {
            let msg = Self::read_msg(reader).await?;
            if msg.get("type").and_then(|t| t.as_str()) != Some("warning") {
                return Some(msg);
            }
        }
        None
    }
}

// ── Session lifecycle ─────────────────────────────────────────────────────────

#[tokio::test]
async fn explorer_ws_connect_and_list_views() {
    let ctx = WsCtx::new().await;
    let repo_id = ctx.repo_id().await;
    let (mut _sink, mut stream) = ctx.connect(&repo_id).await;

    // First message after connect should be the views list (sent by client on connect,
    // but we need to send list_views explicitly since the client does it).
    _sink
        .send(Message::Text(
            json!({"type": "list_views"}).to_string().into(),
        ))
        .await
        .unwrap();

    let msg = WsCtx::read_msg_skip_warnings(&mut stream).await;
    assert!(msg.is_some(), "Should receive a views response");
    let msg = msg.unwrap();
    assert_eq!(msg["type"], "views", "First response should be views list");
    let views = msg["views"].as_array().unwrap();
    // System default views should be seeded
    assert!(
        views.len() >= 4,
        "Should have at least 4 system default views, got {}",
        views.len()
    );
}

#[tokio::test]
async fn explorer_ws_save_and_load_view() {
    let ctx = WsCtx::new().await;
    let repo_id = ctx.repo_id().await;
    let (mut sink, mut stream) = ctx.connect(&repo_id).await;

    // Save a view
    sink.send(Message::Text(
        json!({
            "type": "save_view",
            "name": "My Test View",
            "description": "A test view",
            "query": {"scope": {"type": "all"}}
        })
        .to_string()
        .into(),
    ))
    .await
    .unwrap();

    // Read until we get a views response
    let mut views_msg = None;
    for _ in 0..10 {
        if let Some(msg) = WsCtx::read_msg(&mut stream).await {
            if msg["type"] == "views" {
                views_msg = Some(msg);
                break;
            }
        }
    }
    assert!(views_msg.is_some(), "Should receive updated views list");
    let views = views_msg.unwrap();
    let view_list = views["views"].as_array().unwrap();
    let saved = view_list
        .iter()
        .find(|v| v["name"] == "My Test View")
        .expect("Saved view should appear in the list");
    let view_id = saved["id"].as_str().unwrap();

    // Load it back
    sink.send(Message::Text(
        json!({ "type": "load_view", "view_id": view_id })
            .to_string()
            .into(),
    ))
    .await
    .unwrap();

    let mut query_msg = None;
    for _ in 0..10 {
        if let Some(msg) = WsCtx::read_msg(&mut stream).await {
            if msg["type"] == "view_query" {
                query_msg = Some(msg);
                break;
            }
        }
    }
    assert!(query_msg.is_some(), "Should receive view_query on load");
    let q = query_msg.unwrap();
    assert_eq!(q["query"]["scope"]["type"], "all");
}

#[tokio::test]
async fn explorer_ws_delete_view() {
    let ctx = WsCtx::new().await;
    let repo_id = ctx.repo_id().await;
    let (mut sink, mut stream) = ctx.connect(&repo_id).await;

    // Save a view first
    sink.send(Message::Text(
        json!({
            "type": "save_view",
            "name": "To Delete",
            "description": "Will be deleted",
            "query": {"scope": {"type": "all"}}
        })
        .to_string()
        .into(),
    ))
    .await
    .unwrap();

    // Read views response to get the ID (skip warnings from deprecated ?token= auth)
    let mut view_id = String::new();
    for _ in 0..10 {
        if let Some(msg) = WsCtx::read_msg(&mut stream).await {
            if msg["type"] == "views" {
                let views = msg["views"].as_array().unwrap();
                if let Some(v) = views.iter().find(|v| v["name"] == "To Delete") {
                    view_id = v["id"].as_str().unwrap().to_string();
                    break;
                }
            }
            // Skip warning/status messages
        }
    }
    assert!(!view_id.is_empty(), "Should find the saved view");

    // Delete it
    sink.send(Message::Text(
        json!({ "type": "delete_view", "view_id": view_id })
            .to_string()
            .into(),
    ))
    .await
    .unwrap();

    // Verify it's gone from the updated views list
    let mut found_deleted = true;
    for _ in 0..10 {
        if let Some(msg) = WsCtx::read_msg(&mut stream).await {
            if msg["type"] == "views" {
                let views = msg["views"].as_array().unwrap();
                found_deleted = views.iter().any(|v| v["name"] == "To Delete");
                break;
            }
        }
    }
    assert!(!found_deleted, "Deleted view should not appear in list");
}

#[tokio::test]
async fn explorer_ws_invalid_message() {
    let ctx = WsCtx::new().await;
    let repo_id = ctx.repo_id().await;
    let (mut sink, mut stream) = ctx.connect(&repo_id).await;

    // Send garbage JSON
    sink.send(Message::Text("{not valid json}".into()))
        .await
        .unwrap();

    let msg = WsCtx::read_msg_skip_warnings(&mut stream).await;
    assert!(msg.is_some(), "Should receive an error");
    assert_eq!(msg.unwrap()["type"], "error");
}

#[tokio::test]
async fn explorer_ws_message_too_long() {
    let ctx = WsCtx::new().await;
    let repo_id = ctx.repo_id().await;
    let (mut sink, mut stream) = ctx.connect(&repo_id).await;

    // Send a message exceeding MAX_USER_MESSAGE_LENGTH (10000 chars)
    let long_text = "x".repeat(10001);
    sink.send(Message::Text(
        json!({
            "type": "message",
            "text": long_text,
            "canvas_state": {}
        })
        .to_string()
        .into(),
    ))
    .await
    .unwrap();

    let msg = WsCtx::read_msg_skip_warnings(&mut stream).await;
    assert!(
        msg.is_some(),
        "Should receive an error for too-long message"
    );
    let msg = msg.unwrap();
    assert_eq!(msg["type"], "error");
    assert!(msg["message"].as_str().unwrap().contains("too long"));
}

#[tokio::test]
async fn explorer_ws_rate_limiting() {
    // Deterministic rate-limit verification.
    //
    // The default 1000ms window cannot be asserted against a live agent
    // run: the handler processes the second Message only after the first
    // message's agent cycle completes, and a Message arriving *during* a
    // cycle is intentionally dropped (not queued, not answered). Under
    // parallel test load the cycle outlives the window, so the second
    // message is either dropped or legitimately accepted — firing both
    // messages immediately asserts a race (durable finding b5ed01bf:
    // intermittent "Second rapid message should be rate limited").
    //
    // Instead: widen the window far beyond any agent-cycle duration via
    // the documented GYRE_EXPLORER_MSG_INTERVAL_MS override, wait for the
    // first message's cycle to finish (the handler always ends a Message
    // cycle with a trailing "ready" status), then send the second message
    // and require the rate-limit rejection. The only other Message-type
    // sender in this binary (explorer_ws_message_too_long) is unaffected:
    // its length check rejects the frame before the rate limiter runs.
    std::env::set_var("GYRE_EXPLORER_MSG_INTERVAL_MS", "3600000");

    let ctx = WsCtx::new().await;
    let repo_id = ctx.repo_id().await;
    let (mut sink, mut stream) = ctx.connect(&repo_id).await;

    // First message: accepted (the session's first Message is never
    // rate-limited) and runs a full agent cycle.
    let msg = json!({
        "type": "message",
        "text": "hello",
        "canvas_state": {}
    });
    sink.send(Message::Text(msg.to_string().into()))
        .await
        .unwrap();

    // Wait for the cycle to complete: the trailing "ready" status is sent
    // exactly once per Message cycle, after the agent run (success or
    // error) and after replaying any queued view CRUD messages.
    let mut saw_ready = false;
    for _ in 0..30 {
        match WsCtx::read_msg(&mut stream).await {
            Some(m) if m["type"] == "status" && m["status"] == "ready" => {
                saw_ready = true;
                break;
            }
            Some(_) => continue,
            None => break,
        }
    }
    assert!(saw_ready, "First message should complete its agent cycle");

    // Second message, sent after the cycle: it must be rejected by the
    // rate limiter (elapsed since the first message is far below the
    // widened interval), not accepted into a new agent cycle.
    sink.send(Message::Text(msg.to_string().into()))
        .await
        .unwrap();

    let mut got_rate_limit = false;
    for _ in 0..10 {
        match WsCtx::read_msg(&mut stream).await {
            Some(resp) => {
                // If the second message were wrongly accepted, the server
                // would emit a "thinking" status for a new agent cycle —
                // fail fast on that instead of timing out.
                assert!(
                    !(resp["type"] == "status" && resp["status"] == "thinking"),
                    "Second message was accepted into a new agent cycle instead of being rate-limited"
                );
                if resp["type"] == "error" {
                    if let Some(m) = resp["message"].as_str() {
                        if m.contains("wait") {
                            got_rate_limit = true;
                            break;
                        }
                    }
                }
            }
            None => break,
        }
    }
    assert!(
        got_rate_limit,
        "Second rapid message should be rate limited"
    );
}

#[tokio::test]
async fn explorer_ws_save_view_validation() {
    let ctx = WsCtx::new().await;
    let repo_id = ctx.repo_id().await;
    let (mut sink, mut stream) = ctx.connect(&repo_id).await;

    // Try to save with invalid query
    sink.send(Message::Text(
        json!({
            "type": "save_view",
            "name": "Bad View",
            "description": "Invalid query",
            "query": {"scope": {"type": "invalid_type"}}
        })
        .to_string()
        .into(),
    ))
    .await
    .unwrap();

    let msg = WsCtx::read_msg_skip_warnings(&mut stream).await;
    assert!(msg.is_some());
    let msg = msg.unwrap();
    assert_eq!(msg["type"], "error", "Invalid query should produce error");
}

#[tokio::test]
async fn explorer_ws_session_limit_is_per_server_instance() {
    // The per-user concurrent session limit is a property of a server
    // instance (AppState), not of the OS process. Two servers in the same
    // process must not evict each other's sessions even for the same
    // authenticated user - the global dev token authenticates every
    // connection as the same ("default", "system") principal.
    let ctx_a = WsCtx::new().await;
    let ctx_b = WsCtx::new().await;
    let repo_a = ctx_a.repo_id().await;
    let repo_b = ctx_b.repo_id().await;

    // Fill server A up to the per-user cap (default: 3 concurrent sessions).
    let (mut sink1, mut stream1) = ctx_a.connect(&repo_a).await;
    let (_sink2, _stream2) = ctx_a.connect(&repo_a).await;
    let (_sink3, _stream3) = ctx_a.connect(&repo_a).await;

    // A same-user connection on server B (same process) must NOT evict
    // server A's oldest session.
    let (_sink_b, _stream_b) = ctx_b.connect(&repo_b).await;
    // Give a buggy (process-global) registry time to process the eviction
    // before probing session 1.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    sink1
        .send(Message::Text(json!({"type": "list_views"}).to_string()))
        .await
        .unwrap();
    let msg = WsCtx::read_msg_skip_warnings(&mut stream1).await;
    assert!(
        msg.as_ref().is_some_and(|m| m["type"] == "views"),
        "Session on server A must survive a same-user connection on server B; \
         got {:?}",
        msg
    );

    // Within ONE server the cap is still enforced: a 4th connection evicts
    // the oldest session with a "replaced" error.
    let (_sink4, _stream4) = ctx_a.connect(&repo_a).await;
    let mut eviction = None;
    for _ in 0..10 {
        match WsCtx::read_msg(&mut stream1).await {
            Some(m) if m["type"] == "error" => {
                eviction = Some(m);
                break;
            }
            Some(_) => continue,
            None => break,
        }
    }
    let m = eviction.expect("4th connection on the same server should evict the oldest session");
    assert!(
        m["message"].as_str().unwrap().contains("replaced"),
        "evicted session should report replacement, got: {}",
        m["message"]
    );
}
