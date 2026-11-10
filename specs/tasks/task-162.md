---
title: "Message bus — wire MessageConsumer dispatcher and event TTL expiry job"
spec_ref: "message-bus.md §Relationship to Notifications"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "message-bus.md §Relationship to Notifications"
  - "message-bus.md §Implementation Notes"
commits: ["5192ff750f58b79d2553a317e9d9b802292ad3e1", "51221dc02d235b9737d645d541650e771d3e89e5"]
---

## Spec Excerpt

### §Relationship to Notifications (message-bus.md)

The notification system becomes a **consumer** of the message bus. The server's message-send path (after storing the message) clones it into a bounded `tokio::sync::mpsc` channel. A background task drains the channel and dispatches to registered consumers:

```rust
pub trait MessageConsumer: Send + Sync {
    async fn on_message(&self, message: &Message);
}
```

### §Implementation Notes (message-bus.md)

Event-tier messages have a configurable TTL (default 7 days, `GYRE_EVENT_TTL_SECS`). A background job calls `expire_events()` on the `MessageRepository` to delete expired events.

## Implementation Plan

1. **MessageConsumer dispatcher**: The bounded mpsc channel (256 capacity) already exists in `lib.rs`. The receiver loop currently drains with a no-op. Implement:
   - A `MessageDispatcher` struct that holds a `Vec<Arc<dyn MessageConsumer>>`
   - A `spawn_message_consumer()` function that spawns a background task to drain the channel and dispatch to all registered consumers
   - Wire the notification system as the first consumer: when it receives `GateFailure`, `BudgetWarning`, `AgentError`, etc., it creates notifications for relevant human users via `NotificationRepository`

2. **Event TTL expiry job**: Add a scheduled background job (like the existing `spawn_budget_daily_reset`) that periodically (every hour) calls:
   - `message_repo.expire_events(now_ms - event_ttl_ms)` for Event-tier cleanup
   - `message_repo.expire_acked_inboxes(now_ms - dead_inbox_ttl_ms)` for completed/orphaned agent inbox cleanup
   - Read `GYRE_EVENT_TTL_SECS` (default 604800 = 7 days) and `GYRE_DEAD_INBOX_TTL_SECS` (default 604800) from env

3. Add tests for the dispatcher and TTL job.

## Acceptance Criteria

