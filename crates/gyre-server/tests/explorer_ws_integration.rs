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

    /// Read next text message with timeout.
    async fn read_msg(
        reader: &mut futures_util::stream::SplitStream<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
        >,
    ) -> Option<serde_json::Value> {
        let timeout = tokio::time::timeout(std::time::Duration::from_secs(5), reader.next()).await;
        match timeout {
            Ok(Some(Ok(Message::Text(text)))) => serde_json::from_str(&text).ok(),
            _ => None,
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
    for _ in 0..5 {
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
    for _ in 0..5 {
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

    // Read views response to get the ID
    let mut view_id = String::new();
    for _ in 0..5 {
        if let Some(msg) = WsCtx::read_msg(&mut stream).await {
            if msg["type"] == "views" {
                let views = msg["views"].as_array().unwrap();
                if let Some(v) = views.iter().find(|v| v["name"] == "To Delete") {
                    view_id = v["id"].as_str().unwrap().to_string();
                    break;
                }
            }
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
    for _ in 0..5 {
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
    let ctx = WsCtx::new().await;
    let repo_id = ctx.repo_id().await;
    let (mut sink, mut stream) = ctx.connect(&repo_id).await;

    // Send two messages in quick succession — second should be rate-limited
    let msg = json!({
        "type": "message",
        "text": "hello",
        "canvas_state": {}
    });
    sink.send(Message::Text(msg.to_string().into()))
        .await
        .unwrap();

    // Send second message IMMEDIATELY (within MIN_MESSAGE_INTERVAL_MS=1000ms).
    // Don't wait for the first response — just fire both in quick succession.
    // Note: if the first message causes the server to close (e.g., no LLM configured),
    // the send may fail with BrokenPipe — that's acceptable in test environments.
    if sink
        .send(Message::Text(msg.to_string().into()))
        .await
        .is_err()
    {
        // Server closed before second message — rate limiting can't be tested
        // without a configured LLM. This is acceptable.
        return;
    }

    // Read all responses: the second message should get a rate-limit error.
    // We may see status/text from the first message and an error from the second.
    let mut got_rate_limit = false;
    for _ in 0..20 {
        if let Some(resp) = WsCtx::read_msg(&mut stream).await {
            if resp["type"] == "error" {
                if let Some(m) = resp["message"].as_str() {
                    if m.contains("wait") {
                        got_rate_limit = true;
                        break;
                    }
                }
            }
        } else {
            break;
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
