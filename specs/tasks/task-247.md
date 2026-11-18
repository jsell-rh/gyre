---
title: "Wire the SQLite/PostgreSQL MessageRepository into production AppState"
spec_ref: "Spec gap spec-c62aac6701125cb09e0c736c / db-message-repository-wiring / 60b177f85a11"
depends_on: []
progress: ready-for-review
commits: ["68c3bf22bf274bdf7d35ca1c4cbbfbe5e245629f", "68b63791eeffb2e7eed84d09b3d18dbd4a16e9a9"]
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

The production wiring change (commit 68c3bf22) is a single, surgical edit in
`build_state` (crates/gyre-server/src/lib.rs): `AppState.messages` was set
unconditionally to `Arc::new(mem::MemMessageRepository::default())`; it now
uses the same `store!` macro every sibling durable port in the same struct
literal uses, so with `GYRE_DATABASE_URL` set the Diesel adapter
(`SqliteStorage` / `PgStorage`, migration `2026-03-24-000017_messages`)
serves every `state.messages` operation (send, poll, ack, WS replay,
expiry jobs), and `MemMessageRepository` remains only the no-database
fallback — exactly where sibling durable ports fall back to mem. The port
trait, both Diesel adapters, and the mem implementation are unchanged.

This round (commit 68b63791) repairs the two review-round-1 findings in
`crates/gyre-server/tests/message_persistence.rs` and one latent assertion
bug found while repairing them.

### Review repairs

- **Env-race serialization:** all tests in the binary now hold
  `static ENV_LOCK: tokio::sync::Mutex<()>` around their
  `GYRE_DATABASE_URL` set/remove and every `build_state` call, the same
  pattern `tests/task163_dependency_persistence.rs` documents. Under plain
  `cargo test -p gyre-server --test message_persistence -- --test-threads=2`
  the tests can no longer cross-contaminate or silently degrade to the mem
  adapter.
- **HTTP-level regression (acceptance criteria 1 and 2):** the new
  `message_endpoints_and_ws_replay_survive_restart_on_sqlite` test drives
  the real router over a real TCP listener with `GYRE_DATABASE_URL` set:
  POST a Directed message via `/api/v1/workspaces/:id/messages` (real
  validation/signing/persist path — no fabricated Message literals), verify
  the messages-table row (`to_type='agent'`, `acknowledged=0`) through a
  third-party `SqliteStorage` pool that shares no state with either
  AppState, and poll it back via `GET /api/v1/agents/:id/messages`; then
  abort the listener, drop the AppState, rebuild a second server over the
  same DB URL, and verify the Directed message via poll and
  `?acknowledged=false` crash-recovery poll, the Event-tier message via
  `GET /api/v1/workspaces/:id/messages`, and the WebSocket Subscribe
  replay (`ws.rs` `list_by_workspace`).
- **Latent assertion bug fixed:** the port-level test previously asserted a
  Directed (agent-inbox) message appears in the workspace feed. That
  contradicts the `MessageRepository` contract — `list_by_workspace` is
  Event-tier only on BOTH adapters (mem and SQLite filter
  `to_type != 'agent'`, sqlite/message.rs:344-346) — so the old assertion
  would fail whenever the test actually ran. The test now asserts the
  Directed message is excluded from the workspace feed and from Subscribe
  replay, matching the contract.

### Behavioral evidence

- `crates/gyre-server/tests/message_persistence.rs` (own test binary,
  ENV_LOCK-serialized):
  - `messages_survive_restart_on_sqlite` — port-level restart proof:
    Directed + Event messages stored via instance 1 survive a rebuild
    (poll paths, unacked count, find_by_id), ack is idempotent and its
    state survives a further restart.
  - `event_ttl_expiry_persists_across_restart_on_sqlite` — `expire_events`
    deletes exactly the stale Event on a rebuilt instance and the deletion
    survives another restart.
  - `message_endpoints_and_ws_replay_survive_restart_on_sqlite` — the
    HTTP/WS-level regression described above; it fails against the
    HashMap-backed mem adapter (instance 2 sees an empty store) and
    against the old unconditional mem wiring.
- Probes run in this sandbox (its build wrapper defers gyre-server test
  compilation to the independent verifier): `cargo check -p gyre-server
  --tests` clean; `cargo test -p gyre-adapters --lib sqlite::message` —
  33 passed, 0 failed (send/poll/ack/count/expiry behavior against the
  SQLite adapter, AC 4).

### Verifier must execute

- `cargo test -p gyre-server --test message_persistence` — all three
  tests, including under `-- --test-threads=2` (the ENV_LOCK makes
  intra-binary parallelism safe).
- The existing message-bus suites on the wired path: `cargo test -p
  gyre-server --lib api::messages` and `--lib message_dispatcher` (AC 4).

### Coverage rows

`specs/coverage/system/message-bus.md` rows 3 (Core Principle), 11
(Storage), and 16 (Edge Cases) were amended in commit 625690a1 with the
wiring evidence and remain accurate; no unrelated rows touched. The
Directed at-least-once guarantee (ce88ec9) and the server-restart
edge-case row (e5d4cc99) remain owned by their dependent tasks.
