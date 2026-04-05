//! Explorer WebSocket handler.
//!
//! WS /api/v1/repos/:repo_id/explorer
//!
//! Receives user messages with canvas state, runs a multi-turn LLM agent loop
//! with tool calling (graph_summary, graph_query_dryrun, graph_nodes,
//! graph_edges, search), streams text responses and view queries back.
//! Also handles saved view CRUD over the same WebSocket.
//!
//! Auth: Bearer token in initial HTTP upgrade (via AuthenticatedAgent extractor
//! or ?token= query parameter). The WebSocket itself does NOT handle auth messages.
//!
//! **Preferred auth method**: Use the `Authorization: Bearer <token>` header on
//! the initial HTTP upgrade request. The `?token=` query parameter is supported
//! for backwards compatibility and browser environments that cannot set headers
//! on WebSocket upgrades, but header-based auth should be preferred in all other
//! cases to avoid token leakage in server logs and browser history.
//!
//! ## Architecture Note: Claude Agent SDK (default) vs LLM Port (fallback)
//!
//! Per spec, the Claude Agent SDK (`@anthropic-ai/claude-agent-sdk`) is the default
//! path. The SDK subprocess (`scripts/explorer-agent.mjs`) uses `query()` with MCP
//! tools served by the Gyre server. The native `llm_port` Rust path is the fallback
//! when the SDK script is not available (e.g. no Node.js) or explicitly disabled via
//! `GYRE_EXPLORER_SDK=0`. Both paths provide the same conversation-with-tools pattern
//! including self-check loop, refinement budget, and conversation history management.

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use gyre_common::view_query::{ExplorerClientMessage, ExplorerServerMessage, SavedViewSummary};
use gyre_common::Id;
use gyre_ports::saved_view::SavedView;
use gyre_ports::{
    ContentBlock, ConversationContent, ConversationMessage, ToolCall, ToolDefinition,
};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::{auth::AuthenticatedAgent, AppState};

/// Maximum accumulated text buffer from SDK subprocess (1 MB).
const MAX_SDK_ACCUMULATED_TEXT: usize = 1_024 * 1_024;

/// Maximum per-session graph cache size (nodes + edges combined count).
const MAX_GRAPH_CACHE_ENTRIES: usize = 50_000;

/// Safely serialize an ExplorerServerMessage to JSON.
/// On serialization failure, returns a minimal valid JSON error message
/// instead of an empty string (which would confuse the client).
fn serialize_msg(msg: &ExplorerServerMessage) -> Option<String> {
    match serde_json::to_string(msg) {
        Ok(s) => Some(s),
        Err(e) => {
            warn!("Failed to serialize explorer message: {e}");
            // Return a minimal valid error message the client can parse
            Some(r#"{"type":"error","message":"Internal serialization error"}"#.to_string())
        }
    }
}

/// Send a WebSocket ping if the interval has elapsed (non-blocking check).
/// Returns `Err(())` if the connection is dead (pong timeout or send failure).
async fn maybe_send_ping(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    ping_interval: &mut tokio::time::Interval,
    last_pong: &Arc<std::sync::Mutex<std::time::Instant>>,
    pong_timeout: std::time::Duration,
) -> Result<(), ()> {
    // Poll the interval without blocking: if a tick is ready, consume it and
    // send a ping; otherwise return immediately.
    let tick_ready = futures_util::future::poll_fn(|cx| match ping_interval.poll_tick(cx) {
        std::task::Poll::Ready(_) => std::task::Poll::Ready(true),
        std::task::Poll::Pending => std::task::Poll::Ready(false),
    })
    .await;
    if tick_ready {
        if last_pong.lock().unwrap().elapsed() > pong_timeout {
            warn!("Explorer WS: no pong during agent run, closing dead connection");
            return Err(());
        }
        if sender.send(Message::Ping(vec![].into())).await.is_err() {
            return Err(());
        }
    }
    Ok(())
}

/// Global per-user concurrent session counter, keyed by (tenant_id, agent_id)
/// to prevent cross-tenant interference.
/// Uses std::sync::Mutex for Drop compatibility (Drop cannot be async).
static ACTIVE_SESSIONS: std::sync::LazyLock<std::sync::Mutex<HashMap<String, usize>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));

