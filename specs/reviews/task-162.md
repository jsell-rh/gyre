# Review — task-162 (Message bus — wire MessageConsumer dispatcher and event TTL expiry job)

Spec: `specs/system/message-bus.md` §Relationship to Notifications ("The notification system becomes a consumer of the message bus... the server's message-send path (after storing the message) clones it into a bounded tokio::sync::mpsc channel. A background task drains the channel and dispatches to registered consumers") and §Implementation Notes ("Event-tier messages have a configurable TTL (default 7 days, GYRE_EVENT_TTL_SECS). A background job calls expire_events() on the MessageRepository to delete expired events").

Candidate: `f873628e` on branch `pipeline/task-162/d9d7f0681c074fc0acbae4ea9777b0a1-1`, assigned base `6082be2f` (current main, task-165). Attributed commits (frontmatter): `55ac5b52`, `dd716a98`, `6d20863c`, `5192ff75`, `51221dc0` — all verified on-branch. This round is a **merge repair**: the previously approved task-162 candidate `da50b3ab` (PR #810) conflicted with main; merge commit `dce2a618` resolved it.

Verdict: **complete (approved)**.

## Merge-repair delta (this round's focus)

- `git diff da50b3ab f873628e` over the entire task-162 implementation surface (`gyre-common`, `gyre-ports`, `message_dispatcher.rs`, `lib.rs`, `main.rs`, `gate_executor.rs`, `merge_processor.rs`, `reconciliation.rs`, `mem.rs`, `api/messages.rs`, `mcp.rs`, `notifications.rs`, `jobs.rs`) is **empty** — the implementation is byte-identical to the approved candidate.
- The only delta is main's task-165 supply-chain work (domain `stack.rs`, `git_http.rs`, adapters agent `attestation_level` field, CLI). No semantic interaction: no `gyre-common`/`gyre-ports` changes on either side; the agent delta adds only `attestation_level: Option<u8>` (serde default, `None` at construction) and does not touch `spawned_by`/`workspace_id` used by the bridge.
- The single conflicted path (`specs/coverage/SUMMARY.md` TOTAL row) was recomputed correctly: summing the merged per-spec rows yields exactly the file's TOTAL (802/220/0/172/247/163), preserving both message-bus.md 100% and supply-chain.md 78%.

## Implementation (verified by source reading + probes, evidence in /tmp/stage/review-evidence/evidence.md)

- **Dispatcher**: `MessageDispatcher` holds `Vec<Arc<dyn MessageConsumer>>`; `spawn_message_consumer()` (message_dispatcher.rs) takes single ownership of the bounded (256) mpsc receiver via `AppState::take_message_dispatch_rx` (second spawn = logged no-op) and spawns the background drain task; wired in `main.rs` after `build_state`. All three send paths (`emit_event`, REST `send_message`, MCP `message.send`) clone into the channel **after** persisting, with warn-on-full consumer-drop logging — exactly the specced "one mpsc::send, bounded backpressure, drops logged" hot-path contract (lib.rs:709-733).
- **Notification bridge** (first registered consumer): GateFailure → rank-3 GateFailure for the MR author agent's spawning user via the shared `notify_gate_failure` helper (single creation path — gate_executor's former sync call removed); spec-policy violations (gate_type `spec_enforcement`) route to `notify_spec_policy_violation` (merge_processor's former sync call removed); AgentError → rank-2 AgentEscalation for the spawning user; ReconciliationCompleted → rank-6 MetaSpecDrift for Admin/Developer/Owner members (the former inline fan-out in `emit_reconciliation_completed` removed in favor of the bus path, per HSI §8 p6). BudgetWarning is deliberately **not** bridged — the synchronous budget-check path (api/budget.rs → `notify_agent_budget_warning`, per-period dedupe, spec-defined audience) owns it; a bridge fan-out duplicated and over-broadened delivery (round-2 regression, now pinned by `notification_bridge_does_not_create_budget_warning_notifications`). Scope handling is fail-closed throughout: unresolvable workspace/tenant → skip-and-log, never a fabricated "default" identity.
- **TTL job**: `spawn_message_expiry` runs hourly (`MissedTickBehavior::Delay`), calling `expire_events(now − GYRE_EVENT_TTL_SECS·1000)` and `expire_acked_inboxes(now − GYRE_DEAD_INBOX_TTL_SECS·1000)`; both env vars default to 604800 with warn-on-invalid fallback (`env_u64`); registered as `message_expiry` in the admin job registry for on-demand trigger, with its own executing loop matching the `spawn_budget_daily_reset` pattern (no double loop). The mem adapter mirrors SQLite's `ack_reason` contract ("explicit" vs "agent_completed"/"agent_orphaned") so `expire_acked_inboxes` deletes only dead-agent inboxes on both backends (check-mem-port-contracts passes).
- **Tests**: hard and behavior-anchored — `message_expiry_deletes_old_events_and_dead_inboxes` pins retention boundaries (unacked Directed and explicit-ack retained), `message_expiry_env_ttl_respected` pins the env-var criterion (1s TTL deletes only >1s-old events, env-mutating tests serialized), `spawn_message_consumer_end_to_end` drives the real channel → dispatcher → bridge → NotificationRepository chain, `failed_gate_creates_gate_failure_notification_via_bridge` pins the whole rewired gate-failure chain, and the round-1/round-2 regression tests (agent-directed copy ignored; budget fan-out absent) kill the previously found duplicate-delivery bugs.

## Probes run (this sandbox)

- `cargo check -p gyre-server --lib` → exit 0 (8m19s, cold).
- `cargo check -p gyre-server --lib --tests` → exit 0 (all task tests type-check).
- `cargo test -p gyre-adapters expire` → **11 passed, 0 failed** (SQLite expiry semantics incl. explicit-ack retention, agent_orphaned, recent-message preservation).
- Mechanical: check-arch, check-dead-message-kinds, check-mem-port-contracts, check-inert-enforcement, check-mcp-write-tools, check-migration-versions → all exit 0.
- Server lib test *execution* is blocked by this sandbox's 4 GiB build wrapper (verbatim error in evidence.md §4) — an infrastructure restriction, not a code defect. The implementation is byte-identical to the previously verified candidate, and the tests compile clean.

**Verifier must run** (recorded): `cargo test -p gyre-server --lib message_dispatcher`; `cargo test -p gyre-server --lib failed_gate_creates_gate_failure_notification_via_bridge failing_gate_notifies_author_with_gate_result_payload`; `cargo test -p gyre-server --lib require_spec_ref_blocks_mr_without_spec_ref require_signed_approval_blocks_unsigned_spec_approval`; `cargo test -p gyre-server --lib reconciliation_completed_fires_when_all_tasks_terminal`; `cargo test --all` (task acceptance criterion).

Findings:

- (none)

Non-blocking observations (not findings):

- `docs/server-config.md` does not yet list `GYRE_EVENT_TTL_SECS`/`GYRE_DEAD_INBOX_TTL_SECS`; the spec itself names both vars and the code reads them, so this is a docs-only gap for a follow-up.
- A comment in `message_dispatcher.rs` describes ralph_loop's agent-directed GateFailure copy; the current ralph_loop emits `GateResult` (Agent-directed) instead. The dedupe filter remains correct (GateResult is not a bridge trigger; the filter is defensive for any future Agent-directed GateFailure emitter) — comment precision only.
- `message_expiry_deletes_old_events_and_dead_inboxes` leaves its env vars set after finishing; only message_dispatcher.rs reads them and both env-mutating tests serialize on `TEST_ENV_LOCK`, so this is inert within the test binary.
