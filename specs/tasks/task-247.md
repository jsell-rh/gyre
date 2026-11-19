---
title: "Wire the SQLite/PostgreSQL MessageRepository into production AppState"
spec_ref: "Spec gap spec-c62aac6701125cb09e0c736c / db-message-repository-wiring / 60b177f85a11"
depends_on: []
progress: complete
commits: ["b2fd012c4257acac1648bfe9a509fede4fd25d8c", "68b63791eeffb2e7eed84d09b3d18dbd4a16e9a9", "68c3bf22bf274bdf7d35ca1c4cbbfbe5e245629f"]
---

## Required behavior

Per message-bus.md Core Principle, the database table must be on the production path: 'The existing WebSocket + a database table + the existing in-memory broadcast channel.' In build_state (crates/gyre-server/src/lib.rs:1697) the messages port is currently set unconditionally to `Arc::new(mem::MemMessageRepository::default())` while every sibling durable port in the same struct literal selects its SQLite/PostgreSQL adapter via the store! macro or an inline pg_db/sqlite_db conditional. Replace the unconditional mem assignment with the same store! wiring pattern the sibling ports use (store!(dyn MessageRepository, mem::MemMessageRepository::default()) or the equivalent inline conditional), so that with GYRE_DATABASE_URL set the Diesel adapter (crates/gyre-adapters/src/sqlite/message.rs or postgres/message.rs, migration 2026-03-24-000017_messages) serves every state.messages operation (send, poll, ack, WS replay, expire jobs), and the mem adapter is used only where sibling durable ports fall back to mem. Do not change the port trait, the adapters, or the mem implementation in this task — only the production wiring. This is the persistence foundation for the Directed at-least-once guarantee (gap ce88ec9) and the server-restart edge-case row (gap e5d4cc99), which are owned by dependent tasks.

## Acceptance Criteria

- [ ] With GYRE_DATABASE_URL pointing at a real SQLite file, a Directed message sent via POST /api/v1/workspaces/:id/messages is present in the messages table (to_type='agent', acknowledged=0) and is returned by GET /api/v1/agents/:id/messages.
- [ ] Restart persistence: build a server against a SQLite GYRE_DATABASE_URL, store an unacked Directed message and an Event-tier workspace message, tear down the process/AppState, rebuild against the same database URL, and verify both messages are still returned (Directed via poll, Event via GET /api/v1/workspaces/:id/messages and via Subscribe replay) — the at-least-once and TTL guarantees hold across restart.
- [ ] The wired port is the Diesel adapter, not MemMessageRepository, whenever GYRE_DATABASE_URL is set (behavioral proof via the restart test above, which fails against the HashMap-backed mem adapter).
- [ ] Existing message-bus behavior is unchanged on the wired DB path: send response shape (acknowledged excluded), 429 queue-depth rejection via count_unacked, idempotent ack, and Event TTL expiry via expire_events all still pass their existing tests against the SQLite adapter.

## Verification intent