/// Maximum concurrent explorer sessions per user.
/// Override with GYRE_EXPLORER_MAX_SESSIONS env var.
fn max_sessions_per_user() -> usize {
    std::env::var("GYRE_EXPLORER_MAX_SESSIONS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3)
}

/// Max tool-use turns (LLM calls for graph exploration before generating a view query).
/// This is separate from the refinement budget so they don't compete.
const MAX_TOOL_TURNS: usize = 5;

/// Max refinement-only turns (view query self-check loop).
/// Per spec: 3 dedicated refinement turns for the self-check loop.
/// These are tracked independently from tool-use turns.
const MAX_REFINEMENT_TURNS: usize = 3;

/// Max messages per session before requiring reconnect (prevents unbounded history).
/// Set high enough for deep conversational exploration (100 back-and-forth turns).
/// Override with GYRE_EXPLORER_MAX_MESSAGES env var.
fn max_session_messages() -> usize {
    std::env::var("GYRE_EXPLORER_MAX_MESSAGES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200)
}

/// Max conversation history entries before summarization window.
const MAX_CONVERSATION_HISTORY: usize = 20;

/// Max length of a single user message in characters.
const MAX_USER_MESSAGE_LENGTH: usize = 10_000;

/// Minimum interval between LLM-triggering user messages (rate limiting), in milliseconds.
/// Applies to `Message` type. SaveView/DeleteView have a separate rate limit
/// (max 10 per 60s). LoadView/ListViews are not rate-limited (read-only DB queries).
/// Override with GYRE_EXPLORER_MSG_INTERVAL_MS env var.
fn min_message_interval_ms() -> u64 {
    std::env::var("GYRE_EXPLORER_MSG_INTERVAL_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1000)
}

/// WebSocket ping interval in seconds (keeps connections alive through proxies).
const WS_PING_INTERVAL_SECS: u64 = 30;

/// POST /api/v1/ws-ticket — Issue a short-lived, single-use ticket for WebSocket auth.
///
/// Browser WebSocket API cannot send custom headers. Instead of leaking the
/// real auth token in a ?token= query parameter (visible in logs, browser
/// history, proxy caches), we issue a ticket: a 30-second, single-use opaque
/// token. The client connects with ?ticket=<ticket> instead.
pub async fn issue_ws_ticket(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
) -> impl IntoResponse {
    let ticket = state.ws_tickets.issue(auth);
    axum::Json(json!({ "ticket": ticket }))
}

pub async fn explorer_ws(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    auth: AuthenticatedAgent,
) -> impl IntoResponse {
    // Repo-scoped authorization: verify the user has access to this repo's tenant.
    // The AuthenticatedAgent extractor already validates the token.
    // Additional ABAC checks happen through the repo lookup below.
    ws.on_upgrade(move |socket| handle_explorer_session(socket, state, repo_id, auth))
}

async fn handle_explorer_session(
    socket: WebSocket,
    state: Arc<AppState>,
    repo_id: String,
    auth: AuthenticatedAgent,
) {
    let (mut sender, mut receiver) = socket.split();

    // Per-user concurrent session limit (prevents unbounded LLM cost).
    // Key includes tenant_id to prevent cross-tenant interference.
    let session_user = format!("{}:{}", auth.tenant_id, auth.agent_id);
    // Guard: decrement on drop (even if we return early or panic below).
    // Defined before the increment so the guard can be created immediately after.
    struct SessionGuard(String);
    impl Drop for SessionGuard {
        fn drop(&mut self) {
            let mut sessions = ACTIVE_SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(count) = sessions.get_mut(&self.0) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    sessions.remove(&self.0);
                }
            }
        }
    }
    let session_allowed = {
        let mut sessions = ACTIVE_SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
        let count = sessions.entry(session_user.clone()).or_default();
        if *count >= max_sessions_per_user() {
            false
        } else {
            *count += 1;
            true
        }
    };
    if !session_allowed {
        let err = ExplorerServerMessage::Error {
            message: format!(
                "Too many concurrent explorer sessions (max {}). Close an existing session first.",
                max_sessions_per_user()
            ),
        };
        let _ = sender
            .send(Message::Text(
                serialize_msg(&err).unwrap_or_default().into(),
            ))
            .await;
        return;
    }
    // Create guard immediately after increment so any subsequent panic decrements.
    let _session_guard = SessionGuard(session_user);

    // Verify the user has access to this repo via workspace → tenant chain.
    let rid = Id::new(&repo_id);
    let repo_workspace_id = match state.repos.find_by_id(&rid).await {
        Ok(Some(repo)) => repo.workspace_id.clone(),
        Ok(None) => {
            let err = ExplorerServerMessage::Error {
                message: format!("Repository not found: {repo_id}"),
            };
            let _ = sender
                .send(Message::Text(
                    serialize_msg(&err).unwrap_or_default().into(),
                ))
                .await;
            return;
        }
        Err(e) => {
            warn!("Failed to look up repository {repo_id}: {e}");
            let err = ExplorerServerMessage::Error {
                message: "Repository lookup failed".to_string(),
            };
            let _ = sender
                .send(Message::Text(
                    serialize_msg(&err).unwrap_or_default().into(),
                ))
                .await;
            return;
        }
    };
    // Check the workspace belongs to the user's tenant.
    match state.workspaces.find_by_id(&repo_workspace_id).await {
        Ok(Some(ws)) => {
            if ws.tenant_id.as_str() != auth.tenant_id {
                let err = ExplorerServerMessage::Error {
                    message: "Access denied: repo not in your tenant".to_string(),
                };
                let _ = sender
                    .send(Message::Text(
                        serialize_msg(&err).unwrap_or_default().into(),
                    ))
                    .await;
                return;
            }
        }
        _ => {
            // If workspace lookup fails, deny access.
            let err = ExplorerServerMessage::Error {
                message: "Access denied: workspace not found".to_string(),
            };
            let _ = sender
                .send(Message::Text(
                    serialize_msg(&err).unwrap_or_default().into(),
                ))
                .await;
            return;
        }
    }
    // Check workspace-level membership: the user must have a membership record
    // for this workspace (any role suffices for read-only explorer access).
    // Agent tokens (user_id=None) also require workspace scope verification
    // via their assigned workspace_id to prevent cross-workspace access.
    if let Some(ref user_id) = auth.user_id {
        match state
            .workspace_memberships
            .find_by_user_and_workspace(user_id, &repo_workspace_id)
            .await
        {
            Ok(Some(_)) => { /* has membership, proceed */ }
            Ok(None) => {
                let err = ExplorerServerMessage::Error {
                    message: "Access denied: not a member of this workspace".to_string(),
                };
                let _ = sender
                    .send(Message::Text(
                        serialize_msg(&err).unwrap_or_default().into(),
                    ))
                    .await;
                return;
            }
            Err(e) => {
                warn!(
                    "Workspace membership check failed for user {}: {e}",
                    user_id
                );
                let err = ExplorerServerMessage::Error {
                    message: "Access denied: membership check failed".to_string(),
                };
                let _ = sender
                    .send(Message::Text(
                        serialize_msg(&err).unwrap_or_default().into(),
                    ))
                    .await;
                return;
            }
        }
    } else if auth.agent_id == "system" {
        // System/dev token — admin access, skip workspace scope verification.
        // The AuthenticatedAgent extractor already verified this is the global auth token.
    } else {
        // No user_id means agent/service token — verify workspace scope.
        // Tenant-level auth was already verified by the AuthenticatedAgent extractor.
        // Additionally, verify the agent's workspace_id matches the repo's workspace
        // to prevent cross-workspace access within the same tenant.
        let agent_id = Id::new(&auth.agent_id);
        match state.agents.find_by_id(&agent_id).await {
            Ok(Some(agent)) => {
                if agent.workspace_id != repo_workspace_id {
                    warn!(
                        agent_id = %auth.agent_id,
                        agent_workspace = %agent.workspace_id,
                        repo_workspace = %repo_workspace_id,
                        "Agent token workspace mismatch — denying explorer access"
                    );
                    let err = ExplorerServerMessage::Error {
                        message: "Access denied: agent workspace does not match repo workspace"
                            .to_string(),
                    };
                    let _ = sender
                        .send(Message::Text(
                            serialize_msg(&err).unwrap_or_default().into(),
                        ))
                        .await;
                    return;
                }
            }
            Ok(None) => {
                // Agent record not found — deny access. Without a workspace
                // binding we cannot verify cross-workspace isolation.
                warn!(
                    agent_id = %auth.agent_id,
                    "Agent record not found for workspace check; denying access"
                );
                let err = ExplorerServerMessage::Error {
                    message: "Access denied: agent record not found".to_string(),
                };
                let _ = sender
                    .send(Message::Text(
                        serialize_msg(&err).unwrap_or_default().into(),
                    ))
                    .await;
                return;
            }
            Err(e) => {
                warn!(
                    agent_id = %auth.agent_id,
                    error = ?e,
                    "Failed to look up agent for workspace check"
                );
                let err = ExplorerServerMessage::Error {
                    message: "Access denied: workspace verification failed".to_string(),
                };
                let _ = sender
                    .send(Message::Text(
                        serialize_msg(&err).unwrap_or_default().into(),
                    ))
                    .await;
                return;
            }
        }
    }

    info!(repo_id = %repo_id, user = %auth.agent_id, "Explorer WebSocket session started");

    // Send deprecation warning if client used ?token= query parameter auth.
    if auth.deprecated_token_auth {
        warn!(
            repo_id = %repo_id,
            user = %auth.agent_id,
            "Explorer WS session using deprecated ?token= auth"
        );
        let warning = ExplorerServerMessage::Warning {
            message: "Using deprecated ?token= auth. Please use ticket-based auth \
                      (POST /api/v1/ws-ticket) for security. The ?token= parameter \
                      leaks credentials in server logs and browser history."
                .to_string(),
        };
        let _ = sender
            .send(Message::Text(
                serialize_msg(&warning).unwrap_or_default().into(),
            ))
            .await;
    }

    // Cancellation channel: set to true to cancel the running agent query.
    let (cancel_tx, mut cancel_rx) = tokio::sync::watch::channel(false);

    // Maintain conversation history across messages within a session.
    let mut conversation_history: Vec<ConversationMessage> = Vec::new();
    let mut message_count: usize = 0;
    let mut last_message_time = std::time::Instant::now()
        .checked_sub(std::time::Duration::from_secs(10))
        .unwrap_or_else(std::time::Instant::now);
    // Rate limiter for SaveView/DeleteView: max 10 operations per 60 seconds.
    let mut view_op_timestamps: Vec<std::time::Instant> = Vec::new();
    // Graph data cache with TTL (30s). Re-fetch when stale, not on every message.
    let mut cached_nodes: Option<Vec<gyre_common::graph::GraphNode>> = None;
    let mut cached_edges: Option<Vec<gyre_common::graph::GraphEdge>> = None;
    // Cached graph_summary: computed from cached_nodes/cached_edges, invalidated
    // whenever the graph cache is invalidated.
    let mut cached_graph_summary: Option<String> = None;
    let mut cache_time = std::time::Instant::now()
        .checked_sub(std::time::Duration::from_secs(60))
        .unwrap_or_else(std::time::Instant::now);

    // Buffer for messages received during agent execution that should be
    // processed after the agent completes (SaveView, DeleteView, LoadView, ListViews).
    let mut queued_messages: Vec<ExplorerClientMessage> = Vec::new();
    // Replay buffer: queued messages are moved here after agent completes,
    // then drained at the top of the main loop before reading from the WebSocket.
    let mut replay_buffer: std::collections::VecDeque<ExplorerClientMessage> = std::collections::VecDeque::new();

    // Ping/pong keepalive: keeps connections alive through proxies/load balancers.
    // Track last pong to detect dead connections (2 missed pongs = dead).
    let mut ping_interval =
        tokio::time::interval(std::time::Duration::from_secs(WS_PING_INTERVAL_SECS));
    ping_interval.tick().await; // consume the immediate first tick
    let last_pong = Arc::new(std::sync::Mutex::new(std::time::Instant::now()));
    let pong_timeout = std::time::Duration::from_secs(WS_PING_INTERVAL_SECS * 2 + 5);

    loop {
        // First, drain any messages queued during agent execution.
        let client_msg: ExplorerClientMessage = if let Some(replayed) = replay_buffer.pop_front() {
            replayed
        } else {
            // No queued messages — read from WebSocket.
            let msg = tokio::select! {
                incoming = receiver.next() => {
                    match incoming {
                        Some(Ok(Message::Text(text))) => text,
                        Some(Ok(Message::Close(_))) | None => break,
                        Some(Ok(Message::Pong(_))) => {
                            *last_pong.lock().unwrap() = std::time::Instant::now();
                            continue;
                        }
                        Some(Ok(_)) => continue,
                        Some(Err(e)) => {
                            warn!("WebSocket error: {e}");
                            break;
                        }
                    }
                }
                _ = ping_interval.tick() => {
                    // Check for dead connection (no pong received within timeout)
                    if last_pong.lock().unwrap().elapsed() > pong_timeout {
                        warn!(user = %auth.agent_id, "Explorer WS: no pong received in {:?}, closing dead connection", pong_timeout);
                        break;
                    }
                    if sender.send(Message::Ping(vec![].into())).await.is_err() {
                        break; // Client disconnected
                    }
                    continue;
                }
            };

            // Check raw frame length BEFORE deserialization to prevent CPU exhaustion
            // from deeply nested JSON structures. Two thresholds:
            // 1. MAX_USER_MESSAGE_LENGTH + envelope: tighter limit that catches oversized
            //    user text before spending CPU on JSON parsing.
            // 2. MAX_RAW_FRAME_SIZE: hard upper bound for any message type.
            //
            // The user's text field is embedded in the JSON envelope along with
            // canvas_state, so we allow 4x the user message limit as headroom for the
            // JSON structure (canvas_state, type tag, etc.).
            const MAX_RAW_FRAME_SIZE: usize = MAX_USER_MESSAGE_LENGTH * 4 + 32768;
            if msg.len() > MAX_RAW_FRAME_SIZE {
                let err = ExplorerServerMessage::Error {
                    message: format!(
                        "Message too large ({} bytes). Maximum frame size is {} bytes.",
                        msg.len(),
                        MAX_RAW_FRAME_SIZE
                    ),
                };
                let _ = sender
                    .send(Message::Text(
                        serialize_msg(&err).unwrap_or_default().into(),
                    ))
                    .await;
                continue;
            }

            match serde_json::from_str(&msg) {
                Ok(m) => m,
                Err(e) => {
                    warn!(error = ?e, "Invalid explorer WebSocket message");
                    let err = ExplorerServerMessage::Error {
                        message: "Invalid message format".to_string(),
                    };
                    let _ = sender
                        .send(Message::Text(
                            serialize_msg(&err).unwrap_or_default().into(),
                        ))
                        .await;
                    continue;
                }
            }
        };

        match client_msg {
            ExplorerClientMessage::Message { text, canvas_state } => {
                // Input length validation: reject oversized messages.
                if text.len() > MAX_USER_MESSAGE_LENGTH {
                    let err = ExplorerServerMessage::Error {
                        message: format!(
                            "Message too long ({} chars). Maximum is {} characters.",
                            text.len(),
                            MAX_USER_MESSAGE_LENGTH
                        ),
                    };
                    let _ = sender
                        .send(Message::Text(
                            serialize_msg(&err).unwrap_or_default().into(),
                        ))
                        .await;
                    continue;
                }

                // Rate limiting: enforce minimum interval between messages.
                let now = std::time::Instant::now();
                let elapsed = now.duration_since(last_message_time).as_millis() as u64;
                if elapsed < min_message_interval_ms() {
                    let err = ExplorerServerMessage::Error {
                        message: "Please wait before sending another message.".to_string(),
                    };
                    let _ = sender
                        .send(Message::Text(
                            serialize_msg(&err).unwrap_or_default().into(),
                        ))
                        .await;
                    continue;
                }
                last_message_time = now;

                // Session message limit: prevent unbounded history growth.
                message_count += 1;

                // Invalidate graph cache if older than 30 seconds (balance freshness vs cost).
                // Do NOT reset cache_time here — let the actual reload (in run_explorer_agent)
                // set it when fresh data is fetched. Otherwise we'd mark stale data as fresh.
                if cache_time.elapsed() > std::time::Duration::from_secs(30) {
                    cached_nodes = None;
                    cached_edges = None;
                    cached_graph_summary = None;
                }
                // Heuristic cache invalidation: if the user's message suggests they
                // expect fresh data (e.g. after a push), force-invalidate regardless
                // of TTL. Catches the common case where a user pushes code and
                // immediately asks the explorer about changes.
                //
                // Two-tier strategy to reduce false positives:
                // 1. Strong signals: multi-word phrases that unambiguously indicate
                //    a desire for fresh data. Always invalidate.
                // 2. Staleness + entity heuristic: if cache is >60s old AND the
                //    message contains a code-entity word (function, endpoint, type,
                //    module, etc.), refresh. This catches "Show me the new endpoint
                //    I added" without false-positive on "What is the latest version
                //    of search?" when the cache is still fresh.
                {
                    let q_lower = text.to_lowercase();

                    // Tier 1: strong freshness signals (multi-word or unambiguous)
                    let strong_signals = [
                        "what changed",
                        "after push",
                        "just pushed",
                        "just committed",
                        "just added",
                        "just created",
                        "just deleted",
                        "just removed",
                        "just modified",
                        "just updated",
                        "new code",
                        "refresh",
                        "reload graph",
                        "i added",
                        "i created",
                        "i deleted",
                        "i removed",
                        "i modified",
                        "i updated",
                        "i changed",
                        "recently added",
                        "recently created",
                        "recently changed",
                        "recently modified",
                        "recently deleted",
                        "recently removed",
                    ];
                    let strong_match = strong_signals.iter().any(|kw| q_lower.contains(kw));

                    // Tier 2: time-based staleness + code-entity word.
                    // Only triggers when cache is >60s old.
                    let stale = cache_time.elapsed() > std::time::Duration::from_secs(60);
                    let entity_words = [
                        "endpoint",
                        "function",
                        "method",
                        "type",
                        "struct",
                        "module",
                        "interface",
                        "handler",
                        "route",
                        "api",
                        "class",
                        "trait",
                        "enum",
                        "component",
                        "service",
                    ];
                    let has_entity = entity_words.iter().any(|w| q_lower.contains(w));
                    let change_words = [
                        "new", "added", "created", "deleted", "removed", "modified", "updated",
                        "changed", "recent", "latest",
                    ];
                    let has_change = change_words.iter().any(|w| {
                        // Word-boundary match to avoid false positives like
                        // "latest" matching inside unrelated sentences when cache
                        // is fresh. We check that the word appears as a standalone
                        // token (preceded/followed by non-alphanumeric or string boundary).
                        q_lower
                            .split(|c: char| !c.is_alphanumeric() && c != '_')
                            .any(|token| token == *w)
                    });
                    let stale_entity_match = stale && has_entity && has_change;

                    if strong_match || stale_entity_match {
                        cached_nodes = None;
                        cached_edges = None;
                        cached_graph_summary = None;
                    }
                }
                if message_count > max_session_messages() {
                    let err = ExplorerServerMessage::Error {
                        message:
                            "Session message limit reached. Please reconnect for a fresh session."
                                .to_string(),
                    };
                    // Send error and flush before closing to ensure client receives it
                    let _ = sender
                        .send(Message::Text(
                            serialize_msg(&err).unwrap_or_default().into(),
                        ))
                        .await;
                    let _ = sender.flush().await;
                    let _ = sender.close().await;
                    break;
                }

                // Send thinking status
                send_status(&mut sender, "thinking").await;

                // Reset cancel signal for this run
                let _ = cancel_tx.send(false);

                // Run the agent loop with conversation history, while
                // concurrently listening for Cancel messages on the WebSocket.
                // This allows the user to cancel a slow query mid-flight.
                let agent_result = tokio::select! {
                    result = run_explorer_agent(
                        &state,
                        &repo_id,
                        &text,
                        &canvas_state,
                        &mut sender,
                        &mut cancel_rx,
                        &mut conversation_history,
                        &mut cached_nodes,
                        &mut cached_edges,
                        &mut cached_graph_summary,
                        &auth,
                        &repo_workspace_id,
                        &mut ping_interval,
                        last_pong.clone(),
                        pong_timeout,
                    ) => result,
                    _ = async {
                        // Listen for Cancel messages while the agent is running.
                        // View CRUD messages (SaveView, DeleteView, LoadView, ListViews)
                        // are queued and processed after the agent completes.
                        loop {
                            match receiver.next().await {
                                Some(Ok(Message::Text(raw))) => {
                                    if let Ok(client_msg) = serde_json::from_str::<ExplorerClientMessage>(&raw) {
                                        if matches!(client_msg, ExplorerClientMessage::Cancel) {
                                            info!(user = %auth.agent_id, "Cancel received during agent run");
                                            let _ = cancel_tx.send(true);
                                            break;
                                        }
                                        // Queue view CRUD messages for processing after agent completes.
                                        match &client_msg {
                                            ExplorerClientMessage::SaveView { .. }
                                            | ExplorerClientMessage::DeleteView { .. }
                                            | ExplorerClientMessage::LoadView { .. }
                                            | ExplorerClientMessage::ListViews => {
                                                debug!(user = %auth.agent_id, "Queuing message during agent run");
                                                queued_messages.push(client_msg);
                                            }
                                            _ => {
                                                // Message type (e.g. another Message) ignored during agent run
                                            }
                                        }
                                    }
                                }
                                Some(Ok(Message::Close(_))) | None => break,
                                Some(Ok(Message::Pong(_))) => {
                                    *last_pong.lock().unwrap() = std::time::Instant::now();
                                }
                                _ => {}
                            }
                        }
                    } => {
                        // Agent was cancelled or connection closed during execution
                        Ok(())
                    }
                };

                // If the agent loaded fresh graph data, reset cache_time now.
                if cached_nodes.is_some() && cache_time.elapsed() > std::time::Duration::from_secs(30) {
                    cache_time = std::time::Instant::now();
                }

                match agent_result {
                    Ok(()) => {}
                    Err(e) => {
                        warn!(repo_id = %repo_id, error = ?e, "Explorer query failed");
                        let err = ExplorerServerMessage::Error {
                            message: "Explorer query failed. Please try again.".to_string(),
                        };
                        let _ = sender
                            .send(Message::Text(
                                serialize_msg(&err).unwrap_or_default().into(),
                            ))
                            .await;
                    }
                }

                // Re-queue messages that were buffered during agent execution
                // so they get processed on subsequent loop iterations.
                if !queued_messages.is_empty() {
                    debug!(
                        count = queued_messages.len(),
                        "Replaying messages queued during agent run"
                    );
                    for queued_msg in queued_messages.drain(..) {
                        replay_buffer.push_back(queued_msg);
                    }
                }

                // Include graph data age so the UI can display freshness
                let age = cache_time.elapsed().as_secs();
                send_status_full(&mut sender, "ready", None, Some(age)).await;
            }

            ExplorerClientMessage::SaveView {
                name,
                description,
                query,
                scope,
            } => {
                // Rate limit: max 10 SaveView/DeleteView ops per 60 seconds
                let now_view = std::time::Instant::now();
                view_op_timestamps.retain(|t| now_view.duration_since(*t).as_secs() < 60);
                if view_op_timestamps.len() >= 10 {
                    let err = ExplorerServerMessage::Error {
                        message: "Too many view operations. Please wait before saving again."
                            .to_string(),
                    };
                    let _ = sender
                        .send(Message::Text(
                            serialize_msg(&err).unwrap_or_default().into(),
                        ))
                        .await;
                    continue;
                }
                view_op_timestamps.push(now_view);

                // Validate name/description length limits
                if name.len() > 200 {
                    let err = ExplorerServerMessage::Error {
                        message: "View name exceeds 200 character limit".to_string(),
                    };
                    let _ = sender
                        .send(Message::Text(
                            serialize_msg(&err).unwrap_or_default().into(),
                        ))
                        .await;
                    continue;
                }
                if let Some(ref desc) = description {
                    if desc.len() > 2000 {
                        let err = ExplorerServerMessage::Error {
                            message: "View description exceeds 2000 character limit".to_string(),
                        };
                        let _ = sender
                            .send(Message::Text(
                                serialize_msg(&err).unwrap_or_default().into(),
                            ))
                            .await;
                        continue;
                    }
                }
                // Validate the view query before saving.
                match serde_json::from_value::<gyre_common::view_query::ViewQuery>(query.clone()) {
                    Ok(parsed) => {
                        let errors = parsed.validate();
                        if !errors.is_empty() {
                            let err = ExplorerServerMessage::Error {
                                message: format!("Invalid view query: {}", errors.join("; ")),
                            };
                            let _ = sender
                                .send(Message::Text(
                                    serialize_msg(&err).unwrap_or_default().into(),
                                ))
                                .await;
                            continue;
                        }
                        // Log non-fatal warnings (e.g. unknown palette) but don't block save
                        for w in parsed.warnings() {
                            info!(repo_id = %repo_id, "SaveView warning: {}", w);
                        }
                    }
                    Err(e) => {
                        let err = ExplorerServerMessage::Error {
                            message: format!("Invalid view query format: {e}"),
                        };
                        let _ = sender
                            .send(Message::Text(
                                serialize_msg(&err).unwrap_or_default().into(),
                            ))
                            .await;
                        continue;
                    }
                }

                // Use workspace_id cached from session start to avoid TOCTOU.
                let workspace_id = repo_workspace_id.to_string();
                let now = crate::api::now_secs();
                // When scope is "workspace", save the view as workspace-scoped
                // so it appears in workspace-level view listings.
                let effective_repo_id = if scope.as_deref() == Some("workspace") {
                    "__workspace__".to_string()
                } else {
                    repo_id.clone()
                };
                let view = SavedView {
                    id: crate::api::new_id(),
                    repo_id: Id::new(&effective_repo_id),
                    workspace_id: Id::new(&workspace_id),
                    tenant_id: Id::new(&auth.tenant_id),
                    name,
                    description,
                    query_json: serde_json::to_string(&query).unwrap_or_else(|_| "{}".to_string()),
                    created_by: auth.agent_id.clone(),
                    created_at: now,
                    updated_at: now,
                    is_system: false,
                };
                match state.saved_views.create(view).await {
                    Ok(_v) => {
                        // Re-fetch the full view list so the client gets all views, not just the new one.
                        let tenant_id = Id::new(&auth.tenant_id);
                        match state
                            .saved_views
                            .list_by_repo_and_tenant(&rid, &tenant_id)
                            .await
                        {
                            Ok(all_views) => {
                                let summaries: Vec<SavedViewSummary> = all_views
                                    .into_iter()
                                    .map(|v| SavedViewSummary {
                                        id: v.id.to_string(),
                                        name: v.name,
                                        description: v.description,
                                        created_at: v.created_at,
                                        is_system: v.is_system,
                                    })
                                    .collect();
                                let msg = ExplorerServerMessage::Views { views: summaries };
                                let _ = sender
                                    .send(Message::Text(
                                        serialize_msg(&msg).unwrap_or_default().into(),
                                    ))
                                    .await;
                            }
                            Err(_) => {
                                // Fall back to just the new view
                                let msg = ExplorerServerMessage::Views {
                                    views: vec![SavedViewSummary {
                                        id: _v.id.to_string(),
                                        name: _v.name,
                                        description: _v.description,
                                        created_at: _v.created_at,
                                        is_system: _v.is_system,
                                    }],
                                };
                                let _ = sender
                                    .send(Message::Text(
                                        serialize_msg(&msg).unwrap_or_default().into(),
                                    ))
                                    .await;
                            }
                        }
                    }
                    Err(e) => {
                        warn!(error = ?e, "Failed to save view");
                        let err = ExplorerServerMessage::Error {
                            message: "Failed to save view. Please try again.".to_string(),
                        };
                        let _ = sender
                            .send(Message::Text(
                                serialize_msg(&err).unwrap_or_default().into(),
                            ))
                            .await;
                    }
                }
            }

            ExplorerClientMessage::LoadView { view_id } => {
                let vid = Id::new(&view_id);
                match state.saved_views.get(&vid).await {
                    Ok(Some(v)) => {
                        // Verify the view belongs to this repo or is a workspace-scoped view.
                        // Workspace-scoped views use "__workspace__" as repo_id and are
                        // accessible from any repo within that workspace.
                        let is_repo_view = v.repo_id.as_str() == repo_id;
                        let is_workspace_view = v.repo_id.as_str() == "__workspace__"
                            && v.workspace_id == repo_workspace_id;
                        if !is_repo_view && !is_workspace_view {
                            let err = ExplorerServerMessage::Error {
                                message: "View does not belong to this repository".to_string(),
                            };
                            let _ = sender
                                .send(Message::Text(
                                    serialize_msg(&err).unwrap_or_default().into(),
                                ))
                                .await;
                            continue;
                        }
                        // Verify tenant access.
                        if v.tenant_id.as_str() != auth.tenant_id {
                            let err = ExplorerServerMessage::Error {
                                message: "Access denied".to_string(),
                            };
                            let _ = sender
                                .send(Message::Text(
                                    serialize_msg(&err).unwrap_or_default().into(),
                                ))
                                .await;
                            continue;
                        }
                        let query: serde_json::Value = match serde_json::from_str(&v.query_json) {
                            Ok(q) => q,
                            Err(e) => {
                                warn!(view_id = %view_id, error = ?e, "Malformed view query JSON");
                                let err = ExplorerServerMessage::Error {
                                    message: "Saved view has invalid query data".to_string(),
                                };
                                let _ = sender
                                    .send(Message::Text(
                                        serialize_msg(&err).unwrap_or_default().into(),
                                    ))
                                    .await;
                                continue;
                            }
                        };
                        // Check for stale references in the loaded view query.
                        // Covers Focus, Concept, and Filter scopes.
                        let mut stale_warning = None;
                        if let Ok(vq) = serde_json::from_value::<gyre_common::view_query::ViewQuery>(
                            query.clone(),
                        ) {
                            // Collect node names to verify from all view query sections
                            let mut nodes_to_check: Vec<String> = Vec::new();
                            match &vq.scope {
                                gyre_common::view_query::Scope::Focus { ref node, .. } => {
                                    if node != "$clicked" && node != "$selected" {
                                        nodes_to_check.push(node.clone());
                                    }
                                }
                                gyre_common::view_query::Scope::Concept {
                                    ref seed_nodes, ..
                                } => {
                                    for sn in seed_nodes {
                                        if sn != "$clicked" && sn != "$selected" {
                                            nodes_to_check.push(sn.clone());
                                        }
                                    }
                                }
                                gyre_common::view_query::Scope::Filter {
                                    computed: Some(ref expr),
                                    ..
                                } => {
                                    // Extract node names from computed expressions
                                    // e.g. "$governed_by('deleted-spec.md')" -> "deleted-spec.md"
                                    extract_computed_node_refs(expr, &mut nodes_to_check);
                                }
                                _ => {}
                            }
                            // Check group node names for staleness
                            for group in &vq.groups {
                                for gn in &group.nodes {
                                    if !gn.starts_with('$') {
                                        nodes_to_check.push(gn.clone());
                                    }
                                }
                            }
                            // Check callout node names for staleness
                            for callout in &vq.callouts {
                                if !callout.node.starts_with('$') {
                                    nodes_to_check.push(callout.node.clone());
                                }
                            }
                            // Check narrative step node names for staleness
                            for step in &vq.narrative {
                                if !step.node.starts_with('$') {
                                    nodes_to_check.push(step.node.clone());
                                }
                            }
                            if !nodes_to_check.is_empty() {
                                // Refresh graph cache if needed
                                if cached_nodes.is_none() {
                                    let rid = Id::new(&repo_id);
                                    cached_nodes = Some(
                                        state
                                            .graph_store
                                            .list_nodes(&rid, None)
                                            .await
                                            .unwrap_or_default(),
                                    );
                                    cached_edges = Some(
                                        state
                                            .graph_store
                                            .list_edges(&rid, None)
                                            .await
                                            .unwrap_or_default(),
                                    );
                                }
                                if let Some(ref graph_nodes) = cached_nodes {
                                    let missing: Vec<&str> = nodes_to_check
                                        .iter()
                                        .filter(|node| {
                                            !graph_nodes.iter().any(|n| {
                                                n.deleted_at.is_none()
                                                    && (n.name == **node
                                                        || n.qualified_name == **node
                                                        || n.id.to_string() == **node)
                                            })
                                        })
                                        .map(|s| s.as_str())
                                        .collect();
                                    if !missing.is_empty() {
                                        stale_warning = Some(format!(
                                            "Warning: node(s) {} not found in current graph — this saved view may be stale.",
                                            missing.join(", ")
                                        ));
                                    }
                                }
                            }
                        }
                        if let Some(warning) = stale_warning {
                            let warn_msg = ExplorerServerMessage::Text {
                                content: warning,
                                done: true,
                            };
                            let _ = sender
                                .send(Message::Text(
                                    serialize_msg(&warn_msg).unwrap_or_default().into(),
                                ))
                                .await;
                        }
                        let msg = ExplorerServerMessage::ViewQuery {
                            query,
                            explanation: None,
                        };
                        let _ = sender
                            .send(Message::Text(
                                serialize_msg(&msg).unwrap_or_default().into(),
                            ))
                            .await;
                    }
                    Ok(None) => {
                        let err = ExplorerServerMessage::Error {
                            message: format!("View not found: {view_id}"),
                        };
                        let _ = sender
                            .send(Message::Text(
                                serialize_msg(&err).unwrap_or_default().into(),
                            ))
                            .await;
                    }
                    Err(e) => {
                        warn!(view_id = %view_id, error = ?e, "Failed to load view");
                        let err = ExplorerServerMessage::Error {
                            message: "Failed to load view. Please try again.".to_string(),
                        };
                        let _ = sender
                            .send(Message::Text(
                                serialize_msg(&err).unwrap_or_default().into(),
                            ))
                            .await;
                    }
                }
            }

            ExplorerClientMessage::DeleteView { view_id } => {
                // Rate limit: max 10 SaveView/DeleteView ops per 60 seconds
                let now_view = std::time::Instant::now();
                view_op_timestamps.retain(|t| now_view.duration_since(*t).as_secs() < 60);
                if view_op_timestamps.len() >= 10 {
                    let err = ExplorerServerMessage::Error {
                        message: "Too many view operations. Please wait before deleting again."
                            .to_string(),
                    };
                    let _ = sender
                        .send(Message::Text(
                            serialize_msg(&err).unwrap_or_default().into(),
                        ))
                        .await;
                    continue;
                }
                view_op_timestamps.push(now_view);

                let vid = Id::new(&view_id);
                match state.saved_views.get(&vid).await {
                    Ok(Some(v)) => {
                        // Verify the view belongs to this repo (or is workspace-scoped)
                        // and tenant.
                        let is_repo_view = v.repo_id.as_str() == repo_id;
                        let is_workspace_view = v.repo_id.as_str() == "__workspace__"
                            && v.workspace_id == repo_workspace_id;
                        if !is_repo_view && !is_workspace_view {
                            let err = ExplorerServerMessage::Error {
                                message: "View does not belong to this repository".to_string(),
                            };
                            let _ = sender
                                .send(Message::Text(
                                    serialize_msg(&err).unwrap_or_default().into(),
                                ))
                                .await;
                            continue;
                        }
                        if v.tenant_id.as_str() != auth.tenant_id {
                            let err = ExplorerServerMessage::Error {
                                message: "Access denied".to_string(),
                            };
                            let _ = sender
                                .send(Message::Text(
                                    serialize_msg(&err).unwrap_or_default().into(),
                                ))
                                .await;
                            continue;
                        }
                        // Don't allow deleting system views.
                        if v.is_system {
                            let err = ExplorerServerMessage::Error {
                                message: "Cannot delete system views".to_string(),
                            };
                            let _ = sender
                                .send(Message::Text(
                                    serialize_msg(&err).unwrap_or_default().into(),
                                ))
                                .await;
                            continue;
                        }
                        // Only the creator or a human admin can delete views.
                        // Agent tokens (user_id=None) can ONLY delete views they
                        // created — they never get admin override, preventing agents
                        // from deleting human-created views.
                        let is_human_admin = auth.user_id.is_some()
                            && auth
                                .roles
                                .iter()
                                .any(|r| matches!(r, gyre_domain::user::UserRole::Admin));
                        if v.created_by != auth.agent_id && !is_human_admin {
                            let err = ExplorerServerMessage::Error {
                                message: "Access denied: you can only delete your own views"
                                    .to_string(),
                            };
                            let _ = sender
                                .send(Message::Text(
                                    serialize_msg(&err).unwrap_or_default().into(),
                                ))
                                .await;
                            continue;
                        }
                        // Use delete_scoped for defense-in-depth tenant isolation.
                        let tid = Id::new(&auth.tenant_id);
                        match state.saved_views.delete_scoped(&vid, &tid).await {
                            Ok(_) => {
                                // Return updated view list
                                let tenant_id = Id::new(&auth.tenant_id);
                                if let Ok(all_views) = state
                                    .saved_views
                                    .list_by_repo_and_tenant(&rid, &tenant_id)
                                    .await
                                {
                                    let summaries: Vec<SavedViewSummary> = all_views
                                        .into_iter()
                                        .map(|v| SavedViewSummary {
                                            id: v.id.to_string(),
                                            name: v.name,
                                            description: v.description,
                                            created_at: v.created_at,
                                            is_system: v.is_system,
                                        })
                                        .collect();
                                    let msg = ExplorerServerMessage::Views { views: summaries };
                                    let _ = sender
                                        .send(Message::Text(
                                            serialize_msg(&msg).unwrap_or_default().into(),
                                        ))
                                        .await;
                                }
                            }
                            Err(e) => {
                                warn!(view_id = %view_id, error = ?e, "Failed to delete view");
                                let err = ExplorerServerMessage::Error {
                                    message: "Failed to delete view".to_string(),
                                };
                                let _ = sender
                                    .send(Message::Text(
                                        serialize_msg(&err).unwrap_or_default().into(),
                                    ))
                                    .await;
                            }
                        }
                    }
                    Ok(None) => {
                        let err = ExplorerServerMessage::Error {
                            message: format!("View not found: {view_id}"),
                        };
                        let _ = sender
                            .send(Message::Text(
                                serialize_msg(&err).unwrap_or_default().into(),
                            ))
                            .await;
                    }
                    Err(e) => {
                        warn!(view_id = %view_id, error = ?e, "Failed to look up view");
                        let err = ExplorerServerMessage::Error {
                            message: "Failed to look up view".to_string(),
                        };
                        let _ = sender
                            .send(Message::Text(
                                serialize_msg(&err).unwrap_or_default().into(),
                            ))
                            .await;
                    }
                }
            }

            ExplorerClientMessage::Cancel => {
                // Cancel signal: set the watch channel to true.
                // The running agent loop checks this at each turn boundary.
                // If no agent is running, this is a no-op (the signal resets on next Message).
                debug!(user = %auth.agent_id, "Cancel requested by user");
                let _ = cancel_tx.send(true);
            }

            ExplorerClientMessage::ListViews => {
                // ListViews is read-only — not rate-limited (unlike SaveView/DeleteView).
                let tenant_id = Id::new(&auth.tenant_id);
                // Query repo-scoped views AND workspace-scoped views (repo_id="__workspace__")
                // then merge them, deduplicating by ID.
                let workspace_view_id = Id::new("__workspace__");
                let (repo_views, ws_views) = tokio::join!(
                    state.saved_views.list_by_repo_and_tenant(&rid, &tenant_id),
                    state.saved_views.list_by_repo_and_tenant(&workspace_view_id, &tenant_id),
                );
                match repo_views {
                    Ok(mut views) => {
                        // Merge workspace-scoped views, filtering to same workspace.
                        if let Ok(ws) = ws_views {
                            for v in ws {
                                if v.workspace_id == repo_workspace_id {
                                    views.push(v);
                                }
                            }
                        }
                        // Views are already filtered by tenant_id at the SQL level.
                        let mut summaries: Vec<SavedViewSummary> = views
                            .into_iter()
                            .map(|v| SavedViewSummary {
                                id: v.id.to_string(),
                                name: v.name,
                                description: v.description,
                                created_at: v.created_at,
                                is_system: v.is_system,
                            })
                            .collect();

                        // Seed any missing system default views. Check each expected
                        // view by name individually so that partially-seeded sets get
                        // completed (e.g. if a new system view is added in a release).
                        let existing_system_names: std::collections::HashSet<String> = summaries
                            .iter()
                            .filter(|s| s.is_system)
                            .map(|s| s.name.clone())
                            .collect();
                        {
                            let now = crate::api::now_secs();
                            let defaults = system_default_views();
                            for (name, description, query_json) in &defaults {
                                // Skip if a system view with this name already exists
                                if existing_system_names.contains(*name) {
                                    continue;
                                }
                                let view = SavedView {
                                    id: crate::api::new_id(),
                                    repo_id: rid.clone(),
                                    workspace_id: repo_workspace_id.clone(),
                                    tenant_id: Id::new(&auth.tenant_id),
                                    name: name.to_string(),
                                    description: Some(description.to_string()),
                                    query_json: query_json.to_string(),
                                    created_by: "system".to_string(),
                                    created_at: now,
                                    updated_at: now,
                                    is_system: true,
                                };
                                match state.saved_views.create(view).await {
                                    Ok(v) => {
                                        summaries.push(SavedViewSummary {
                                            id: v.id.to_string(),
                                            name: v.name,
                                            description: v.description,
                                            created_at: v.created_at,
                                            is_system: v.is_system,
                                        });
                                    }
                                    Err(e) => {
                                        // The unique index idx_saved_views_no_dup_system
                                        // (tenant_id, workspace_id, repo_id, name, is_system)
                                        // prevents duplicate system views. A UNIQUE constraint
                                        // error here means another session already seeded this
                                        // view — safe to ignore. Log other errors at warn level.
                                        let err_msg = e.to_string();
                                        if err_msg.contains("UNIQUE constraint") {
                                            debug!(name = %name, "System view already seeded by concurrent session");
                                        } else {
                                            warn!(error = ?e, "Failed to seed default view: {name}");
                                        }
                                    }
                                }
                            }
                        }

                        let msg = ExplorerServerMessage::Views { views: summaries };
                        let _ = sender
                            .send(Message::Text(
                                serialize_msg(&msg).unwrap_or_default().into(),
                            ))
                            .await;
                    }
                    Err(e) => {
                        warn!(repo_id = %repo_id, error = ?e, "Failed to list views");
                        let err = ExplorerServerMessage::Error {
                            message: "Failed to list views. Please try again.".to_string(),
                        };
                        let _ = sender
                            .send(Message::Text(
                                serialize_msg(&err).unwrap_or_default().into(),
                            ))
                            .await;
                    }
                }
            }
        }
    }

    info!(repo_id = %repo_id, "Explorer WebSocket session ended");
}

async fn send_status(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    status: &str,
) -> bool {
    send_status_with_path(sender, status, None).await
}

async fn send_status_with_path(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    status: &str,
    agent_path: Option<&str>,
) -> bool {
    send_status_full(sender, status, agent_path, None).await
}

async fn send_status_full(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    status: &str,
    agent_path: Option<&str>,
    graph_data_age_secs: Option<u64>,
) -> bool {
    let msg = ExplorerServerMessage::Status {
        status: status.to_string(),
        agent_path: agent_path.map(|s| s.to_string()),
        graph_data_age_secs,
    };
    sender
        .send(Message::Text(
            serialize_msg(&msg).unwrap_or_default().into(),
        ))
        .await
        .is_ok()
}

/// Stream text to the client. When the SDK subprocess provides real token-level
/// streaming (small chunks), messages are forwarded directly. For large text blocks
/// from the native LlmPort path (which returns the entire response at once after
/// LLM inference), this function sends the first sentence immediately so the user
/// sees something right away instead of nothing for 3-15 seconds, then streams the
/// rest in small chunks with short delays for a natural display cadence.
/// Returns false if the client disconnected.
async fn stream_text(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    text: &str,
    done: bool,
) -> bool {
    // If the text is already small (typical of real streaming tokens from SDK),
    // forward it directly without re-chunking.
    const CHUNK_THRESHOLD: usize = 80;

    if text.len() <= CHUNK_THRESHOLD || done {
        let msg = ExplorerServerMessage::Text {
            content: text.to_string(),
            done,
        };
        return sender
            .send(Message::Text(
                serialize_msg(&msg).unwrap_or_default().into(),
            ))
            .await
            .is_ok();
    }

    // Large text block (from native LlmPort path): simulate streaming.
    //
    // NOTE: Real streaming is not possible here because the LlmPort trait's
    // `stream_complete` method doesn't support tool calling. The
    // `complete_with_tools` method returns the full response at once.
    // The SDK path (explorer-agent.mjs) does real streaming via the
    // Claude Agent SDK.
    //
    // Strategy: send the first clause/sentence immediately (no delay) so the
    // user sees content within milliseconds of the LLM response arriving,
    // then drip the rest in small token-sized chunks (~60 chars) with minimal
    // delays to approximate real streaming cadence.

    // Find the end of the first clause (comma, period, newline, etc.)
    // or fall back to ~60 chars at a word boundary. Using a shorter initial
    // chunk gets content on screen faster.
    let first_chunk_end = {
        let search_limit = text.len().min(120);
        let mut end = None;
        for (i, ch) in text[..search_limit].char_indices() {
            if i > 10 && (ch == '.' || ch == '\n' || ch == '?' || ch == '!' || ch == ',') {
                end = Some(i + ch.len_utf8());
                break;
            }
        }
        end.unwrap_or_else(|| {
            // No clause boundary found — break at word boundary near 60 chars
            let target = text.len().min(60);
            let bytes = text.as_bytes();
            let mut pos = target;
            while pos > 0 && bytes[pos - 1] != b' ' && bytes[pos - 1] != b'\n' {
                pos -= 1;
            }
            if pos == 0 {
                target
            } else {
                pos
            }
        })
    };

    // Send the first chunk immediately (no delay)
    let first_chunk = &text[..first_chunk_end];
    let msg = ExplorerServerMessage::Text {
        content: first_chunk.to_string(),
        done: false,
    };
    if sender
        .send(Message::Text(
            serialize_msg(&msg).unwrap_or_default().into(),
        ))
        .await
        .is_err()
    {
        return false;
    }

    // Stream the rest in small token-sized chunks for natural display.
    // 60 chars ≈ 10-15 tokens, 8ms delay ≈ realistic inter-token timing.
    const TARGET_CHUNK: usize = 60;
    const CHUNK_DELAY_MS: u64 = 8;
    let mut start = first_chunk_end;
    let bytes = text.as_bytes();
    while start < bytes.len() {
        tokio::time::sleep(std::time::Duration::from_millis(CHUNK_DELAY_MS)).await;

        let end = (start + TARGET_CHUNK).min(bytes.len());
        let chunk_end = if end >= bytes.len() {
            bytes.len()
        } else {
            // Break at word boundary to avoid splitting mid-word
            let mut pos = end;
            while pos > start && bytes[pos] != b' ' && bytes[pos] != b'\n' {
                pos -= 1;
            }
            if pos == start {
                end
            } else {
                pos + 1
            }
        };

        let chunk = &text[start..chunk_end];
        let is_last = chunk_end >= bytes.len();
        let msg = ExplorerServerMessage::Text {
            content: chunk.to_string(),
            done: is_last && done,
        };
        if sender
            .send(Message::Text(
                serialize_msg(&msg).unwrap_or_default().into(),
            ))
            .await
            .is_err()
        {
            return false;
        }
        start = chunk_end;
    }
    true
}

// ── Tool definitions for the explorer agent ──────────────────────────────────

fn explorer_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "graph_summary".to_string(),
            description: "Get a condensed summary of the repo's knowledge graph: node/edge counts, top types by fields, top functions by calls, modules, test coverage, spec coverage (governed/unspecced), and risk indicators (high-complexity untested code, unspecced hot paths).".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        ToolDefinition {
            name: "graph_query_dryrun".to_string(),
            description: "Dry-run a view query against the knowledge graph. Returns matched node count, names, resolved groups/callouts/narrative, and warnings. Use this to validate queries before sending to the frontend.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "object",
                        "description": "View query JSON (scope, emphasis, groups, callouts, narrative, annotation, edges, zoom)"
                    }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: "graph_nodes".to_string(),
            description: "Query graph nodes by ID, name pattern, or node type. Returns up to 50 nodes with details (id, name, qualified_name, node_type, file_path, etc.).".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "node_id": { "type": "string", "description": "Look up a specific node by its ID" },
                    "name_pattern": { "type": "string", "description": "Substring match on name or qualified_name (case-insensitive)" },
                    "node_type": { "type": "string", "description": "Filter by: package, module, type, interface, function, endpoint, component, table, constant, field, spec" }
                }
            }),
        },
        ToolDefinition {
            name: "graph_edges".to_string(),
            description: "Query graph edges by source/target node ID or edge type. Returns up to 100 edges.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "node_id": { "type": "string", "description": "Find all edges connected to this node" },
                    "edge_type": { "type": "string", "description": "Filter by: contains, implements, depends_on, calls, field_of, returns, routes_to, governed_by, renders, persists_to, produced_by" },
                    "source_id": { "type": "string", "description": "Filter by source node" },
                    "target_id": { "type": "string", "description": "Filter by target node" }
                }
            }),
        },
        ToolDefinition {
            name: "node_provenance".to_string(),
            description: "Get provenance (creation/modification history) for specific nodes. Shows who created or modified the node, when, and in which commit.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "node_id": { "type": "string", "description": "Node ID to get provenance for" },
                    "name_pattern": { "type": "string", "description": "Find nodes by name and return their provenance" }
                }
            }),
        },
        ToolDefinition {
            name: "search".to_string(),
            description: "Full-text search across the knowledge graph. Searches node names, qualified names, doc comments, file paths, and spec paths.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search term (case-insensitive substring match)" },
                    "limit": { "type": "integer", "description": "Max results to return (default 30)" }
                },
                "required": ["query"]
            }),
        },
    ]
}

