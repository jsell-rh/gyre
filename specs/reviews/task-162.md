# Review — task-162 (Message bus — MessageConsumer dispatcher and event TTL expiry job)

Spec: `specs/system/message-bus.md` §"Relationship to Notifications" (dispatch to
registered `MessageConsumer`s off the send hot path; notification system as first
consumer) and §"Implementation Notes" (Event-tier TTL via `GYRE_EVENT_TTL_SECS`,
background job calling `expire_events()`).
Base `8c2d1775`, candidate `decd03be` (product commits `cb865b79`, `6b72222c`,
`f5346ff0`, `244a98cc`; the `process:`/merge commits in range are lifecycle-only).
Verdict: **complete / approved**.

## Probes (SKIP_WEB_BUILD=1, evidence in /tmp/stage/review-evidence)

- `cargo test -p gyre-server --lib message_dispatcher` → **9 passed** (bridge unit
  tests for GateFailure/BudgetWarning/AgentError/ReconciliationCompleted,
  viewer-role exclusion, unknown-workspace skip, fan-out, e2e
  `spawn_message_consumer`, TTL retention boundaries, env-var TTL override).
- `cargo test -p gyre-server --lib gate_executor` → **25 passed**, incl.
  `failed_gate_creates_gate_failure_notification_via_bridge` (run_gate →
  emit_event → dispatcher → bridge → exactly one p3 for the spawning user).
- `cargo test -p gyre-server --lib notifications` → **9 passed** (no callers
  broken by `notify_gate_failure` removal).
- Full package lib suite: `cargo test -p gyre-server --lib` → **1199 passed,
  0 failed** — no other test depended on the removed synchronous
  `emit_reconciliation_completed` notification path or the old
  `notify_gate_failure` helper.
- **Mutation probes (tests bind to production behavior, not mirrors):**
  - Dispatcher spawn disabled (`tokio::spawn` replaced with `drop(rx)`) → both
    e2e tests FAIL (`bridge must create the p6/p3 notification`, left: 0).
    Source restored afterwards; re-run 9/9 green, tree clean at `decd03be`.
  - `run_message_expiry` `expire_events` call stubbed to `0` → both TTL tests
    FAIL. Source restored; green.
- Mechanical checks clean: arch, mem-port-contracts, dead-message-kinds,
  in-memory-state-stores, fabricated-scope-defaults, scope-literal-defaults,
  inert-enforcement.

## Verified working (no findings)

- **Dispatcher is real.** `spawn_message_consumer()` (message_dispatcher.rs)
  takes the 256-capacity mpsc receiver via `AppState::take_message_dispatch_rx()`
  (single-owner via `Arc<Mutex<Option<Receiver>>>`; second call is a logged
  no-op) and spawns a drain task dispatching to `Vec<Arc<dyn MessageConsumer>>`.
  Wired from `main.rs` after `build_state`. `build_state`/`test_state_inner` no
  longer spawn the old no-op drain loop — the receiver sits in the mutex until
  taken, and `emit_event`'s `try_send` handles `Full` (warn) and `Closed`
  (debug) without blocking the send path, exactly per spec §"Relationship to
  Notifications" ("consumers are downstream observers; drops do not affect the
  at-least-once Directed guarantee").
- **NotificationBridge is a genuine consumer.** GateFailure → p3 for the MR
  author agent's spawning user; BudgetWarning → p7 for Admin/Developer/Owner
  members; AgentError → p5 AgentEscalation for the failing agent's spawning
  user; ReconciliationCompleted → p6 MetaSpecDrift for Admin/Developer/Owner.
  Tenant is resolved from the workspace record (no fabricated `"default"` —
  the old sync path in `run_gate` passed a literal `"default"` tenant, now
  gone); unknown workspace / missing spawning user → skip + log, not
  mis-attribution. Viewer role excluded from workspace fan-out (tested).
