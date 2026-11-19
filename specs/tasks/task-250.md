---
title: "WebSocket replay correctness: newest-1000 window and strictly-greater last_seen boundary"
spec_ref: "Spec gap spec-c62aac6701125cb09e0c736c / ws-replay-window / 60b177f85a11"
depends_on: []
progress: complete
commits: ["e1c299d4b026ca2f9016a95a236cbd18d60bfaee"]
---

## Required behavior

Per message-bus.md Delivery/WebSocket: the server replays persisted Event-tier messages with created_at > last_seen, capped at 1000 messages total across all subscribed workspaces, and 'If more than 1000 exist, the server sends the newest 1000 and includes a {"type": "ReplayCatchUp", "truncated": true} message.' Two deviations in the Subscribe replay handler (crates/gyre-server/src/ws.rs:150-195) and the adapters: (1) Truncation window: the handler fetches the newest replay_limit+1 (1001) messages per workspace, reverses to oldest-first, and sends the FIRST 1000 it encounters — i.e. the OLDEST 1000 of the newest 1001, silently dropping the newest message. Fix so the delivered set is the NEWEST 1000 qualifying messages across all subscribed workspaces (delivered oldest-first), with ReplayCatchUp{truncated:true} emitted exactly when more than 1000 qualified. (2) Boundary comparison: the spec and the protocol.rs doc comment require strictly greater (created_at > last_seen), but both MemMessageRepository::list_by_workspace and the SQLite adapter filter created_at >= since, so a message created exactly at the client's last_seen timestamp is re-delivered on every reconnect. Make the replay path deliver only strictly-newer messages while preserving the documented REST workspace-query semantics of the `since` filter (implement the strict comparison in the adapters and keep both adapters in agreement — the mem-port-contracts invariant — or filter strictly at the replay call site; either way the observable replay contract is strictly-greater and both storage backends behave identically).

## Acceptance Criteria

- [ ] Newest-window probe: seed 1001 Event-tier messages with distinct created_at into a workspace, Subscribe with last_seen=0, and assert the single newest message IS among the 1000 replayed messages and that a ReplayCatchUp{truncated:true} arrives (current code drops the newest message).
- [ ] Truncation signal: with ≤1000 qualifying messages, no ReplayCatchUp is sent; with >1000, exactly one ReplayCatchUp with truncated:true is sent after the 1000 messages.
- [ ] Cross-workspace cap: with messages in two subscribed workspaces totaling >1000, the replay delivers the newest 1000 across BOTH workspaces combined (not per-workspace caps), oldest-first, plus the truncation signal.
- [ ] Strict boundary: a message with created_at exactly equal to the Subscribe last_seen value is NOT replayed; one created 1ms later IS. Verified against both the mem adapter and the SQLite adapter (identical behavior on both storage backends).
- [ ] Delivery ordering of the replayed window remains oldest-first, and Directed/Telemetry messages remain excluded from replay.

## Verification intent

Boundary probe: persist an Event-tier message, capture its created_at value T, reconnect and Subscribe with last_seen=T; the current >= filter re-delivers the message on every reconnect — the test asserts the replay stream is empty (or contains only strictly-newer messages) and therefore fails against the current mem and SQLite list_by_workspace implementations, passing only when the strictly-greater comparison is enforced on the replay path.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/message-bus.md`. Source: `4127964001113dda08e4852404a68ba560ceb764`; plan: `151021041bfb974bd08554e6182cff451df6efb83f37d8a11f6dfdcc1950d05e`.

[
  {
    "id": "57b856585e62b254ad3ab3d4",
    "decision": "gap",
    "requirement_quote": "If more than 1000 exist, the server sends the newest 1000 and includes a `{\"type\": \"ReplayCatchUp\", \"truncated\": true}` message — the client can use `GET /api/v1/workspaces/:id/messages` with cursor pagination for the full history.",
    "detail": "The subscription machinery is implemented: the Subscribe WsMessage variant (scopes, last_seen Option<u64> epoch millis, session_id Option<String>), the catch-all #[serde(other)] Unknown variant, replay of persisted Event-tier messages through state.messages.list_by_workspace with a 1000-message cap and a ReplayCatchUp{truncated:true} signal, telemetry never replayed (never persisted), Directed excluded from replay (list_by_workspace filters to_type != 'agent'), connection identity established at auth time so Destination::Agent(id) is delivered only to the connection whose verified agent id matches (shared-token/Admin connections are excluded), Workspace delivery gated on subscription, and Broadcast delivered to all. Two specific deviations from the specced replay behavior remain: (1) truncation window — the handler fetches the newest replay_limit+1 messages per workspace, reverses to oldest-first, and sends the first 1000 it encounters, i.e. the OLDEST 1000 of the newest 1001, dropping the newest message; the spec requires 'the server sends the newest 1000'. (2) Boundary comparison — the spec (and the protocol.rs doc comment 'created_at > last_seen') requires strictly greater, but both the mem and SQLite list_by_workspace implementations filter created_at >= since, so a message created exactly at the client's last_seen timestamp is re-delivered on every reconnect. The Subscribe-membership authorization gap is counted under Telemetry Store (2b439cf6) where the spec states it.",
    "evidence": [
      {
        "file": "crates/gyre-common/src/protocol.rs",
        "quote": "Subscribe {\n        scopes: Vec<SubscribeScope>,\n        last_seen: Option<u64>,\n        #[serde(default)]\n        session_id: Option<String>,\n    },",
        "detail": "The Subscribe variant exists with the specced shape: workspace scopes, optional last_seen (epoch millis), optional session_id for presence tracking."
      },
      {
        "file": "crates/gyre-server/src/ws.rs",
        "quote": "// Replay Event-tier messages since last_seen, oldest-first.\n                                    let mut replayed = 0usize;\n                                    let replay_limit = 1000usize;",
        "detail": "Replay on subscribe is implemented with a 1000-message cap, queries state.messages (the MessageRepository), and emits ReplayCatchUp{truncated:true} when the cap is hit."
      },
      {
        "file": "crates/gyre-server/src/ws.rs",
        "quote": "// list_by_workspace returns newest-first; reverse to oldest-first.\n                                            messages.reverse();\n                                            for m in messages {\n                                                if replayed >= replay_limit {\n                                                    truncated = true;\n                                                    break;\n                                                }",
        "detail": "The handler fetches the newest replay_limit+1 per workspace and then sends the first 1000 in oldest-first order — the oldest 1000 of the newest 1001 — whereas the spec requires sending the newest 1000; the newest message in the window is the one dropped."
      },
      {
        "file": "crates/gyre-adapters/src/sqlite/message.rs",
        "quote": "query = query.filter(messages::created_at.ge(since_ts as i64));",
        "detail": "The SQLite (and identically mem) list_by_workspace since-filter is inclusive (>=), so a message created exactly at the client's last_seen is replayed on reconnect; the spec requires created_at > last_seen (strictly greater, no re-delivery)."
      },
      {
        "file": "crates/gyre-server/src/ws.rs",
        "quote": "caller.user_id.is_none()\n                                    && !caller\n                                        .roles\n                                        .contains(&gyre_domain::UserRole::Admin)\n                                    && caller.agent_id == aid.as_str()",
        "detail": "Destination filtering is implemented per spec: agent-targeted messages reach only the connection whose verified agent identity matches (shared-token/Admin connections are excluded and must use REST poll), workspace delivery is subscription-gated, broadcast reaches all."
      }
    ]
  }
]

