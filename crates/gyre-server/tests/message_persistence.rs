//! Integration test: the unified message bus is persistent forge state
//! (message-bus.md §Core Principle — "The existing WebSocket + a database
//! table + the existing in-memory broadcast channel").
//!
//! Proves that with `GYRE_DATABASE_URL` pointing at a SQLite file,
//! `build_state` wires `AppState.messages` to the persistent
//! `MessageRepository` (SqliteStorage), so Directed and Event-tier messages
//! survive a restart — a second `build_state` over the same DB file sees the
//! messages written by the first, with delivery state (ack) and TTL expiry
//! still enforced.
//!
//! This test lives in its own test binary (own process) because
//! `build_state` reads `GYRE_DATABASE_URL` from the process environment;
//! other integration binaries must not see the variable. Within this
//! binary, the tests serialize on `ENV_LOCK`: libtest runs the tests in one
//! process, and an interleaved `set_var`/`remove_var` from a sibling test
//! would silently point another test's `build_state` at the wrong database
//! (or drop it to the mem adapter) — the same hazard
//! `task163_dependency_persistence.rs` documents.
//!
//! Against the old wiring (`Arc::new(mem::MemMessageRepository::default())`
//! hardcoded in `build_state`) the second instance sees an empty HashMap and
//! these tests fail — that is the behavioral proof that the Diesel adapter,
//! not the mem stand-in, is on the production path whenever a database is
//! configured.

use futures_util::{SinkExt, StreamExt};
use gyre_common::message::{Destination, Message, MessageKind, MessageOrigin};
use gyre_common::Id;
use gyre_domain::Agent;
use gyre_ports::MessageRepository as _;
use gyre_server::{abac_middleware, build_router, build_state, AppState};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio_tungstenite::tungstenite::Message as WsFrame;

/// `GYRE_DATABASE_URL` is process-global and `build_state` reads it at
/// construction; the tests in this binary must not interleave env access.
static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const TOKEN: &str = "message-persistence-token";

fn make_directed(id: &str, agent_id: &str, workspace_id: &str, created_at: u64) -> Message {
    Message {
        id: Id::new(id),
        tenant_id: Id::new("tenant-1"),
        from: MessageOrigin::Server,
        workspace_id: Some(Id::new(workspace_id)),
        to: Destination::Agent(Id::new(agent_id)),
        kind: MessageKind::TaskAssignment,
        payload: None,
        created_at,
        signature: Some("sig".to_string()),
        key_id: Some("kid-1".to_string()),
        acknowledged: false,
    }
}

fn make_workspace_event(
    id: &str,
    workspace_id: &str,
    kind: MessageKind,
    created_at: u64,
) -> Message {
    Message {
        id: Id::new(id),
        tenant_id: Id::new("tenant-1"),
        from: MessageOrigin::Server,
        workspace_id: Some(Id::new(workspace_id)),
        to: Destination::Workspace(Id::new(workspace_id)),
        kind,
        payload: None,
        created_at,
        signature: Some("sig".to_string()),
        key_id: Some("kid-1".to_string()),
        acknowledged: false,
    }
}

