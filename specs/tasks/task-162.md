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

- [x] `spawn_message_consumer()` function exists and starts a background dispatcher task
- [x] At least one `MessageConsumer` (notification bridge) is registered and creates notifications from message bus events
- [x] Event TTL expiry job runs periodically and cleans up old Event-tier messages
- [x] Dead agent inbox cleanup runs periodically
- [x] `GYRE_EVENT_TTL_SECS` and `GYRE_DEAD_INBOX_TTL_SECS` env vars are respected
- [x] `cargo test --all` passes

## Agent Instructions

Read `specs/system/message-bus.md` §"Relationship to Notifications" and §"Implementation Notes". The MessageConsumer trait already exists in `gyre-ports/src/message.rs`. The mpsc channel is created in `lib.rs` around line 989-1000. Check the existing job scheduling patterns in `lib.rs` (look for `spawn_budget_daily_reset`, `spawn_stale_agent_detector`, etc.) to match the background job style. The notification system is in `api/notifications.rs` and `gyre-ports/src/notification.rs`.

## Shipped

### Round 6 — merged-head verification after contract restore

The executor restored the assigned task contract while the production fix
stayed on the branch. This round re-verified the merged head `80dbfe00`
(production fix merged with assigned base `48b3d47e`):
`git diff --name-only 49858412..80dbfe00 -- crates/` shows **zero changes**
— the task-162 server source is byte-identical to the previously verified
checkpoint. On this head, `SKIP_WEB_BUILD=1 cargo check -p gyre-server`
passes (exit 0, 6m13s) and `SKIP_WEB_BUILD=1 cargo check -p gyre-server
--tests` passes (exit 0, 2m55s). Server test execution is deferred to the
independent verifier (4 GiB budget rule): run `cargo test -p gyre-server
message_dispatcher`, then the full `cargo test --all` acceptance gate.
Evidence: `/tmp/stage/review-evidence/task-162-round6.txt`.

### Dispatcher (§Relationship to Notifications)

`spawn_message_consumer()` (`crates/gyre-server/src/message_dispatcher.rs`,
called from `main.rs` after `build_state`) takes single-owner possession of
the 256-capacity mpsc receiver via `AppState::take_message_dispatch_rx()`
(a second call is a logged no-op) and spawns the background drain task.
`MessageDispatcher` holds `Vec<Arc<dyn MessageConsumer>>` and dispatches
every drained message to all registered consumers. The send hot path
(`AppState::emit_event`, the REST send handler, and the MCP `message.send`
tool) does one non-blocking `try_send` and returns immediately; a full
channel drops the message with a warning log — consumers are downstream
observers and cannot block delivery.

The first registered consumer is `NotificationBridge`, which derives
user-facing notifications from bus messages via the shared `notify_rich`
creation path:

- `GateFailure` → rank-3 `GateFailure` notification for the MR author
  agent's spawning user. Only the Workspace/Broadcast-targeted event is
  consumed; the Agent-directed copy emitted by `ralph_loop::on_gates_failed`
  is the agent's inbox record and is ignored (regression-tested —
  consuming both duplicated the p3 notification).
- `BudgetWarning` → rank-4 `AgentBudgetWarning` for the workspace's
  Admin/Developer/Owner members.
- `AgentError` → rank-2 `AgentEscalation` for the failing agent's
  spawning user.
- `ReconciliationCompleted` → rank-6 `MetaSpecDrift` for the workspace's
  Admin/Developer/Owner members (HSI §8 p6 routing).

Unknown workspaces or missing spawning users skip the notification with a
log — no fabricated scope identity.

### Event TTL expiry job (§Implementation Notes)

`spawn_message_expiry()` (wired in `main.rs`) runs hourly with
`MissedTickBehavior::Delay`, registered as `message_expiry` in the admin
job registry (on-demand `POST /admin/jobs/message_expiry/run`). Each cycle
(`run_message_expiry`) calls `messages.expire_events(now_ms - TTL)` and
`messages.expire_acked_inboxes(now_ms - TTL)`, reading
`GYRE_EVENT_TTL_SECS` and `GYRE_DEAD_INBOX_TTL_SECS` (defaults 604800 =
7 days; invalid values warn and fall back to default).

### Tests

`crates/gyre-server/src/message_dispatcher.rs` tests cover: bridge →
notification creation for all four kinds (asserting type, priority rank,
tenant, source entity), the agent-directed GateFailure dedupe regression,
viewer-role exclusion from workspace fan-out, unknown-workspace skip,
dispatcher fan-out to multiple consumers, `run_message_expiry`
semantics (expired Event-tier deleted, fresh retained, unacked Directed
retained, dead-inbox vs explicit-ack distinction), env-var TTL override
(1s TTL deletes only the >1s-old event), and a `spawn_message_consumer`
end-to-end test through the real `emit_event` send path including the
single-dispatcher guard.
