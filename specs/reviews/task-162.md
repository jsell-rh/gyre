# Review — task-162 (Message bus — wire MessageConsumer dispatcher and event TTL expiry job)

Spec: `specs/system/message-bus.md` §Relationship to Notifications (notification system becomes a bus consumer; bounded mpsc channel cloned on the send path; background drain task dispatches to `MessageConsumer`s) and §Implementation Notes (Event-tier TTL, default 7 days `GYRE_EVENT_TTL_SECS`; background job calls `expire_events()`).

Candidate: `171bbac4eba8fcc5494300d0d695aafd412ff235` on `pipeline/task-162/5f324879f9ae42579daee96488d76b4d-2`, assigned base `b5a872087`. Review scope: the full `b5a872087..171bbac4` diff (14 files: `message_dispatcher.rs` new, `lib.rs`/`main.rs`/`mem.rs`/`gate_executor.rs`/`reconciliation.rs`/`notifications.rs`/`jobs.rs`/`middleware.rs`, spec/coverage bookkeeping). All earlier branch-merge tips (fefde294, 3ced150b, 3c39ab1f7, e5415ae12, 7373979e8) are ancestors of the assigned base, so the base..candidate diff is the complete content scope regardless of `commits:` frontmatter state (see process note below).

Verdict: **approved** (round 3 of this review lineage; repairs for findings 73a69995/e7aa777ea verified in place).

## What was verified

**Dispatcher (§Relationship to Notifications).** `spawn_message_consumer()` (message_dispatcher.rs) takes the 256-capacity mpsc receiver via `AppState::take_message_dispatch_rx()` — single-owner via `Arc<Mutex<Option<Receiver>>>` (second call is a logged no-op, asserted by test) — and spawns `MessageDispatcher::run()`, which dispatches every drained message to all `Arc<dyn MessageConsumer>`s. The no-op drain spawns in both `build_state` and `test_state_inner` are removed; the receiver is now stored in `AppState` instead. All three send paths clone into the channel after store (lib.rs `emit_event` with logged non-blocking `try_send` fallbacks; api/messages.rs:313; mcp.rs:2046). Wired in main.rs alongside the other background jobs.

**NotificationBridge.** Registered as the sole consumer: GateFailure→p3 for the MR author agent's spawning user, BudgetWarning→p7 for Admin/Developer/Owner members, AgentError→p5 AgentEscalation for the failing agent's spawning user, ReconciliationCompleted→p6 MetaSpecDrift for Admin/Developer/Owner. Tenant is resolved from the workspace record (`tenant_for_workspace` returns `None` on lookup failure → skip + warn; no fabricated `"default"` scope — consistent with the AGENTS.md invariant). Viewer role excluded (tested). The gate executor's old synchronous p3 path and `notify_gate_failure` helper are removed (single creation path via the bridge); `emit_reconciliation_completed()` now only emits the event. The HSI §8 p3 amendment is recorded in `specs/system/human-system-interface.md` (amendment table + routing table) — spec amended via the documented mechanism, not contradicted silently.

**TTL expiry job (§Implementation Notes).** `spawn_message_expiry()` runs hourly (3600s, `MissedTickBehavior::Delay`), job-registry-tracked as `message_expiry` (registered in jobs.rs with a handler for `POST /admin/jobs/message_expiry/run`; the standalone loop is the only periodic scheduler — no `jobs::spawn_job` call for it, so no double-scheduling). Each cycle calls `expire_events(now − GYRE_EVENT_TTL_SECS·1000)` and `expire_acked_inboxes(now − GYRE_DEAD_INBOX_TTL_SECS·1000)`; both env vars default to 604800 via `env_u64` with warn-on-invalid fallback. `MemMessageRepository` now tracks ack reasons (`explicit` vs `acknowledge_all` reason), and its `expire_acked_inboxes` deletes only agent-targeted messages acked `agent_completed`/`agent_orphaned` — matching the SQLite adapter's Diesel predicate exactly; `acknowledge` gained the `!acknowledged` idempotence guard, matching SQLite's `acknowledged.eq(0)` filter. Lock ordering (store → ack_reasons) consistent across all three methods.

## Test runs (this checkout, candidate head, SKIP_WEB_BUILD=1)

- `cargo test -p gyre-server --lib message_dispatcher` → **9 passed, 0 failed**.
- `cargo test -p gyre-server --lib gate_executor` → **25 passed, 0 failed** (incl. `failed_gate_creates_gate_failure_notification_via_bridge`).
- `cargo test -p gyre-server --lib reconciliation::tests::reconciliation_completed_fires_when_all_tasks_terminal` → **1 passed** (73a69995 repair: spawns the real dispatcher and polls — verified below to be regression-killing).
- `cargo test -p gyre-server --lib` (full) → **1626 passed, 0 failed**.
- Mechanical checks: arch, mem-port-contracts, dead-message-kinds, fabricated-scope-defaults, scope-literal-defaults, inert-enforcement, in-memory-state-stores, byte-slice-truncation, task-commit-attribution → **all exit 0**.

## Mutation probes (all restored; `git status --short` clean after each)

1. Disable the bridge's `on_gate_failure` arm → `failed_gate_creates_gate_failure_notification_via_bridge` and `notification_bridge_creates_gate_failure_notification` **both FAIL** (0 notifications). The p3 path is genuinely the bridge, not a leftover synchronous creator.
2. Hardcode `DEFAULT_EVENT_TTL_SECS` in `run_message_expiry` (ignore `GYRE_EVENT_TTL_SECS`) → `message_expiry_deletes_old_events_and_dead_inboxes` and `message_expiry_env_ttl_respected` **both FAIL**. Env-var respect is genuinely tested.
3. Make `spawn_message_consumer` take the rx but not spawn the dispatcher → `spawn_message_consumer_end_to_end`, `failed_gate_creates_gate_failure_notification_via_bridge`, and `reconciliation_completed_fires_when_all_tasks_terminal` **all FAIL**. The end-to-end wiring is real, not test-synchronous shortcuts.

Evidence under `/tmp/stage/review-evidence` (task162-*.txt).

## Findings

- (none — code)

## Process note (non-blocking, recorded for the controller)

The assigned candidate `171bbac4e` is an executor checkpoint ("process: record task-162 branch commits") that re-derived `commits:` from task-labeled commits only, dropping the five unlabeled merge commits — the fifth recurrence of the mechanism documented in the task body's "fourth recurrence" note. Not blocking here, for three reasons: (a) the mechanical invariant (task-labeled product-surface commits listed) holds and `check-task-commit-attribution.sh` passes; (b) every merge tip merged on this branch is an ancestor of the assigned base `b5a872087`, so the base..candidate diff — the scope actually reviewed — contains all content-bearing changes including the 0c8b6fa1e mem.rs resolution; (c) the durable record in the task body explicitly anticipates this frontmatter state and designates it wrong-not-authoritative. The 73a69995/e7aa777ea repair states were verified in the diff itself (5192ff750 present; `notify_gate_failure` absent).

— Reviewer, 2026-10-08 (candidate `171bbac4`)
