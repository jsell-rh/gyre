---
title: "Message bus — wire MessageConsumer dispatcher and event TTL expiry job"
spec_ref: "message-bus.md §Relationship to Notifications"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "message-bus.md §Relationship to Notifications"
  - "message-bus.md §Implementation Notes"
commits: ["6b72222c91925c4ea3b2ab78f4a32b8c625a11ff", "f5346ff06385571ace9a54d203d62f398a8f6211", "244a98cccc76a7d5fcc2b4a34f0b70b4f9ddfc79", "cb865b793c9165c073e0ba41bf30a44c07d1e19c"]
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

Note for reviewers: two repo checks fail identically at the pristine base
commit 8c2d1775 and are unrelated to this task —
`check-notification-test-coverage.sh` (its awk uses gawk-only 3-arg `match()`;
mawk on this host exits 2 with a syntax error before any file is scanned) and
`check-task-commit-attribution.sh` (a task-210 commit `a781ede2` on main is
missing from task-210's frontmatter; task-162's attribution is complete).