- [ ] `spawn_message_consumer()` function exists and starts a background dispatcher task
- [ ] At least one `MessageConsumer` (notification bridge) is registered and creates notifications from message bus events
- [ ] Event TTL expiry job runs periodically and cleans up old Event-tier messages
- [ ] Dead agent inbox cleanup runs periodically
- [ ] `GYRE_EVENT_TTL_SECS` and `GYRE_DEAD_INBOX_TTL_SECS` env vars are respected
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/message-bus.md` §"Relationship to Notifications" and §"Implementation Notes". The MessageConsumer trait already exists in `gyre-ports/src/message.rs`. The mpsc channel is created in `lib.rs` around line 989-1000. Check the existing job scheduling patterns in `lib.rs` (look for `spawn_budget_daily_reset`, `spawn_stale_agent_detector`, etc.) to match the background job style. The notification system is in `api/notifications.rs` and `gyre-ports/src/notification.rs`.

## Shipped

**Dispatcher (§Relationship to Notifications).** `spawn_message_consumer()`
(message_dispatcher.rs) takes ownership of the 256-capacity mpsc receiver via
`AppState::take_message_dispatch_rx()` (single-owner — a second call is a
logged no-op) and spawns the background drain task.
`MessageDispatcher::run()` dispatches every message to all registered
`Arc<dyn MessageConsumer>`s. The first registered consumer is
`NotificationBridge`, which derives Inbox notifications from Event-tier
messages:

- `GateFailure` → p3 `GateFailure` for the MR author agent's spawning user
  (tenant resolved from the workspace record, not fabricated). The gate
  executor's old synchronous p3 path and its `notify_gate_failure` helper are
  removed — single creation path via the bridge, per HSI §8 p3 (amended).
- `BudgetWarning` → p7 for workspace Admin/Developer/Owner members.
- `AgentError` → p5 `AgentEscalation` for the failing agent's spawning user.
- `ReconciliationCompleted` → p6 `MetaSpecDrift` for Admin/Developer/Owner
  members (HSI §8 p6). `emit_reconciliation_completed()` in lib.rs now only
  emits the event; the inline notification-creation code is gone.

All three send paths (AppState::emit_event, REST send handler, MCP
message.send) clone into the channel; `try_send` failures log and never block
the send path (best-effort tier per spec).

**TTL expiry job (§Implementation Notes).** `spawn_message_expiry()` runs the
hourly job (3600s, MissedTickBehavior::Delay, job-registry-tracked as
`message_expiry`, also triggerable via `POST /admin/jobs/message_expiry/run`).
Each cycle calls `expire_events(now − GYRE_EVENT_TTL_SECS·1000)` and
`expire_acked_inboxes(now − GYRE_DEAD_INBOX_TTL_SECS·1000)` on the
`MessageRepository`; both env vars default to 604800 with warn-on-invalid
fallback. `MemMessageRepository` now tracks ack reasons so the dead-inbox
contract (only `agent_completed`/`agent_orphaned` bulk-acks are cleaned;
explicit acks and unacked Directed messages are retained) matches the SQLite
adapter.

**Test evidence** (SKIP_WEB_BUILD=1, recorded under /tmp/stage/review-evidence):

- `cargo test -p gyre-server --lib message_dispatcher` — 9 passed (bridge
  unit tests for all four kinds incl. GateFailure, viewer-role exclusion,
  unknown-workspace skip, dispatcher fan-out, end-to-end
  spawn_message_consumer, TTL retention boundaries, env-var TTL override).
- `cargo test -p gyre-server --lib gate_executor` — 25 passed, incl.
  `failed_gate_creates_gate_failure_notification_via_bridge` (run_gate →
  emit_event → dispatcher → bridge → exactly one p3).
- `cargo test -p gyre-server --lib notifications` — 9 passed (no callers
  broken by `notify_gate_failure` removal).
- `cargo check -p gyre-server --tests` — clean, zero warnings.
- Mechanical checks re-run clean: arch, byte-slice truncation, dead message
  kinds, fabricated/scope-literal defaults, inert enforcement, message
  delivery parity.

Full workspace suite and exact-head GitHub CI remain owned by verification
and publication, per assignment constraints.

### Merge repairs (findings 9c0ccfee, e7aa777ea context)

Two merges against main, both touching only the
`crates/gyre-server/src/main.rs` import list (the same conflict class:
this branch's `message_dispatcher` import vs. main's new imports):

- **9e5a0171d** against main tip fefde294 (`bootstrap_default_tenant`):
  resolved as the union; file body auto-merged cleanly with both sides'
  startup wiring intact.
- **5626a68fc** against main tip 3ced150b (task-140's
  `seed_builtin_personas`): same union resolution; body auto-merged with
  both `seed_builtin_personas(&state)` and
  `message_dispatcher::spawn_message_consumer`/`spawn_message_expiry`
  wiring present.

Both merge commits carry no task label, so `check-task-commit-attribution.sh`
does not force their listing; they are recorded in `commits:` for review
scoping (and their resolutions are also visible in the base..candidate diff
regardless of frontmatter state — see the review-repair note below for why
that fallback matters). No task-owned file needed semantic repair
in either merge. Focused suites re-run green
post-merge (see Test evidence); `cargo check -p gyre-server --bins`
clean; arch, dead-message-kinds, fabricated-scope-defaults, and
task-commit-attribution checks all pass.
`specs/coverage/system/message-bus.md` (14/14, 3 n/a) and
`specs/coverage/SUMMARY.md` (message-bus 100%) merged consistently.

A later merge of the assigned base into this branch, **92db84be1**
(7373979e8, task-100's Platform Model Secrets Rotation Background Job),
auto-merged cleanly with no conflict — it touches api/, mcp.rs,
secret_rotation.rs, stale_agents.rs and specs, not main.rs or any
message-bus-owned file. Verified at that head: main.rs retains
`bootstrap_default_tenant`, `seed_builtin_personas`, and
`message_dispatcher::spawn_message_consumer`/`spawn_message_expiry` wiring
together; `cargo check -p gyre-server --tests` clean; focused suites
(message_dispatcher 9, gate_executor 25, reconciliation wave-completion 1)
all pass. Earlier merges **62862a9e8** (3c39ab1f7) and **d588cc879**
(e5415ae12, task-099) likewise merged cleanly with no task-owned conflict.

A still later merge of the assigned base into this branch, **0c8b6fa1e**
(b5a872087, task-108's admin user impersonation), had one conflict in
`crates/gyre-server/src/mem.rs` (both sides edited the `test_state_inner`
builder): resolved as the union — this branch's `message_dispatch` channel
pair with the receiver stored in `AppState` (single-owner, taken by
`spawn_message_consumer`) is kept, main's no-op drain spawn for the
dispatch receiver is dropped as superseded by the real dispatcher, and
main's meta-spec binding store wiring
(`MemMetaSpecRepository::with_binding_store` shared with
`meta_spec_bindings`, task-116's delete-guard contract) is taken. The
HEAD side's duplicated `agent_inbox_max`/`user_workspace_state` fields
were dropped (already present once in the auto-merged region). Verified
at that head: `cargo check -p gyre-server --tests` clean; focused suites
message_dispatcher (9), gate_executor (25), reconciliation (33),
meta_spec (40), notifications (12) all pass; arch and
task-commit-attribution checks pass. Evidence under
/tmp/stage/review-evidence (task162-merge-repair-*.txt).

Note for reviewers: `check-notification-test-coverage.sh` still exits 2 on
this host before scanning any file -- its awk uses gawk-only 3-arg `match()`
and this host has mawk 1.3.4. Identical failure at pristine base;
host-tooling issue, unrelated to this task.

A latest merge of the repair base into this branch, **05b32d067**
(fca5de5fb, merge of main into fix/admission-policy-preflight and the
branch's first conflict in `jobs.rs`), had two conflicts, both the same
union class as the earlier main.rs import-list repairs:

- `crates/gyre-server/src/jobs.rs`: this branch's `message_expiry` job
  registration vs. main's `task_scheduler` job registration - resolved as
  the union; the registry now registers both jobs.
- `crates/gyre-server/src/main.rs`: import list - union keeps both
  `message_dispatcher` and `signal_chain` alongside
  `seed_builtin_personas`; the auto-merged body already wired
  `signal_chain::spawn_task_scheduler(state.clone())` and
  `message_dispatcher::spawn_message_consumer`/`spawn_message_expiry`.

Verified at 05b32d067: `cargo check -p gyre-server --tests --bins` clean;
focused suites message_dispatcher (9), gate_executor (25),
reconciliation (36), notifications (12), jobs (11), signal_chain (19)
all pass; arch, dead-message-kinds, and task-commit-attribution checks
pass. Evidence under /tmp/stage/review-evidence
(task162-merge-repair-05b32d067.txt).

### Review repairs (findings 73a69995, e7aa777ea)

- **Failing test (73a69995, code):**
  `reconciliation::tests::reconciliation_completed_fires_when_all_tasks_terminal`
  failed at 152e67033 because the p6 MetaSpecDrift notification moved into
  the async NotificationBridge while the test asserted it synchronously
  without ever draining the bus. Fixed in 5192ff750 by spawning the real
  dispatcher (`spawn_message_consumer`, same pattern as the gate_executor
  bridge test) and polling for the bridge-created notification. The test
  kills a real regression: removing the bridge's ReconciliationCompleted
  arm or the dispatcher wiring makes it fail again.
- **Stale frontmatter (73a69995, process):** merge commit 9e5a0171d was
  re-listed in `commits:` (086b140f8) so the main.rs startup-wiring
  conflict resolution is inside review scoping.
- **Frontmatter regression (e7aa777ea, process):** head cea3f23b4 had
  again dropped 9e5a0171d from `commits:`; 4a10321c3 restored it and
  added the new merge 5626a68fc.
- **Frontmatter regression, third recurrence (c8626b1b3, process):** the
  executor's own checkpoint commit c1ae3230b ("process: record task-162
  branch commits") rewrote `commits:` back to the task-labeled commits
  only, dropping both unlabeled merge commits again and falsifying the
  prior "recorded in commits: above" claims at that head. The bb874e517
  repair re-listed them, but see the fourth recurrence below.
- **Frontmatter regression, fourth recurrence (3d2b75ed, process):** the
  same checkpoint mechanism struck again — the executor's commit
  683402c05 ("process: record task-162 branch commits", one commit after
  the bb874e517 repair) rewrote `commits:` back to the two task-labeled
  commits only, and the third-recurrence note's sentence "Repaired at the
  current head ... re-listed" was false at the assigned candidate
  683402c05. This note therefore records the durable facts, independent
  of any frontmatter state a future checkpoint may produce:

  - The branch's merge commits and their resolutions are:
    **9e5a0171d** (main tip fefde294) and **5626a68fc** (main tip
    3ced150b) — main.rs import-list conflicts resolved as the union,
    keeping `message_dispatcher` imports alongside
    `bootstrap_default_tenant` and `seed_builtin_personas` respectively;
    **62862a9e8** (3c39ab1f7) and **d588cc879** (e5415ae12) — clean
    auto-merges; **92db84be1** (assigned base 7373979e8) — clean
    auto-merge, main.rs untouched; **0c8b6fa1e** (b5a872087, task-108) —
    mem.rs conflict resolved as the union (see above); **05b32d067**
    (fca5de5fb) — jobs.rs and main.rs conflicts resolved as unions
    (see above).
  - `commits:` at the head that shipped this note lists all nine
    content-bearing branch commits (both task-labeled commits plus all
    seven merges) so every merge resolution is inside review scoping.
    If an executor checkpoint re-derives `commits:` from task-labeled
    commits only and drops the merges again, that frontmatter state is
    wrong, not this record: the merge resolutions remain reviewable via
    the base..candidate diff and via the task-owned main.rs wiring in
    listed commit 51221dc02.

Full `cargo test -p gyre-server --lib` re-run post-fix: 1494 passed,
0 failed (was 1493/1 at 152e67033). Re-verified at the merged head
92db84be1 (after the task-100 base merge): `cargo check -p gyre-server
--tests` clean, and focused suites message_dispatcher (9), gate_executor
(25), and reconciliation wave-completion (1) all pass. Evidence recorded
under /tmp/stage/review-evidence.
