# Review — task-247 (Wire the SQLite/PostgreSQL MessageRepository into production AppState)

Spec: `specs/system/message-bus.md` §Core Principle ("The existing WebSocket + a
database table + the existing in-memory broadcast channel"); gap 587ed8cf.
Commits under review: `68c3bf22` (product), `625690a1`/`c44e19a5` (docs/process).
Base `5eff43dd` → candidate `c44e19a5`.
Verdict: **needs-revision** — the production wiring itself is correct, minimal, and
verified; both findings are in the test evidence that claims the acceptance criteria.

## Round 1 (2026-11-18)

Probes run (checkout at candidate `c44e19a5`; evidence in
`/tmp/stage/review-evidence/`):

- `cargo check -p gyre-server` (via /tmp/stage/bin/cargo) → exit 0 (7m 28s cold).
- `cargo check -p gyre-server --test message_persistence` → exit 0 (2m 07s cold).
- `cargo test -p gyre-adapters --lib sqlite::message` → **33 passed, 0 failed**
  (real SQLite: store/find, cursor list_after/list_by_workspace, list_unacked,
  count_unacked, idempotent acknowledge/acknowledge_all, expire_events/
  expire_acked_inboxes, duplicate-id and Broadcast-destination rejection,
  pre-acknowledged stored as unacked).
- Source diff inspection base→candidate (4 files: lib.rs wiring, new test binary,
  coverage matrix, task file) and grep sweep of GYRE_DATABASE_URL usage across
  `crates/gyre-server/tests/`.
- `bash scripts/check-task-commit-attribution.sh` scope reasoning: product commit
  `68c3bf22` is recorded in task-247.md `commits:`; `625690a1` is docs-only,
  `c44e19a5` is process-type — both outside the script's product-surface scope.

Verified working (no findings):

- **The production wiring is correct and surgical.** `build_state`
  (crates/gyre-server/src/lib.rs) now sets `messages: store!(dyn
  gyre_ports::MessageRepository, mem::MemMessageRepository::default())`
  (lib.rs:1697-1700), the exact pattern every sibling durable port in the same
  struct literal uses: Diesel adapter (`SqliteStorage`/`PgStorage`, migration
  `2026-03-24-000017_messages`) whenever GYRE_DATABASE_URL resolves, mem only on
  the no-database fallback. The unconditional
  `Arc::new(mem::MemMessageRepository::default())` is gone. Port trait, both Diesel
  adapters, and the mem implementation are untouched by the diff, exactly as the
  task requires. Every `state.messages` consumer (api/messages.rs send/poll/ack/
  workspace query, ws.rs:158 Subscribe replay, mcp.rs message tools, the expiry
  job) is unchanged code now served by the Diesel adapter on DB-backed
  deployments — the "database table" component of the Core Principle triad is on
  the production path.
- **The restart-persistence concept of the new test is genuine.** Two fresh
  `build_state` instances over the same SQLite file, with Directed (list_unacked/
  list_after/count_unacked/find_by_id) and Event (list_by_workspace) assertions on
  the rebuilt instance, ack idempotence, and durable ack state on a third instance;
  a second test proves expire_events deletes durable state across a rebuild. The
  test fails against the old mem wiring — it is a real probe of the changed
  behavior, not self-confirming.
- **AC4 adapter-side behaviors are independently confirmed** against SQLite by
  the 33-test adapters suite run in this review (idempotent ack, TTL expiry,
  cursor listing, queue-depth count signal, error contracts).
- **Coverage rows 3/11/16 are honestly amended** — they claim AppState-level proof
  and mark it "pending independent verifier run" rather than claiming endpoint
  execution; no unrelated rows touched.

Findings:

- [-] **F1 (major, test): unserialized process-global env access in the new test
  binary.** `crates/gyre-server/tests/message_persistence.rs` has two
  `#[tokio::test]` fns that each `set_var("GYRE_DATABASE_URL", ...)` on entry and
  `remove_var` on exit with no shared lock. Under plain `cargo test --test
  message_persistence` the harness runs both concurrently in one process: test B's
  `set_var` can land between test A's `set_var` and A's `build_state` (A's
  instances silently built against B's temp DB), or B's `remove_var` can land
  before A's instance-2/3 builds (A silently degrades to the mem adapter and
  fails). The file's "own test binary (own process)" header comment addresses
  cross-binary contamination only — not intra-binary parallelism. The repo's own
  sibling binary for the identical pattern, `tests/task163_dependency_persistence.rs`
  (lines 19-22), documents this exact hazard and serializes on
  `static ENV_LOCK: tokio::sync::Mutex<()>`; the new file must adopt the same
  pattern (or equivalent serialization) without weakening the persistence
  assertions. Verifier serial execution / nextest per-test processes mask the
  race — that masking is why it must be fixed in source.
- [-] **F2 (major, test): AC1/AC2 are endpoint-level acceptance criteria and no
  shipped test executes the endpoints on the SQLite path.** AC1 requires a
  Directed message "sent via POST /api/v1/workspaces/:id/messages" to be present
  in the messages table (to_type='agent', acknowledged=0) and returned by
  GET /api/v1/agents/:id/messages; AC2 requires the Event to be verified via
  GET /api/v1/workspaces/:id/messages and via Subscribe replay after the rebuild.
  The new test proves the wiring at the port level only — it fabricates Message
  literals (placeholder signature "sig"/key_id "kid-1", bypassing send_message's
  validation and signing) and calls `state.messages.*` directly; its comments
  mention POST/GET/Subscribe but execute none of them. No other test closes the
  gap: `tests/api_integration.rs::agent_messages_send_and_receive` (the only
  HTTP-level send/poll test) builds state without GYRE_DATABASE_URL (mem
  adapter), and e2e_ralph_loop.rs / explorer_ws_integration.rs never set the var
  (grep of GYRE_DATABASE_URL across crates/gyre-server/tests/). Repair: one
  focused HTTP-level regression in the SQLite-backed test binary driving the real
  router — POST a Directed message, assert the messages table row and the GET
  poll; after a `build_state` rebuild over the same URL, assert the workspace GET
  and the WS Subscribe replay (ws.rs:158 list_by_workspace) return the Event.
  Port-only proof must not be recorded as endpoint execution for AC1/AC2.

Test-inflation check: none found — the new tests fail against the old wiring and
assert durable state across genuine rebuilds; the defects are race-safety (F1) and
coverage level vs the task contract (F2), not self-confirmation.

Scope check: clean — only the wiring line in lib.rs plus the new test file and
docs; port trait, adapters, mem implementation, handlers, and ws.rs untouched.

Verifier must run after revision (server test compilation is out of this sandbox's
budget): `cargo test -p gyre-server --test message_persistence` (both tests, plus
`-- --test-threads=2` to confirm F1 is fixed), `cargo test -p gyre-server --lib
api::messages`, `cargo test -p gyre-server --lib message_dispatcher`,
`cargo test -p gyre-adapters --lib sqlite::message` (33/33 ok in this review).