/// Round-trip Directed + Event-tier messages through two genuinely fresh
/// `build_state` instances over the same SQLite file: store via instance 1,
/// drop it, build instance 2 (a server restart), and require every message
/// to still be visible — Directed via poll paths (list_after / list_unacked /
/// count_unacked / find_by_id), Event via list_by_workspace.
#[tokio::test(flavor = "multi_thread")]
async fn messages_survive_restart_on_sqlite() {
    let _env = ENV_LOCK.lock().await;
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("gyre.db");
    let db_url = format!("sqlite://{}", db_path.display());
    unsafe { std::env::set_var("GYRE_DATABASE_URL", &db_url) };

    let agent = Id::new("msg-persist-agent");
    let ws = Id::new("msg-persist-ws");
    let directed = make_directed(
        "msg-persist-directed-1",
        agent.as_str(),
        ws.as_str(),
        1_700_000_000,
    );
    let event = make_workspace_event(
        "msg-persist-event-1",
        ws.as_str(),
        MessageKind::TaskCreated,
        1_700_000_050,
    );

    // Instance 1: store both messages, then prove they are visible in this
    // instance (the wired adapter serves send/poll in-process too).
    {
        let state = build_state("msg-persist-token", "http://127.0.0.1:1", None);
        state.messages.store(&directed).await.unwrap();
        state.messages.store(&event).await.unwrap();

        let polled = state
            .messages
            .list_after(&agent, 0, None, 100)
            .await
            .unwrap();
        assert_eq!(
            polled.len(),
            1,
            "directed message must be pollable in-instance"
        );
        assert_eq!(polled[0].id, directed.id);

        assert_eq!(
            state.messages.count_unacked(&agent).await.unwrap(),
            1,
            "count_unacked must see the unacked directed message in-instance"
        );
    }

    // Instance 2: a genuinely fresh AppState + fresh SqliteStorage pool over
    // the same file — a server restart. The mem adapter would see nothing.
    let state2 = build_state("msg-persist-token", "http://127.0.0.1:1", None);

    // Directed tier: poll paths after restart.
    let unacked = state2.messages.list_unacked(&agent, 100).await.unwrap();
    assert_eq!(
        unacked.len(),
        1,
        "unacked directed message must survive restart"
    );
    assert_eq!(unacked[0].id, directed.id);
    assert!(
        !unacked[0].acknowledged,
        "stored message must be unacknowledged"
    );
    assert_eq!(unacked[0].kind, MessageKind::TaskAssignment);
    assert_eq!(unacked[0].signature.as_deref(), Some("sig"));
    assert_eq!(unacked[0].key_id.as_deref(), Some("kid-1"));

    let polled = state2
        .messages
        .list_after(&agent, 0, None, 100)
        .await
        .unwrap();
    assert!(
        polled.iter().any(|m| m.id == directed.id),
        "directed message must be returned by a first poll (after_ts=0) after restart"
    );

    let found = state2
        .messages
        .find_by_id(&directed.id)
        .await
        .unwrap()
        .expect("directed message must be findable by id after restart");
    assert_eq!(found.to, Destination::Agent(agent.clone()));

    assert_eq!(
        state2.messages.count_unacked(&agent).await.unwrap(),
        1,
        "count_unacked must still enforce the queue-depth signal after restart"
    );

    // Event tier: workspace listing after restart (GET workspace messages /
    // Subscribe replay both read this path).
    let ws_msgs = state2
        .messages
        .list_by_workspace(&ws, None, None, None, None, None)
        .await
        .unwrap();
    assert!(
        ws_msgs.iter().any(|m| m.id == event.id),
        "event-tier workspace message must survive restart"
    );
    // The workspace feed is Event-tier only: both adapters' contract excludes
    // Directed (agent-inbox) messages from list_by_workspace — they are
    // accessed via list_after / list_unacked.
    assert!(
        ws_msgs.iter().all(|m| m.id != directed.id),
        "directed (agent-inbox) messages must NOT appear in the workspace event feed"
    );

    // Acknowledge on instance 2, twice — idempotent (message-bus.md §Ack).
    state2
        .messages
        .acknowledge(&directed.id, &agent)
        .await
        .unwrap();
    state2
        .messages
        .acknowledge(&directed.id, &agent)
        .await
        .unwrap();
    assert_eq!(
        state2.messages.count_unacked(&agent).await.unwrap(),
        0,
        "ack must clear the unacked count"
    );
    drop(state2);

    // Instance 3: ack state itself must survive the restart (crash recovery
    // reads list_unacked; a lost ack would re-deliver forever or, worse, a
    // lost message would silently vanish).
    let state3 = build_state("msg-persist-token", "http://127.0.0.1:1", None);
    assert!(
        state3
            .messages
            .list_unacked(&agent, 100)
            .await
            .unwrap()
            .is_empty(),
        "acknowledged state must survive restart"
    );
    let reloaded = state3
        .messages
        .find_by_id(&directed.id)
        .await
        .unwrap()
        .expect("acknowledged directed message must still exist after restart");
    assert!(
        reloaded.acknowledged,
        "ack flag must persist across restart"
    );
    drop(state3);

    unsafe { std::env::remove_var("GYRE_DATABASE_URL") };
}

