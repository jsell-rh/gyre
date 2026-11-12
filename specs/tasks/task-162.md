---
title: "Message bus — wire MessageConsumer dispatcher and event TTL expiry job"
spec_ref: "message-bus.md §Relationship to Notifications"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "message-bus.md §Relationship to Notifications"
  - "message-bus.md §Implementation Notes"
commits: ["dd716a986330b7509a7e1e93469d94fdb457628f", "6d20863c02e07c0a51873fdbb735c61bef80af68", "5192ff750f58b79d2553a317e9d9b802292ad3e1", "51221dc02d235b9737d645d541650e771d3e89e5"]
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

### Dispatcher (§Relationship to Notifications)

`spawn_message_consumer()` (crates/gyre-server/src/message_dispatcher.rs) takes
ownership of the 256-capacity mpsc receiver via `AppState::take_message_dispatch_rx()`
(single-owner — a second call is a logged no-op) and spawns the background drain
task. `MessageDispatcher` holds `Vec<Arc<dyn MessageConsumer>>` and dispatches
every drained message to all registered consumers. The first registered consumer is
`NotificationBridge`, which derives Inbox notifications from Event-tier messages
(ranks per the unified user-management taxonomy):

- `GateFailure` → rank-3 `GateFailure` for the MR author agent's spawning user,
  resolved through the MR → author agent → spawner chain; tenant resolved from
  the workspace record, never fabricated. The gate executor emits the
  `GateFailure` event and no longer creates the Inbox item synchronously —
  single creation path via the bridge (HSI §8 p3 amendment). The bridge
  consumes only Workspace/Broadcast-targeted GateFailure events — two
  production emitters publish GateFailure for the same failed gate
  (`gate_executor::run_gate`, Workspace-targeted; `ralph_loop::on_gates_failed`,
  Agent-targeted copy for the agent's next session), and the Agent-directed
  copy is the agent's inbox record, not a human-notification trigger
  (review round-1 repair, commit dd716a986).
- `BudgetWarning` → rank-4 `AgentBudgetWarning` for workspace
  Admin/Developer/Owner members.
- `AgentError` → rank-2 `AgentEscalation` for the failing agent's spawning user.
- `ReconciliationCompleted` → rank-6 `MetaSpecDrift` for workspace
  Admin/Developer/Owner members (HSI §8 p6).

Bridge creation delegates to `notifications::notify_rich`, so bridge-derived
notifications carry the same structured `NotificationSource` and deep-link
derivation as every other producer (one creation path end to end). All three
send paths (`AppState::emit_event`, REST send handler, MCP `message.send`)
clone into the channel via non-blocking `try_send`; a full channel drops with
a warning log at all three call sites and never blocks the send path, per
spec.

### Event TTL expiry job (§Implementation Notes)

`spawn_message_expiry()` (message_dispatcher.rs) starts the hourly job
(3600s interval, `MissedTickBehavior::Delay`). Each cycle runs
`run_message_expiry`: `expire_events(now_ms − GYRE_EVENT_TTL_SECS·1000)` for
Event-tier cleanup plus `expire_acked_inboxes(now_ms −
GYRE_DEAD_INBOX_TTL_SECS·1000)` for completed/orphaned agent inbox cleanup.
Both env vars default to 604800 (7 days); invalid values warn and fall back.
Implemented by the SQLite, PostgreSQL, and mem `MessageRepository` adapters.
Registered as `message_expiry` in the admin job registry
(`POST /admin/jobs/message_expiry/run`) and wired in `main.rs` alongside the
other background jobs.

### Tests

- `spawn_message_consumer_end_to_end` — emits a `ReconciliationCompleted`
  event through the real send path and asserts the rank-6 notification
  arrives via the background dispatcher; also asserts the single-owner
  receiver (second spawn is a no-op).
- `notification_bridge_creates_*` — GateFailure (rank-3, MR-author chain),
  BudgetWarning (rank-4, role fan-out), AgentError (rank-2, spawner),
  MetaSpecDrift (rank-6, role fan-out; Viewer excluded), plus
  unknown-workspace skip.
- `notification_bridge_ignores_agent_directed_gate_failure_copy` — regression
  for the two-emitter GateFailure dedupe.
- `dispatcher_dispatches_to_all_consumers` — channel drain fan-out.
- `message_expiry_deletes_old_events_and_dead_inboxes` — retention boundaries
  (unacked Directed and explicit-ack retained) and
  `message_expiry_env_ttl_respected` (1s TTL deletes only >1s-old events).
- Adapter-level expiry tests in `gyre-adapters` (sqlite message.rs).

`cargo check -p gyre-server` passes on the rebased head (this sandbox
cannot compile the gyre-server test binary within the memory budget). The
verifier must run: `cargo test -p gyre-server --lib message_dispatcher` and
`cargo test --all`.
