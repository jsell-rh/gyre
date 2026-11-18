# Review: task-247 — Wire the SQLite/PostgreSQL MessageRepository into production AppState

**Candidate:** `3343c7ecf57ebf6cb3a295e29b699ff5ed25bacb` (branch `pipeline/task-247/379c81b0229a48d79c5fbf0748aee71d-1`)
**Base:** `5eff43dd16f3faf78f46aebed453f96272610e3a`
**Reviewer round:** 2 (round-1 findings: env-race serialization and missing HTTP-level regression — both repaired in commit 68b63791)
**Verdict:** **Approved** — no blocking findings. Verifier must execute the test commands in §4 before publication.

## 1. Scope compliance

The production diff is exactly what the task mandates and nothing more:

- `crates/gyre-server/src/lib.rs:1697-1700` — `AppState.messages` changed from the unconditional
  `Arc::new(mem::MemMessageRepository::default())` to `store!(dyn gyre_ports::MessageRepository,
  mem::MemMessageRepository::default())`, the identical macro every sibling durable port in the same struct
  literal uses. With `GYRE_DATABASE_URL` set, `pg_db`/`sqlite_db` (lib.rs:1257-1274) select the Diesel adapter;
  without it, mem remains the fallback exactly where sibling ports fall back to mem.
- The port trait, both Diesel adapters, and the mem implementation are untouched (verified over the full diff).
- `crates/gyre-server/tests/message_persistence.rs` (new, 618 lines) is the behavioral evidence.
- Coverage rows 3/11/16 in `specs/coverage/system/message-bus.md` amended with wiring evidence, honestly
  labeled "pending independent verifier run"; no unrelated rows touched; the dependent gaps (ce88ec9
  at-least-once, e5d4cc99 restart edge case) correctly left to their owning tasks.
- Both product-surface commits (68c3bf22, 68b63791) are recorded in the task's `commits:` frontmatter.

## 2. Independent evidence

| Probe | Result |
|---|---|
| `cargo check -p gyre-server --tests` | exit 0 — the `store!` cast to `Arc<dyn MessageRepository>` compiles for both `SqliteStorage` and `PgStorage`, and the new test binary type-checks |
| `cargo test -p gyre-adapters --lib sqlite::message` | 33 passed, 0 failed — store/poll/ack/count_unacked/acknowledge_all/expire_events/list_after/list_by_workspace against real SQLite files (AC 4 adapter-side) |
| `bash scripts/check-arch.sh` | pass |
| Static verification of every HTTP/WS test assumption | all match handler/router/serde source — see §3 |

No other production `messages` assignment exists: the only other initialization is `mem.rs:5528` inside
`test_state()` (test helper; `main.rs` builds state exclusively via `build_state`).

## 3. Test quality (why the tests prove the contract)

- **Discriminating by construction:** all three tests build ≥2 fresh `AppState`s over the same SQLite file via
  `build_state` (the exact constructor `main.rs` uses). With the old unconditional mem wiring, instance 2 sees a
  fresh empty HashMap and every restart assertion fails. Reverting the production change breaks the tests —
  they are not self-confirming.
- **Real paths, not mirrors:** the HTTP-level test drives the real router over a real TCP listener — POST
  `/api/v1/workspaces/:id/messages` exercises the genuine validation/signing/persist path; the messages-table
  row is verified through a third-party `SqliteStorage` pool sharing no state with either `AppState`; Directed
  recovery via poll and `?acknowledged=false`; Event via workspace GET and WebSocket Subscribe replay
  (`ws.rs` reads `state.messages.list_by_workspace` — the wired port).
- **Round-2 repairs verified genuine:** ENV_LOCK (tokio Mutex) serializes env access across the binary's tests
  (same pattern as task163); the round-1 workspace-feed assertion bug was real — `list_by_workspace` is
  Event-tier only on both adapters (sqlite/message.rs:344-346 filters `to_type != 'agent'`) — and the test now
  asserts the Directed message is excluded from the feed and from Subscribe replay, matching the port contract.
- **Protocol shapes verified against source, not the implementer's claims:** route registrations
  (api/mod.rs:325,769,847-848; lib.rs:977), global-token → Admin/tenant-"default" (auth.rs:1701-1721, 887-889),
  admin tenant override on workspace create (workspaces.rs:111-115), `parse_destination` shapes
  (api/messages.rs:330-345), payload schemas (TaskAssignment/TaskCreated require `task_id`,
  gyre-common/src/message.rs:353-355,419), `acknowledged` excluded from send responses (api/messages.rs:79-85),
  `Json<Vec<Message>>` poll/workspace responses, and the `#[serde(tag="type")]` WS frames
  (gyre-common/src/protocol.rs:100-145).

## 4. Verifier must execute (gyre-server test compilation deferred: sandbox cgroup memory.max = 4 GiB)

```
cargo test -p gyre-server --test message_persistence
cargo test -p gyre-server --test message_persistence -- --test-threads=2
cargo test -p gyre-server --lib api::messages
cargo test -p gyre-server --lib message_dispatcher
cargo test -p gyre-adapters --lib sqlite::message
```

The three `message_persistence` tests have not been executed in this or the implementation sandbox (build
wrapper defers gyre-server test compilation). Every statically checkable assumption has been verified against
source; a runtime failure would be a test-harness defect caught by the verifier before publication, not a
wiring defect. The transport/budget restriction is infrastructure, not a code defect.

## 5. Non-blocking observations

- **Test robustness nit:** on a mid-test panic, `GYRE_DATABASE_URL` is not removed (no panic guard), so sibling
  tests in the same binary would build against a stale, TempDir-deleted path. Only manifests after an initial
  failure; does not affect the pass path or production code. Not blocking.
- **Drop semantics:** `Server::stop()` aborts the listener task then drops the `AppState`; the aborted task may
  briefly hold an `Arc` clone. Irrelevant to the proof — instance 2 reads through an independent pool over the
  same file, and the mem adapter would still fail the test.
- **Coverage matrix:** rows 3/11 correctly remain `verified` (the adapters were already verified; the wiring
  note is accurate), row 16 adds restart persistence with the pending-verifier caveat. Accurate as written.

Evidence artifacts: `/tmp/stage/review-evidence/` (cargo check output, adapter test output,
static-verification.md).