- **HSI §8 p3 amendment is legitimate spec-lifecycle reconciliation, not
  contradiction.** message-bus.md §"Relationship to Notifications" (the task's
  canonical spec) requires the GateFailure notification to be derived by the
  MessageConsumer; HSI §8 p3 previously said "synchronous in gate evaluation
  handler". The conflict is resolved by amending HSI §8 p3 (and its amendment
  table) to the consumer path, with the sync path and `notify_gate_failure`
  helper removed — single creation path. HSI coverage row 43 updated to match.
  The reliability trade-off (best-effort drop on channel full) is exactly what
  message-bus.md specifies for consumers.
- **TTL job is real and periodic.** `spawn_message_expiry()` (wired in
  `main.rs`) runs a 3600s interval loop (MissedTickBehavior::Delay,
  `mark_scheduled`/`record_cycle` — same self-managed-loop pattern as
  spawn_stale_agent_detector; the jobs.rs `register` entry is admin-registry
  bookkeeping like the other module loops, so no double execution). Each cycle
  calls `expire_events(now − GYRE_EVENT_TTL_SECS·1000)` and
  `expire_acked_inboxes(now − GYRE_DEAD_INBOX_TTL_SECS·1000)`; `env_u64`
  defaults 604800 with warn-on-invalid fallback; env override proven by
  `message_expiry_env_ttl_respected` (1s TTL deletes only the >1s-old event)
  and the mutation probe.
- **Mem/SQLite contract parity for the ack-reason change.** Port docs:
  `expire_events` deletes non-agent-targeted messages older than cutoff;
  `expire_acked_inboxes` matches `ack_reason IN ('agent_completed',
  'agent_orphaned')`. SQLite (sqlite/message.rs:274-322, 382-421) enforces:
  `acknowledge`/`acknowledge_all` only update unacked rows and record
  ack_reason; the mem adapter now mirrors this exactly (new `ack_reasons` map,
  `!m.acknowledged` guard, retain-predicates matching the SQL filters,
  documented lock order). The TTL test pins the retention boundaries that
  distinguish the two contracts (unacked Directed retained by expire_events;
  explicit ack retained by expire_acked_inboxes; only bulk dead-agent acks
  cleaned).

## Pre-existing failures reproduced identically at pristine base `8c2d1775`

(worktree check, evidence files `*-BASE.txt`; not candidate defects):

- `check-notification-test-coverage.sh` exits 2 on this host's mawk (gawk-only
  3-arg `match()`) before scanning any file — identical at base. Manual
  application of its intent to the new code: every `NotificationBridge`
  emission path (`state.notifications.create`) has tests that query
  `state.notifications` and assert type/priority — satisfied.
- `check-task-commit-attribution.sh` fails on task-210 commit `a781ede2`
  missing from task-210's frontmatter (on main) — identical at base;
  task-162's own attribution is complete (all four task-labeled product
  commits listed).

## Minor (not blocking, recorded for completeness)

- `api/messages.rs:299` and `mcp.rs:1989` still use `let _ = try_send(...)`
  without a warn on `Full` — pre-existing lines untouched by this diff; spec's
  "dropped with a warning log" is currently honored only on the `emit_event`
  path (where all four bridge-consumed kinds originate). The task's Shipped
  notes phrase this loosely ("try_send failures log") — the code is correct
  (drop, never block) on all three paths.
- `message_expiry_deletes_old_events_and_dead_inboxes` leaves
  `GYRE_EVENT_TTL_SECS=1000` set if it runs after
  `message_expiry_env_ttl_respected` (which restores defaults); harmless since
  only `run_message_expiry` reads these vars and both TTL tests are serialized
  by `TEST_ENV_LOCK`.
- Pre-existing `run_gate` fallback `workspace_id.unwrap_or_else(|| Id::new("default"))`
  (gate_executor.rs:108, unchanged by this task) makes the emitted GateFailure
  event carry a fabricated workspace when the MR lookup fails; the bridge
  safely skips such events (workspace lookup fails → skip), so the new consumer
  path fails closed.

Full workspace suite and exact-head CI remain with verification/publication per
assignment constraints.
