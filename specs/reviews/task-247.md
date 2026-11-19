# Review: task-247 — Wire the SQLite/PostgreSQL MessageRepository into production AppState

**Candidate:** `ea171c530b4ed3856d7fe3cf55cdf9fdcd1b05cc` (base `5eff43dd16f3faf78f46aebed453f96272610e3a`)
**Verdict:** approved (2026-10-08, independent review round)

## Scope of the diff

- `crates/gyre-server/src/lib.rs` — exactly the required production change: the
  unconditional `messages: Arc::new(mem::MemMessageRepository::default())` in
  `build_state` is replaced with
  `store!(dyn gyre_ports::MessageRepository, mem::MemMessageRepository::default())`,
  the same conditional every sibling durable port in the same struct literal
  uses (pg_db → PgStorage, else sqlite_db → SqliteStorage, else mem fallback).
- `crates/gyre-server/tests/message_persistence.rs` — new dedicated test binary
  (ENV_LOCK-serialized env access, own process per cargo test binary).
- `specs/coverage/system/message-bus.md`, `specs/tasks/task-247.md` — docs only.
- The port trait, both Diesel adapters, and the mem implementation are
  **unchanged** (diff for `gyre-adapters`, `gyre-ports`, `mem.rs` is empty),
  as the task requires.

## Review evidence (probe logs in /tmp/stage/review-evidence)

1. **Wiring correctness.** `store!` (lib.rs:1291) expands to the
   pg_db/sqlite_db/mem conditional; `SqliteStorage` and `PgStorage` both
   `impl MessageRepository` (sqlite/message.rs:163, postgres/message.rs:163).
   `cargo check -p gyre-server --tests` → exit 0 (9m16s), proving the cast and
   the whole test target type-check. `scripts/check-arch.sh` → passed.
2. **Adapter reality.** `cargo test -p gyre-adapters --lib` → 579 passed,
   0 failed, including all 33 `sqlite::message` tests (store, composite-cursor
   poll, idempotent ack, count_unacked, expire_events, workspace feed,
   cross-agent isolation) against real SQLite files.
3. **Mutation-killing property of the new tests.** The restart tests build a
   second (and third) `AppState` via `build_state` over the same SQLite URL and
   require Directed poll / unacked / find_by_id and Event workspace-feed /
   Subscribe-replay reads to return instance-1 data. Under the old wiring the
   second instance's `MemMessageRepository` is a fresh empty HashMap, so every
   post-restart assertion fails — the tests cannot pass with the production
   behavior disabled. The HTTP-level test additionally drives the real router
   over a real TCP listener and verifies the `messages` table row
   (to_type='agent', acknowledged=0) through an independent third-party
   `SqliteStorage` pool, exactly acceptance criteria 1–3.
4. **No cross-test contamination.** Only `message_persistence.rs` among the
   gyre-server test binaries both sets `GYRE_DATABASE_URL` and touches
   `state.messages`; all other message-using binaries run without a database
   and keep the (unchanged) mem fallback.
5. **Formatting.** `scripts/check-rustfmt-diff.py 5eff43dd` → clean
   (2 Rust files checked).

## Sandbox restriction (not a code defect)

The executor's build wrapper refuses `cargo test -p gyre-server` (code
generation exceeds the 4 GiB budget). The composed test run is therefore
delegated to the independent verifier, which **must** execute:

1. `cargo test -p gyre-server --test message_persistence`
2. `cargo test -p gyre-server` (regression over all server binaries)

See `/tmp/stage/review-evidence/blocked-probe-gyre-server-test.md`.

## Coverage honesty

Rows 3, 11, 16 in `specs/coverage/system/message-bus.md` were updated with the
task-247 wiring evidence and explicitly marked "pending independent verifier
run" — no fake completion. The dependent gaps (ce88ec9 Directed at-least-once,
e5d4cc99 restart row) are correctly left to their own tasks; row 16 keeps its
pre-existing partial status (agent-completion inbox cleanup still missing).