## Shipped

### Behavior

The Subscribe replay handler (crates/gyre-server/src/ws.rs:150-228) now
implements the message-bus.md §Delivery/WebSocket replay contract exactly:

- **Newest-1000 window across all subscribed workspaces combined.** The
  handler fetches the newest 1001 qualifying messages per subscribed
  workspace (the adapters return newest-first), merges the pools, sorts by
  (created_at DESC, id DESC) — the same composite order the adapters use
  within a workspace — truncates to the newest 1000, reverses, and delivers
  oldest-first. The old code delivered the OLDEST 1000 of the newest 1001
  per workspace in subscription order (dropping the single newest message)
  and effectively capped per workspace; both deviations are gone.
- **Truncation signal.** ReplayCatchUp{truncated:true} is emitted exactly
  when more than 1000 messages qualified in total across the subscribed
  workspaces — once, after the 1000-message window; never at or below 1000.
- **Strictly-greater last_seen boundary.** The replay path queries with
  since = last_seen + 1ms, which for integer epoch milliseconds is exactly
  created_at > last_seen: a message created exactly at the client's
  last_seen is no longer re-delivered on reconnect, while one 1ms later is.
  The inclusive `since` semantics of the REST workspace query
  (GET /api/v1/workspaces/:id/messages) are unchanged, and because the
  shift is applied at the single replay call site, the mem, SQLite, and
  Postgres backends behave identically by construction (no adapter
  changes; the mem-port-contracts invariant is preserved).

### Test evidence

- In-crate WS tests against the mem-backed AppState (real loopback
  WebSocket; each replay response is fenced by a Ping/Pong round trip so
  the assertions see the complete Subscribe output):
  `ws_replay_delivers_newest_1000_with_single_truncation_signal` (1001
  seeded events: the newest IS delivered, the oldest is the one dropped,
  exactly 1000 messages + one catch-up, Directed excluded, oldest-first),
  `ws_replay_no_truncation_signal_at_exactly_1000_messages` (no catch-up
  at the cap edge), `ws_replay_cross_workspace_combined_newest_1000` (two
  workspaces, 1200 qualifying: newest 1000 combined = 500/500, not a
  per-workspace cap), `ws_replay_strict_last_seen_boundary_on_mem`
  (at-boundary message not replayed; the 1ms-later one is).
- SQLite path: tests/message_persistence.rs
  `ws_replay_newest_window_and_strict_boundary_on_sqlite` drives the real
  router over the wired SqliteStorage: newest-1000 window + single
  truncation signal over 1001 persisted events, then reconnects with
  last_seen = the newest created_at (empty replay) and last_seen 1ms below
  it (exactly the newest message replayed).

Both behavioral suites require gyre-server test compilation, which the
implement-stage build wrapper defers to the independent verifier:
`cargo check -p gyre-server --tests` passed here (exit 0; see
review-evidence 01-cargo-check.txt), and the exact required probes are
recorded in review-evidence 03-deferred-server-tests.txt
(`cargo test -p gyre-server --lib ws::tests::ws_replay` and
`cargo test -p gyre-server --test message_persistence ws_replay_newest_window_and_strict_boundary_on_sqlite`).
The window-selection algorithm was additionally validated offline against
a brute-force spec oracle over 4000 randomized trials including heavy
timestamp ties (review-evidence 02-algorithm-ground-truth-simulation.txt).

Coverage row 9 (Delivery) in specs/coverage/system/message-bus.md was
updated with the concrete source evidence for this fix.

Telemetry-tier exclusion from replay is structural and unchanged:
telemetry is never persisted to the messages store (emit_telemetry writes
only the in-memory TelemetryBuffer), so the replay query cannot return it.
