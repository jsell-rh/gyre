---
title: "Message bus — wire MessageConsumer dispatcher and event TTL expiry job"
spec_ref: "message-bus.md §Relationship to Notifications"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "message-bus.md §Relationship to Notifications"
  - "message-bus.md §Implementation Notes"
commits: ["244a98cccc76a7d5fcc2b4a34f0b70b4f9ddfc79"]
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

**MessageConsumer dispatcher (§Relationship to Notifications).** `crates/gyre-server/src/message_dispatcher.rs`: `MessageDispatcher` holds `Vec<Arc<dyn MessageConsumer>>`; `spawn_message_consumer(state)` takes exclusive ownership of the AppState mpsc receiver (`take_message_dispatch_rx`, single-owner — second spawn is a logged no-op) and spawns the background drain task. The first consumer, `NotificationBridge`, creates Inbox notifications from bus events: `GateFailure`→p3 for the MR author agent's spawning user, `BudgetWarning`→p7 for workspace Admin/Developer/Owner members, `AgentError`→p5 AgentEscalation for the failing agent's spawning user, `ReconciliationCompleted`→p6 MetaSpecDrift for Admin/Developer/Owner members. Scope identities are resolved by lookup (workspace tenant, MR author, agent spawned_by) — no fabricated `"default"` scope on lookup failure; unresolvable messages are skipped with a warning. All three send paths (`AppState::emit_event`, REST `POST /workspaces/:id/messages`, MCP `message.send`) clone each stored message into the bounded channel; backpressure drops are logged, never block the send path. `main.rs` spawns the dispatcher at startup.

The gate executor's old direct `notify_gate_failure` call was removed — the bridge is now the single p3 creation path, per the HSI §8 p3 amendment ("Via MessageConsumer consuming GateFailure events") recorded in `specs/system/human-system-interface.md`. The now-dead `notify_gate_failure` helper in `notifications.rs` was deleted.

**Event TTL expiry job (§Implementation Notes).** `spawn_message_expiry(state)` runs hourly (3600s interval, Delay missed-tick behavior) and calls `run_message_expiry`: `expire_events(now_ms − GYRE_EVENT_TTL_SECS·1000)` plus `expire_acked_inboxes(now_ms − GYRE_DEAD_INBOX_TTL_SECS·1000)` on `MessageRepository` — real DELETE queries in both SQLite and Postgres adapters. Defaults: 604800 (7 days) each; invalid env values warn and fall back. The job is registered as `message_expiry` in the admin job registry (`POST /admin/jobs/message_expiry/run` triggers on demand) and wired in `main.rs`.

**Test evidence.** `cargo test -p gyre-server --lib message_dispatcher` — 9 passed: dispatcher fan-out to multiple consumers, per-kind bridge notification creation (GateFailure p3/BudgetWarning p7/AgentError p5/ReconciliationCompleted p6 incl. Viewer-role exclusion), unknown-workspace skip, end-to-end `spawn_message_consumer` (emit_event → channel → bridge → notification; second spawn rejected), TTL expiry boundaries (expired Event deleted, fresh Event retained, unacked Directed retained, explicit-ack retained, dead inbox deleted), and env-var TTL respect (1s TTL deletes only >1s-old events). `cargo test -p gyre-server --lib -- gate notification` — 80 passed. New end-to-end test `gate_executor::tests::failed_gate_creates_gate_failure_notification_via_bridge` pins the rewired gate-failure chain; mutation-verified (disabling the `GateFailure` emission makes it fail, restoring passes).
