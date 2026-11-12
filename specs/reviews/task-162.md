# Review — task-162 (Message bus — wire MessageConsumer dispatcher and event TTL expiry job)

Spec: `specs/system/message-bus.md` §"Relationship to Notifications", §"Implementation Notes".
Candidate under review: `d467ca94b` (base `1e61f6637fba1a1fe8b851eaac13691d1230f665`).
Product-surface commits (scoping per `commits:` frontmatter at the candidate):
`51221dc02d235b9737d645d541650e771d3e89e5`, `5192ff750f58b79d2553a317e9d9b802292ad3e1`,
`6d20863c02e07c0a51873fdbb735c61bef80af68`, `dd716a986330b7509a7e1e93469d94fdb457628f`.
Verdict: **complete** (round 2). Evidence: `/tmp/stage/review-evidence/review-round2.md`
(+ per-probe logs in the same directory).

## Round 1 (candidate `c409c3bb7`, checkpoint 134030714) — needs-revision

Two findings (durable record 11a6f509):

- **F1 (code, blocking)**: NotificationBridge consumed `GateFailure` with no
  destination/tier filter while two production emitters publish the same
  failed gate (gate_executor `run_gate`, Workspace-targeted; ralph_loop
  `on_gates_failed`, Agent-targeted copy for the agent's next session) — the
  MR author's spawning user received TWO identical rank-3 notifications per
  failed gate, contradicting HSI §8 p3 "single creation path".
- **F2 (code, minor)**: REST send handler and MCP `message.send` used a bare
  `let _ = try_send` — silent consumer drop where the spec requires a warning
  log.

## Round 2 (candidate `d467ca94b`) — repairs verified, full re-review

Probes (this sandbox, HEAD = candidate): `cargo check -p gyre-server --tests`
→ clean (8m45s); `cargo fmt -p gyre-server -- --check` → clean; mechanical
invariants arch / dead-message-kinds / mem-port-contracts / abac-route-registry /
mcp-write-tools → all OK. Commit attribution passes at the candidate commit
(the working-tree FAIL is the stage's uncommitted pristine reset of the task
file — `commits: []` in the working tree vs. the full list at `d467ca94b`;
all three task-labeled product commits are recorded there). gyre-server test
compilation deferred to the verifier (4 GiB budget); required runs listed
below.

- **F1 repaired** (`dd716a986`): `on_gate_failure` now consumes only
  Workspace/Broadcast-targeted GateFailure events; the Agent-directed copy is
  the agent's inbox record. Regression
  `notification_bridge_ignores_agent_directed_gate_failure_copy` feeds both
  emitters' message shapes and asserts exactly one rank-3 notification — it
  fails if the destination filter is removed.
- **F2 repaired** (`dd716a986`): all three send paths (`emit_event`, REST
  `send_message`, MCP `message.send`) mirror the TrySendError::Full warn
  match; no silent consumer drops remain.

Verified working (no findings):

- **Dispatcher is real and single-owner.** `spawn_message_consumer()` takes
  the 256-capacity mpsc receiver via `AppState::take_message_dispatch_rx()`
  (second call = logged no-op) and spawns the drain task; `MessageDispatcher`
  holds `Vec<Arc<dyn MessageConsumer>>`; wired in main.rs after build_state.
  `emit_event` persists before dispatch, per spec ("after storing the
  message").
- **NotificationBridge is a genuine consumer.** GateFailure → rank-3 for the
  MR author agent's spawning user (MR → author agent → spawner chain, skip on
  break; tenant from the workspace record, never fabricated), BudgetWarning →
  rank-4 AgentBudgetWarning for Admin/Developer/Owner (Viewer excluded,
  tested), AgentError → rank-2 AgentEscalation for the spawner,
  ReconciliationCompleted → rank-6 MetaSpecDrift for Admin/Developer/Owner.
  All creation goes through the shared `notify_rich` (one creation path,
  structured source + deep links). The removed synchronous paths leave no
  stragglers: `notify_gate_failure` deleted (no remaining callers),
  `emit_reconciliation_completed` no longer creates notifications inline.
- **TTL job is real.** `spawn_message_expiry()` hourly (3600s,
  MissedTickBehavior::Delay), registry-tracked as `message_expiry`
  (on-demand `POST /admin/jobs/message_expiry/run`); `run_message_expiry`
  calls `expire_events(now − GYRE_EVENT_TTL_SECS·1000)` and
  `expire_acked_inboxes(now − GYRE_DEAD_INBOX_TTL_SECS·1000)`, defaults
  604800 each, warn-on-invalid env parse. The mem adapter now enforces the
  same dead-inbox contract as SQLite/Postgres (only
  `agent_completed`/`agent_orphaned` bulk-acks cleaned; explicit acks and
  unacked Directed retained) — check-mem-port-contracts passes.
- **Tests are hard.** Exact-count assertions throughout; TTL retention
  boundaries (expired Event deleted; fresh Event / unacked Directed /
  explicit-ack retained); env-var override; e2e through the real send path
  (emit_event → dispatcher → bridge) with polling;
  `failed_gate_creates_gate_failure_notification_via_bridge` pins the whole
  rewired chain (run_gate → event → bridge → exactly one rank-3).

Non-blocking observations (for the auditor, not this task's sections):

- **Dead-inbox cleanup is a production no-op today**: no production code
  calls `acknowledge_all(reason)` (only tests), so `expire_acked_inboxes`
  deletes 0 rows until agent-completion/orphan paths bulk-ack. Pre-existing,
  documented in specs/coverage/system/message-bus.md row 16 (Edge Cases) —
  outside this task's coverage sections. The job itself is real and runs.
- `message_expiry_deletes_old_events_and_dead_inboxes` does not restore env
  vars after running (sibling env test does); serialized by `TEST_ENV_LOCK`,
  no other reader — harmless.

Verifier must execute: `cargo test -p gyre-server --lib message_dispatcher`,
`--lib gate_executor`, `--lib notifications`, `--lib ralph_loop`,
`--lib reconciliation`, and the full `cargo test --all`.
