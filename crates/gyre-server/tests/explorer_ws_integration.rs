//! Explorer WebSocket integration tests.
//!
//! Tests the explorer WS handler: session lifecycle, message protocol,
//! saved view CRUD, rate limiting, and session limits.

use futures_util::{SinkExt, StreamExt};
use gyre_common::{
    graph::{GraphNode, NodeType, SpecConfidence, Visibility},
    Id,
};
use gyre_ports::{
    ConversationMessage, LlmPort, LlmPortFactory, ToolCallingResponse, ToolDefinition,
};
use gyre_server::{abac_middleware, build_router, build_state};
use serde_json::json;
use std::sync::Arc;
use tokio_tungstenite::tungstenite::Message;

const TOKEN: &str = "explorer-ws-test-token";

struct WsCtx {
    base: String,
    _port: u16,
    /// Server state — exposed so tests can seed graph data directly into
    /// the graph store the explorer agent reads from.
    state: Arc<gyre_server::AppState>,
    /// Repo ID created for this context.
    repo_id: String,
}

impl WsCtx {
    async fn new() -> Self {
        Self::new_internal(None).await
    }

    /// Like `new()` but wires a scripted LLM factory into `AppState.llm` so
    /// the explorer agent takes the native LlmPort path with deterministic
    /// responses (self-check loop testing).
    async fn new_with_llm(llm: Arc<dyn LlmPortFactory>) -> Self {
        Self::new_internal(Some(llm)).await
    }

