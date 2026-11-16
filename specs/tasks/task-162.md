---
title: "Message bus — wire MessageConsumer dispatcher and event TTL expiry job"
spec_ref: "message-bus.md §Relationship to Notifications"
depends_on: []
progress: complete
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

This round was a merge repair: PR #810 (head da50b3ab, previously approved task-162 implementation) conflicted with current main 6082be2f (task-165 supply-chain work). The single conflicted path (`specs/coverage/SUMMARY.md` TOTAL row) was resolved by recomputing the totals as the sums of the merged per-spec rows, preserving both sides' coverage gains (message-bus.md 100%, supply-chain.md 78%). The task-162 implementation is byte-identical to the retained candidate — `git diff da50b3ab HEAD` over `message_dispatcher.rs`, `main.rs`, `lib.rs`, `api/messages.rs`, `mcp.rs` is empty, and main's changes touched no message/notification types (`gyre-common`/`gyre-ports` diff between the sides is empty), so there is no semantic interaction.

### Behavior (retained implementation, verified intact after merge)

- `spawn_message_consumer()` (crates/gyre-server/src/message_dispatcher.rs) takes single ownership of the bounded (256) dispatch channel receiver via `AppState::take_message_dispatch_rx` and spawns the background drain task; a second spawn is a logged no-op. Wired in `main.rs` after `build_state`.
- `MessageDispatcher` holds `Vec<Arc<dyn MessageConsumer>>` and dispatches every drained message off the send hot path; all three send paths (`emit_event`, `api/messages.rs`, `mcp.rs message.send`) clone into the channel; full-channel drops are logged warnings.
- `NotificationBridge` is the registered consumer: GateFailure → rank-3 GateFailure for the MR author's spawning user; spec-policy violations (`gate_type "spec_enforcement"`) → `notify_spec_policy_violation`; AgentError → rank-2 AgentEscalation for the spawning user; ReconciliationCompleted → rank-6 MetaSpecDrift for Admin/Developer/Owner members. BudgetWarning is deliberately not bridged — the synchronous budget-check path owns that notification (single-path repair from review round 2).
- `spawn_message_expiry()` runs hourly (`MissedTickBehavior::Delay`), registered as `message_expiry` in the admin job registry. Each cycle calls `expire_events(now − GYRE_EVENT_TTL_SECS·1000)` and `expire_acked_inboxes(now − GYRE_DEAD_INBOX_TTL_SECS·1000)`; both env vars default to 604800 (7 days), warn-and-fall-back on invalid values. Wired in `main.rs`.

### Test evidence

- `cargo check -p gyre-server --lib` and `cargo check -p gyre-server --lib --tests` pass in this sandbox (exit 0) on the merged head.
- Server test compilation is blocked by this sandbox's 4 GiB build wrapper (verbatim error recorded in `/tmp/stage/review-evidence/evidence.md`); the verifier must run `cargo test -p gyre-server --lib message_dispatcher` (dispatcher/bridge/TTL unit tests incl. `message_expiry_env_ttl_respected` for the env-var criterion), `failing_gate_notifies_author_with_gate_result_payload`, the `require_spec_ref`/`require_signed_approval` merge-processor tests, and the merge_processor/gate_executor/budget suites.
- Mechanical checks pass here: `check-arch.sh`, `check-dead-message-kinds.sh`, `check-inert-enforcement.sh`, `check-abac-route-registry.sh`, `check-mcp-write-tools.sh`, `check-migration-versions.sh`, `check-mem-port-contracts.sh`, `check-notification-toggle-coverage.sh`, `cargo fmt --check`. `check-notification-test-coverage.sh` and `check-event-emission-coverage.sh` cannot run under this sandbox's mawk (lacks gawk 3-arg `match()`); the scripts are untouched by this merge and pass in CI with gawk.
