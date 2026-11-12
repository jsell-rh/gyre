# Review — task-162 (Message bus — wire MessageConsumer dispatcher and event TTL expiry job)

Spec: `specs/system/message-bus.md` §"Relationship to Notifications", §"Implementation Notes".
Candidate under review: `1764ac370db8284bf31e987d04810d9ad2814bc3` (base `d31906c5a16ab8045ab435dfad2bd7dc1ce3f616`).
Product-surface commits (scoping per `commits:` frontmatter at the candidate):
`51221dc02d235b9737d645d541650e771d3e89e5`, `5192ff750f58b79d2553a317e9d9b802292ad3e1`,
`6d20863c02e07c0a51873fdbb735c61bef80af68`, `dd716a986330b7509a7e1e93469d94fdb457628f`.
Verdict: **approved** (round 3 — rebase re-review). Evidence:
`/tmp/stage/review-evidence/review-round3.md` (+ per-probe logs in the same directory).

## Round 3 (candidate `1764ac370`, base `d31906c5`) — rebase re-review, approved

This candidate is the round-2-approved work rebased onto assignment base
`d31906c5` (tip commit only restores the task file; all code comes from the
scoped commits above). Full independent re-review of the base→candidate diff;
no prior approval was carried forward.

Probes (clean worktree at the candidate): `cargo check -p gyre-server` PASS;
`cargo check -p gyre-server --tests` PASS (all new test code compiles);
`cargo fmt -p gyre-server -- --check` PASS; commit-attribution PASS at the
candidate commit; 15 mechanical invariant scripts (arch, mem-port-contracts,
dead-message-kinds, abac-route-registry, inert-enforcement, scope-defaults ×3,
in-memory-state-stores, unbounded-external-http, byte-slice-truncation,
relative-path-defaults, lossy-secret-conversion, forwarded-header-trust,
migration-versions, forged-scope-fields) all PASS; no rebase conflict markers
in crates/ or specs/. gyre-server test execution is blocked by the sandbox
4 GiB wrapper and delegated to the verifier (runs listed below).

Verified (details in evidence file):

- **Dispatcher real and single-owner** — `spawn_message_consumer()` takes the
  256-cap mpsc receiver via `AppState::take_message_dispatch_rx()` (second call
  logged no-op), spawns the drain task, wired in main.rs;
  `MessageDispatcher` holds `Vec<Arc<dyn MessageConsumer>>`. The old no-op
  drain in `build_state`/`mem::test_state_inner` is gone.
- **NotificationBridge is a genuine consumer** — GateFailure→rank-3 for the MR
  author agent's spawning user (MR→agent→spawner chain, tenant resolved from
  the workspace record, skip-not-fabricate), BudgetWarning→rank-4
  AgentBudgetWarning for Admin/Developer/Owner, AgentError→rank-2
  AgentEscalation for the spawner, ReconciliationCompleted→rank-6 MetaSpecDrift
  for Admin/Developer/Owner; all via shared `notify_rich`.
- **Round-1 dedupe repair present** — `on_gate_failure` consumes only
  Workspace/Broadcast-targeted events; verified against both real emitters
  (gate_executor Workspace event vs ralph_loop Agent-directed copy). No
  double-notification paths remain: `notify_gate_failure` deleted with zero
  callers, `emit_reconciliation_completed` emits only, no production
  BudgetWarning/AgentError bus emitters, GateFailure is server-only (not
  forgeable via message.send). HSI §8 p3 amended consistently with canonical
  message-bus.md.
- **Round-2 drop-logging repair present** — all three send paths warn on
  `TrySendError::Full`; no silent consumer drops.
- **TTL job real** — hourly `spawn_message_expiry` (registry-tracked
  `message_expiry`, admin on-demand run, spawned once in main.rs — no double
  loop); `expire_events(now − GYRE_EVENT_TTL_SECS·1000)` +
  `expire_acked_inboxes(now − GYRE_DEAD_INBOX_TTL_SECS·1000)`, defaults
  604800, warn-on-invalid env parse; both env vars tested.
- **Mem adapter parity fix is real** — mem `expire_acked_inboxes` now matches
  the SQLite/Postgres `ack_reason ∈ {agent_completed, agent_orphaned}` contract
  (pre-task mem deleted every acked old message); `acknowledge` records
  "explicit" idempotently, matching diesel semantics.
- **Tests are hard** — exact-count assertions; TTL retention boundaries
  (expired Event deleted; fresh Event / unacked Directed / explicit-ack
  retained on distinct agents); env override; e2e through the real send path;
  `failed_gate_creates_gate_failure_notification_via_bridge` pins run_gate →
  event → bridge → exactly one rank-3; regression for the two-emitter dedupe.

Non-blocking observations (auditor notes, not this task's sections):
dispatcher task holds a `Sender` via `AppState` so its exit path is unreachable
(intended lifetime); TTL env test does not restore vars (serialized, no other
reader); dead-inbox cleanup is a production no-op until `acknowledge_all`
callers exist (pre-existing, coverage row 16); `web/dist/` regeneration is
verifier-owned.

Verifier must execute: `cargo test -p gyre-server --lib message_dispatcher`,
`--lib gate_executor`, `--lib notifications`, `--lib reconciliation`,
`--lib mem`, and the full `cargo test --all`.

## Round 2 (candidate `d467ca94b`, base `1e61f6637`) — complete

Round-1 findings F1 (GateFailure double-notification from two emitters) and
F2 (silent consumer drops) repaired in `dd716a986` and verified: destination
filter + regression test; TrySendError::Full warn at all three send paths.
Full record in pipeline history (checkpoint `d6221f741`).

## Round 1 (candidate `c409c3bb7`) — needs-revision

F1 (blocking): NotificationBridge consumed both GateFailure emitters →
duplicate rank-3 notifications per failed gate. F2 (minor): bare
`let _ = try_send` at REST/MCP send paths.