/// Event-tier TTL expiry (message-bus.md §Event Tier): a workspace event older
/// than the retention horizon is deleted by `expire_events` against the same
/// DB file across a restart, while newer events survive. Proves the expiry
/// job operates on persisted state, not a process-local copy.
#[tokio::test(flavor = "multi_thread")]
async fn event_ttl_expiry_persists_across_restart_on_sqlite() {
    let _env = ENV_LOCK.lock().await;
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("gyre.db");
    let db_url = format!("sqlite://{}", db_path.display());
    unsafe { std::env::set_var("GYRE_DATABASE_URL", &db_url) };

    let ws = Id::new("msg-ttl-ws");
    let old_event = make_workspace_event(
        "msg-ttl-old",
        ws.as_str(),
        MessageKind::TaskTransitioned,
        1_000_000_000,
    );
    let fresh_event = make_workspace_event(
        "msg-ttl-fresh",
        ws.as_str(),
        MessageKind::TaskTransitioned,
        4_000_000_000,
    );

    // Instance 1: store both events.
    {
        let state = build_state("msg-ttl-token", "http://127.0.0.1:1", None);
        state.messages.store(&old_event).await.unwrap();
        state.messages.store(&fresh_event).await.unwrap();
    }

    // Instance 2 (restart): expire everything older than the fresh event.
    let state2 = build_state("msg-ttl-token", "http://127.0.0.1:1", None);
    let deleted = state2.messages.expire_events(2_000_000_000).await.unwrap();
    assert_eq!(deleted, 1, "exactly the stale event must be expired");

    let ws_msgs = state2
        .messages
        .list_by_workspace(&ws, None, None, None, None, None)
        .await
        .unwrap();
    assert!(
        ws_msgs.iter().all(|m| m.id != old_event.id),
        "stale event must be gone after expiry"
    );
    assert!(
        ws_msgs.iter().any(|m| m.id == fresh_event.id),
        "fresh event must survive expiry"
    );
    drop(state2);

    // Instance 3 (second restart): the deletion itself is durable.
    let state3 = build_state("msg-ttl-token", "http://127.0.0.1:1", None);
    let ws_msgs = state3
        .messages
        .list_by_workspace(&ws, None, None, None, None, None)
        .await
        .unwrap();
    assert!(
        ws_msgs.iter().all(|m| m.id != old_event.id),
        "expiry must survive restart"
    );
    assert_eq!(ws_msgs.len(), 1, "only the fresh event remains");

    unsafe { std::env::remove_var("GYRE_DATABASE_URL") };
}

// ── HTTP-level regression (task acceptance criteria 1 + 2) ───────────────────
//
// The port-level tests above prove the wiring; this test drives the REAL
// router over a real TCP listener: POST a Directed message through
// /api/v1/workspaces/:id/messages (real signing + real persist), verify the
// messages table row via a third-party SqliteStorage pool, poll it back via
// GET /api/v1/agents/:id/messages, then tear the server down, rebuild the
// AppState over the same DB file, and verify the Directed message via poll,
// the Event-tier message via GET /api/v1/workspaces/:id/messages, and the
// WebSocket Subscribe replay (ws.rs list_by_workspace).

/// A real gyre-server bound to a random port, built from `build_state` over
/// the given `GYRE_DATABASE_URL` — the same construction path main.rs uses.
struct Server {
    base: String,
    port: u16,
    state: Arc<AppState>,
    task: tokio::task::JoinHandle<()>,
}

impl Server {
    async fn start(db_url: &str) -> Server {
        unsafe { std::env::set_var("GYRE_DATABASE_URL", db_url) };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base = format!("http://127.0.0.1:{port}");
        let state = build_state(TOKEN, &base, None);
        abac_middleware::seed_builtin_policies(&state)
            .await
            .expect("seed built-in ABAC policies");
        let app = build_router(Arc::clone(&state));
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Server {
            base,
            port,
            state,
            task,
        }
    }

    /// Tear the instance down: abort the listener and drop the AppState —
    /// the in-process equivalent of killing the server process.
    async fn stop(self) {
        self.task.abort();
        drop(self.state);
    }

    async fn post(&self, path: &str, body: serde_json::Value) -> reqwest::Response {
        reqwest::Client::new()
            .post(format!("{}{path}", self.base))
            .header("Authorization", format!("Bearer {TOKEN}"))
            .json(&body)
            .send()
            .await
            .unwrap()
    }

