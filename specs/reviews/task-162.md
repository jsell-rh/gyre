# Review — task-162 (Message bus — wire MessageConsumer dispatcher and event TTL expiry job)

Spec: `specs/system/message-bus.md` §Relationship to Notifications ("The notification system becomes a **consumer** of the message bus… the server's message-send path (after storing the message) clones it into a bounded `tokio::sync::mpsc` channel. A background task drains the channel and dispatches to registered consumers") and §Implementation Notes (Event-tier TTL, `GYRE_EVENT_TTL_SECS`, default 7 days; background job calls `expire_events()`; dead-inbox cleanup `GYRE_DEAD_INBOX_TTL_SECS`).
Candidate under review: `d3fdbf21` (base `48b3d47e`). Server source byte-identical to the round-8 verified head `3c345bf9` (`git diff --name-only 3c345bf9..d3fdbf21` → only `specs/tasks/task-162.md`).
Verdict: **complete**.

## Round 9 (independent review of the executor-restored contract head)

Probes run in this checkout at `d3fdbf21` (evidence: `/tmp/stage/review-evidence/task-162-round9.txt` and per-command logs alongside):

- `cargo check -p gyre-server` → **exit 0** (7m01s, web assets built by build.rs).
- `cargo check -p gyre-server --lib --tests` → **exit 0** (type-checks all new `#[cfg(test)]` code).
- `cargo fmt -p gyre-server -- --check` → **exit 0**.
- 19 mechanical invariant scripts (`check-arch.sh`, `check-task-commit-attribution.sh`, `check-dead-message-kinds.sh`, `check-abac-*`, `check-mcp-write-tools.sh`, `check-mem-port-contracts.sh`, `check-in-memory-state-stores.sh`, etc.) → all **exit 0**. No `scripts/` or `*-exemptions.txt` file touched by the diff.

### Dispatcher (§Relationship to Notifications) — verified genuine

- `spawn_message_consumer()` (`message_dispatcher.rs:86`) takes single-owner possession of the 256-capacity mpsc receiver via `AppState::take_message_dispatch_rx()` (second call: logged no-op) and `tokio::spawn`s `MessageDispatcher::run`, which drains the channel and dispatches every message to `Vec<Arc<dyn MessageConsumer>>`. Wired in `main.rs` after `build_state`, before `build_router`.
- All three send paths — `AppState::emit_event` (lib.rs), REST `send_message` (api/messages.rs), MCP `message.send` (mcp.rs) — store the message **before** cloning into the dispatch channel (spec's "after storing the message"), with one non-blocking `try_send`; a full channel logs a warning and drops (spec: "consumer drops do not affect delivery guarantees"). The base's no-op drain loop is gone from both `build_state` and `mem::test_state_inner`.
- `NotificationBridge` is the registered consumer. GateFailure → rank-3 for the MR author agent's spawning user; BudgetWarning → rank-4 AgentBudgetWarning for Admin/Developer/Owner; AgentError → rank-2 AgentEscalation for the spawning user; ReconciliationCompleted → rank-6 MetaSpecDrift for Admin/Developer/Owner (HSI §8 p6). Creation delegates to the shared `notify_rich`, so bridge notifications carry the same structured source/deep link as every producer. Unresolvable workspace/tenant/spawning-user → skip + log (no fabricated scope identity).
- The GateFailure dedupe premise is real, not invented: `gate_executor::run_gate` emits the Workspace-targeted event, `ralph_loop::on_gates_failed` emits an Agent-directed copy of the same failure; the bridge consumes only Workspace/Broadcast-targeted events (regression-tested — `notification_bridge_ignores_agent_directed_gate_failure_copy`).
- Single creation path for p3 holds: the synchronous `notify_gate_failure` helper is deleted with zero remaining callers, the gate-executor inline call is removed, and the only other `NotificationType::GateFailure` constructions are test seeds. The HSI §8 p3 amendment (spec lifecycle amendment table) documents the rerouting; coverage rows 12/15 were updated to `implemented` with notes that match the shipped behavior.
- End-to-end anchoring: `failed_gate_creates_gate_failure_notification_via_bridge` (gate_executor tests) runs a real failing TestCommand gate through `run_gate` with the real dispatcher spawned and asserts exactly one rank-3 notification; `spawn_message_consumer_end_to_end` goes through the real `emit_event` path and asserts the p6 notification plus the single-dispatcher guard; the reconciliation wave test spawns the dispatcher and polls for the async p6 delivery.

### Event TTL expiry job (§Implementation Notes) — verified genuine

- `spawn_message_expiry()` (wired in `main.rs`) runs hourly (3600 s, `MissedTickBehavior::Delay`, immediate first tick) calling `run_message_expiry`: `messages.expire_events(now_ms − GYRE_EVENT_TTL_SECS·1000)` and `messages.expire_acked_inboxes(now_ms − GYRE_DEAD_INBOX_TTL_SECS·1000)`. Defaults 604 800 each; `env_u64` warns and falls back on invalid values. Registered as `message_expiry` in the admin job registry (on-demand `POST /api/v1/admin/jobs/message_expiry/run` via the existing `registry.trigger` route), with `mark_scheduled`/`record_cycle` matching the module-loop job pattern — no double execution (`register()` only adds the handler; the dispatcher-side loop is the sole scheduler).
- The port methods and both SQL adapters pre-exist on base (verified at `48b3d47e`); the mem adapter was extended to mirror the SQLite `ack_reason` contract ("explicit" vs `agent_completed`/`agent_orphaned`), and `check-mem-port-contracts.sh` passes.
- TTL tests assert real retention boundaries: expired Event-tier deleted, fresh retained, unacked Directed retained, `agent_completed` dead inbox deleted, explicit-ack retained; a 1 s env override deletes only the >1 s-old event. Both env-mutating tests serialize on a module mutex.

### Test-run deferral (4 GiB budget rule)

Server test compilation/execution was not run in this round. `cargo check -p gyre-server --lib --tests` (exit 0) covers test-code type checking; the verifier must execute: `cargo test -p gyre-server --lib message_dispatcher`, `gate_executor`, `reconciliation`, `notifications`, then the full `cargo test --all` acceptance gate.

### Non-blocking observations

- `env_u64(...) * 1000` is an unchecked u64 multiply — an absurd env value (≥ ~1.8e16 s) would overflow (debug panic / release wrap). Defaults and invalid-parse fallbacks are correct; not reachable with any sane configuration.
- `MessageKind::BudgetWarning`/`AgentError` still have no production emitter (pre-existing entries in `dead-message-kind-exemptions.txt`, unchanged by this task), so those two bridge handlers are currently dormant; budget notifications still arrive via the synchronous `api/budget.rs` path — no user-facing regression, no duplication. Closing that emitter gap is separate work.
- The bridge's `Broadcast` arm for GateFailure is unreachable in practice (Broadcast messages carry `workspace_id: None` → early return). Harmless.

Findings: none.

— Review round 9, 2026-11-12
