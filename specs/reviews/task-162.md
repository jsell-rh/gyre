# Review — task-162: Message bus MessageConsumer dispatcher and event TTL expiry job

**Candidate:** `c1ae3230b49eed3e7724877640bbf166bd866615` (base `3ced150b26c9f5e3d0613a955a9d998365acbffa`)
**Round:** 3 (after findings 73a69995, e7aa777ea)
**Verdict:** **rejected** — one process finding; the production code itself is sound.

## What was verified (independently, at the assigned candidate)

- **Dispatcher (§Relationship to Notifications).** `spawn_message_consumer()`
  (message_dispatcher.rs) takes the 256-capacity mpsc receiver via
  `AppState::take_message_dispatch_rx()` (single-owner; second call logged
  no-op) and spawns the drain task. `MessageDispatcher` holds
  `Vec<Arc<dyn MessageConsumer>>`; `NotificationBridge` is registered first
  and handles GateFailure→p3, BudgetWarning→p7, AgentError→p5
  AgentEscalation, ReconciliationCompleted→p6 MetaSpecDrift. All three send
  paths clone into the channel (`lib.rs` emit_event, `api/messages.rs:306`,
  `mcp.rs:2028`); `try_send` failures are logged, never block. The old no-op
  drain task in `build_state`/`mem::test_state` is gone. Tenant is resolved
  from the workspace record with unknown-workspace skip (no fabricated
  scope); the old hardcoded-`"default"` tenant in the gate-executor p3 path
  is removed along with `notify_gate_failure`.
- **TTL expiry job (§Implementation Notes).** `spawn_message_expiry()` runs
  hourly (3600s, MissedTickBehavior::Delay), job-registry-tracked as
  `message_expiry` with an on-demand admin trigger; each cycle calls
  `expire_events(now − GYRE_EVENT_TTL_SECS·1000)` and
  `expire_acked_inboxes(now − GYRE_DEAD_INBOX_TTL_SECS·1000)`; both env vars
  default to 604800 with warn-on-invalid fallback. Mem adapter
  `expire_events`/`expire_acked_inboxes` semantics match the SQLite adapter
  (non-agent + age; agent-targeted + ack_reason ∈ {agent_completed,
  agent_orphaned} + age); mem now tracks ack reasons to honor that contract.
- **Spec amendment.** HSI §8 p3 amended in
  specs/system/human-system-interface.md (single creation path via the
  bridge), recorded in that spec's amendments table — spec amended rather
  than contradicted.

### Probe evidence (/tmp/stage/review-evidence/)

- `cargo test -p gyre-server --lib message_dispatcher` — 9 passed.
- `cargo test -p gyre-server --lib reconciliation::tests::reconciliation_completed_fires_when_all_tasks_terminal`
  (the 73a69995 fix) — passed.
- `cargo test -p gyre-server --lib gate_executor::tests::failed_gate_creates_gate_failure_notification_via_bridge`
  — passed. **Mutation probe:** disabling the bridge's `GateFailure` arm
  makes this test FAIL with `left: 0, right: 1` — the test kills real
  regressions, not self-confirming. Source restored after the probe
  (working tree clean).
- Full `cargo test -p gyre-server --lib` — 1499 passed, 0 failed.
- Mechanical checks exit 0: check-arch, check-task-commit-attribution,
  check-dead-message-kinds, check-fabricated-scope-defaults,
  check-scope-literal-defaults, check-mem-port-contracts.

### Observation (not a finding against this task)

No production path calls `acknowledge_all(..., "agent_completed"/
"agent_orphaned")` (only adapter test modules do), so
`expire_acked_inboxes` deletes 0 rows in production until the row-16 Edge
Cases gap (agent completion cleanup feed) is closed. That gap is
pre-existing, explicitly tracked in
specs/coverage/system/message-bus.md row 16, and outside this task's
coverage sections (rows 12/15); the job itself is implemented as the plan
specified.

## Finding (rejection cause)

**Process — frontmatter regression, third recurrence (73a69995,
e7aa777ea).** The e7aa777ea repair (4a10321c3) restored the frontmatter to
`[51221dc02, 9e5a0171d, 5192ff750, 5626a68fc]`, but the assigned head
c1ae3230b — a `gyre-pipeline`-authored "process: record task-162 branch
commits" checkpoint commit one minute later — rewrote it back to
`[5192ff750, 51221dc02]`. The Shipped note (lines 154-156) still claims
"`commits:` now lists 51221dc02, 9e5a0171d, 5192ff750, and the new merge
5626a68fc, and this note is accurate at head" — false at the assigned
candidate. The merge commits carrying the main.rs import-conflict
resolutions are again outside the verifier's commit-scoped rounds.
Mitigations unchanged (attribution check passes; task-owned wiring is in
listed 51221dc02; merge content visible in the base..candidate diff), but
two prior rounds flagged exactly this and the repair does not hold at head.

**Required fix:** re-add 9e5a0171d and 5626a68fc to the `commits:`
frontmatter (or rewrite the Shipped note to not claim they are listed) in a
follow-up commit that survives the executor's checkpoint rewrite — the
regression is reintroduced by the checkpoint tooling deriving the list from
task-labeled commits only.
