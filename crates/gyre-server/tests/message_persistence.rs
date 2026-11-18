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
//! other integration binaries must not see the variable.
//!
//! Against the old wiring (`Arc::new(mem::MemMessageRepository::default())`
//! hardcoded in `build_state`) the second instance sees an empty HashMap and
//! this test fails — that is the behavioral proof that the Diesel adapter,
//! not the mem stand-in, is on the production path whenever a database is
//! configured.

use gyre_common::message::{Destination, Message, MessageKind, MessageOrigin};
use gyre_common::Id;
use gyre_server::build_state;
use tempfile::TempDir;

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
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("gyre.db");
    let db_url = format!("sqlite://{}", db_path.display());
    unsafe { std::env::set_var("GYRE_DATABASE_URL", &db_url) };

    let agent = Id::new("msg-persist-agent");
    let ws = Id::new("msg-persist-ws");
    let directed = make_directed("msg-persist-directed-1", agent.as_str(), ws.as_str(), 1_700_000_000);
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
        assert_eq!(polled.len(), 1, "directed message must be pollable in-instance");
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
    assert_eq!(unacked.len(), 1, "unacked directed message must survive restart");
    assert_eq!(unacked[0].id, directed.id);
    assert!(!unacked[0].acknowledged, "stored message must be unacknowledged");
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
    assert!(
        ws_msgs.iter().any(|m| m.id == directed.id),
        "directed message must also be visible in the workspace feed after restart"
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
        state3.messages.list_unacked(&agent, 100).await.unwrap().is_empty(),
        "acknowledged state must survive restart"
    );
    let reloaded = state3
        .messages
        .find_by_id(&directed.id)
        .await
        .unwrap()
        .expect("acknowledged directed message must still exist after restart");
    assert!(reloaded.acknowledged, "ack flag must persist across restart");
    drop(state3);

    unsafe { std::env::remove_var("GYRE_DATABASE_URL") };
}

/// Event-tier TTL expiry (message-bus.md §Event Tier): a workspace event older
/// than the retention horizon is deleted by `expire_events` against the same
/// DB file across a restart, while newer events survive. Proves the expiry
/// job operates on persisted state, not a process-local copy.
#[tokio::test(flavor = "multi_thread")]
async fn event_ttl_expiry_persists_across_restart_on_sqlite() {
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
