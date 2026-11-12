---
title: "Message bus — wire MessageConsumer dispatcher and event TTL expiry job"
spec_ref: "message-bus.md §Relationship to Notifications"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "message-bus.md §Relationship to Notifications"
  - "message-bus.md §Implementation Notes"
commits: ["51221dc02d235b9737d645d541650e771d3e89e5", "5192ff750f58b79d2553a317e9d9b802292ad3e1", "6d20863c02e07c0a51873fdbb735c61bef80af68", "dd716a986330b7509a7e1e93469d94fdb457628f"]
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
a warning log at all three call sites (review round-1 repair extended the
warn match from `emit_event` to the REST and MCP handlers) and never blocks
the send path, per spec.

### TTL expiry job (§Implementation Notes)

`spawn_message_expiry()` runs the hourly job (3600s interval,
`MissedTickBehavior::Delay`, job-registry-tracked as `message_expiry`,
on-demand via `POST /admin/jobs/message_expiry/run`). Each cycle calls
`expire_events(now − GYRE_EVENT_TTL_SECS·1000)` and
`expire_acked_inboxes(now − GYRE_DEAD_INBOX_TTL_SECS·1000)` on the
`MessageRepository`; both env vars default to 604800 (7 days) with
warn-on-invalid fallback. The dead-inbox contract is enforced in both
adapters: only `agent_completed`/`agent_orphaned` bulk-acks are cleaned;
explicit acks and unacked Directed messages are retained.

### Tests

Bridge unit tests for all four message kinds (recipient resolution,
viewer-role exclusion, unknown-workspace skip), the Agent-directed
GateFailure dedupe regression (Workspace event + identical Agent copy →
exactly one rank-3 notification), dispatcher fan-out to multiple consumers,
end-to-end `spawn_message_consumer` through the real send path, TTL retention
boundaries (expired Event deleted, fresh Event / unacked Directed /
explicit-ack retained), env-var TTL override, and
`failed_gate_creates_gate_failure_notification_via_bridge` (run_gate →
emit_event → dispatcher → bridge → exactly one rank-3 notification). The
reconciliation wave-completion test drains the bus via the real dispatcher,
proving the p6 path.

### Verification status

Fresh probes at dd716a986: `cargo check -p gyre-server` and
`cargo check -p gyre-server --tests` clean, `cargo fmt -p gyre-server --
--check` clean, arch / dead-message-kinds / mcp-write-tools /
abac-route-registry checks pass. Gyre-server test compilation is deferred
to the independent verifier (4 GiB budget); the verifier must execute:
`cargo test -p gyre-server --lib message_dispatcher` (includes the new
`notification_bridge_ignores_agent_directed_gate_failure_copy` regression),
`--lib gate_executor`, `--lib notifications`, `--lib ralph_loop`, `--lib
reconciliation`, and the full `cargo test --all` gate.
