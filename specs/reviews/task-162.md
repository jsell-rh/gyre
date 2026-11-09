# Review — task-162 (Message bus — wire MessageConsumer dispatcher and event TTL expiry job)

Spec: `specs/system/message-bus.md` §Relationship to Notifications + §Implementation Notes.
Candidate under review: `152e67033b2b424a56789be02337943928dd6c88` (base `fefde2946`). Scoping commits in frontmatter: `51221dc02` (implementation) — see F2 on the merge-repair commit.
Verdict: **needs-revision** (one blocking test regression; the implementation itself is genuine — see verification below).

## Round 1

Evidence under `/tmp/stage/review-evidence/` (all runs `SKIP_WEB_BUILD=1`, `/tmp/stage/bin/cargo`, this checkout at the candidate).

Test runs:

- `cargo test -p gyre-server --lib message_dispatcher` → **9 passed** (bridge unit tests for GateFailure/BudgetWarning/AgentError/ReconciliationCompleted, viewer exclusion, unknown-workspace skip, dispatcher fan-out, end-to-end `spawn_message_consumer`, TTL retention boundaries, env TTL override).
- `cargo test -p gyre-server --lib gate_executor` → **25 passed** incl. `failed_gate_creates_gate_failure_notification_via_bridge`.
- `cargo test -p gyre-server --lib notifications` → **12 passed** (no caller broken by `notify_gate_failure` removal).
- `cargo test -p gyre-server --lib` (full lib suite) → **1493 passed, 1 FAILED**: `reconciliation::tests::reconciliation_completed_fires_when_all_tasks_terminal` panics at `reconciliation.rs:3405` ("expected MetaSpecDrift notification on wave completion"). Deterministic (reproduced twice). Passes at base `fefde2946` (verified in a detached worktree) — **candidate regression**, see F1.
- `cargo check -p gyre-server --tests` → clean (bins + integration targets compile).
- Mutation probes (source restored after each, `git status --porcelain` clean):
  - `env_u64()` forced to return defaults → both TTL tests **FAIL** — `GYRE_EVENT_TTL_SECS`/`GYRE_DEAD_INBOX_TTL_SECS` reads are genuinely exercised.
  - `NotificationBridge::on_message` early-return → `failed_gate_creates_gate_failure_notification_via_bridge` + all four bridge unit tests **FAIL** — the run_gate → emit_event → dispatcher → bridge → notification chain is real, not structural.
- Mechanical checks all exit 0: arch, dead-message-kinds, mem-port-contracts, fabricated-scope-defaults, scope-literal-defaults, inert-enforcement, task-commit-attribution.

Verified working (no findings against these):

- **Dispatcher is real and single-owner.** `spawn_message_consumer()` (message_dispatcher.rs) takes the 256-capacity mpsc receiver via `AppState::take_message_dispatch_rx()` ( Mutex<Option<Rx>>.take() — second spawn is a logged no-op, pinned by `spawn_message_consumer_end_to_end`), spawns the drain task, and `MessageDispatcher::run()` dispatches every message to all `Arc<dyn MessageConsumer>`s. Wired in main.rs after `build_state`. The old no-op drain loop in `build_state`/`test_state_inner` is gone; `emit_event` still `try_send`s after store+broadcast, with Full/Closed now logged instead of silently dropped. REST (api/messages.rs:306) and MCP (mcp.rs:2028) send paths also clone into the channel.
- **NotificationBridge fan-out is genuine.** GateFailure→p3 for the MR author agent's spawning user (tenant resolved from the workspace record — the removed sync path hardcoded tenant `"default"`, an improvement); BudgetWarning→p7 and ReconciliationCompleted→p6 for Admin/Developer/Owner members (Viewer excluded, tested); AgentError→p5 AgentEscalation for the spawning user. Unknown workspace / missing spawned_by → skip+log, no fabricated identities.
- **TTL job is real.** `spawn_message_expiry()` hourly (3600s, MissedTickBehavior::Delay, job-registry-tracked as `message_expiry`, admin-triggerable, registered in jobs.rs) calls `expire_events(now − GYRE_EVENT_TTL_SECS·1000)` and `expire_acked_inboxes(now − GYRE_DEAD_INBOX_TTL_SECS·1000)`; defaults 604800 with warn-on-invalid fallback. Both adapters' `expire_*` Diesel implementations pre-exist and are untouched. `MemMessageRepository` now tracks ack reasons so the dead-inbox contract (only `agent_completed`/`agent_orphaned` bulk-acks cleaned; explicit acks and unacked Directed retained) matches SQLite — mem-port-contracts check passes.
- **Spec amendment is coherent, not implementation-around-spec.** HSI §8 p3 changed from "synchronous in gate evaluation handler" to "via MessageConsumer consuming GateFailure events", recorded in HSI's own amendment ledger, consistent with message-bus.md §Relationship to Notifications ("the notification system becomes a consumer of the message bus"). Single creation path (old `notify_gate_failure` + inline `emit_reconciliation_completed` notification code removed, not duplicated).

Findings:

- **F1 (blocking, code):** `crates/gyre-server/src/reconciliation.rs:3405` — pre-existing test `reconciliation_completed_fires_when_all_tasks_terminal` deterministically fails at the candidate. The candidate removed the inline p6 MetaSpecDrift creation from `emit_reconciliation_completed()` (lib.rs diff) and moved it to the async NotificationBridge, but this test never spawns the dispatcher (`spawn_message_consumer`), so no notification is ever created and the final `assert!` fails. Passes at base `fefde2946`. Violates the acceptance criterion "cargo test --all passes". Reproduce: `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib reconciliation::tests::reconciliation_completed_fires_when_all_tasks_terminal`. Fix: spawn the dispatcher in the test (as `failed_gate_creates_gate_failure_notification_via_bridge` does) and/or poll for the async notification.
- **F2 (minor, process/doc):** `specs/tasks/task-162.md` — the shipped merge-repair note states the merge commit `9e5a0171d` is "recorded in `commits:` above", but the final candidate commit `152e67033` removed it from the frontmatter (now lists only `51221dc02`). The mechanical attribution check still passes (`9e5a0171d` is not task-labeled), but the note is inaccurate at head, and the merge commit — which contains the main.rs import-conflict resolution, i.e. the candidate's own startup wiring — sits outside the review-scoping commit list. Recommend re-adding `9e5a0171d` to `commits:` or correcting the note.