// ── Claude Agent SDK subprocess runner ──────────────────────────────────────

/// Run the explorer agent via the Claude Agent SDK (Node.js subprocess).
///
/// Spawns `scripts/explorer-agent.mjs`, pipes the request as JSON to stdin,
/// reads JSON lines from stdout, and forwards text/view_query/status messages
/// to the WebSocket client.
///
/// SDK is the default path when the script exists. Opt-out via `GYRE_EXPLORER_SDK=0`.
/// Falls back to native LlmPort when the SDK script doesn't exist or is explicitly disabled.
async fn run_explorer_agent_sdk(
    state: &AppState,
    repo_id: &str,
    user_question: &str,
    canvas_state: &gyre_common::view_query::CanvasState,
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    conversation_history: &mut Vec<ConversationMessage>,
    workspace_id: &Id,
    ping_interval: &mut tokio::time::Interval,
    last_pong: Arc<std::sync::Mutex<std::time::Instant>>,
    pong_timeout: std::time::Duration,
) -> anyhow::Result<()> {
    let server_url = format!(
        "http://localhost:{}",
        std::env::var("GYRE_PORT").unwrap_or_else(|_| "3000".into())
    );
    // SECURITY: The SDK subprocess uses the server's master auth_token (not the
    // authenticated user's token) because there is no per-user token to forward.
    // Access to this code path is gated to admin-only users in run_explorer_agent()
    // to prevent privilege escalation (a read-only member would otherwise get
    // server-level graph access via the subprocess).
    let token = &state.auth_token;
    let system_prompt = build_system_prompt();
    // Use workspace-scoped model resolution for consistent model selection
    let (model, _) =
        crate::llm_helpers::resolve_llm_model(state, workspace_id, "explorer-agent").await;

    // Serialize conversation history for the SDK, preserving tool call context
    let history_json: Vec<serde_json::Value> = conversation_history
        .iter()
        .map(|m| {
            let text = match &m.content {
                ConversationContent::Text(t) => t.clone(),
                ConversationContent::Blocks(blocks) => {
                    let mut parts = Vec::new();
                    for b in blocks {
                        match b {
                            ContentBlock::Text { text } => parts.push(text.clone()),
                            ContentBlock::ToolUse { name, input, .. } => {
                                parts.push(format!(
                                    "[Called tool '{}' with: {}]",
                                    name,
                                    serde_json::to_string(input).unwrap_or_default()
                                ));
                            }
                            ContentBlock::ToolResult { content, .. } => {
                                // Truncate large tool results to keep context manageable
                                let truncated: String = content.chars().take(800).collect();
                                parts.push(format!("[Tool result: {}]", truncated));
                            }
                        }
                    }
                    parts.join("\n")
                }
            };
            json!({ "role": m.role, "content": text })
        })
        .collect();

    let input = json!({
        "question": user_question,
        "canvas_state": canvas_state,
        "repo_id": repo_id,
        "server_url": server_url,
        "token": token,
        "model": model,
        "system_prompt": system_prompt,
        "history": history_json,
    });

    let sdk_script = std::env::var("GYRE_EXPLORER_SDK_PATH")
        .unwrap_or_else(|_| "scripts/explorer-agent.mjs".to_string());
    // Clear environment and only pass needed variables to prevent
    // leaking DATABASE_URL, API keys, etc. to the subprocess.
    let mut child_cmd = tokio::process::Command::new("node");
    child_cmd
        .arg(&sdk_script)
        .env_clear()
        .env("GYRE_API_URL", &server_url)
        .env("GYRE_API_TOKEN", token)
        .env("GYRE_LLM_MODEL", &model)
        .env("GYRE_REPO_ID", repo_id)
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", std::env::var("HOME").unwrap_or_default())
        .env("NODE_PATH", std::env::var("NODE_PATH").unwrap_or_default())
        // Forward Vertex AI credentials if present
        .envs(std::env::vars().filter(|(k, _)| {
            k.starts_with("GOOGLE_")
                || k.starts_with("GCLOUD_")
                || k.starts_with("CLOUDSDK_")
                || k == "VERTEX_PROJECT"
                || k == "VERTEX_LOCATION"
                || k.starts_with("GYRE_VERTEX_")
                || k.starts_with("ANTHROPIC_")
        }))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        // Create a new process group so we can kill the entire tree on timeout
        .process_group(0);
    let mut child = child_cmd.spawn()?;

    // Helper: kill the process group (child + all descendants) on Unix.
    // Falls back to child.kill() on other platforms.
    #[allow(unused_variables)]
    let kill_tree = |child: &tokio::process::Child| {
        #[cfg(unix)]
        if let Some(pid) = child.id() {
            // Signal the entire process group (negative PID).
            // process_group(0) above creates a new group with the child as leader.
            use std::process::Command;
            let _ = Command::new("kill")
                .args(["-TERM", "--", &format!("-{}", pid)])
                .status();
        }
    };

    // Write input to stdin
    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        stdin.write_all(input.to_string().as_bytes()).await?;
        drop(stdin);
    }

    // Read stdout line by line and forward to WebSocket, with a timeout to
    // prevent the session from being blocked forever if Node.js hangs.
    const SDK_TIMEOUT_SECS: u64 = 60;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(SDK_TIMEOUT_SECS);

    // Accumulate assistant response text for conversation history
    let mut accumulated_text = String::new();

    if let Some(stdout) = child.stdout.take() {
        use tokio::io::{AsyncBufReadExt, BufReader};
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        loop {
            // Send keepalive ping if the interval has elapsed (prevents
            // proxy/load-balancer from closing the WebSocket during long
            // SDK subprocess runs).
            if maybe_send_ping(sender, ping_interval, &last_pong, pong_timeout)
                .await
                .is_err()
            {
                kill_tree(&child);
                let _ = child.kill().await;
                anyhow::bail!("WebSocket connection lost during SDK agent run");
            }
            let line_result = tokio::time::timeout_at(deadline, lines.next_line()).await;
            match line_result {
                Err(_) => {
                    // Timeout: kill the entire process tree and bail.
                    kill_tree(&child);
                    let _ = child.kill().await;
                    warn!("Explorer SDK process timed out after {SDK_TIMEOUT_SECS}s");
                    anyhow::bail!(
                        "Explorer SDK process timed out after {SDK_TIMEOUT_SECS} seconds"
                    );
                }
                Ok(Err(e)) => {
                    warn!("Explorer SDK stdout read error: {e}");
                    break;
                }
                Ok(Ok(None)) => break, // EOF
                Ok(Ok(Some(line))) => {
                    if let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) {
                        let msg_type = msg.get("type").and_then(|t| t.as_str()).unwrap_or("");
                        match msg_type {
                            "text" => {
                                let content =
                                    msg.get("content").and_then(|c| c.as_str()).unwrap_or("");
                                let done =
                                    msg.get("done").and_then(|d| d.as_bool()).unwrap_or(false);
                                // Cap accumulated text to prevent memory exhaustion
                                if accumulated_text.len() + content.len()
                                    <= MAX_SDK_ACCUMULATED_TEXT
                                {
                                    accumulated_text.push_str(content);
                                } else if accumulated_text.len() < MAX_SDK_ACCUMULATED_TEXT {
                                    let remaining =
                                        MAX_SDK_ACCUMULATED_TEXT - accumulated_text.len();
                                    accumulated_text.push_str(&content[..remaining]);
                                    warn!("Explorer SDK accumulated text exceeded {MAX_SDK_ACCUMULATED_TEXT} bytes, truncating");
                                }
                                if !stream_text(sender, content, done).await {
                                    break; // Client disconnected
                                }
                            }
                            "view_query" => {
                                if let Some(query) = msg.get("query") {
                                    let explanation = msg
                                        .get("explanation")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string());
                                    let view_msg = ExplorerServerMessage::ViewQuery {
                                        query: query.clone(),
                                        explanation,
                                    };
                                    if sender
                                        .send(Message::Text(
                                            serialize_msg(&view_msg).unwrap_or_default().into(),
                                        ))
                                        .await
                                        .is_err()
                                    {
                                        break; // Client disconnected
                                    }
                                }
                            }
                            "status" => {
                                if let Some(status) = msg.get("status").and_then(|s| s.as_str()) {
                                    if !send_status(sender, status).await {
                                        break; // Client disconnected
                                    }
                                }
                            }
                            "error" => {
                                let err_msg = msg
                                    .get("message")
                                    .and_then(|m| m.as_str())
                                    .unwrap_or("Explorer SDK error");
                                let err = ExplorerServerMessage::Error {
                                    message: err_msg.to_string(),
                                };
                                if sender
                                    .send(Message::Text(
                                        serialize_msg(&err).unwrap_or_default().into(),
                                    ))
                                    .await
                                    .is_err()
                                {
                                    break; // Client disconnected
                                }
                            }
                            "done" => break,
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    // Wait for the child with the same deadline.
    match tokio::time::timeout_at(deadline, child.wait()).await {
        Err(_) => {
            kill_tree(&child);
            let _ = child.kill().await;
            warn!("Explorer SDK process timed out waiting for exit");
            anyhow::bail!("Explorer SDK process timed out after {SDK_TIMEOUT_SECS} seconds");
        }
        Ok(Err(e)) => {
            anyhow::bail!("Explorer SDK process wait error: {e}");
        }
        Ok(Ok(status)) => {
            if !status.success() {
                anyhow::bail!("Explorer SDK process exited with status: {}", status);
            }
        }
    }

    // Add both user and assistant messages to conversation history
    // so subsequent SDK messages have full context.
    conversation_history.push(ConversationMessage {
        role: "user".to_string(),
        content: ConversationContent::Text(user_question.to_string()),
    });
    // Use the actual accumulated assistant text (not a placeholder)
    // so multi-turn conversations have real context.
    let assistant_text = if accumulated_text.is_empty() {
        "[No response from SDK subprocess]".to_string()
    } else {
        accumulated_text
    };
    conversation_history.push(ConversationMessage {
        role: "assistant".to_string(),
        content: ConversationContent::Text(assistant_text),
    });

    Ok(())
}

/// Fallback when no LLM is configured — generate template-based view queries
/// from simple keyword matching. Better than a dead-end "not configured" message.
async fn handle_no_llm_fallback(
    user_question: &str,
    canvas_state: &gyre_common::view_query::CanvasState,
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    cached_nodes: &mut Option<Vec<gyre_common::graph::GraphNode>>,
    cached_edges: &mut Option<Vec<gyre_common::graph::GraphEdge>>,
    repo_id: &str,
    state: &AppState,
) -> anyhow::Result<()> {
    // Ensure graph is loaded
    if cached_nodes.is_none() {
        let rid = Id::new(repo_id);
        if let Ok(n) = state.graph_store.list_nodes(&rid, None).await {
            if let Ok(e) = state.graph_store.list_edges(&rid, None).await {
                *cached_nodes = Some(n);
                *cached_edges = Some(e);
            }
        }
    }

    let q = user_question.to_lowercase();
    let selected_name = canvas_state
        .selected_node
        .as_ref()
        .map(|n| n.name.clone())
        .unwrap_or_default();

    // Pattern matching for common questions
    if q.contains("blast") || q.contains("break") || q.contains("impact") {
        let target = if !selected_name.is_empty() {
            selected_name.clone()
        } else {
            // Extract candidate node names from the question by filtering out
            // common English words, then validate against actual graph nodes.
            const STOP_WORDS: &[&str] = &[
                "what", "would", "where", "when", "which", "while", "with", "will", "could",
                "should", "about", "after", "before", "between", "does", "from", "have", "here",
                "just", "like", "make", "many", "more", "most", "much", "must", "need", "only",
                "other", "over", "some", "such", "take", "than", "that", "their", "them", "then",
                "there", "these", "they", "this", "those", "through", "under", "very", "were",
                "your", "show", "tell", "give", "find", "look", "know", "think", "want", "also",
                "been", "being", "came", "come", "each", "even", "good", "into", "long", "back",
                "going", "break", "blast", "impact", "radius", "changes", "affects", "affected",
            ];
            let candidates: Vec<&str> = user_question
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .filter(|w| w.len() > 2)
                .filter(|w| !STOP_WORDS.iter().any(|sw| sw.eq_ignore_ascii_case(w)))
                .collect();
            let mut matched = String::new();
            if let Some(ref nodes) = cached_nodes {
                let active: Vec<_> = nodes.iter().filter(|n| n.deleted_at.is_none()).collect();
                for cand in &candidates {
                    if let Some(n) = active.iter().find(|n| n.name.eq_ignore_ascii_case(cand)) {
                        matched = n.name.clone();
                        break;
                    }
                }
                if matched.is_empty() {
                    let cand_lower: Vec<String> =
                        candidates.iter().map(|c| c.to_lowercase()).collect();
                    for kw in &cand_lower {
                        if let Some(n) = active.iter().find(|n| {
                            n.name.to_lowercase().contains(kw.as_str())
                                || n.qualified_name.to_lowercase().contains(kw.as_str())
                        }) {
                            matched = n.name.clone();
                            break;
                        }
                    }
                }
            }
            if matched.is_empty() {
                candidates
                    .first()
                    .map_or("$selected".to_string(), |s| s.to_string())
            } else {
                matched
            }
        };
        stream_text(sender, &format!("Showing blast radius for **{}**. Click any node to see its impact.\n\n*Note: LLM is not configured — using template query. Set GYRE_VERTEX_PROJECT for conversational exploration.*", target), true).await;
        let query = json!({
            "scope": { "type": "focus", "node": target, "edges": ["calls", "implements", "field_of"], "direction": "incoming", "depth": 10 },
            "emphasis": { "tiered_colors": ["#ef4444", "#f97316", "#eab308", "#94a3b8"], "dim_unmatched": 0.12 },
            "zoom": "fit",
            "annotation": { "title": format!("Blast radius: {}", target) }
        });
        let msg = ExplorerServerMessage::ViewQuery {
            query,
            explanation: None,
        };
        let _ = sender
            .send(Message::Text(
                serialize_msg(&msg).unwrap_or_default().into(),
            ))
            .await;
    } else if q.contains("test")
        && (q.contains("gap") || q.contains("coverage") || q.contains("untested"))
    {
        stream_text(sender, "Showing test coverage gaps — functions not reachable from any test.\n\n*Note: LLM is not configured — using template query.*", true).await;
        let query = json!({
            "scope": { "type": "test_gaps" },
            "emphasis": { "highlight": { "matched": { "color": "#ef4444", "label": "Untested" } }, "dim_unmatched": 0.3 },
            "zoom": "fit",
            "annotation": { "title": "Test coverage gaps" }
        });
        let msg = ExplorerServerMessage::ViewQuery {
            query,
            explanation: None,
        };
        let _ = sender
            .send(Message::Text(
                serialize_msg(&msg).unwrap_or_default().into(),
            ))
            .await;
    } else if q.contains("hot") || q.contains("most called") || q.contains("critical") {
        stream_text(sender, "Showing hot paths — most-called functions.\n\n*Note: LLM is not configured — using template query.*", true).await;
        let query = json!({
            "scope": { "type": "all" },
            "emphasis": { "heat": { "metric": "incoming_calls", "palette": "blue-red" } },
            "zoom": "fit",
            "annotation": { "title": "Hot paths" }
        });
        let msg = ExplorerServerMessage::ViewQuery {
            query,
            explanation: None,
        };
        let _ = sender
            .send(Message::Text(
                serialize_msg(&msg).unwrap_or_default().into(),
            ))
            .await;
    } else if q.contains("complex") {
        stream_text(
            sender,
            "Showing complexity map.\n\n*Note: LLM is not configured — using template query.*",
            true,
        )
        .await;
        let query = json!({
            "scope": { "type": "all" },
            "emphasis": { "heat": { "metric": "complexity", "palette": "blue-red" } },
            "zoom": "fit",
            "annotation": { "title": "Complexity map" }
        });
        let msg = ExplorerServerMessage::ViewQuery {
            query,
            explanation: None,
        };
        let _ = sender
            .send(Message::Text(
                serialize_msg(&msg).unwrap_or_default().into(),
            ))
            .await;
    } else {
        // Default: try a search and show results as a focus query if possible.
        // Extract meaningful keywords from the question, stripping common English
        // words and short tokens, then match against actual node names in the graph.
        const FALLBACK_STOP_WORDS: &[&str] = &[
            "a", "an", "the", "is", "are", "was", "were", "be", "been", "being", "have", "has",
            "had", "do", "does", "did", "will", "would", "could", "should", "shall", "may",
            "might", "can", "must", "need", "dare", "about", "above", "after", "again", "all",
            "also", "and", "any", "at", "back", "because", "before", "between", "both", "but",
            "by", "came", "come", "day", "each", "even", "find", "for", "from", "get", "give",
            "go", "going", "good", "got", "great", "her", "here", "him", "his", "how", "if", "in",
            "into", "it", "its", "just", "know", "let", "like", "long", "look", "make", "many",
            "me", "more", "most", "much", "my", "new", "no", "not", "now", "of", "on", "one",
            "only", "or", "other", "our", "out", "over", "own", "people", "run", "say", "see",
            "she", "show", "so", "some", "still", "such", "take", "tell", "than", "that", "their",
            "them", "then", "there", "these", "they", "thing", "think", "this", "those", "through",
            "to", "too", "under", "up", "us", "use", "very", "want", "way", "we", "well", "what",
            "when", "where", "which", "while", "who", "why", "with", "work", "you", "your",
        ];
        let mut found = String::new();
        if let Some(ref nodes) = cached_nodes {
            let active: Vec<_> = nodes.iter().filter(|n| n.deleted_at.is_none()).collect();
            // 1. Try exact substring match of full query against node names
            for n in &active {
                if n.name.to_lowercase().contains(&q)
                    || n.qualified_name.to_lowercase().contains(&q)
                {
                    found = n.name.clone();
                    break;
                }
            }
            // 2. If no exact match, extract keywords and try matching each
            if found.is_empty() {
                let keywords: Vec<String> = user_question
                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                    .filter(|w| w.len() > 2)
                    .filter(|w| {
                        !FALLBACK_STOP_WORDS
                            .iter()
                            .any(|sw| sw.eq_ignore_ascii_case(w))
                    })
                    .map(|w| w.to_lowercase())
                    .collect();
                for kw in &keywords {
                    if let Some(n) = active.iter().find(|n| n.name.to_lowercase() == *kw) {
                        found = n.name.clone();
                        break;
                    }
                }
                if found.is_empty() {
                    for kw in &keywords {
                        if let Some(n) = active.iter().find(|n| {
                            n.name.to_lowercase().contains(kw.as_str())
                                || n.qualified_name.to_lowercase().contains(kw.as_str())
                        }) {
                            found = n.name.clone();
                            break;
                        }
                    }
                }
            }
        }
        if !found.is_empty() {
            stream_text(sender, &format!("Found **{}** — showing its connections.\n\n*Note: LLM is not configured. Set GYRE_VERTEX_PROJECT for conversational exploration.*", found), true).await;
            let query = json!({
                "scope": { "type": "focus", "node": found, "edges": ["calls", "implements", "contains"], "direction": "both", "depth": 3 },
                "emphasis": { "dim_unmatched": 0.15 },
                "zoom": "fit",
                "annotation": { "title": format!("Connections: {}", found) }
            });
            let msg = ExplorerServerMessage::ViewQuery {
                query,
                explanation: None,
            };
            let _ = sender
                .send(Message::Text(
                    serialize_msg(&msg).unwrap_or_default().into(),
                ))
                .await;
        } else {
            // No keyword or node name match — surface interesting graph patterns
            // instead of a dead-end message.
            let mut suggestions = Vec::new();
            if let Some(ref nodes) = cached_nodes {
                // Find top 5 most complex functions lacking test coverage
                let mut candidates: Vec<_> = nodes
                    .iter()
                    .filter(|n| n.deleted_at.is_none())
                    .filter(|n| {
                        matches!(
                            n.node_type,
                            gyre_common::NodeType::Function | gyre_common::NodeType::Method
                        )
                    })
                    .filter(|n| n.complexity.unwrap_or(0) > 5)
                    .filter(|n| n.test_coverage.map_or(true, |c| c < 0.5))
                    .collect();
                candidates
                    .sort_by(|a, b| b.complexity.unwrap_or(0).cmp(&a.complexity.unwrap_or(0)));
                for n in candidates.iter().take(5) {
                    suggestions.push(format!(
                        "- **{}** (complexity: {}, test coverage: {})",
                        n.name,
                        n.complexity.unwrap_or(0),
                        n.test_coverage
                            .map_or("none".to_string(), |c| format!("{:.0}%", c * 100.0))
                    ));
                }
            }
            if suggestions.is_empty() {
                stream_text(
                    sender,
                    "LLM is not configured — conversational exploration requires GYRE_VERTEX_PROJECT and GYRE_VERTEX_LOCATION.\n\nYou can still use saved views, the filter panel, and search (/) to explore the codebase. Try asking about \"blast radius\", \"test gaps\", \"hot paths\", or \"complexity\".",
                    true,
                ).await;
            } else {
                let msg = format!(
                    "I couldn't find a specific match, but here are high-complexity functions that may need attention (low or no test coverage):\n\n{}\n\nTry clicking one of these names, or ask about \"blast radius\", \"test gaps\", \"hot paths\", or \"complexity\".\n\n*Note: LLM is not configured — set GYRE_VERTEX_PROJECT for conversational exploration.*",
                    suggestions.join("\n")
                );
                stream_text(sender, &msg, true).await;
                // Show a complexity heat map so the user gets something visual
                let query = json!({
                    "scope": { "type": "filter", "node_types": ["function", "method"], "computed": "$where(complexity, '>', 5)" },
                    "emphasis": { "heat": { "metric": "complexity", "palette": "blue-red" } },
                    "zoom": "fit",
                    "annotation": { "title": "High-complexity functions" }
                });
                let view_msg = ExplorerServerMessage::ViewQuery {
                    query,
                    explanation: None,
                };
                let _ = sender
                    .send(Message::Text(
                        serialize_msg(&view_msg).unwrap_or_default().into(),
                    ))
                    .await;
            }
        }
    }
    Ok(())
}

// ── Explorer agent loop ──────────────────────────────────────────────────────

async fn run_explorer_agent(
    state: &AppState,
    repo_id: &str,
    user_question: &str,
    canvas_state: &gyre_common::view_query::CanvasState,
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    cancel_rx: &mut tokio::sync::watch::Receiver<bool>,
    conversation_history: &mut Vec<ConversationMessage>,
    cached_nodes: &mut Option<Vec<gyre_common::graph::GraphNode>>,
    cached_edges: &mut Option<Vec<gyre_common::graph::GraphEdge>>,
    cached_graph_summary: &mut Option<String>,
    auth: &AuthenticatedAgent,
    workspace_id: &Id,
    ping_interval: &mut tokio::time::Interval,
    last_pong: Arc<std::sync::Mutex<std::time::Instant>>,
    pong_timeout: std::time::Duration,
) -> anyhow::Result<()> {
    // Check if SDK-based explorer agent should be used.
    // Per spec: Claude Agent SDK is the default path. The SDK provides the
    // spec-mandated `query()` interface with MCP tools. The native LlmPort
    // is the fallback when the SDK script is not available or GYRE_EXPLORER_SDK=0.
    // SDK script path: MUST use absolute path via GYRE_EXPLORER_SDK_PATH env var.
    // Relative paths are resolved against the server binary's directory (not CWD)
    // to prevent hijacking via writable agent worktrees.
    let sdk_script_path = std::env::var("GYRE_EXPLORER_SDK_PATH").unwrap_or_else(|_| {
        // Resolve relative to the executable's directory, not CWD
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_default();
        exe_dir
            .join("scripts/explorer-agent.mjs")
            .to_string_lossy()
            .to_string()
    });
    // Validate SDK path: must be an absolute path within a known safe directory
    // (the executable's directory tree or /usr/). This prevents arbitrary code
    // execution via a compromised GYRE_EXPLORER_SDK_PATH env var.
    let sdk_path = std::path::Path::new(&sdk_script_path);
    // Validate SDK path: MUST exist and canonicalize to a known safe directory.
    // Reject non-existent paths to prevent TOCTOU attacks (file could be
    // replaced between validation and execution).
    let sdk_path_valid = if let Ok(canonical) = sdk_path.canonicalize() {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()));
        let in_exe_dir = exe_dir
            .as_ref()
            .map(|d| canonical.starts_with(d))
            .unwrap_or(false);
        let in_usr = canonical.starts_with("/usr/");
        let in_nix = canonical.starts_with("/nix/");
        in_exe_dir || in_usr || in_nix
    } else {
        // Path doesn't exist — reject rather than allow unchecked execution
        false
    };
    // Per spec: SDK is the default path when available. Opt-out via GYRE_EXPLORER_SDK=0.
    // Falls back to native LlmPort when SDK script doesn't exist or is explicitly disabled.
    let sdk_disabled = std::env::var("GYRE_EXPLORER_SDK").unwrap_or_default() == "0";
    let use_sdk = sdk_path_valid && !sdk_disabled;
    if use_sdk {
        // SECURITY: SDK path uses the server's master auth_token, not the user's.
        // Restrict to admin-only to prevent RBAC bypass (a read-only member would
        // otherwise get server-level graph access through the subprocess).
        let is_admin = auth
            .roles
            .iter()
            .any(|r| matches!(r, gyre_domain::user::UserRole::Admin));
        if !is_admin {
            warn!(
                user = %auth.agent_id,
                "Non-admin attempted SDK explorer path — falling through to native LLM"
            );
        } else {
            send_status_with_path(sender, "thinking", Some("sdk")).await;
            return run_explorer_agent_sdk(
                state,
                repo_id,
                user_question,
                canvas_state,
                sender,
                conversation_history,
                workspace_id,
                ping_interval,
                last_pong.clone(),
                pong_timeout,
            )
            .await;
        }
    }

    // Send agent path indicator for native LLM path
    send_status_with_path(sender, "thinking", Some("native")).await;

    let llm = match &state.llm {
        Some(llm) => llm.clone(),
        None => {
            // No LLM configured — provide a useful local-only fallback.
            // Generate template-based view queries from the user's question.
            return handle_no_llm_fallback(
                user_question,
                canvas_state,
                sender,
                cached_nodes,
                cached_edges,
                repo_id,
                state,
            )
            .await;
        }
    };

    let (model, _) =
        crate::llm_helpers::resolve_llm_model(state, workspace_id, "explorer-agent").await;
    let llm_port = llm.for_model(&model);

    // Load graph data (cached across messages in the session).
    let rid = Id::new(repo_id);
    if cached_nodes.is_none() {
        match state.graph_store.list_nodes(&rid, None).await {
            Ok(n) => {
                let edge_result = state.graph_store.list_edges(&rid, None).await;
                match edge_result {
                    Ok(e) => {
                        // Enforce cache size limit to prevent memory exhaustion
                        if n.len() + e.len() > MAX_GRAPH_CACHE_ENTRIES {
                            let original_nodes = n.len();
                            let original_edges = e.len();
                            warn!(
                                "Graph too large for session cache ({} nodes + {} edges > {}), truncating — keeping most important nodes (by connectivity + recency)",
                                original_nodes, original_edges, MAX_GRAPH_CACHE_ENTRIES
                            );
                            // Sort by importance: highly-connected foundational nodes
                            // should be kept even if they haven't been modified recently.
                            // Build incoming-edge-count map, then sort by
                            // incoming_count DESC, last_modified_at DESC.
                            let mut incoming_count: std::collections::HashMap<String, usize> =
                                std::collections::HashMap::new();
                            for edge in &e {
                                *incoming_count
                                    .entry(edge.target_id.to_string())
                                    .or_insert(0) += 1;
                            }
                            let mut sorted_nodes = n;
                            // Sort by importance: connectivity (primary) + architectural significance.
                            // Boost foundational types (Trait/Interface/Spec/Package/Module)
                            // since the spec says "default zoom shows boundaries and interfaces."
                            sorted_nodes.sort_by(|a, b| {
                                let a_inc =
                                    incoming_count.get(&a.id.to_string()).copied().unwrap_or(0);
                                let b_inc =
                                    incoming_count.get(&b.id.to_string()).copied().unwrap_or(0);
                                let arch_boost = |n: &gyre_common::graph::GraphNode| -> usize {
                                    match n.node_type {
                                        gyre_common::NodeType::Trait
                                        | gyre_common::NodeType::Interface
                                        | gyre_common::NodeType::Package
                                        | gyre_common::NodeType::Module
                                        | gyre_common::NodeType::Spec => 1000,
                                        gyre_common::NodeType::Endpoint
                                        | gyre_common::NodeType::Type
                                        | gyre_common::NodeType::Class
                                        | gyre_common::NodeType::Enum => 100,
                                        _ => 0,
                                    }
                                };
                                (b_inc + arch_boost(b))
                                    .cmp(&(a_inc + arch_boost(a)))
                                    .then_with(|| b.last_modified_at.cmp(&a.last_modified_at))
                            });
                            let kept_count = MAX_GRAPH_CACHE_ENTRIES / 2;
                            let kept_node_ids: std::collections::HashSet<String> = sorted_nodes
                                .iter()
                                .take(kept_count)
                                .map(|n| n.id.to_string())
                                .collect();
                            // Keep edges where BOTH endpoints are retained (avoids dangling refs)
                            let kept_edges: Vec<_> = e
                                .into_iter()
                                .filter(|e| {
                                    kept_node_ids.contains(&e.source_id.to_string())
                                        && kept_node_ids.contains(&e.target_id.to_string())
                                })
                                .collect();
                            *cached_nodes =
                                Some(sorted_nodes.into_iter().take(kept_count).collect());
                            *cached_edges = Some(kept_edges);
                            *cached_graph_summary = None;
                            // Warn the user via WebSocket (Vision Principle 2: "Right context, not more context")
                            // Explicitly note that computed metrics are affected — silent truncation
                            // makes test coverage gaps, blast radius, and risk scores unsound.
                            let warning_msg = format!(
                                "**Note:** This repository's graph ({} nodes, {} edges) exceeds \
                                 the session limit. Showing the {} most important nodes (by \
                                 connectivity and recency). **Computed metrics (test coverage, \
                                 blast radius, risk scores) are approximate** because they operate \
                                 on truncated data. Less-connected or older nodes may be missing.",
                                original_nodes, original_edges, kept_count
                            );
                            stream_text(sender, &warning_msg, false).await;
                        } else {
                            *cached_nodes = Some(n);
                            *cached_edges = Some(e);
                            *cached_graph_summary = None;
                        }
                    }
                    Err(e) => {
                        warn!("Failed to load graph edges: {e}");
                        stream_text(
                            sender,
                            "Failed to load graph data. The graph store may be unavailable.",
                            true,
                        )
                        .await;
                        return Ok(());
                    }
                }
            }
            Err(e) => {
                warn!("Failed to load graph nodes: {e}");
                stream_text(
                    sender,
                    "Failed to load graph data. The graph store may be unavailable.",
                    true,
                )
                .await;
                return Ok(());
            }
        }
    }
    let nodes = cached_nodes.as_ref().unwrap();
    let edges = cached_edges.as_ref().unwrap();

    // Build system prompt with repo context so LLM doesn't need an extra graph_summary call
    let mut system_prompt = build_system_prompt();
    {
        let mut type_counts = std::collections::HashMap::new();
        let mut edge_counts = std::collections::HashMap::new();
        for n in nodes.iter() {
            *type_counts
                .entry(
                    gyre_domain::view_query_resolver::node_type_str_pub(&n.node_type).to_string(),
                )
                .or_insert(0u32) += 1;
        }
        for e in edges.iter() {
            *edge_counts
                .entry(
                    gyre_domain::view_query_resolver::edge_type_str_pub(&e.edge_type).to_string(),
                )
                .or_insert(0u32) += 1;
        }
        let mut type_summary: Vec<_> = type_counts.iter().collect();
        type_summary.sort_by(|a, b| b.1.cmp(a.1));
        let type_str: Vec<_> = type_summary
            .iter()
            .map(|(k, v)| format!("{}: {}", k, v))
            .collect();
        let edge_str: Vec<_> = edge_counts
            .iter()
            .map(|(k, v)| format!("{}: {}", k, v))
            .collect();
        system_prompt.push_str(&format!(
            "\n\n## Current Repository Context\nRepo ID: {}\nGraph: {} nodes, {} edges\nNode types: {}\nEdge types: {}",
            repo_id,
            nodes.len(),
            edges.len(),
            type_str.join(", "),
            edge_str.join(", "),
        ));
    }
    let tools = explorer_tool_definitions();

    // Build user message with structured canvas context as JSON
    // (structured format lets the LLM parse interaction sequences semantically)
    let canvas_json = {
        let mut ctx = serde_json::Map::new();
        if let Some(ref sel) = canvas_state.selected_node {
            // Enrich selected node context with graph data for better LLM reasoning
            let mut node_ctx = json!({
                "name": sel.qualified_name.as_deref().unwrap_or(&sel.name),
                "type": sel.node_type,
                "id": sel.id,
            });
            // Look up full node details from the graph cache
            if let Some(ref graph_nodes) = cached_nodes {
                if let Some(full_node) = graph_nodes.iter().find(|n| n.id.to_string() == sel.id) {
                    if !full_node.file_path.is_empty() {
                        node_ctx["file_path"] = json!(full_node.file_path);
                    }
                    if full_node.line_start > 0 {
                        node_ctx["line_start"] = json!(full_node.line_start);
                    }
                    if let Some(ref sp) = full_node.spec_path {
                        node_ctx["spec_path"] = json!(sp);
                    }
                    if let Some(c) = full_node.complexity {
                        node_ctx["complexity"] = json!(c);
                    }
                    if let Some(tc) = full_node.test_coverage {
                        node_ctx["test_coverage"] = json!(tc);
                    }
                }
            }
            ctx.insert("selected_node".to_string(), node_ctx);
        }
        if !canvas_state.visible_tree_groups.is_empty() {
            ctx.insert(
                "visible_groups".to_string(),
                json!(canvas_state.visible_tree_groups),
            );
        }
        if let Some(ref lens) = canvas_state.active_lens {
            if !lens.is_empty() {
                ctx.insert("active_lens".to_string(), json!(lens));
            }
        }
        if !canvas_state.recent_interactions.is_empty() {
            let last_10: Vec<_> = canvas_state
                .recent_interactions
                .iter()
                .rev()
                .take(10)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .map(|i| {
                    let mut m = serde_json::Map::new();
                    m.insert("action".to_string(), json!(i.action));
                    if let Some(ref node) = i.node {
                        m.insert("node".to_string(), json!(node));
                    }
                    if let Some(ref detail) = i.detail {
                        m.insert("detail".to_string(), json!(detail));
                    }
                    serde_json::Value::Object(m)
                })
                .collect();
            ctx.insert("recent_interactions".to_string(), json!(last_10));
        }
        if let Some(ref aq) = canvas_state.active_query {
            ctx.insert("active_query".to_string(), aq.clone());
        }
        ctx
    };
    let canvas_summary = if canvas_json.is_empty() {
        String::new()
    } else {
        format!(
            "<canvas_state>\n{}\n</canvas_state>\n\n",
            serde_json::to_string_pretty(&serde_json::Value::Object(canvas_json))
                .unwrap_or_default()
        )
    };
    let user_content = format!("{canvas_summary}{user_question}");

    // Truncate conversation history to prevent unbounded growth.
    // Keep the most recent messages, preceded by a synthetic summary.
    // Ensure correct role alternation: the Claude API requires messages to
    // alternate user/assistant. We insert a synthetic "user" summary followed
    // by an "assistant" acknowledgment so the first real message (regardless
    // of its role) doesn't create a role collision.
    if conversation_history.len() > MAX_CONVERSATION_HISTORY {
        let keep_recent = MAX_CONVERSATION_HISTORY - 3; // leave room for 2 summary + 1 possible skip

        // Build a concise summary of dropped messages instead of silently discarding.
        let dropped_count = conversation_history.len() - keep_recent;
        let mut topics = Vec::new();
        let mut tool_names_used = Vec::new();
        for msg in conversation_history.iter().take(dropped_count) {
            let text = match &msg.content {
                ConversationContent::Text(t) => t.clone(),
                ConversationContent::Blocks(blocks) => {
                    let mut parts = Vec::new();
                    for b in blocks {
                        match b {
                            ContentBlock::Text { text } => parts.push(text.clone()),
                            ContentBlock::ToolUse { name, input, .. } => {
                                tool_names_used.push(name.clone());
                                // Keep graph_query_dryrun inputs intact — the query
                                // JSON is compact and essential for understanding
                                // what the LLM explored in earlier turns.
                                if name == "graph_query_dryrun" {
                                    if let Ok(json_str) = serde_json::to_string(input) {
                                        parts.push(format!("[tool call: {} {}]", name, json_str));
                                    }
                                }
                            }
                            ContentBlock::ToolResult { content, .. } => {
                                // Include a generous snippet of tool results to preserve
                                // grounding data (node counts, names, warnings) that the
                                // LLM needs for accurate claims in later turns.
                                let snippet: String = content.chars().take(800).collect();
                                parts.push(format!(
                                    "[tool result: {}{}]",
                                    snippet.replace('\n', " "),
                                    if content.len() > 800 { "..." } else { "" }
                                ));
                            }
                        }
                    }
                    parts.join(" ")
                }
            };
            // Extract the first complete sentence (up to 200 chars) of each
            // user message as a topic hint. A sentence boundary gives more
            // semantic context than a hard character cut.
            if msg.role == "user" && !text.is_empty() {
                let flat = text.replace('\n', " ");
                let limit = flat.len().min(200);
                let snippet = if let Some(pos) =
                    flat[..limit].find(|c: char| c == '.' || c == '?' || c == '!')
                {
                    // Include the punctuation character
                    flat[..=pos].to_string()
                } else {
                    // No sentence boundary found — take up to 200 chars at a
                    // word boundary to avoid cutting mid-word.
                    let chars: String = flat.chars().take(200).collect();
                    if chars.len() < flat.len() {
                        if let Some(last_space) = chars.rfind(' ') {
                            format!("{}...", &chars[..last_space])
                        } else {
                            format!("{}...", chars)
                        }
                    } else {
                        chars
                    }
                };
                topics.push(snippet);
            }
        }
        let topic_summary = if topics.is_empty() {
            "general exploration".to_string()
        } else {
            topics.join("; ")
        };
        let tool_summary = if tool_names_used.is_empty() {
            String::new()
        } else {
            tool_names_used.sort();
            tool_names_used.dedup();
            format!(" Tools used: {}", tool_names_used.join(", "))
        };

        let mut recent: Vec<ConversationMessage> =
            conversation_history.split_off(conversation_history.len() - keep_recent);
        conversation_history.clear();

        // Insert user→assistant pair with meaningful summary.
        conversation_history.push(ConversationMessage {
            role: "user".to_string(),
            content: ConversationContent::Text(format!(
                "[System: {} earlier messages were summarized. Topics discussed: {}.{}]",
                dropped_count, topic_summary, tool_summary
            )),
        });
        conversation_history.push(ConversationMessage {
            role: "assistant".to_string(),
            content: ConversationContent::Text(
                "Understood. I have context from the earlier discussion and will continue building on it.".to_string(),
            ),
        });

        // If the first recent message is "assistant", we already have an assistant
        // message above — skip it to avoid assistant→assistant.
        if recent.first().map(|m| m.role.as_str()) == Some("assistant") {
            recent.remove(0);
        }

        conversation_history.extend(recent);
    }

    // Add to persistent conversation history
    conversation_history.push(ConversationMessage {
        role: "user".to_string(),
        content: ConversationContent::Text(user_content),
    });

    let selected_node_id = canvas_state.selected_node.as_ref().map(|n| n.id.as_str());

    // Multi-turn agent loop with self-check.
    // Two independent budgets:
    //   - MAX_TOOL_TURNS (5): LLM calls for tool exploration
    //   - MAX_REFINEMENT_TURNS (3): dedicated view query self-check refinements
    // Total possible LLM calls = MAX_TOOL_TURNS + MAX_REFINEMENT_TURNS = 8.
    let mut refinement_count = 0;
    let mut tool_turn_count = 0;
    let mut view_query_sent = false;
    let max_total_turns = MAX_TOOL_TURNS + MAX_REFINEMENT_TURNS;
    for _turn in 0..max_total_turns {
        // Send keepalive ping if the interval has elapsed (prevents
        // proxy/load-balancer from closing the WebSocket during long agent runs).
        if maybe_send_ping(sender, ping_interval, &last_pong, pong_timeout)
            .await
            .is_err()
        {
            anyhow::bail!("WebSocket connection lost during agent run");
        }

        // Check for cancellation at the top of each turn
        if *cancel_rx.borrow() {
            info!("Explorer agent cancelled by user");
            stream_text(sender, "*Query cancelled.*", true).await;
            return Ok(());
        }

        // Send a status update so the user sees feedback during LLM inference
        // (which can take 3-15 seconds). Without this the UI appears frozen.
        let status_text = if tool_turn_count > 0 {
            "Analyzing..."
        } else {
            "Thinking..."
        };
        let status_msg = ExplorerServerMessage::Status {
            status: status_text.to_string(),
            agent_path: None,
            graph_data_age_secs: None,
        };
        let _ = sender
            .send(Message::Text(
                serialize_msg(&status_msg).unwrap_or_default().into(),
            ))
            .await;

        // Timeout on LLM calls to prevent blocking the session indefinitely.
        // If the client disconnects, subsequent send() calls will fail and break the loop.
        let llm_future =
            llm_port.complete_with_tools(&system_prompt, conversation_history, &tools, Some(8192));
        let response =
            match tokio::time::timeout(std::time::Duration::from_secs(60), llm_future).await {
                Ok(result) => result?,
                Err(_) => {
                    warn!("Explorer LLM call timed out after 60s");
                    stream_text(
                        sender,
                        "\n\n*(Response timed out. Please try a simpler question.)*",
                        true,
                    )
                    .await;
                    break;
                }
            };

        // If the LLM returned text, stream it to the client
        if !response.text.is_empty() {
            let has_tool_calls =
                !response.tool_calls.is_empty() && response.stop_reason == "tool_use";
            let is_final = !has_tool_calls;

            // Check for view_query blocks in the text
            let (clean_text, view_query) = parse_view_query_from_text(&response.text);

            // Stream the clean text (without view_query block)
            if !clean_text.is_empty() {
                if !stream_text(sender, &clean_text, is_final && view_query.is_none()).await {
                    break; // Client disconnected
                }
            }

            // If a view query was found, perform server-enforced self-check
            if let Some(query_json) = view_query {
                // Dry-run the query server-side, with validation first
                let dry_run_result = if let Ok(query) =
                    serde_json::from_value::<gyre_common::view_query::ViewQuery>(query_json.clone())
                {
                    // Validate query before dry-run to prevent malformed queries
                    let validation_errors = query.validate();
                    let validation_warnings = query.warnings();
                    if !validation_errors.is_empty() || !validation_warnings.is_empty() {
                        let mut dr = gyre_domain::view_query_resolver::dry_run(
                            &query,
                            &nodes,
                            &edges,
                            selected_node_id,
                        );
                        for err in validation_errors {
                            dr.warnings.push(format!("Validation: {}", err));
                        }
                        for w in validation_warnings {
                            dr.warnings.push(format!("Warning: {}", w));
                        }
                        Some(dr)
                    } else {
                        Some(gyre_domain::view_query_resolver::dry_run(
                            &query,
                            &nodes,
                            &edges,
                            selected_node_id,
                        ))
                    }
                } else {
                    warn!(
                        "Failed to deserialize view query for self-check — rejecting invalid query"
                    );
                    // Don't send unvalidated queries to the frontend.
                    // Stream a warning with the raw JSON so the user can see what was attempted.
                    let raw_preview = serde_json::to_string_pretty(&query_json)
                        .unwrap_or_else(|_| query_json.to_string());
                    let truncated = if raw_preview.len() > 500 {
                        format!("{}...", &raw_preview[..500])
                    } else {
                        raw_preview
                    };
                    stream_text(
                        sender,
                        &format!(
                            "\n\n*Warning: Generated view query had invalid structure and was not applied.*\n\n```json\n{}\n```",
                            truncated
                        ),
                        true,
                    ).await;
                    break;
                };

                if let Some(ref dr) = dry_run_result {
                    // Collect actionable warnings. Include "may be cluttered"
                    // warnings so the LLM knows when to narrow scope, but
                    // downgrade them: only trigger refinement if there are
                    // also non-clutter warnings or the result is very large.
                    let actionable_warnings: Vec<&String> = dr
                        .warnings
                        .iter()
                        .filter(|w| {
                            // [info] prefixed warnings are informational — never trigger
                            // refinement. These are inherent to the data model and cannot
                            // be fixed by query changes (e.g., "SHA-based diff is approximate").
                            if w.starts_with("[info]") {
                                return false;
                            }
                            // Unfixable warnings: inherent to the data source, not
                            // resolvable by query refinement. Skip to avoid wasting
                            // a self-check turn.
                            const UNFIXABLE_PREFIXES: &[&str] = &[
                                "SHA-based diff shows an approximate",
                                "Graph data truncated",
                            ];
                            if UNFIXABLE_PREFIXES.iter().any(|p| w.starts_with(p)) {
                                return false;
                            }
                            // Validation errors are always actionable
                            if w.starts_with("Validation:")
                                || w.starts_with("Computed expression error:")
                            {
                                return true;
                            }
                            // "0 nodes" is actionable unless it's an interactive query
                            if w.contains("matched 0 nodes") {
                                return true;
                            }
                            // Clutter warnings are informational for All/TestGaps scopes
                            if w.contains("may be cluttered") {
                                let is_broad_scope = matches!(
                                    serde_json::from_value::<gyre_common::view_query::ViewQuery>(
                                        query_json.clone()
                                    ),
                                    Ok(q) if matches!(q.scope,
                                        gyre_common::view_query::Scope::All
                                        | gyre_common::view_query::Scope::TestGaps
                                    )
                                );
                                // Still report to LLM, but only trigger refinement for narrow scopes
                                return !is_broad_scope;
                            }
                            true
                        })
                        .collect();
                    if !actionable_warnings.is_empty() && refinement_count < MAX_REFINEMENT_TURNS {
                        // Self-check failed: inject dry-run results back as a synthetic
                        // tool_use → tool_result pair so the LLM can use its native
                        // tool-use architecture for refinement (rather than a plain
                        // user text message which breaks the tool-use flow).
                        refinement_count += 1;
                        send_status(sender, "refining").await;

                        // Synthetic tool_use ID for the dry-run self-check
                        let synthetic_tool_id = format!("selfcheck_{}", refinement_count);

                        // Assistant message: text + synthetic tool_use block
                        conversation_history.push(ConversationMessage {
                            role: "assistant".to_string(),
                            content: ConversationContent::Blocks(vec![
                                ContentBlock::Text {
                                    text: response.text.clone(),
                                },
                                ContentBlock::ToolUse {
                                    id: synthetic_tool_id.clone(),
                                    name: "graph_query_dryrun".to_string(),
                                    input: json!({ "query": query_json }),
                                },
                            ]),
                        });

                        // Tool result with the dry-run feedback
                        let feedback = format!(
                            "Dry-run FAILED. Please refine the <view_query>.\n\n- matched_nodes: {}\n- warnings: {:?}\n- matched names (sample): {:?}",
                            dr.matched_nodes,
                            dr.warnings,
                            &dr.matched_node_names[..dr.matched_node_names.len().min(10)]
                        );
                        conversation_history.push(ConversationMessage {
                            role: "user".to_string(),
                            content: ConversationContent::Blocks(vec![ContentBlock::ToolResult {
                                tool_use_id: synthetic_tool_id,
                                content: feedback,
                            }]),
                        });
                        continue;
                    }
                }

                // Send finalized text done before view_query to avoid race
                if !clean_text.is_empty() {
                    let done_msg = ExplorerServerMessage::Text {
                        content: String::new(),
                        done: true,
                    };
                    if sender
                        .send(Message::Text(
                            serialize_msg(&done_msg).unwrap_or_default().into(),
                        ))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }

                // Send the view query to the frontend, with the LLM's preceding
                // text as explanation so the UI can show what the visualization reveals.
                let explanation_text = if clean_text.is_empty() {
                    None
                } else {
                    Some(clean_text.clone())
                };
                let view_msg = ExplorerServerMessage::ViewQuery {
                    query: query_json.clone(),
                    explanation: explanation_text,
                };
                view_query_sent = true;
                if sender
                    .send(Message::Text(
                        serialize_msg(&view_msg).unwrap_or_default().into(),
                    ))
                    .await
                    .is_err()
                {
                    break;
                }

                // Record the assistant response containing the view query in
                // conversation history so follow-up messages have context.
                conversation_history.push(ConversationMessage {
                    role: "assistant".to_string(),
                    content: ConversationContent::Text(response.text.clone()),
                });
            }
        }

        // If no tool calls or stop_reason indicates completion, we're done
        if response.tool_calls.is_empty() || response.stop_reason != "tool_use" {
            // Add final assistant response to conversation history
            // (skip if already pushed during view_query extraction above)
            if !view_query_sent {
                conversation_history.push(ConversationMessage {
                    role: "assistant".to_string(),
                    content: ConversationContent::Text(response.text.clone()),
                });
            }

            // If max_tokens and no view_query was already sent (avoid double-done)
            if response.stop_reason == "max_tokens" && !response.text.is_empty() && !view_query_sent
            {
                let done_msg = ExplorerServerMessage::Text {
                    content: "\n\n*(Response truncated due to length)*".to_string(),
                    done: true,
                };
                let _ = sender
                    .send(Message::Text(
                        serialize_msg(&done_msg).unwrap_or_default().into(),
                    ))
                    .await;
            }
            break;
        }

        if !send_status(sender, "refining").await {
            break; // Client disconnected
        }

        // Add the assistant's response (with tool_use blocks) to the conversation
        let mut assistant_blocks = Vec::new();
        if !response.text.is_empty() {
            assistant_blocks.push(ContentBlock::Text {
                text: response.text.clone(),
            });
        }
        for tc in &response.tool_calls {
            assistant_blocks.push(ContentBlock::ToolUse {
                id: tc.id.clone(),
                name: tc.name.clone(),
                input: tc.input.clone(),
            });
        }
        conversation_history.push(ConversationMessage {
            role: "assistant".to_string(),
            content: ConversationContent::Blocks(assistant_blocks),
        });

        // Execute each tool call and collect results
        let mut result_blocks = Vec::new();
        for tc in &response.tool_calls {
            // Use cached graph_summary when available to avoid recomputing on every call.
            let result = if tc.name == "graph_summary" {
                if let Some(ref cached) = cached_graph_summary {
                    cached.clone()
                } else {
                    let computed = execute_tool(tc, repo_id, nodes, edges, selected_node_id, state).await;
                    *cached_graph_summary = Some(computed.clone());
                    computed
                }
            } else {
                execute_tool(tc, repo_id, nodes, edges, selected_node_id, state).await
            };
            result_blocks.push(ContentBlock::ToolResult {
                tool_use_id: tc.id.clone(),
                content: result,
            });
        }

        // Add tool results as a user message
        conversation_history.push(ConversationMessage {
            role: "user".to_string(),
            content: ConversationContent::Blocks(result_blocks),
        });

        // Track tool-use turn and enforce budget
        tool_turn_count += 1;
        if tool_turn_count >= MAX_TOOL_TURNS {
            info!("Explorer agent hit max tool turns ({MAX_TOOL_TURNS}), forcing final response");
            // Notify the user that we're synthesizing the final answer
            let status_msg = ExplorerServerMessage::Status {
                status: "Synthesizing answer...".to_string(),
                agent_path: None,
                graph_data_age_secs: None,
            };
            let _ = sender
                .send(Message::Text(
                    serialize_msg(&status_msg).unwrap_or_default().into(),
                ))
                .await;
            // Force one final response without tools so the LLM synthesizes
            let final_response = llm_port
                .complete_with_tools(
                    &system_prompt,
                    &conversation_history,
                    &[], // no tools → forces text-only response
                    Some(4096),
                )
                .await;
            if let Ok(final_resp) = final_response {
                if !final_resp.text.is_empty() {
                    let (text_only, vq_json) = parse_view_query_from_text(&final_resp.text);
                    stream_text(sender, &text_only, true).await;
                    // Check for view_query in the final response too
                    if let Some(vq) = vq_json {
                        let view_msg = ExplorerServerMessage::ViewQuery {
                            query: vq,
                            explanation: None,
                        };
                        let _ = sender
                            .send(Message::Text(
                                serialize_msg(&view_msg).unwrap_or_default().into(),
                            ))
                            .await;
                    }
                }
            }
            break;
        }
    }

    Ok(())
}

/// Execute a tool call against the knowledge graph.
async fn execute_tool(
    tool_call: &ToolCall,
    repo_id: &str,
    nodes: &[gyre_common::graph::GraphNode],
    edges: &[gyre_common::graph::GraphEdge],
    selected_node_id: Option<&str>,
    _state: &AppState,
) -> String {
    match tool_call.name.as_str() {
        "graph_summary" => {
            let summary =
                gyre_domain::view_query_resolver::compute_graph_summary(repo_id, nodes, edges);
            serde_json::to_string_pretty(&summary).unwrap_or_else(|e| format!("Error: {e}"))
        }
        "graph_query_dryrun" => {
            let query_value = tool_call.input.get("query").cloned().unwrap_or_default();
            match serde_json::from_value::<gyre_common::view_query::ViewQuery>(query_value) {
                Ok(query) => {
                    let result = gyre_domain::view_query_resolver::dry_run(
                        &query,
                        nodes,
                        edges,
                        selected_node_id,
                    );
                    serde_json::to_string_pretty(&result).unwrap_or_else(|e| format!("Error: {e}"))
                }
                Err(e) => format!("Invalid view query: {e}"),
            }
        }
        "graph_nodes" => {
            let node_id_filter = tool_call.input.get("node_id").and_then(|v| v.as_str());
            let name_pattern = tool_call
                .input
                .get("name_pattern")
                .and_then(|v| v.as_str())
                .map(|s| s.to_lowercase());
            let node_type_filter = tool_call
                .input
                .get("node_type")
                .and_then(|v| v.as_str())
                .map(|s| s.to_lowercase());

            let filtered: Vec<serde_json::Value> = nodes
                .iter()
                .filter(|n| n.deleted_at.is_none())
                .filter(|n| {
                    // If node_id is specified, exact match only
                    if let Some(nid) = node_id_filter {
                        return n.id.to_string() == nid;
                    }
                    if let Some(ref pat) = name_pattern {
                        n.name.to_lowercase().contains(pat)
                            || n.qualified_name.to_lowercase().contains(pat)
                    } else {
                        true
                    }
                })
                .filter(|n| {
                    if let Some(ref nt) = node_type_filter {
                        format!("{:?}", n.node_type).to_lowercase() == *nt
                    } else {
                        true
                    }
                })
                .take(50)
                .map(|n| {
                    json!({
                        "id": n.id.to_string(),
                        "name": n.name,
                        "qualified_name": n.qualified_name,
                        "node_type": format!("{:?}", n.node_type).to_lowercase(),
                        "file_path": n.file_path,
                        "line_start": n.line_start,
                        "visibility": format!("{:?}", n.visibility).to_lowercase(),
                        "spec_path": n.spec_path,
                        "complexity": n.complexity,
                        "test_node": n.test_node,
                    })
                })
                .collect();

            format!(
                "{} nodes:\n{}",
                filtered.len(),
                serde_json::to_string_pretty(&filtered).unwrap_or_default()
            )
        }
        "graph_edges" => {
            let node_id = tool_call.input.get("node_id").and_then(|v| v.as_str());
            let edge_type = tool_call.input.get("edge_type").and_then(|v| v.as_str());
            let source_id = tool_call.input.get("source_id").and_then(|v| v.as_str());
            let target_id = tool_call.input.get("target_id").and_then(|v| v.as_str());

            // Pre-build node lookups for O(1) access instead of O(N) per edge
            let node_info: std::collections::HashMap<&gyre_common::Id, (&str, Option<&str>, &str)> =
                nodes
                    .iter()
                    .map(|n| {
                        (
                            &n.id,
                            (
                                n.name.as_str(),
                                n.spec_path.as_deref(),
                                n.qualified_name.as_str(),
                            ),
                        )
                    })
                    .collect();

            let filtered: Vec<serde_json::Value> = edges
                .iter()
                .filter(|e| e.deleted_at.is_none())
                .filter(|e| {
                    if let Some(nid) = node_id {
                        e.source_id.to_string() == nid || e.target_id.to_string() == nid
                    } else {
                        true
                    }
                })
                .filter(|e| {
                    if let Some(et) = edge_type {
                        format!("{:?}", e.edge_type).to_lowercase() == et.to_lowercase()
                    } else {
                        true
                    }
                })
                .filter(|e| {
                    source_id.map_or(true, |s| e.source_id.to_string() == s)
                        && target_id.map_or(true, |t| e.target_id.to_string() == t)
                })
                .take(100)
                .map(|e| {
                    let (source_name, source_spec, _source_qn) = node_info
                        .get(&e.source_id)
                        .copied()
                        .unwrap_or(("?", None, ""));
                    let (target_name, target_spec, _target_qn) = node_info
                        .get(&e.target_id)
                        .copied()
                        .unwrap_or(("?", None, ""));
                    let mut edge_json = json!({
                        "id": e.id.to_string(),
                        "source_id": e.source_id.to_string(),
                        "source_name": source_name,
                        "target_id": e.target_id.to_string(),
                        "target_name": target_name,
                        "edge_type": format!("{:?}", e.edge_type).to_lowercase(),
                    });
                    // Include spec_path for source/target so LLM can annotate flow
                    // traces with governing specs without extra graph_nodes calls.
                    if let Some(sp) = source_spec {
                        if !sp.is_empty() {
                            edge_json["source_spec_path"] = json!(sp);
                        }
                    }
                    if let Some(sp) = target_spec {
                        if !sp.is_empty() {
                            edge_json["target_spec_path"] = json!(sp);
                        }
                    }
                    edge_json
                })
                .collect();

            format!(
                "{} edges:\n{}",
                filtered.len(),
                serde_json::to_string_pretty(&filtered).unwrap_or_default()
            )
        }
        "node_provenance" => {
            let node_id = tool_call.input.get("node_id").and_then(|v| v.as_str());
            let name_pattern = tool_call
                .input
                .get("name_pattern")
                .and_then(|v| v.as_str())
                .map(|s| s.to_lowercase());

            let target_nodes: Vec<&gyre_common::graph::GraphNode> = nodes
                .iter()
                .filter(|n| n.deleted_at.is_none())
                .filter(|n| {
                    if let Some(nid) = node_id {
                        n.id.to_string() == nid
                    } else if let Some(ref pat) = name_pattern {
                        n.name.to_lowercase().contains(pat)
                            || n.qualified_name.to_lowercase().contains(pat)
                    } else {
                        false
                    }
                })
                .take(10)
                .collect();

            let provenance: Vec<serde_json::Value> = target_nodes
                .iter()
                .map(|n| {
                    json!({
                        "id": n.id.to_string(),
                        "name": n.name,
                        "qualified_name": n.qualified_name,
                        "node_type": format!("{:?}", n.node_type).to_lowercase(),
                        "created_sha": n.created_sha,
                        "created_at": n.created_at,
                        "last_modified_sha": n.last_modified_sha,
                        "last_modified_by": n.last_modified_by,
                        "last_modified_at": n.last_modified_at,
                        "first_seen_at": n.first_seen_at,
                        "file_path": n.file_path,
                        "spec_path": n.spec_path,
                    })
                })
                .collect();

            format!(
                "{} nodes:\n{}",
                provenance.len(),
                serde_json::to_string_pretty(&provenance).unwrap_or_default()
            )
        }
        "search" => {
            let query = tool_call
                .input
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_lowercase();
            let limit = tool_call
                .input
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(30)
                .min(50) as usize; // Cap at 50 to limit conversation history bloat

            let results: Vec<serde_json::Value> = nodes
                .iter()
                .filter(|n| n.deleted_at.is_none())
                .filter(|n| {
                    n.name.to_lowercase().contains(&query)
                        || n.qualified_name.to_lowercase().contains(&query)
                        || n.file_path.to_lowercase().contains(&query)
                        || n.doc_comment
                            .as_ref()
                            .map_or(false, |d| d.to_lowercase().contains(&query))
                        || n.spec_path
                            .as_ref()
                            .map_or(false, |s| s.to_lowercase().contains(&query))
                })
                .take(limit)
                .map(|n| {
                    json!({
                        "id": n.id.to_string(),
                        "name": n.name,
                        "qualified_name": n.qualified_name,
                        "node_type": format!("{:?}", n.node_type).to_lowercase(),
                        "file_path": n.file_path,
                        "spec_path": n.spec_path,
                        "doc_comment": n.doc_comment.as_deref().map(|d| if d.len() > 100 { &d[..100] } else { d }),
                    })
                })
                .collect();

            format!(
                "{} results:\n{}",
                results.len(),
                serde_json::to_string_pretty(&results).unwrap_or_default()
            )
        }
        other => format!("Unknown tool: {other}"),
    }
}

fn build_system_prompt() -> String {
    r##"You are the Gyre Explorer agent. You help humans understand codebases they didn't write by generating interactive visualizations of the knowledge graph.

## Context
User messages may include a <canvas_state> JSON block showing what's currently selected/visible:
- "selected_node": { name, type, id } — the user clicked on this node, $selected resolves to it
- "visible_groups": [...] — tree groups currently expanded on screen
- "active_lens": "structural"|"evaluative" — current analysis mode
- "recent_interactions": [{ action, node?, detail? }, ...] — the user's recent click/zoom/query history (up to 10 entries, newest last)
- "active_query": {...} — the currently applied view query

## $clicked vs $selected
- $selected = the node currently highlighted in the UI (set when user message is sent)
- $clicked = alias for an interactive mode — use in "focus" scope to create click-to-explore views where each click re-runs the query from the clicked node
- In practice, both resolve to the same node ID at query time. The difference is intent: $clicked implies the query should be re-evaluated on future clicks.

## Available Tools
- graph_summary: Returns a condensed summary of the repo's knowledge graph including node/edge counts, top types, top functions, modules, test/spec coverage, and risk indicators.
- graph_query_dryrun: Dry-run a view query against the knowledge graph. Returns matched node count, names, and warnings. Always validate queries before sending.
- graph_nodes: Query graph nodes by ID, name pattern, or node type. Returns up to 50 nodes with details.
- graph_edges: Query graph edges by source/target node ID or edge type. Returns up to 100 edges.
- node_provenance: Returns creation and modification history for a specific node, including commit SHAs, authors, and timestamps. Use this to answer questions about who created/modified code and when.
- search: Full-text search across the knowledge graph. Searches node names, qualified names, doc comments, file paths, and spec paths.

## Workflow
1. Repository context is pre-loaded below — skip graph_summary unless you need detailed rankings or updated counts
2. If you need specific nodes, call graph_nodes with a name_pattern
3. If you need relationships, call graph_edges
4. For provenance questions ("who created X?", "when was Y modified?"), call node_provenance
5. Generate a view query JSON and validate it with graph_query_dryrun
6. If the dry-run has warnings, refine and dry-run again
7. Output the view query in a <view_query>{ ... JSON ... }</view_query> block
8. Provide a text explanation of what the visualization reveals

## Grounding Rules
- EVERY claim must be traceable to actual nodes/edges from tool results
- When naming nodes, use the exact qualified_name from graph_nodes results
- Never invent node names — always verify via graph_nodes or search first
- When explaining structure, cite the specific edge types connecting nodes
- If you're unsure whether a node exists, search for it before referencing it
- For provenance questions (who created/modified what, when), use node_provenance to get creation and modification history

## View Query Grammar

### Scope Types
- `all`: Show everything
- `focus`: BFS from a node. Fields: node (name or "$clicked"), edges (array), direction ("outgoing"/"incoming"/"both"), depth (number)
  **IMPORTANT:** Focus scope defaults to direction="incoming" and depth=5. For blast radius queries, always specify direction="incoming" explicitly — the default is "incoming" but being explicit prevents confusion. For dependency/trace queries, set direction="outgoing" explicitly. The default direction "incoming" shows callers/dependents (blast radius); "outgoing" shows callees/dependencies; "both" includes both and produces misleading blast radius results because it mixes callers with callees.
- `filter`: Filter by node_types (array), computed (expression), or name_pattern
- `test_gaps`: Functions not reachable from any test
- `diff`: Changed nodes between commits. Fields: from_commit, to_commit (SHA prefixes, min 4 chars). For temporal diff, prefix with ~ and use epoch seconds: "~1711929600"
- `concept`: Cross-cutting concept. Fields: seed_nodes (array), expand_edges (array), expand_depth (number), expand_direction ("outgoing"/"incoming"/"both")

### Edge Types
calls, contains, implements, depends_on, field_of, returns, routes_to, governed_by, renders, persists_to, produced_by

### Node Types
package, module, type, interface, function, endpoint, component, table, constant, field, spec

Specs are first-class entities in the knowledge graph (Vision Principle 3). They appear as nodes with node_type "spec" and are linked to code via GovernedBy edges. When users ask about spec coverage, use $governed_by or filter by node_type "spec".

### Computed Expressions (for filter scope)
- `$where(property, 'op', value)` — property: complexity, churn, test_coverage, incoming_calls, outgoing_calls, field_count, test_fragility, risk_score. op: >, >=, <, <=, ==
  - risk_score = churn × complexity × (1 - test_coverage) — composite risk metric
  - NOTE: span_duration, span_count, error_rate require OTLP runtime data and are NOT available in static analysis. Use structural metrics (complexity, churn, risk_score) instead.
- `$callers(node, depth?)`, `$callees(node, depth?)` — call graph traversal
- `$implementors(trait)` — types implementing a trait
- `$fields(type)` — fields of a type
- `$descendants(module)`, `$ancestors(node)` — containment hierarchy
- `$governed_by(spec_path)` — nodes governed by a spec
- `$test_unreachable`, `$test_reachable` — test coverage
- `$ungoverned` — nodes with no spec governance (no GovernedBy edge, no spec_path)
- `$intersect(A, B)`, `$union(A, B)`, `$diff(A, B)` — set operations on expressions
- `$reachable(node, [edge_types], direction, depth)` — general BFS

### Emphasis
- `highlight`: { matched: { color, label } } for matched nodes
- `dim_unmatched`: opacity 0.0-1.0 for non-matched
- `tiered_colors`: array of colors by BFS depth (e.g. ["#ef4444", "#f97316", "#eab308", "#94a3b8"])
- `heat`: { metric, palette } — metric: incoming_calls, outgoing_calls, complexity, churn, churn_count_30d, test_fragility, test_coverage, field_count, risk_score
- `badges`: { template } — e.g. "{{count}} calls"

### Other Fields
- `edges`: { filter: [edge types] }
- `zoom`: "fit" or "current"
- `annotation`: { title, description } — $name for focused node name, {{count}} for result count, {{group_count}} for group count
- `groups`: array of { name, nodes: [qualified node names], color }
- `callouts`: array of { node: "qualified_name", text, color }
- `narrative`: array of { node: "qualified_name", text, order }

## Rules
1. Use exact qualified names from tool results — never guess
2. Always explain what the visualization reveals and why it matters
3. Use $selected when referring to the user's clicked node
4. For interactive (click-to-explore) queries, use $clicked in the focus node
5. Keep groups focused (< 20 nodes each)
6. Prefer "fit" zoom
7. ALWAYS dry-run before finalizing
8. Refine if dry-run returns warnings
9. When the user's intent is ambiguous or could match multiple interpretations, ASK a clarifying question rather than silently picking one interpretation. For example: "Did you mean the SearchService type or the search_service module?" or "Are you looking for test coverage gaps or call-graph structure?" — engaging the human's judgment is the whole point.
10. Surface signal, not noise: default to views that reveal what's architecturally significant (boundaries, interfaces, uncovered specs) rather than showing everything. Right context > all context.

## Feedback Loop Support (Vision Principle 5: Observe → Understand → Decide → Encode → Execute)
- When you identify a problem (ungoverned code, missing test coverage, coupling issues), don't just show it — suggest what the user could do about it
- If a node lacks spec governance, suggest creating a spec: "This area would benefit from a spec. Click the spec badge on [NodeName] to create one."
- If you see structural issues, frame them as spec-level decisions: "Consider whether [Module] should depend on [OtherModule], and encode that decision in a boundary spec."
- Help the user move from Understand → Decide → Encode, not just Observe → Understand

## Output Format
- Conversational explanation (what it shows, what's interesting, what to look at)
- <view_query>{ ... JSON ... }</view_query> block"##.to_string()
}

/// Parse the LLM response to extract view query blocks from text.
fn parse_view_query_from_text(text: &str) -> (String, Option<serde_json::Value>) {
    let mut text_parts = Vec::new();
    let mut view_query = None;

    let mut remaining = text;
    while let Some(start) = remaining.find("<view_query>") {
        let before = &remaining[..start];
        if !before.trim().is_empty() {
            text_parts.push(before.trim().to_string());
        }

        let after_tag = &remaining[start + 12..];
        if let Some(end) = after_tag.find("</view_query>") {
            let json_str = after_tag[..end].trim();
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_str) {
                view_query = Some(parsed);
            }
            remaining = &after_tag[end + 13..];
        } else {
            text_parts.push(remaining.to_string());
            remaining = "";
            break;
        }
    }

    if !remaining.trim().is_empty() {
        text_parts.push(remaining.trim().to_string());
    }

    (text_parts.join("\n\n"), view_query)
}

/// Returns the system default views seeded on first ListViews access.
/// Delegates to saved_views::system_default_views() — single source of truth.
fn system_default_views() -> Vec<(&'static str, &'static str, serde_json::Value)> {
    crate::api::saved_views::system_default_views()
        .into_iter()
        .map(|(name, desc, json_str)| {
            let query: serde_json::Value =
                serde_json::from_str(json_str).unwrap_or(json!({"scope": {"type": "all"}}));
            (name, desc, query)
        })
        .collect()
}

/// Extract node name references from computed expressions.
/// E.g. `$governed_by('my-spec.md')` yields `"my-spec.md"`;
/// `$intersect($callers(FooService), $governed_by('bar.md'))` yields
/// `"FooService"` and `"bar.md"`.
fn extract_computed_node_refs(expr: &str, out: &mut Vec<String>) {
    let bytes = expr.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'(' {
            i += 1;
            let start = i;
            let mut depth = 1u32;
            while i < bytes.len() && depth > 0 {
                match bytes[i] {
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    _ => {}
                }
                if depth > 0 {
                    i += 1;
                }
            }
            let inner = &expr[start..i];
            for arg in inner.split(',') {
                let arg = arg.trim();
                if arg.starts_with('$') {
                    extract_computed_node_refs(arg, out);
                } else if !arg.is_empty() {
                    let cleaned = arg
                        .trim_start_matches('\'')
                        .trim_end_matches('\'')
                        .trim_start_matches('"')
                        .trim_end_matches('"');
                    if !cleaned.is_empty() {
                        out.push(cleaned.to_string());
                    }
                }
            }
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_view_query_from_text_with_query() {
        let text = r#"Here is the blast radius for TaskPort.

<view_query>{"scope": {"type": "focus", "node": "TaskPort", "edges": ["calls"], "direction": "incoming", "depth": 5}, "emphasis": {"dim_unmatched": 0.12}, "zoom": "fit", "annotation": {"title": "Blast radius: TaskPort"}}</view_query>

This shows all callers of TaskPort."#;

        let (clean, query) = parse_view_query_from_text(text);
        assert!(clean.contains("blast radius"));
        assert!(clean.contains("callers of TaskPort"));
        assert!(query.is_some());
        let q = query.unwrap();
        assert_eq!(q["scope"]["type"], "focus");
        assert_eq!(q["scope"]["node"], "TaskPort");
    }

    #[test]
    fn test_parse_view_query_no_query() {
        let text = "I don't understand your question. Could you be more specific?";
        let (clean, query) = parse_view_query_from_text(text);
        assert_eq!(clean, text);
        assert!(query.is_none());
    }

    #[test]
    fn test_parse_view_query_only_query() {
        let text = r#"<view_query>{"scope": {"type": "all"}}</view_query>"#;
        let (clean, query) = parse_view_query_from_text(text);
        assert!(clean.is_empty());
        assert!(query.is_some());
    }

    #[test]
    fn test_explorer_tools_are_defined() {
        let tools = explorer_tool_definitions();
        assert_eq!(tools.len(), 6);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"graph_summary"));
        assert!(names.contains(&"graph_query_dryrun"));
        assert!(names.contains(&"graph_nodes"));
        assert!(names.contains(&"graph_edges"));
        assert!(names.contains(&"node_provenance"));
        assert!(names.contains(&"search"));
    }

    #[test]
    fn test_system_prompt_contains_key_instructions() {
        let prompt = build_system_prompt();
        assert!(prompt.contains("graph_summary"), "missing graph_summary");
        assert!(
            prompt.contains("graph_query_dryrun"),
            "missing graph_query_dryrun"
        );
        assert!(prompt.contains("view_query"), "missing view_query");
        assert!(prompt.contains("Scope"), "missing Scope");
        assert!(prompt.contains("Emphasis"), "missing Emphasis");
        assert!(prompt.contains("ALWAYS dry-run"), "missing ALWAYS dry-run");
    }

    #[test]
    fn test_max_agent_turns() {
        assert_eq!(
            MAX_TOOL_TURNS, 5,
            "Tool-use budget: 5 turns for graph exploration"
        );
        assert_eq!(
            MAX_REFINEMENT_TURNS, 3,
            "Spec requires max 3 dedicated refinement turns for self-check"
        );
    }

    #[test]
    fn test_client_message_deserialization() {
        // Verify the server correctly deserializes the expected message format.
        let msg_json =
            r#"{"type":"message","text":"What is TaskPort?","canvas_state":{"zoom_level":1.5}}"#;
        let msg: ExplorerClientMessage = serde_json::from_str(msg_json).unwrap();
        match msg {
            ExplorerClientMessage::Message { text, canvas_state } => {
                assert_eq!(text, "What is TaskPort?");
                assert!((canvas_state.zoom_level - 1.5).abs() < f64::EPSILON);
            }
            _ => panic!("Expected Message variant"),
        }
    }

    #[test]
    fn test_save_view_message_with_all_fields() {
        let msg_json = r#"{"type":"save_view","name":"My View","description":"A test view","query":{"scope":{"type":"all"}}}"#;
        let msg: ExplorerClientMessage = serde_json::from_str(msg_json).unwrap();
        match msg {
            ExplorerClientMessage::SaveView {
                name,
                description,
                query,
                scope,
            } => {
                assert_eq!(name, "My View");
                assert_eq!(description.as_deref(), Some("A test view"));
                assert_eq!(query["scope"]["type"], "all");
                assert!(scope.is_none());
            }
            _ => panic!("Expected SaveView variant"),
        }
    }

    #[test]
    fn test_save_view_message_with_workspace_scope() {
        let msg_json = r#"{"type":"save_view","name":"WS View","query":{"scope":{"type":"all"}},"scope":"workspace"}"#;
        let msg: ExplorerClientMessage = serde_json::from_str(msg_json).unwrap();
        match msg {
            ExplorerClientMessage::SaveView { scope, .. } => {
                assert_eq!(scope.as_deref(), Some("workspace"));
            }
            _ => panic!("Expected SaveView variant"),
        }
    }

    #[test]
    fn test_server_message_serialization() {
        // Verify streamed text format
        let msg = ExplorerServerMessage::Text {
            content: "Hello".to_string(),
            done: false,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"text""#));
        assert!(json.contains(r#""done":false"#));

        // Verify view query format
        let msg = ExplorerServerMessage::ViewQuery {
            query: json!({"scope": {"type": "all"}}),
            explanation: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"view_query""#));
    }

    #[test]
    fn test_grounding_and_canvas_instructions() {
        let prompt = build_system_prompt();
        assert!(
            prompt.contains("Grounding Rules"),
            "System prompt should contain grounding instructions"
        );
        assert!(
            prompt.contains("canvas_state"),
            "System prompt should explain canvas state format"
        );
        assert!(
            prompt.contains("EVERY claim must be traceable"),
            "System prompt should require grounded claims"
        );
    }

    // ── parse_view_query_from_text with malformed JSON ──────────────────────

    #[test]
    fn test_parse_view_query_malformed_json() {
        let text = r#"Here is the view:

<view_query>{not valid json!!!}</view_query>

Some trailing text."#;
        let (clean, query) = parse_view_query_from_text(text);
        assert!(
            query.is_none(),
            "Malformed JSON should not parse as a view query"
        );
        assert!(clean.contains("Here is the view"));
        assert!(clean.contains("Some trailing text"));
    }

    #[test]
    fn test_parse_view_query_unclosed_tag() {
        let text = r#"Start text <view_query>{"scope": {"type": "all"}} no closing tag"#;
        let (clean, query) = parse_view_query_from_text(text);
        assert!(
            query.is_none(),
            "Unclosed view_query tag should not produce a query"
        );
        // The text from the unclosed tag onwards is captured as-is
        assert!(clean.contains("Start text"));
    }

    // ── parse_view_query_from_text with multiple blocks (should take last) ──

    #[test]
    fn test_parse_view_query_multiple_blocks_takes_last() {
        let text = r#"First attempt:

<view_query>{"scope": {"type": "all"}}</view_query>

Actually, let me refine that:

<view_query>{"scope": {"type": "test_gaps"}}</view_query>

Done."#;
        let (clean, query) = parse_view_query_from_text(text);
        assert!(query.is_some(), "Should parse a view query");
        let q = query.unwrap();
        assert_eq!(
            q["scope"]["type"], "test_gaps",
            "Should take the LAST view_query block, not the first"
        );
        assert!(clean.contains("First attempt"));
        assert!(clean.contains("refine"));
        assert!(clean.contains("Done"));
    }

    // ── System prompt contains all tool descriptions ────────────────────────

    #[test]
    fn test_system_prompt_contains_all_tool_names() {
        let prompt = build_system_prompt();
        let tools = explorer_tool_definitions();
        for tool in &tools {
            assert!(
                prompt.contains(&tool.name),
                "System prompt should mention tool '{}' but it doesn't",
                tool.name
            );
        }
    }

    #[test]
    fn test_system_prompt_contains_computed_expressions() {
        let prompt = build_system_prompt();
        // All important computed expressions should be documented
        let expressions = [
            "$where",
            "$callers",
            "$callees",
            "$implementors",
            "$fields",
            "$descendants",
            "$ancestors",
            "$governed_by",
            "$test_unreachable",
            "$test_reachable",
            "$intersect",
            "$union",
            "$diff",
            "$reachable",
        ];
        for expr in &expressions {
            assert!(
                prompt.contains(expr),
                "System prompt should document computed expression '{}' but it doesn't",
                expr
            );
        }
    }

    #[test]
    fn test_system_prompt_contains_scope_types() {
        let prompt = build_system_prompt();
        let scope_types = ["all", "focus", "filter", "test_gaps", "diff", "concept"];
        for scope in &scope_types {
            assert!(
                prompt.contains(scope),
                "System prompt should document scope type '{}' but it doesn't",
                scope
            );
        }
    }

    #[test]
    fn test_system_prompt_contains_emphasis_types() {
        let prompt = build_system_prompt();
        let emphasis_types = [
            "highlight",
            "dim_unmatched",
            "tiered_colors",
            "heat",
            "badges",
        ];
        for emph in &emphasis_types {
            assert!(
                prompt.contains(emph),
                "System prompt should document emphasis type '{}' but it doesn't",
                emph
            );
        }
    }

    #[test]
    fn test_system_default_views_are_valid() {
        let defaults = system_default_views();
        assert_eq!(defaults.len(), 6, "Should have 6 system default views");

        let expected_names = [
            "Architecture Overview",
            "Test Coverage Gaps",
            "Hot Paths",
            "Blast Radius (click)",
            "Spec Coverage",
            "Ungoverned Risk",
        ];
        for (i, (name, description, query_json)) in defaults.iter().enumerate() {
            assert_eq!(*name, expected_names[i]);
            assert!(
                !description.is_empty(),
                "View '{}' should have a description",
                name
            );
            // Verify the query JSON is a valid ViewQuery
            let parsed =
                serde_json::from_value::<gyre_common::view_query::ViewQuery>(query_json.clone());
            assert!(
                parsed.is_ok(),
                "Default view '{}' has invalid query JSON: {:?}",
                name,
                parsed.err()
            );
        }
    }

    #[test]
    fn test_max_total_turns_matches_spec() {
        // 5 tool-use turns + 3 dedicated refinement turns = 8 max LLM calls
        assert_eq!(MAX_TOOL_TURNS, 5, "Max tool-use turns should be 5");
        assert_eq!(
            MAX_REFINEMENT_TURNS, 3,
            "Max refinement turns should be 3 per spec (dedicated, not shared)"
        );
    }

    // ── Agent flow and self-check loop tests ──────────────────────────────

    #[test]
    fn test_parse_view_query_preserves_complex_json() {
        // Ensure nested emphasis, groups, callouts, and narrative survive round-trip
        let query_json = r##"{"scope":{"type":"focus","node":"$clicked","edges":["calls","contains"],"direction":"incoming","depth":3},"emphasis":{"highlight":{"matched":{"color":"#ef4444","label":"blast"}},"dim_unmatched":0.12,"tiered_colors":["#ef4444","#f97316","#eab308"]},"groups":[{"name":"Core","nodes":["mod::core"],"color":"#3b82f6"}],"callouts":[{"node":"fn::main","text":"entry point","color":"#10b981"}],"zoom":"fit","annotation":{"title":"Blast radius: $name","description":"Shows {{count}} affected nodes"}}"##;
        let text = format!(
            "Here is the blast radius view.\n\n<view_query>{}</view_query>\n\nNotice the tiered colors.",
            query_json
        );
        let (clean, query) = parse_view_query_from_text(&text);
        assert!(query.is_some(), "Complex nested JSON should parse");
        let q = query.unwrap();
        assert_eq!(q["scope"]["type"], "focus");
        assert_eq!(q["scope"]["node"], "$clicked");
        assert_eq!(q["emphasis"]["dim_unmatched"], 0.12);
        assert!(q["groups"].is_array());
        assert!(q["callouts"].is_array());
        assert!(clean.contains("blast radius view"));
        assert!(clean.contains("tiered colors"));
    }

    #[test]
    fn test_parse_view_query_whitespace_around_tags() {
        // Tags with leading/trailing whitespace inside the JSON block
        let text = r#"Result:

<view_query>
  {
    "scope": { "type": "test_gaps" },
    "zoom": "fit"
  }
</view_query>

Explanation."#;
        let (clean, query) = parse_view_query_from_text(text);
        assert!(query.is_some(), "Whitespace-padded JSON should parse");
        assert_eq!(query.unwrap()["scope"]["type"], "test_gaps");
        assert!(clean.contains("Result"));
        assert!(clean.contains("Explanation"));
    }

    #[test]
    fn test_parse_view_query_text_mentioning_view_query_tag_literally() {
        // Text that mentions <view_query> as a string but doesn't contain a real block
        let text = "Use the <view_query> tag to output JSON. No actual block here.";
        let (_clean, query) = parse_view_query_from_text(text);
        // The parser will try to find </view_query> after the opening tag;
        // since there is none, the query should be None.
        assert!(
            query.is_none(),
            "Mentioning the tag without a closing tag should not produce a query"
        );
    }

    #[test]
    fn test_parse_view_query_empty_json_object() {
        let text = "<view_query>{}</view_query>";
        let (clean, query) = parse_view_query_from_text(text);
        assert!(query.is_some(), "Empty JSON object should parse");
        assert!(clean.is_empty());
        // An empty object is valid JSON but not a valid ViewQuery; the parser
        // only extracts JSON, validation happens later in the agent loop.
        assert!(query.unwrap().is_object());
    }

    #[test]
    fn test_parse_view_query_extracts_last_when_first_is_invalid() {
        // First block has invalid JSON, second has valid JSON -- should get the valid one
        let text = r#"Attempt 1:

<view_query>{invalid json}</view_query>

Attempt 2:

<view_query>{"scope": {"type": "all"}}</view_query>"#;
        let (_clean, query) = parse_view_query_from_text(text);
        assert!(query.is_some(), "Should parse the second valid block");
        assert_eq!(query.unwrap()["scope"]["type"], "all");
    }

    // ── system_default_views validation ───────────────────────────────────

    #[test]
    fn test_system_default_views_have_valid_scope_types() {
        let defaults = system_default_views();
        let valid_scopes = ["all", "focus", "filter", "test_gaps", "diff", "concept"];
        for (name, _desc, query) in &defaults {
            let scope_type = query["scope"]["type"].as_str().unwrap_or("");
            assert!(
                valid_scopes.contains(&scope_type),
                "Default view '{}' has invalid scope type '{}'; expected one of {:?}",
                name,
                scope_type,
                valid_scopes
            );
        }
    }

    #[test]
    fn test_system_default_views_all_deserialize_to_view_query() {
        // Each default must parse into a valid ViewQuery AND pass validation
        let defaults = system_default_views();
        for (name, _desc, query_json) in &defaults {
            let parsed =
                serde_json::from_value::<gyre_common::view_query::ViewQuery>(query_json.clone());
            assert!(
                parsed.is_ok(),
                "Default view '{}' failed deserialization: {:?}",
                name,
                parsed.err()
            );
            let vq = parsed.unwrap();
            let errors = vq.validate();
            assert!(
                errors.is_empty(),
                "Default view '{}' has validation errors: {:?}",
                name,
                errors
            );
        }
    }

    #[test]
    fn test_system_default_views_names_are_unique() {
        let defaults = system_default_views();
        let names: Vec<&str> = defaults.iter().map(|(n, _, _)| *n).collect();
        let mut seen = std::collections::HashSet::new();
        for name in &names {
            assert!(seen.insert(name), "Duplicate default view name: '{}'", name);
        }
    }

    #[test]
    fn test_system_default_views_include_interactive_clicked_view() {
        // At least one view should use "$clicked" for interactive exploration
        let defaults = system_default_views();
        let has_clicked = defaults
            .iter()
            .any(|(_, _, q)| q.to_string().contains("$clicked"));
        assert!(
            has_clicked,
            "Default views should include at least one interactive '$clicked' view"
        );
    }

    // ── build_system_prompt agent-loop content ────────────────────────────

    #[test]
    fn test_system_prompt_describes_self_check_workflow() {
        let prompt = build_system_prompt();
        // The prompt must instruct the LLM to dry-run before finalizing
        assert!(
            prompt.contains("dry-run"),
            "System prompt must mention dry-run for self-check"
        );
        assert!(
            prompt.contains("refine"),
            "System prompt must mention refinement on warnings"
        );
        assert!(
            prompt.contains("<view_query>"),
            "System prompt must show the view_query output format tag"
        );
        assert!(
            prompt.contains("</view_query>"),
            "System prompt must show the closing view_query tag"
        );
    }

    #[test]
    fn test_system_prompt_documents_edge_types() {
        let prompt = build_system_prompt();
        let edge_types = [
            "calls",
            "contains",
            "implements",
            "depends_on",
            "field_of",
            "returns",
            "routes_to",
            "governed_by",
            "renders",
            "persists_to",
            "produced_by",
        ];
        for edge in &edge_types {
            assert!(
                prompt.contains(edge),
                "System prompt should document edge type '{}' but it doesn't",
                edge
            );
        }
    }

    #[test]
    fn test_system_prompt_documents_node_types() {
        let prompt = build_system_prompt();
        let node_types = [
            "package",
            "module",
            "type",
            "interface",
            "function",
            "endpoint",
            "component",
            "table",
            "constant",
            "field",
            "spec",
        ];
        for nt in &node_types {
            assert!(
                prompt.contains(nt),
                "System prompt should document node type '{}' but it doesn't",
                nt
            );
        }
    }

    #[test]
    fn test_system_prompt_is_nonempty_and_has_minimum_structure() {
        let prompt = build_system_prompt();
        // The prompt should be substantial (at minimum several KB of instructions)
        assert!(
            prompt.len() > 2000,
            "System prompt should be at least 2000 chars, got {}",
            prompt.len()
        );
        // Must contain the agent identity
        assert!(
            prompt.contains("Gyre Explorer"),
            "System prompt should identify as Gyre Explorer agent"
        );
        // Must contain workflow steps
        assert!(
            prompt.contains("Workflow"),
            "System prompt should have a Workflow section"
        );
        // Must contain output format instructions
        assert!(
            prompt.contains("Output Format"),
            "System prompt should have Output Format section"
        );
        // Must contain rules section
        assert!(
            prompt.contains("Rules"),
            "System prompt should have Rules section"
        );
    }

    #[test]
    fn test_system_prompt_feedback_loop_instructions() {
        let prompt = build_system_prompt();
        // Vision Principle 5 integration
        assert!(
            prompt.contains("Feedback Loop"),
            "System prompt should contain feedback loop instructions"
        );
        assert!(
            prompt.contains("Observe"),
            "Feedback loop should reference the Observe stage"
        );
        assert!(
            prompt.contains("Encode"),
            "Feedback loop should reference the Encode stage"
        );
    }

    #[test]
    fn test_agent_turn_budgets_are_independent() {
        // The agent loop uses two independent budgets. Verify the total.
        let total = MAX_TOOL_TURNS + MAX_REFINEMENT_TURNS;
        assert_eq!(
            total, 8,
            "Total agent loop budget should be 8 (5 tool + 3 refinement)"
        );
        // Tool turns and refinement turns must both be > 0
        assert!(MAX_TOOL_TURNS > 0, "Must allow at least 1 tool turn");
        assert!(
            MAX_REFINEMENT_TURNS > 0,
            "Must allow at least 1 refinement turn for self-check"
        );
    }
}
