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

Production implementation shipped on the branch commits listed in the
frontmatter and re-merged onto assignment base `e5df7847` by merge commit
`35b7094b` (this round resolved the rebase; earlier rounds shipped and
verified the behavior — see coverage rows
`specs/coverage/system/message-bus.md` §12/§15, both `implemented`).

### Dispatcher (§Relationship to Notifications)

`spawn_message_consumer()` (`crates/gyre-server/src/message_dispatcher.rs`,
wired from `main.rs` after `build_state`) takes single-owner possession of
the 256-capacity mpsc receiver via `AppState::take_message_dispatch_rx()`
(a second call is a logged no-op) and spawns the background drain task.
`MessageDispatcher` holds `Vec<Arc<dyn MessageConsumer>>` and dispatches
every drained message to all registered consumers. Every send path
(`AppState::emit_event`, the REST send handler, the MCP `message.send`
tool) does one non-blocking `try_send` and returns immediately; a full
channel drops the message with a warning log — consumers are downstream
observers and cannot block delivery.

The first registered consumer is `NotificationBridge`:

- `GateFailure` → rank-3 notification for the MR author agent's spawning
  user. The bridge consumes only the Workspace/Broadcast-targeted event
  (the Agent-directed copy from `ralph_loop::on_gates_failed` is the
  agent's inbox record, not a trigger — dedupe regression test pins this)
  and forwards the full GateResult payload to the shared
  `notifications::notify_gate_failure` helper. After this round's rebase
  onto `e5df7847`, that helper is the base's richer version, so
  bus-derived notifications carry the full GateResult context (gate
  name/type, failure output, spec_ref, attempt, gate agent, MR-label
  title, tenant resolved from the workspace) and fan out to delivery
  channels exactly like synchronous producers — one creation path.
- `BudgetWarning` → rank-4 `AgentBudgetWarning` for workspace
  Admin/Developer/Owner members.
- `AgentError` → rank-2 `AgentEscalation` for the failing agent's
  spawning user.
- `ReconciliationCompleted` → rank-6 `MetaSpecDrift` for
  Admin/Developer/Owner members (`emit_reconciliation_completed` emits the
  event only; the bridge creates the notifications, HSI §8 p6).

### Event TTL expiry job (§Implementation Notes)

`spawn_message_expiry()` (message_dispatcher.rs, wired from `main.rs`)
runs hourly (`MissedTickBehavior::Delay`) and calls
`run_message_expiry`: `expire_events(now_ms − GYRE_EVENT_TTL_SECS·1000)`
plus `expire_acked_inboxes(now_ms − GYRE_DEAD_INBOX_TTL_SECS·1000)`;
both default to 604800 s (7 days) with warn-on-invalid env fallback. Also
registered as the `"message_expiry"` admin job.

### Test evidence

Focused server tests live in `message_dispatcher.rs` (dispatcher fan-out,
bridge per-kind creation, Agent-copy dedupe, unknown-workspace skip, TTL
boundary retention, env TTL respect, `spawn_message_consumer`
end-to-end), `gate_executor.rs`
(`failed_gate_creates_gate_failure_notification_via_bridge` — the full
run_gate → event → dispatcher → bridge chain), `notifications.rs`
(GateResult payload contract incl. skip-on-unresolvable-workspace), and
`reconciliation.rs` (wave completion → bridge-created MetaSpecDrift).

This sandbox's build wrapper blocks gyre-server test compilation (4 GiB
budget rule), so server test execution is deferred to the independent
verifier; required probes, exit codes, and the mawk limitation for
`check-notification-test-coverage.sh` are recorded in
`/tmp/stage/review-evidence/task-162-rebase-merge.md`. Verified locally at
merge head `35b7094b`: `cargo check -p gyre-server` (exit 0),
`cargo check -p gyre-server --lib --tests` (exit 0 — all test code
type-checks against the merged base APIs), `cargo fmt -p gyre-server
-- --check` (exit 0), `scripts/check-arch.sh`, `check-dead-message-kinds.sh`,
`check-inert-enforcement.sh` (all exit 0), and
`scripts/update-coverage-summary.sh` regenerated `specs/coverage/SUMMARY.md`
from the merged per-spec files (message-bus.md 100%, no `not-started`).