    async fn get(&self, path: &str) -> reqwest::Response {
        reqwest::Client::new()
            .get(format!("{}{path}", self.base))
            .header("Authorization", format!("Bearer {TOKEN}"))
            .send()
            .await
            .unwrap()
    }
}

/// Read the next JSON text frame from a WebSocket stream, skipping Ping/Pong
/// control frames. Returns None on timeout or close.
async fn next_ws_json<S>(rx: &mut S, timeout: Duration) -> Option<serde_json::Value>
where
    S: futures_util::Stream<Item = Result<WsFrame, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return None;
        }
        match tokio::time::timeout(remaining, rx.next()).await {
            Ok(Some(Ok(WsFrame::Text(text)))) => return serde_json::from_str(&text).ok(),
            Ok(Some(Ok(WsFrame::Ping(_) | WsFrame::Pong(_)))) => continue,
            _ => return None,
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn message_endpoints_and_ws_replay_survive_restart_on_sqlite() {
    let _env = ENV_LOCK.lock().await;
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("gyre-http.db");
    let db_url = format!("sqlite://{}", db_path.display());

    // ── Instance 1: a real server over the SQLite file. ────────────────────
    let server1 = Server::start(&db_url).await;

    // Workspace via the public API.
    let ws_resp = server1
        .post(
            "/api/v1/workspaces",
            json!({ "name": "msg-http-ws", "tenant_id": "default" }),
        )
        .await;
    assert_eq!(ws_resp.status(), 201, "workspace create must succeed");
    let ws_id = ws_resp.json::<serde_json::Value>().await.unwrap()["id"]
        .as_str()
        .expect("workspace id")
        .to_string();

    // Agent in that workspace. The agents port is SQLite-backed under
    // GYRE_DATABASE_URL, so the row survives the restart below (the poll
    // endpoint 404s on an unknown agent).
    let agent_id = Id::new("msg-http-agent");
    let mut agent = Agent::new(agent_id.clone(), "msg-http-agent", 0);
    agent.workspace_id = Id::new(&ws_id);
    server1.state.agents.create(&agent).await.unwrap();

    // AC1: Directed message via the real POST endpoint — validation, signing,
    // and persistence all on the wired SQLite path.
    let send_resp = server1
        .post(
            &format!("/api/v1/workspaces/{ws_id}/messages"),
            json!({
                "to": { "agent": agent_id.as_str() },
                "kind": "task_assignment",
                "payload": { "task_id": "TASK-HTTP-1" }
            }),
        )
        .await;
    assert_eq!(send_resp.status(), 201, "directed send must succeed");
    let sent = send_resp.json::<serde_json::Value>().await.unwrap();
    assert!(
        sent["signature"].is_string(),
        "send response must carry the server signature"
    );
    assert!(
        sent.get("acknowledged").is_none(),
        "acknowledged must be excluded from the send response"
    );
    let directed_id = sent["id"].as_str().expect("message id").to_string();

    // Event-tier workspace message via the same endpoint.
    let evt_resp = server1
        .post(
            &format!("/api/v1/workspaces/{ws_id}/messages"),
            json!({
                "to": { "workspace": ws_id },
                "kind": "task_created",
                "payload": { "task_id": "TASK-HTTP-1" }
            }),
        )
        .await;
    assert_eq!(evt_resp.status(), 201, "event send must succeed");
    let event_id = evt_resp.json::<serde_json::Value>().await.unwrap()["id"]
        .as_str()
        .expect("event id")
        .to_string();

    // AC1 table check: a third-party SqliteStorage pool (no AppState
    // involved) must read the rows back from the messages table. The Diesel
    // row mapping round-trips the to_type / acknowledged columns.
    {
        let reader = gyre_adapters::SqliteStorage::new(db_path.to_str().unwrap()).unwrap();
        let directed_row = reader
            .find_by_id(&Id::new(&directed_id))
            .await
            .unwrap()
            .expect("directed message row must exist in the messages table");
        assert_eq!(
            directed_row.to,
            Destination::Agent(agent_id.clone()),
            "messages.to_type must be 'agent' for the directed row"
        );
        assert!(
            !directed_row.acknowledged,
            "messages.acknowledged must be 0 for the unacked directed row"
        );
        assert_eq!(
            directed_row.workspace_id.as_ref(),
            Some(&Id::new(&ws_id)),
            "directed row must carry the workspace scope"
        );
        let event_row = reader
            .find_by_id(&Id::new(&event_id))
            .await
            .unwrap()
            .expect("event message row must exist in the messages table");
        assert_eq!(
            event_row.to,
            Destination::Workspace(Id::new(&ws_id)),
            "messages.to_type must be 'workspace' for the event row"
        );
    }

    // AC1 poll: the GET endpoint must return the directed message.
    let poll_resp = server1
        .get(&format!(
            "/api/v1/agents/{}/messages?after_ts=0",
            agent_id.as_str()
        ))
        .await;
    assert_eq!(poll_resp.status(), 200, "in-instance poll must succeed");
    let polled = poll_resp.json::<serde_json::Value>().await.unwrap();
    let polled = polled
        .as_array()
        .expect("poll response is a JSON array of messages");
    assert!(
        polled
            .iter()
            .any(|m| m["id"] == directed_id && m["payload"]["task_id"] == "TASK-HTTP-1"),
        "polled inbox must contain the directed message with its payload, got: {polled:?}"
    );

    // Tear down: abort the listener and drop the AppState.
    server1.stop().await;

    // ── Instance 2 over the same DB file: a server restart. ────────────────
    // With the old mem wiring every assertion below fails (empty store).
    let server2 = Server::start(&db_url).await;

    // AC2/AC3: the unacked Directed message is still pollable after restart
    // (at-least-once delivery holds across restart).
    let poll_resp = server2
        .get(&format!(
            "/api/v1/agents/{}/messages?after_ts=0",
            agent_id.as_str()
        ))
        .await;
    assert_eq!(poll_resp.status(), 200, "post-restart poll must succeed");
    let polled = poll_resp.json::<serde_json::Value>().await.unwrap();
    let polled = polled.as_array().expect("poll array");
    assert!(
        polled.iter().any(|m| m["id"] == directed_id),
        "directed message must survive restart via the poll endpoint"
    );

    let unacked_resp = server2
        .get(&format!(
            "/api/v1/agents/{}/messages?acknowledged=false",
            agent_id.as_str()
        ))
        .await;
    assert_eq!(unacked_resp.status(), 200);
    let unacked = unacked_resp.json::<serde_json::Value>().await.unwrap();
    assert!(
        unacked
            .as_array()
            .expect("unacked array")
            .iter()
            .any(|m| m["id"] == directed_id),
        "directed message must still be unacked after restart (crash-recovery poll)"
    );

    // AC2: the Event-tier message is still returned by the workspace GET.
    let ws_msgs_resp = server2
        .get(&format!("/api/v1/workspaces/{ws_id}/messages"))
        .await;
    assert_eq!(ws_msgs_resp.status(), 200);
    let ws_msgs = ws_msgs_resp.json::<serde_json::Value>().await.unwrap();
    assert!(
        ws_msgs
            .as_array()
            .expect("workspace messages array")
            .iter()
            .any(|m| m["id"] == event_id),
        "event-tier message must survive restart via the workspace messages endpoint"
    );

    // AC2: the Event-tier message is still delivered by the WebSocket
    // Subscribe replay (ws.rs reads state.messages.list_by_workspace — the
    // same persisted table).
    let ws_url = format!("ws://127.0.0.1:{}/ws", server2.port);
    let (ws_stream, _resp) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("connect to /ws");
    let (mut ws_tx, mut ws_rx) = ws_stream.split();
    ws_tx
        .send(WsFrame::Text(
            json!({ "type": "Auth", "token": TOKEN }).to_string(),
        ))
        .await
        .unwrap();
    let auth_result = next_ws_json(&mut ws_rx, Duration::from_secs(5))
        .await
        .expect("AuthResult frame");
    assert_eq!(auth_result["type"], "AuthResult");
    assert_eq!(auth_result["success"], true, "WS auth must succeed");

    ws_tx
        .send(WsFrame::Text(
            json!({
                "type": "Subscribe",
                "scopes": [{ "workspace_id": ws_id }],
                "last_seen": 0
            })
            .to_string(),
        ))
        .await
        .unwrap();

    // Replay is Event-tier only (list_by_workspace excludes agent-inbox
    // messages on both adapters), so the replayed frames must contain the
    // persisted Event and must not leak the Directed inbox message.
    let mut saw_event = false;
    for _ in 0..64 {
        let Some(frame) = next_ws_json(&mut ws_rx, Duration::from_secs(5)).await else {
            break;
        };
        if frame["id"] == event_id {
            saw_event = true;
        }
        assert!(
            frame["id"] != directed_id,
            "Subscribe replay must not deliver Directed (agent-inbox) messages"
        );
        if saw_event {
            break;
        }
    }
    assert!(
        saw_event,
        "Subscribe replay must return the persisted Event-tier message after restart"
    );

    unsafe { std::env::remove_var("GYRE_DATABASE_URL") };
}

// ── WebSocket replay window contract on the wired SQLite adapter (task-250) ──
//
// message-bus.md §Delivery/WebSocket: the replay delivers persisted
// Event-tier messages with created_at > last_seen (strictly greater), the
// NEWEST 1000 across all subscribed workspaces combined, oldest-first, plus
// exactly one ReplayCatchUp{truncated:true} when more than 1000 qualified.
// The in-crate ws.rs tests prove the same contract on the mem-backed
// AppState; this test proves it on the production SQLite path.

/// Connect to the server's /ws endpoint and authenticate with the shared
/// token, returning the authenticated stream.
async fn authed_ws(
    port: u16,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let ws_url = format!("ws://127.0.0.1:{port}/ws");
    let (mut stream, _resp) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("connect to /ws");
    stream
        .send(WsFrame::Text(
            json!({ "type": "Auth", "token": TOKEN }).to_string(),
        ))
        .await
        .unwrap();
    let auth_result = next_ws_json(&mut stream, Duration::from_secs(5))
        .await
        .expect("AuthResult frame");
    assert_eq!(auth_result["type"], "AuthResult");
    assert_eq!(auth_result["success"], true, "WS auth must succeed");
    stream
}

/// Ping/Pong fence: the socket message loop handles one client message at a
/// time, and the Subscribe handler emits the whole replay window (plus the
/// catch-up signal) synchronously before returning to the loop. A Ping sent
/// after the Subscribe therefore fences the complete replay response —
/// every frame received before the Pong is the entire Subscribe output.
async fn fenced_replay_frames(
    stream: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Vec<serde_json::Value> {
    stream
        .send(WsFrame::Text(
            json!({ "type": "Ping", "timestamp": 0 }).to_string(),
        ))
        .await
        .unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let mut frames = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        assert!(
            !remaining.is_zero(),
            "timed out waiting for the Pong fence — incomplete replay response"
        );
        let frame = tokio::time::timeout(remaining, stream.next())
            .await
            .expect("timed out waiting for the Pong fence")
            .expect("socket must stay open")
            .expect("frame must be ok");
        if let WsFrame::Text(text) = frame {
            let v: serde_json::Value = serde_json::from_str(&text).unwrap();
            if v.get("type").and_then(|t| t.as_str()) == Some("Pong") {
                return frames;
            }
            frames.push(v);
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn ws_replay_newest_window_and_strict_boundary_on_sqlite() {
    let _env = ENV_LOCK.lock().await;
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("gyre-replay.db");
    let db_url = format!("sqlite://{}", db_path.display());

    let server = Server::start(&db_url).await;

    // Workspace via the public API (tenant "default" — the shared-token
    // caller's tenant, so Subscribe's per-workspace authorization accepts it).
    let ws_resp = server
        .post(
            "/api/v1/workspaces",
            json!({ "name": "replay-sqlite-ws", "tenant_id": "default" }),
        )
        .await;
    assert_eq!(ws_resp.status(), 201, "workspace create must succeed");
    let ws_id = ws_resp.json::<serde_json::Value>().await.unwrap()["id"]
        .as_str()
        .expect("workspace id")
        .to_string();

    // 1001 Event-tier messages with distinct created_at, seeded through the
    // production-wired SQLite MessageRepository, plus one Directed
    // (agent-inbox) message newer than everything else — replay is
    // Event-tier only, so it must never appear.
    let base_ts = 1_700_000_000u64;
    for i in 0..=1000u64 {
        server
            .state
            .messages
            .store(&make_workspace_event(
                &format!("replay-m-{i:04}"),
                &ws_id,
                MessageKind::TaskCreated,
                base_ts + i,
            ))
            .await
            .unwrap();
    }
    server
        .state
        .messages
        .store(&make_directed(
            "replay-directed",
            "replay-sqlite-agent",
            &ws_id,
            base_ts + 5_000,
        ))
        .await
        .unwrap();

    // ── Newest-1000 window + truncation signal (last_seen = 0). ──────────
    let mut ws = authed_ws(server.port).await;
    ws.send(WsFrame::Text(
        json!({
            "type": "Subscribe",
            "scopes": [{ "workspace_id": ws_id }],
            "last_seen": 0
        })
        .to_string(),
    ))
    .await
    .unwrap();
    let frames = fenced_replay_frames(&mut ws).await;

    let mut messages = Vec::new();
    let mut catchups = Vec::new();
    for f in frames {
        match f.get("type").and_then(|t| t.as_str()) {
            Some("ReplayCatchUp") => catchups.push(f),
            Some(other) => panic!("unexpected control frame in replay response: {other}"),
            None => messages.push(f),
        }
    }

    assert_eq!(
        messages.len(),
        1000,
        "replay must deliver exactly the newest 1000 of the 1001 seeded events"
    );
    assert_eq!(
        catchups.len(),
        1,
        "more than 1000 qualified: exactly one ReplayCatchUp must be sent"
    );
    assert_eq!(catchups[0]["truncated"], json!(true));

    let ids: Vec<&str> = messages
        .iter()
        .map(|m| m["id"].as_str().expect("message id"))
        .collect();
    assert!(
        ids.contains(&"replay-m-1000"),
        "the NEWEST message must be inside the replay window"
    );
    assert!(
        !ids.contains(&"replay-m-0000"),
        "the oldest message must be the one dropped when truncating"
    );
    assert!(
        !ids.contains(&"replay-directed"),
        "Directed (agent-inbox) messages must be excluded from replay"
    );
    // The window is exactly the newest 1000, delivered oldest-first.
    let expected: Vec<String> = (1..=1000).map(|i| format!("replay-m-{i:04}")).collect();
    assert_eq!(
        ids.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        expected,
        "the window must be the newest 1000 messages, delivered oldest-first"
    );
    let created: Vec<u64> = messages
        .iter()
        .map(|m| m["created_at"].as_u64().expect("created_at"))
        .collect();
    assert!(
        created.windows(2).all(|w| w[0] <= w[1]),
        "replay must be delivered oldest-first"
    );

    // ── Strict boundary: last_seen == the newest message's created_at. ───
    // The inclusive (>=) since-filter re-delivered the boundary message on
    // every reconnect; the replay contract is strictly greater, so the
    // replay stream must be empty here.
    let mut ws = authed_ws(server.port).await;
    ws.send(WsFrame::Text(
        json!({
            "type": "Subscribe",
            "scopes": [{ "workspace_id": ws_id }],
            "last_seen": base_ts + 1000
        })
        .to_string(),
    ))
    .await
    .unwrap();
    let frames = fenced_replay_frames(&mut ws).await;
    assert!(
        frames.is_empty(),
        "a message created exactly at last_seen must NOT be replayed (got {} frames)",
        frames.len()
    );

    // ── last_seen 1ms below the newest: exactly the newest is replayed. ──
    let mut ws = authed_ws(server.port).await;
    ws.send(WsFrame::Text(
        json!({
            "type": "Subscribe",
            "scopes": [{ "workspace_id": ws_id }],
            "last_seen": base_ts + 999
        })
        .to_string(),
    ))
    .await
    .unwrap();
    let frames = fenced_replay_frames(&mut ws).await;
    assert_eq!(frames.len(), 1, "only the strictly-newer message qualifies");
    assert_eq!(frames[0]["id"], "replay-m-1000");

    server.stop().await;
    unsafe { std::env::remove_var("GYRE_DATABASE_URL") };
}
