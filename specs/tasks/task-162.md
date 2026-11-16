---
title: "Message bus — wire MessageConsumer dispatcher and event TTL expiry job"
spec_ref: "message-bus.md §Relationship to Notifications"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "message-bus.md §Relationship to Notifications"
  - "message-bus.md §Implementation Notes"
commits: ["55ac5b5279b16786353092bd0e42724f01d12b0b", "dd716a986330b7509a7e1e93469d94fdb457628f", "6d20863c02e07c0a51873fdbb735c61bef80af68", "5192ff750f58b79d2553a317e9d9b802292ad3e1", "51221dc02d235b9737d645d541650e771d3e89e5"]
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

Implements message-bus.md §Relationship to Notifications and §Implementation Notes, plus the review round-2 repairs on the notification bridge (this branch continues the checkpointed task-162 source; the dispatcher and TTL job shipped in the earlier branch commits `dd716a98`/`6d20863c`/`5192ff75` and are intact at HEAD).

### MessageConsumer dispatcher (§Relationship to Notifications)

- `spawn_message_consumer()` (crates/gyre-server/src/message_dispatcher.rs) takes single ownership of the bounded (256) dispatch channel receiver via `AppState::take_message_dispatch_rx` and spawns the background drain task; a second spawn is a logged no-op. Wired in `main.rs` after `build_state`.
- `MessageDispatcher` holds `Vec<Arc<dyn MessageConsumer>>` (the `MessageConsumer` trait lives in `gyre-ports/src/message.rs`) and dispatches every drained message off the send hot path. All three send paths (`emit_event`, `api/messages.rs`, `mcp.rs message.send`) clone into the channel; full-channel consumer drops are logged warnings.
- `NotificationBridge` is the registered consumer: GateFailure → rank-3 GateFailure for the MR author's spawning user; spec-policy violations (`gate_type "spec_enforcement"`) → `notify_spec_policy_violation` with their own title; AgentError → rank-2 AgentEscalation for the spawning user; ReconciliationCompleted → rank-6 MetaSpecDrift for Admin/Developer/Owner members. BudgetWarning is deliberately NOT notified by the bridge — HSI §8 p7 creates budget warnings synchronously in the budget check path (spawning user + workspace Admin per user-management.md); the former bridge fan-out duplicated and over-broadened delivery (review round-2 repair).

### Event TTL expiry job (§Implementation Notes)

- `spawn_message_expiry()` runs hourly (3600s, `MissedTickBehavior::Delay`), registered as `message_expiry` in the admin job registry (`POST /admin/jobs/message_expiry/run` triggers on demand). Each cycle calls `expire_events(now − GYRE_EVENT_TTL_SECS·1000)` and `expire_acked_inboxes(now − GYRE_DEAD_INBOX_TTL_SECS·1000)`; both env vars default to 604800 (7 days) and warn-and-fall-back on invalid values. Wired in `main.rs`.

### Review round-2 repairs in commit `55ac5b52`

1. `failing_gate_notifies_author_with_gate_result_payload` (gate_executor.rs) now spawns `spawn_message_consumer` so the bus is drained — the notification arrives via the bridge (the production wiring) and the test polls with the existing `eventually` helper.
2. Spec-policy merge violations no longer double-notify: `merge_processor::report_spec_policy_violation` emits the GateFailure event only, and the bridge routes `gate_type "spec_enforcement"` events to `notify_spec_policy_violation` — exactly one notification per violation, with the original "Spec policy blocked merge…" title. Both merge-processor spec-policy tests spawn the dispatcher and poll (`eventually_bridge_notification`).
3. The bridge's budget fan-out is removed; replaced by a regression test asserting BudgetWarning events create no bridge notifications (the synchronous budget-check path owns p7).

### Test evidence

- `cargo check -p gyre-server --lib` and `cargo check -p gyre-server --lib --tests` pass in this sandbox (exit 0). Full `cargo test -p gyre-server` compilation exceeds this sandbox's 4 GiB build wrapper and is deferred to the independent verifier; the exact required probes, expected outcomes, and the wrapper's verbatim error are recorded in `/tmp/stage/review-evidence/probes.txt` (message_dispatcher, failing_gate_notifies_author_with_gate_result_payload, both require_spec_ref/require_signed_approval spec-policy tests, plus the merge_processor, gate_executor, and budget suites).
- Mechanical checks pass here: `check-arch.sh`, `check-dead-message-kinds.sh`, `check-inert-enforcement.sh`, `check-notification-test-coverage.sh`, `check-notification-toggle-coverage.sh`, `check-event-emission-coverage.sh`, `cargo fmt --check`.
- Dispatcher/bridge/TTL unit tests (in message_dispatcher.rs, run by the verifier): `dispatcher_dispatches_to_all_consumers`, `spawn_message_consumer_end_to_end` (real send path → dispatcher → bridge → notification), `notification_bridge_creates_gate_failure_notification`, `notification_bridge_routes_spec_policy_violation` (new), `notification_bridge_ignores_agent_directed_gate_failure_copy`, `notification_bridge_does_not_create_budget_warning_notifications` (new), `notification_bridge_creates_agent_error_notification`, `notification_bridge_creates_meta_spec_drift_notification`, `notification_bridge_skips_unknown_workspace`, `message_expiry_deletes_old_events_and_dead_inboxes`, `message_expiry_env_ttl_respected`.
