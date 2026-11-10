# Review — task-162 (Message bus — wire MessageConsumer dispatcher and event TTL expiry job)

Spec: `specs/system/message-bus.md` §Relationship to Notifications (notification system
becomes a `MessageConsumer` of the bus; send path clones into bounded mpsc; background
drain task dispatches) and §Implementation Notes (Event-tier TTL, `GYRE_EVENT_TTL_SECS`
default 7 days, background job calling `expire_events()`).
Assignment: base `fca5de5f`, candidate `639d7dde`. Review scope: the full
`fca5de5f..639d7dde` diff (which subsumes all merge resolutions on the branch — the
`commits:` frontmatter at the candidate head again lists only the two task-labeled
commits due to the executor checkpoint mechanism; see the durable-facts note in
`specs/tasks/task-162.md`, fourth-recurrence paragraph, and the frontmatter note below).
Verdict: **approved**.

## Probes (this checkout at `639d7dde`, SKIP_WEB_BUILD=1, evidence under /tmp/stage/review-evidence)

- `cargo test -p gyre-server --lib message_dispatcher` → **9 passed, 0 failed**
  (bridge unit tests for all four kinds, viewer exclusion, unknown-workspace skip,
  dispatcher fan-out, end-to-end `spawn_message_consumer`, TTL retention boundaries,
  env-var TTL override).
- `cargo test -p gyre-server --lib gate_executor` → **25 passed, 0 failed** (incl.
  `failed_gate_creates_gate_failure_notification_via_bridge`).
- `cargo test -p gyre-server --lib reconciliation::tests::reconciliation_completed_fires_when_all_tasks_terminal`
  → **1 passed** (the previously-failing 73a69995 test, fixed by spawning the real
  dispatcher in 5192ff7).
- `cargo test -p gyre-server --lib jobs::` → 6 passed; `notifications` filter → 12 passed.
- `cargo test -p gyre-server --lib` (full package, all changes live here) →
  **1679 passed, 0 failed**.
- Mechanical: `check-arch.sh`, `check-mem-port-contracts.sh`,
  `check-dead-message-kinds.sh`, `check-inert-enforcement.sh`,
  `check-fabricated-scope-defaults.sh` → all exit 0.

### Mutation probes (source restored and verified clean with `git status`/`git diff` after each)

1. **Dispatcher wiring**: replaced the registered consumer list in
   `spawn_message_consumer` with `Vec::new()` (no consumers) →
   `spawn_message_consumer_end_to_end`, `failed_gate_creates_gate_failure_notification_via_bridge`,
   and `reconciliation_completed_fires_when_all_tasks_terminal` all **FAIL**. All three
   end-to-end tests are genuinely anchored to the bridge + dispatcher wiring, not to
   incidental behavior.
2. **TTL expiry**: replaced the `expire_events`/`expire_acked_inboxes` calls in
   `run_message_expiry` with constant `0` → both `message_expiry_deletes_old_events_and_dead_inboxes`
   and `message_expiry_env_ttl_respected` **FAIL**. The TTL tests kill a disabled
   expiry job.

## Verified (no findings)

- **Dispatcher (§Relationship to Notifications).** `spawn_message_consumer`
  (message_dispatcher.rs) takes the 256-capacity mpsc receiver via
  `AppState::take_message_dispatch_rx()` (single-owner, second call is a logged no-op —
  pinned by the e2e test's final assertion) and spawns the drain task;
  `MessageDispatcher` holds `Vec<Arc<dyn MessageConsumer>>` and dispatches every message.
  The old no-op drain spawns in `build_state`/`test_state_inner` are gone; the receiver
  is stored in `AppState` instead. `emit_event`'s `try_send` failure arms now log
  (Full/Closed) and never block the send path — best-effort tier per spec. The other two
  send paths (api/messages.rs:313, mcp.rs:2051) pre-date this task and now feed the real
  dispatcher.
- **NotificationBridge is real, not structural.** GateFailure→p3 for the MR author
  agent's spawning user (tenant resolved from the workspace record — the removed sync
  path hardcoded `"default"`), BudgetWarning→p7 for Admin/Developer/Owner members,
  AgentError→p5 AgentEscalation for the spawning user, ReconciliationCompleted→p6
  MetaSpecDrift for Admin/Developer/Owner. Unknown-workspace lookups skip with a warn
  rather than fabricating a scope identity (AGENTS.md rule); the pre-existing
  `unwrap_or_else(|| Id::new("default"))` in gate_executor's GateFailure *emitter* is
  base code, untouched by this diff, and harmless downstream because the bridge skips
  unresolvable workspaces. The removed `notify_gate_failure` fallback that notified the
  *agent id* when `spawned_by` was absent is replaced by a skip-with-debug — more
  correct than notifying a non-user identity.
- **Spec amendment, not contradiction.** HSI §8 p3 ("Synchronous in gate evaluation
  handler") is amended in `specs/system/human-system-interface.md` to route p3 via
  `MessageConsumer` consuming `GateFailure` events, and the HSI coverage row 43 notes
  the amendment — the code matches the amended spec, single creation path (the old
  synchronous duplicate is deleted, verified: no remaining `notify_gate_failure`
  callers).
- **TTL job (§Implementation Notes).** `spawn_message_expiry` runs the hourly loop
  (3600 s, `MissedTickBehavior::Delay`, `mark_scheduled` + `record_cycle` — same
  pattern as `spawn_budget_daily_reset`; the registry does not spawn its own schedulers,
  so no double-run). `run_message_expiry` calls
  `expire_events(now − GYRE_EVENT_TTL_SECS·1000)` and
  `expire_acked_inboxes(now − GYRE_DEAD_INBOX_TTL_SECS·1000)`, both env vars default
  604800 with warn-on-invalid fallback, `saturating_sub` on the cutoff. The job is
  registered in the admin job registry (`POST /admin/jobs/message_expiry/run`). The
  SQLite/Postgres adapters already implemented both port methods at base (verified at
  `fca5de5f`); this task wires the caller.
- **Mem adapter contract parity.** `MemMessageRepository` now tracks ack reasons;
  `expire_acked_inboxes` deletes only agent-targeted messages acked
  `agent_completed`/`agent_orphaned` and older than the cutoff — matching the SQLite
  adapter's query (explicit acks and unacked Directed messages retained, asserted in the
  TTL test). `expire_events` retains agent-targeted messages, matching SQLite's
  `to_type.ne("agent")`. Lock ordering (store before ack_reasons) consistent between
  `acknowledge_all` and `expire_acked_inboxes`. `check-mem-port-contracts.sh` passes.
- **No out-of-scope edits.** The diff touches message_dispatcher.rs (new), lib.rs,
  mem.rs, main.rs, jobs.rs, middleware.rs (test-state field), gate_executor.rs,
  notifications.rs, reconciliation.rs, and the spec/coverage/task bookkeeping files.
  No gates weakened, no exemption files grown, no tests deleted.

Findings:

- (none)

Frontmatter note (not a defect in the shipped work): the candidate head `639d7dde` is
another executor checkpoint commit that rewrote `commits:` back to the two task-labeled
commits only. The merge resolutions are inside this review's scope via the
`fca5de5f..639d7dde` diff, which I read in full (jobs.rs/main.rs union resolutions at
`05b32d067`, mem.rs union at `0c8b6fa1e`, and the earlier main.rs import unions — all
confirmed present and consistent in the final tree). The durable-facts note in
`specs/tasks/task-162.md` already documents this failure mode; nothing further to
repair here.
