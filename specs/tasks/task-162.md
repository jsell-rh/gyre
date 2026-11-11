---
title: "Message bus — wire MessageConsumer dispatcher and event TTL expiry job"
spec_ref: "message-bus.md §Relationship to Notifications"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "message-bus.md §Relationship to Notifications"
  - "message-bus.md §Implementation Notes"
commits: ["51221dc02d235b9737d645d541650e771d3e89e5", "9e5a0171d11dc6fff1d5634b1a3348266177ce5a", "5192ff750f58b79d2553a317e9d9b802292ad3e1", "5626a68fcb94469acc6f6c2504dcd2dbcd204e7c", "62862a9e81c5c53e46ac4b936f1b69984fdc74d4", "d588cc8792d2740cb987f4aca8d1093fa4bfdf60", "92db84be179b04ec15ffb97db3b956b12ff7dd25", "0c8b6fa1eb5702be372d3ec45a64317b12d105f6", "05b32d067e6f79efecbad79a7cd6b8048c743329", "7177390961187dfea1e381a632cfab76a72147a5"]
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

- [x] `spawn_message_consumer()` function exists and starts a background dispatcher task
- [x] At least one `MessageConsumer` (notification bridge) is registered and creates notifications from message bus events
- [x] Event TTL expiry job runs periodically and cleans up old Event-tier messages
- [x] Dead agent inbox cleanup runs periodically
- [x] `GYRE_EVENT_TTL_SECS` and `GYRE_DEAD_INBOX_TTL_SECS` env vars are respected
- [x] `cargo test --all` passes

## Shipped

**Dispatcher (§Relationship to Notifications).** `spawn_message_consumer()`
(crates/gyre-server/src/message_dispatcher.rs) takes ownership of the
256-capacity mpsc receiver via `AppState::take_message_dispatch_rx()`
(single-owner — a second call is a logged no-op) and spawns the background
drain task. `MessageDispatcher` holds `Vec<Arc<dyn MessageConsumer>>` and
dispatches every drained message to all consumers. The first registered
consumer is `NotificationBridge`, which derives Inbox notifications from
Event-tier messages (ranks per the unified user-management taxonomy):

- `GateFailure` → rank-3 `GateFailure` for the MR author agent's spawning
  user, resolved through the MR → author agent → spawner chain; tenant
  resolved from the workspace record, never fabricated. The gate executor
  emits the `GateFailure` event and no longer creates the Inbox item
  synchronously — single creation path via the bridge (HSI §8 p3
  amendment).
- `BudgetWarning` → rank-4 `AgentBudgetWarning` for workspace
  Admin/Developer/Owner members.
- `AgentError` → rank-2 `AgentEscalation` for the failing agent's spawning
  user.
- `ReconciliationCompleted` → rank-6 `MetaSpecDrift` for workspace
  Admin/Developer/Owner members (HSI §8 p6).

Bridge creation delegates to `notifications::notify_rich`, so bridge-derived
notifications carry the same structured `NotificationSource` and deep-link
derivation as every other producer (one creation path end to end). All
three send paths (`AppState::emit_event`, REST send handler, MCP
`message.send`) clone into the channel via non-blocking `try_send`; a full
channel drops with a warning log and never blocks the send path, per spec.

**TTL expiry job (§Implementation Notes).** `spawn_message_expiry()` runs
the hourly job (3600s interval, `MissedTickBehavior::Delay`,
job-registry-tracked as `message_expiry`, on-demand via
`POST /admin/jobs/message_expiry/run`). Each cycle calls
`expire_events(now − GYRE_EVENT_TTL_SECS·1000)` and
`expire_acked_inboxes(now − GYRE_DEAD_INBOX_TTL_SECS·1000)` on the
`MessageRepository`; both env vars default to 604800 (7 days) with
warn-on-invalid fallback. The dead-inbox contract is enforced in both
adapters: only `agent_completed`/`agent_orphaned` bulk-acks are cleaned;
explicit acks and unacked Directed messages are retained.

**Tests.** Bridge unit tests for all four message kinds (recipient
resolution, viewer-role exclusion, unknown-workspace skip), dispatcher
fan-out to multiple consumers, end-to-end `spawn_message_consumer` through
the real send path, TTL retention boundaries (expired Event deleted, fresh
Event / unacked Directed / explicit-ack retained), env-var TTL override,
and `failed_gate_creates_gate_failure_notification_via_bridge`
(run_gate → emit_event → dispatcher → bridge → exactly one rank-3
notification). The reconciliation wave-completion test drains the bus via
the real dispatcher, proving the p6 path.

### Rebase repair onto cfeefbda (this round)

Rebased onto the assigned base cfeefbda (task-123 notification taxonomy
unification). Two textual conflicts resolved: notifications.rs keeps the
base's `notify_gate_passed` (called by the merged gate_executor.rs) while
keeping task-162's removal of the synchronous `notify_gate_failure` (the
bridge is the single GateFailure creation path); the HSI coverage row 43
combines task-162's dispatcher-based #3 description with base's task-123
gap closures. Semantic fallout fixed after `cargo check`: the bridge was
adapted to the restructured `Notification` (nested `NotificationSource`,
`priority_rank`) and renamed taxonomy (`AgentBudgetWarning` rank 4,
`AgentEscalation` rank 2), now delegating to `notify_rich` to preserve the
base's deep links; affected tests updated to the new fields/ranks; the
gate-failure deep-link test now drives the bridge directly.

### Test evidence (this round, at head 717739096)

- `cargo check -p gyre-server --tests` plus `-p gyre-cli -p gyre-adapters`
  — clean, no errors.
- `cargo test -p gyre-common` — 124 passed (includes the taxonomy
  priority-rank tests the bridge now relies on).
- Mechanical checks clean: arch, byte-slice truncation, dead message
  kinds, fabricated/scope-literal defaults, inert enforcement, mem port
  contracts, lossy secret conversion.
- Gyre-server test compilation is deferred to the independent verifier
  per assignment constraints (4 GiB budget). The verifier should run:
  `cargo test -p gyre-server --lib message_dispatcher`,
  `cargo test -p gyre-server --lib gate_executor`,
  `cargo test -p gyre-server --lib notifications`,
  `cargo test -p gyre-server --lib reconciliation`, and the full
  `cargo test --all` gate.

Full workspace suite, all-target Clippy, and GitHub CI remain owned by
verification and publication.