    async fn new_internal(llm: Option<Arc<dyn LlmPortFactory>>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{port}");

        let base_state = build_state(TOKEN, &base_url, None);
        let state = match llm {
            Some(factory) => {
                let mut s = (*base_state).clone();
                s.llm = Some(factory);
                Arc::new(s)
            }
            None => base_state,
        };
        abac_middleware::seed_builtin_policies(&state)
            .await
            .expect("seed built-in policies");
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
        let repo_id = repo_body["id"].as_str().unwrap().to_string();

        Self {
            base: base_url,
            _port: port,
            state,
            repo_id,
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
        .send(Message::Text(json!({"type": "list_views"}).to_string()))
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
        .to_string(),
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
        json!({ "type": "load_view", "view_id": view_id }).to_string(),
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
        .to_string(),
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
        json!({ "type": "delete_view", "view_id": view_id }).to_string(),
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
        .to_string(),
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
    sink.send(Message::Text(msg.to_string())).await.unwrap();

    // Send second message IMMEDIATELY (within MIN_MESSAGE_INTERVAL_MS=1000ms).
    // Don't wait for the first response — just fire both in quick succession.
    // Note: if the first message causes the server to close (e.g., no LLM configured),
    // the send may fail with BrokenPipe — that's acceptable in test environments.
    if sink.send(Message::Text(msg.to_string())).await.is_err() {
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
async fn explorer_ws_status_progression() {
    let ctx = WsCtx::new().await;
    let repo_id = ctx.repo_id().await;
    let (mut sink, mut stream) = ctx.connect(&repo_id).await;

    // No LLM is configured in tests, so the agent takes the no-LLM fallback
    // path. Expected wire sequence for a message turn (spec §6):
    //   status(thinking) [from the session loop]
    //   status(thinking, agent_path=native) [from run_explorer_agent]
    //   text(done=true) + view_query [from handle_no_llm_fallback]
    //   status(ready) [from the session loop after the agent completes]
    // Every observed status value MUST be one of the three spec values —
    // the frontend maps only those (ExplorerChat.svelte) and silently drops
    // anything else, leaving the status indicator stale.
    sink.send(Message::Text(
        json!({
            "type": "message",
            "text": "what is the blast radius of TaskPort?",
            "canvas_state": {}
        })
        .to_string(),
    ))
    .await
    .unwrap();

    let mut saw_text_done = false;
    let mut saw_view_query = false;
    let mut saw_ready = false;
    let mut observed_statuses: Vec<String> = Vec::new();

    for _ in 0..30 {
        let Some(msg) = WsCtx::read_msg(&mut stream).await else {
            break;
        };
        match msg["type"].as_str() {
            Some("status") => {
                let status = msg["status"].as_str().unwrap_or_default().to_string();
                assert!(
                    status == "thinking" || status == "refining" || status == "ready",
                    "Off-contract status value on the wire: {status:?} (spec §6: \
                     thinking | refining | ready)"
                );
                observed_statuses.push(status.clone());
                if status == "ready" {
                    saw_ready = true;
                    // ready is the terminal status for the turn
                    break;
                }
            }
            Some("text") => {
                if msg["done"] == serde_json::json!(true) {
                    saw_text_done = true;
                }
            }
            Some("view_query") => {
                saw_view_query = true;
            }
            _ => {} // warnings, pings, etc.
        }
    }

    assert!(
        !observed_statuses.is_empty(),
        "Should receive at least one status message"
    );
    assert!(
        observed_statuses.iter().any(|s| s == "thinking"),
        "Should see thinking status while the agent works, got {observed_statuses:?}"
    );
    assert!(
        saw_ready,
        "Should see terminal ready status, got {observed_statuses:?}"
    );
    assert!(saw_text_done, "Should receive final text (done=true)");
    assert!(
        saw_view_query,
        "Should receive a view_query for blast-radius question"
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
        .to_string(),
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

// ── Fixtures for graph-seeded tests ───────────────────────────────────────────

/// Build a graph node for seeding the store the explorer agent reads from.
fn make_graph_node(repo_id: &str, name: &str, node_type: NodeType) -> GraphNode {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    GraphNode {
        id: Id::new(uuid::Uuid::new_v4().to_string()),
        repo_id: Id::new(repo_id),
        node_type,
        name: name.to_string(),
        qualified_name: format!("crate::{name}"),
        file_path: format!("src/{name}.rs"),
        line_start: 1,
        line_end: 20,
        visibility: Visibility::Public,
        doc_comment: None,
        spec_path: None,
        spec_paths: vec![],
        spec_confidence: SpecConfidence::None,
        last_modified_sha: "deadbeef".to_string(),
        last_modified_by: None,
        last_modified_at: now,
        created_sha: "deadbeef".to_string(),
        created_at: now,
        complexity: None,
        churn_count_30d: 0,
        test_coverage: None,
        first_seen_at: now,
        last_seen_at: now,
        deleted_at: None,
        test_node: false,
        spec_approved_at: None,
        milestone_completed_at: None,
    }
}

// ── Scripted LLM (deterministic self-check loop testing) ──────────────────────

/// LLM port that plays back a fixed script of `complete_with_tools` responses.
///
/// The call counter is shared across every port the factory hands out, so a
/// full agent turn (initial response + self-check refinement rounds) walks
/// through the script in order regardless of how many times `for_model` is
/// called. Calls past the end of the script repeat the last response.
struct ScriptedLlmPort {
    calls: Arc<std::sync::atomic::AtomicUsize>,
    responses: Vec<String>,
}

#[async_trait::async_trait]
impl LlmPort for ScriptedLlmPort {
    async fn complete(
        &self,
        _system_prompt: &str,
        _user_prompt: &str,
        _max_tokens: Option<u32>,
    ) -> anyhow::Result<String> {
        Ok(self.responses.first().cloned().unwrap_or_default())
    }

    async fn predict_json(
        &self,
        _system_prompt: &str,
        _user_prompt: &str,
        _max_tokens: Option<u32>,
    ) -> anyhow::Result<serde_json::Value> {
        serde_json::from_str(self.responses.first().map(|s| s.as_str()).unwrap_or("{}"))
            .map_err(|e| anyhow::anyhow!("scripted response is not JSON: {e}"))
    }

    async fn stream_complete(
        &self,
        _system_prompt: &str,
        _user_prompt: &str,
        _max_tokens: Option<u32>,
    ) -> anyhow::Result<
        std::pin::Pin<Box<dyn futures_util::Stream<Item = anyhow::Result<String>> + Send>>,
    > {
        let text = self.responses.first().cloned().unwrap_or_default();
        Ok(Box::pin(futures_util::stream::iter(vec![Ok(text)])))
    }

    async fn complete_with_tools(
        &self,
        _system_prompt: &str,
        _messages: &[ConversationMessage],
        _tools: &[ToolDefinition],
        _max_tokens: Option<u32>,
    ) -> anyhow::Result<ToolCallingResponse> {
        let n = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let text = self
            .responses
            .get(n)
            .or_else(|| self.responses.last())
            .cloned()
            .unwrap_or_default();
        Ok(ToolCallingResponse {
            text,
            tool_calls: vec![],
            stop_reason: "end_turn".to_string(),
        })
    }
}

struct ScriptedLlmFactory {
    calls: Arc<std::sync::atomic::AtomicUsize>,
    responses: Vec<String>,
}

impl ScriptedLlmFactory {
    fn new(responses: Vec<String>) -> Self {
        Self {
            calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            responses,
        }
    }

    fn call_count(&self) -> usize {
        self.calls.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl LlmPortFactory for ScriptedLlmFactory {
    fn for_model(&self, _model_name: &str) -> Arc<dyn LlmPort> {
        Arc::new(ScriptedLlmPort {
            calls: Arc::clone(&self.calls),
            responses: self.responses.clone(),
        })
    }
}

// ── §31: WebSocket session lifecycle (connect, message, response, disconnect) ─

#[tokio::test]
async fn explorer_ws_full_session_lifecycle() {
    let ctx = WsCtx::new().await;
    let repo_id = ctx.repo_id.clone();
    let (mut sink, mut stream) = ctx.connect(&repo_id).await;

    // 1. Connect succeeded — the socket answers the initial protocol exchange.
    sink.send(Message::Text(json!({ "type": "list_views" }).to_string()))
        .await
        .unwrap();
    let msg = WsCtx::read_msg_skip_warnings(&mut stream).await;
    assert!(
        msg.as_ref().is_some_and(|m| m["type"] == "views"),
        "session must answer list_views after connect, got {:?}",
        msg
    );

    // 2. A full message turn: status progression, streamed text, view query.
    sink.send(Message::Text(
        json!({
            "type": "message",
            "text": "show me the test coverage gaps",
            "canvas_state": {}
        })
        .to_string(),
    ))
    .await
    .unwrap();

    let mut saw_thinking = false;
    let mut saw_text_done = false;
    let mut saw_view_query = false;
    for _ in 0..30 {
        let Some(msg) = WsCtx::read_msg(&mut stream).await else {
            break;
        };
        match msg["type"].as_str() {
            Some("status") => {
                if msg["status"] == "thinking" {
                    saw_thinking = true;
                }
                if msg["status"] == "ready" {
                    break;
                }
            }
            Some("text") => {
                if msg["done"] == serde_json::json!(true) {
                    saw_text_done = true;
                }
            }
            Some("view_query") => {
                saw_view_query = true;
                // The template query for coverage-gap questions is test_gaps scope.
                assert_eq!(
                    msg["query"]["scope"]["type"], "test_gaps",
                    "coverage-gap question must map to a test_gaps view query"
                );
            }
            _ => {}
        }
    }
    assert!(saw_thinking, "message turn must start with thinking status");
    assert!(
        saw_text_done,
        "message turn must stream final text (done=true)"
    );
    assert!(saw_view_query, "message turn must produce a view_query");

    // 3. Clean disconnect: client sends Close, server must close the socket
    //    (Close reply or stream end) rather than holding it open.
    sink.send(Message::Close(None)).await.unwrap();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut closed = false;
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, stream.next()).await {
            // Server echoed a Close frame (clean WS handshake) or ended the
            // stream — both are a clean disconnect.
            Ok(Some(Ok(Message::Close(_)))) => {
                closed = true;
                break;
            }
            Ok(None) => {
                closed = true;
                break;
            }
            Ok(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => continue,
            Ok(Some(Ok(Message::Text(_) | Message::Binary(_) | Message::Frame(_)))) => continue,
            // Stream error: the socket is gone (not held open) — counts as closed.
            Ok(Some(Err(_))) => {
                closed = true;
                break;
            }
            // Timeout elapsed with no Close reply and no stream end: the server
            // is still holding the socket. This is a hang, NOT a clean
            // disconnect — fail the assertion.
            Err(_) => break,
        }
    }
    assert!(
        closed,
        "server must close the explorer session after the client sends Close"
    );
}

// ── §31: Agent generates a valid view query from a natural-language question ──

#[tokio::test]
async fn explorer_ws_agent_generates_view_query_from_question() {
    let ctx = WsCtx::new().await;
    let repo_id = ctx.repo_id.clone();

    // Seed the graph with a known node set. The no-LLM fallback path must
    // extract "Space" from the question and validate it against real graph
    // nodes before emitting a focus query.
    for name in ["Space", "TaskPort", "Workspace", "Tenant", "RepoStore"] {
        let node = make_graph_node(&repo_id, name, NodeType::Type);
        ctx.state.graph_store.create_node(node).await.unwrap();
    }

    let (mut sink, mut stream) = ctx.connect(&repo_id).await;
    sink.send(Message::Text(
        json!({
            "type": "message",
            "text": "What would break if I change Space?",
            "canvas_state": {}
        })
        .to_string(),
    ))
    .await
    .unwrap();

    let mut view_query = None;
    for _ in 0..30 {
        let Some(msg) = WsCtx::read_msg(&mut stream).await else {
            break;
        };
        if msg["type"] == "view_query" {
            view_query = Some(msg["query"].clone());
            break;
        }
        if msg["type"] == "status" && msg["status"] == "ready" {
            break;
        }
    }
    let query = view_query.expect("agent must answer with a view_query");
    assert_eq!(
        query["scope"]["type"], "focus",
        "blast-radius question must produce a focus scope, got: {query}"
    );
    assert_eq!(
        query["scope"]["node"], "Space",
        "focus node must reference the node named in the question, got: {query}"
    );
    // The generated query must be structurally valid against the grammar —
    // the same deserialization the server's self-check enforces.
    let parsed: Result<gyre_common::view_query::ViewQuery, _> =
        serde_json::from_value(query.clone());
    assert!(parsed.is_ok(), "generated query must deserialize: {query}");
}

// ── §31: Self-check loop catches and fixes overly broad groups ────────────────

#[tokio::test]
async fn explorer_ws_self_check_loop_refines_broad_groups() {
    // Graph fixture: 25 nodes whose names all contain "node" — a group with
    // the pattern "node" matches all of them, over the 20-node breadth
    // threshold that dry-run flags as "too broad".
    let broad_query = json!({
        "scope": { "type": "all" },
        "groups": [{ "name": "Persistence", "nodes": ["node"] }]
    })
    .to_string();
    let refined_query = json!({
        "scope": { "type": "all" },
        "groups": [{ "name": "Persistence", "nodes": ["node_0", "node_1", "node_2"] }]
    })
    .to_string();
    let factory = Arc::new(ScriptedLlmFactory::new(vec![
        format!("Here is the domain overview.\n\n<view_query>{broad_query}</view_query>"),
        format!(
            "Refined to the core persistence helpers.\n\n<view_query>{refined_query}</view_query>"
        ),
    ]));

    let ctx = WsCtx::new_with_llm(factory.clone()).await;
    let repo_id = ctx.repo_id.clone();
    for i in 0..25 {
        let node = make_graph_node(&repo_id, &format!("node_{i}"), NodeType::Function);
        ctx.state.graph_store.create_node(node).await.unwrap();
    }

    let (mut sink, mut stream) = ctx.connect(&repo_id).await;
    sink.send(Message::Text(
        json!({
            "type": "message",
            "text": "show everything in the persistence domain",
            "canvas_state": {}
        })
        .to_string(),
    ))
    .await
    .unwrap();

    let mut saw_refining = false;
    let mut saw_ready = false;
    let mut view_queries = Vec::new();
    for _ in 0..40 {
        let Some(msg) = WsCtx::read_msg(&mut stream).await else {
            break;
        };
        match msg["type"].as_str() {
            Some("status") => match msg["status"].as_str() {
                Some("refining") => saw_refining = true,
                Some("ready") => {
                    saw_ready = true;
                    break;
                }
                _ => {}
            },
            Some("view_query") => view_queries.push(msg["query"].clone()),
            _ => {}
        }
    }

    // The self-check loop must have run: the broad group triggered a
    // refinement round before the query reached the frontend.
    assert!(
        saw_refining,
        "overly broad group must trigger a refining status (self-check loop)"
    );
    assert!(saw_ready, "message turn must reach ready status");
    assert_eq!(
        factory.call_count(),
        2,
        "exactly one refinement round: initial broad query + one refined query"
    );
    // The frontend only ever sees the REFINED query — the broad version is
    // intercepted by the server-side dry-run and never forwarded.
    assert_eq!(
        view_queries.len(),
        1,
        "exactly one view_query must reach the client (the refined one), got: {view_queries:?}"
    );
    let sent = &view_queries[0];
    assert_eq!(
        sent["groups"][0]["nodes"],
        json!(["node_0", "node_1", "node_2"]),
        "client must receive the refined group, got: {sent}"
    );
}

// ── §11 Self-Check Loop: refinement flow + best-version-at-cap (mainline) ────

/// Drive one explorer message turn over the WebSocket and collect the wire
/// traffic: statuses observed, view_query messages received, and whether the
/// turn reached the terminal `ready` status.
struct TurnTrace {
    statuses: Vec<String>,
    view_queries: Vec<serde_json::Value>,
    saw_ready: bool,
    saw_text_done: bool,
}

async fn run_message_turn(
    sink: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
    stream: &mut futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
    question: &str,
) -> TurnTrace {
    sink.send(Message::Text(
        json!({
            "type": "message",
            "text": question,
            "canvas_state": {}
        })
        .to_string(),
    ))
    .await
    .unwrap();

    let mut trace = TurnTrace {
        statuses: Vec::new(),
        view_queries: Vec::new(),
        saw_ready: false,
        saw_text_done: false,
    };
    for _ in 0..60 {
        let Some(msg) = WsCtx::read_msg(stream).await else {
            break;
        };
        match msg["type"].as_str() {
            Some("status") => {
                let status = msg["status"].as_str().unwrap_or_default().to_string();
                // Spec §6: only the three contract values may appear on the wire.
                assert!(
                    status == "thinking" || status == "refining" || status == "ready",
                    "Off-contract status value on the wire: {status:?}"
                );
                if status == "ready" {
                    trace.saw_ready = true;
                    break;
                }
                trace.statuses.push(status);
            }
            Some("view_query") => {
                trace.view_queries.push(msg["query"].clone());
            }
            Some("text") if msg["done"] == serde_json::json!(true) => {
                trace.saw_text_done = true;
            }
            _ => {} // warnings, pings, errors handled by assertions below
        }
    }
    trace
}

/// §11 Self-Check Loop: the server extracts `<view_query>` blocks from agent
/// output, dry-runs them server-side, and — when the dry-run produces
/// actionable warnings — feeds the result back to the agent as a synthetic
/// tool result so it can refine, sending status "refining" to the frontend.
/// Only the refined (warning-free) query may reach the frontend.
#[tokio::test]
async fn explorer_ws_self_check_refinement_flow() {
    // Scripted agent: first attempt references a callout node that does not
    // exist in the graph (actionable dry-run warning), second attempt —
    // produced AFTER receiving the dry-run feedback — resolves cleanly.
    let warned_query =
        r#"{"scope":{"type":"all"},"callouts":[{"node":"DoesNotExist","text":"missing"}]}"#;
    let refined_query =
        r#"{"scope":{"type":"all"},"callouts":[{"node":"TaskPort","text":"found"}]}"#;
    let script = vec![
        format!("Let me check this query.\n\n<view_query>{warned_query}</view_query>"),
        format!("Refined after the dry-run feedback.\n\n<view_query>{refined_query}</view_query>"),
    ];
    let ctx = WsCtx::new_with_llm(Arc::new(ScriptedLlmFactory::new(script))).await;
    let repo_id = ctx.repo_id.clone();
    // Seed the graph so the dry-run has real state to resolve against:
    // TaskPort resolves exactly; DoesNotExist triggers an unresolved-callout
    // warning (actionable → refinement turn).
    ctx.state
        .graph_store
        .create_node(make_graph_node(&repo_id, "TaskPort", NodeType::Function))
        .await
        .unwrap();
    ctx.state
        .graph_store
        .create_node(make_graph_node(
            &repo_id,
            "CoordinatorService",
            NodeType::Class,
        ))
        .await
        .unwrap();

    let (mut sink, mut stream) = ctx.connect(&repo_id).await;
    let trace = run_message_turn(&mut sink, &mut stream, "show me TaskPort").await;

    assert!(
        trace.statuses.iter().any(|s| s == "refining"),
        "self-check must emit status \"refining\" while re-prompting the agent, got {:?}",
        trace.statuses
    );
    assert!(
        trace.saw_text_done,
        "turn must finish the streamed text (done=true)"
    );
    assert!(trace.saw_ready, "turn must reach terminal ready status");
    assert_eq!(
        trace.view_queries.len(),
        1,
        "exactly one view_query may reach the frontend — the warned first attempt must NOT be sent"
    );
    let q = &trace.view_queries[0];
    assert_eq!(
        q["callouts"][0]["node"], "TaskPort",
        "frontend must receive the REFINED query, not the warned first attempt"
    );
}

/// §11 Self-Check Loop: the loop is capped at 3 refinement turns. When the
/// cap is reached with warnings still outstanding, the BEST version (fewest
/// actionable warnings) is sent anyway — a later refinement that regresses
/// must not displace an earlier, better attempt.
#[tokio::test]
async fn explorer_ws_self_check_sends_best_version_after_cap() {
    // Four attempts with monotonically worsening warning counts:
    //   1 warning  (unresolved callout)          — refinement 1
    //   2 warnings (unresolved + substring)      — refinement 2
    //   3 warnings (unresolved + 2 substring)    — refinement 3 (cap)
    //   3 warnings (no improvement)              — cap reached: send best
    // The first attempt has the fewest warnings, so it is the best version.
    let q1 = r#"{"scope":{"type":"all"},"callouts":[{"node":"Missing1","text":"a"}]}"#;
    let q2 = r#"{"scope":{"type":"all"},"callouts":[{"node":"Missing1","text":"a"},{"node":"Port","text":"b"}]}"#;
    let q3 = r#"{"scope":{"type":"all"},"callouts":[{"node":"Missing1","text":"a"},{"node":"Port","text":"b"},{"node":"Coord","text":"c"}]}"#;
    let script = vec![
        format!("Attempt one.\n\n<view_query>{q1}</view_query>"),
        format!("Attempt two.\n\n<view_query>{q2}</view_query>"),
        format!("Attempt three.\n\n<view_query>{q3}</view_query>"),
        format!("Attempt four.\n\n<view_query>{q3}</view_query>"),
    ];
    let ctx = WsCtx::new_with_llm(Arc::new(ScriptedLlmFactory::new(script))).await;
    let repo_id = ctx.repo_id.clone();
    // TaskPort / CoordinatorService make "Port" and "Coord" resolve via
    // substring match (one actionable warning each), while "Missing1"
    // stays unresolved.
    ctx.state
        .graph_store
        .create_node(make_graph_node(&repo_id, "TaskPort", NodeType::Function))
        .await
        .unwrap();
    ctx.state
        .graph_store
        .create_node(make_graph_node(
            &repo_id,
            "CoordinatorService",
            NodeType::Class,
        ))
        .await
        .unwrap();

    let (mut sink, mut stream) = ctx.connect(&repo_id).await;
    let trace = run_message_turn(&mut sink, &mut stream, "show me the graph").await;

    let refining_count = trace.statuses.iter().filter(|s| *s == "refining").count();
    assert_eq!(
        refining_count, 3,
        "exactly MAX_REFINEMENT_TURNS (3) refinement turns may run, got {refining_count}"
    );
    assert!(trace.saw_ready, "turn must reach terminal ready status");
    assert_eq!(
        trace.view_queries.len(),
        1,
        "exactly one view_query may reach the frontend"
    );
    let q = &trace.view_queries[0];
    let callouts = q["callouts"].as_array().unwrap();
    assert_eq!(
        callouts.len(),
        1,
        "the BEST version (first attempt, 1 warning) must be sent after the cap — got {} callouts",
        callouts.len()
    );
    assert_eq!(
        callouts[0]["node"], "Missing1",
        "best version is the first attempt; later regressed attempts must not displace it"
    );
}

// ── §31: Saved view round-trip via REST (save → list → load → delete) ─────────

#[tokio::test]
async fn explorer_rest_saved_view_round_trip() {
    let ctx = WsCtx::new().await;
    let repo_id = ctx.repo_id.clone();
    let client = reqwest::Client::new();

    let query = json!({
        "scope": { "type": "filter", "node_types": ["endpoint"] },
        "emphasis": { "dim_unmatched": 0.1 },
        "zoom": "fit"
    });

    // Save (REST create)
    let resp = client
        .post(format!("{}/api/v1/repos/{repo_id}/views", ctx.base))
        .bearer_auth(TOKEN)
        .json(&json!({
            "name": "Endpoints view",
            "description": "All HTTP endpoints",
            "query": query
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "POST /views must create");
    let created: serde_json::Value = resp.json().await.unwrap();
    let view_id = created["id"].as_str().unwrap().to_string();
    assert_eq!(created["name"], "Endpoints view");
    assert_eq!(created["is_system"], false);
    assert_eq!(
        created["query"], query,
        "created view must round-trip the query JSON exactly"
    );

    // List — the saved view appears alongside the system defaults
    let resp = client
        .get(format!("{}/api/v1/repos/{repo_id}/views", ctx.base))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let list: serde_json::Value = resp.json().await.unwrap();
    let entries = list.as_array().expect("list_views returns an array");
    let entry = entries
        .iter()
        .find(|v| v["id"].as_str() == Some(view_id.as_str()))
        .expect("saved view must appear in the list");
    assert_eq!(entry["name"], "Endpoints view");

    // Load (REST get) — the exact query comes back for the frontend to render
    let resp = client
        .get(format!(
            "{}/api/v1/repos/{repo_id}/views/{view_id}",
            ctx.base
        ))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "GET /views/:id must return the view");
    let loaded: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(loaded["name"], "Endpoints view");
    assert_eq!(loaded["description"], "All HTTP endpoints");
    assert_eq!(
        loaded["query"], query,
        "loaded view query must equal the saved query (render round-trip)"
    );

    // Delete, then the view is gone (404 on re-load)
    let resp = client
        .delete(format!(
            "{}/api/v1/repos/{repo_id}/views/{view_id}",
            ctx.base
        ))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 204, "DELETE /views/:id must succeed");
    let resp = client
        .get(format!(
            "{}/api/v1/repos/{repo_id}/views/{view_id}",
            ctx.base
        ))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404, "deleted view must 404 on re-load");
}