Store an unacked Directed message with GYRE_DATABASE_URL set, drop the AppState, rebuild it against the same URL, and poll the agent inbox. With the current wiring (mem::MemMessageRepository unconditionally in build_state, lib.rs:1697) the message is gone — the restart-persistence test fails. The fix only counts if the same probe passes with the Diesel adapter selected by the store! conditional.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/message-bus.md`. Source: `4127964001113dda08e4852404a68ba560ceb764`; plan: `151021041bfb974bd08554e6182cff451df6efb83f37d8a11f6dfdcc1950d05e`.

[
  {
    "id": "587ed8cf6bead4ef7920e4ab",
    "decision": "gap",
    "requirement_quote": "The server is the notary — it validates sender identity, signs directed messages, and routes based on destination. No message broker, no external queue. The existing WebSocket + a database table + the existing in-memory broadcast channel.",
    "detail": "The specced design is three components: the existing WebSocket, a database table, and the existing in-memory broadcast channel. Two of three are on the production path: WebSocket delivery runs over message_broadcast_tx (broadcast::Sender<Message>), and the in-memory channel is that same broadcast channel. The database table is NOT on the wired path: the production AppState constructor build_state (the single constructor main.rs uses) sets `messages: Arc::new(mem::MemMessageRepository::default())` unconditionally, while every sibling durable port in the same struct literal selects the SQLite/PostgreSQL adapter via the store! macro or an inline pg_db/sqlite_db conditional. MemMessageRepository is documented in mem.rs as 'In-memory MessageRepository for tests'. Both Diesel adapters (sqlite/message.rs, postgres/message.rs) and migration 2026-03-24-000017_messages exist and conform, but are dead code in production: with GYRE_DATABASE_URL set, every Directed/Event message (send, poll, ack, WS replay, expiry job — all go through state.messages) is stored in a process-local HashMap, and a server restart loses all of them. This wiring gap also invalidates the Directed at-least-once and Event TTL persistence guarantees (counted under Delivery Guarantees, ce88ec9) and the 'Server restart | Persisted messages survive' edge case (e5d4cc99).",
    "evidence": [
      {
        "file": "crates/gyre-server/src/lib.rs",
        "quote": "messages: Arc::new(mem::MemMessageRepository::default()),",
        "detail": "Inside the production AppState construction in build_state; set unconditionally, with no GYRE_DATABASE_URL conditional. This is the only MessageRepository ever wired into AppState (no `.messages =` reassignment exists anywhere in the server crate)."
      },
      {
        "file": "crates/gyre-server/src/lib.rs",
        "quote": "macro_rules! store {",
        "detail": "The store! macro used by every sibling durable port (workspace_memberships, notifications, meta_specs, ...) selects pg_db/sqlite_db adapters when a database is configured; the `messages` field alone bypasses it, so DB-backed deployments still run the message bus on the in-memory adapter."
      },
      {
        "file": "crates/gyre-server/src/mem.rs",
        "quote": "/// In-memory MessageRepository for tests.",
        "detail": "The adapter that is unconditionally production-wired is itself documented as a test stand-in; its backing store is a HashMap, so Directed/Event messages cannot survive restart in any deployment."
      },
      {
        "file": "crates/gyre-server/src/main.rs",
        "quote": "let state = build_state(&auth_token, &base_url, jwt_config);",
        "detail": "main.rs constructs the server state exclusively through build_state, so the unconditional mem wiring is the production path for every deployment, including GYRE_DATABASE_URL-backed ones."
      },
      {
        "file": "crates/gyre-adapters/src/sqlite/message.rs",
        "quote": "impl MessageRepository for SqliteStorage {",
        "detail": "A conforming durable adapter exists (as does its PostgreSQL twin), proving the 'database table' component could be wired but is not — the messages table and these adapters are unreachable in production."
      }
    ]
  }
]

## Shipped

`build_state` (crates/gyre-server/src/lib.rs) now wires `AppState.messages`
through the same `store!` conditional every sibling durable port uses
(commit 68c3bf22): with `GYRE_DATABASE_URL` set, the Diesel adapter
(`SqliteStorage` / `PgStorage`, migration `2026-03-24-000017_messages`)
serves every `state.messages` operation (send, poll, ack, WS replay,
expiry jobs); `MemMessageRepository` remains only the no-database fallback,
exactly where sibling durable ports fall back to mem. The port trait, both
Diesel adapters, and the mem implementation are unchanged.

### Behavioral evidence

`crates/gyre-server/tests/message_persistence.rs` (own test binary,
ENV_LOCK-serialized, commit 68b63791):

- `messages_survive_restart_on_sqlite` — port-level restart proof:
  Directed + Event messages stored via instance 1 survive an AppState
  rebuild over the same SQLite URL (poll paths, unacked count, find_by_id);
  ack is idempotent and its state survives a further restart.
- `event_ttl_expiry_persists_across_restart_on_sqlite` — `expire_events`
  deletes exactly the stale Event on a rebuilt instance and the deletion
  survives another restart.
- `message_endpoints_and_ws_replay_survive_restart_on_sqlite` — HTTP/WS
  level regression for acceptance criteria 1 and 2: drives the real router
  over a real TCP listener with `GYRE_DATABASE_URL` set, POSTs a Directed
  message via `/api/v1/workspaces/:id/messages`, verifies the
  messages-table row (`to_type='agent'`, `acknowledged=0`) through an
  independent `SqliteStorage` pool, polls it back via
  `GET /api/v1/agents/:id/messages`, then tears down the listener and
  AppState, rebuilds a second server over the same DB URL, and verifies
  the Directed message via poll and `?acknowledged=false`, the Event-tier
  message via `GET /api/v1/workspaces/:id/messages`, and the WebSocket
  Subscribe replay. This test fails against the HashMap-backed mem adapter
  (instance 2 sees an empty store) and against the old unconditional mem
  wiring.

Adapters were verified directly in this sandbox: `cargo test -p
 gyre-adapters --lib sqlite::message` — 33 passed, 0 failed
(send/poll/ack/count/expiry against the SQLite adapter, AC 4), and
`cargo check -p gyre-server --tests` was clean.

### Formatting repair and re-confirmation

Formatting-only repair of the verification finding: rustfmt flagged
changed lines 97, 117, 133, 135, 206, 215 of
`crates/gyre-server/tests/message_persistence.rs`. Applied `rustfmt
--edition 2021` to the file (commit b2fd012c); no behavior change.
`scripts/check-rustfmt-diff.py` against base 5eff43dd was re-run on the
retained candidate ff1957d in this round: changed lines clean
(2 Rust files checked), exit 0 (evidence:
`/tmp/stage/review-evidence/rustfmt-diff-probe.log`). No Rust source
was changed this round — implementation and tests are preserved verbatim
from the retained candidate.

### Verifier must execute

- `cargo test -p gyre-server --test message_persistence` — all three
  tests, including under `-- --test-threads=2` (ENV_LOCK makes
  intra-binary parallelism safe).
- Existing message-bus suites on the wired path: `cargo test -p
  gyre-server --lib api::messages` and `--lib message_dispatcher` (AC 4).

### Coverage rows

`specs/coverage/system/message-bus.md` rows 3 (Core Principle), 11
(Storage), and 16 (Edge Cases) carry the wiring evidence (commit 625690a1);
no unrelated rows touched. The Directed at-least-once guarantee (ce88ec9)
and the server-restart edge-case row (e5d4cc99) remain owned by their
dependent tasks.
