# Review — task-162 (Message bus — wire MessageConsumer dispatcher and event TTL expiry job)

Spec: `specs/system/message-bus.md` §Relationship to Notifications (lines 459–475) and §Implementation Notes (event TTL).
Candidate: `aba936036e3e290e8ae96ad4384bd698655efb82` (merge of branch work into main tip `ed57c723`, merge commit `e19285cc`, merge-repair round for finding d208c79f).
Commits under review per `commits:` frontmatter: `cb865b79`, `6b72222c`, `f5346ff0`, `244a98cc` — all ancestors of the candidate; attribution check passes.
Verdict: **complete** (approved).

## Round 1 (independent review of aba93603)

Test runs (this checkout, branch `pipeline/task-162/4a91fef16c814c7aae998ccfbb3e28f2-2` at `aba93603`, SKIP_WEB_BUILD=1, /tmp/stage/bin/cargo; evidence under /tmp/stage/review-evidence):

- `cargo test -p gyre-server --lib message_dispatcher` → **9 passed, 0 failed** (bridge unit tests for all four kinds, viewer-role exclusion, unknown-workspace skip, dispatcher fan-out, end-to-end `spawn_message_consumer`, TTL retention boundaries, env-var TTL override).
- `cargo test -p gyre-server --lib gate_executor` → **25 passed, 0 failed**, incl. `failed_gate_creates_gate_failure_notification_via_bridge` (run_gate → emit_event → dispatcher → bridge → exactly one p3).
- `cargo test -p gyre-server --lib notification` → **25 passed, 0 failed** (no callers broken by `notify_gate_failure` removal).
- `cargo test -p gyre-server --lib reconciliation` → **1 passed** (kind roundtrip intact after `emit_reconciliation_completed` rewrite).
- `cargo test -p gyre-server --lib jobs::` → **6 passed** (registry incl. the new `message_expiry` registration).
- `cargo check -p gyre-server --bins --tests` → **clean, zero warnings**. The `--bins` target matters: it compiles `main.rs`, where `spawn_message_consumer` + `spawn_message_expiry` are wired — lib tests alone would not catch a binary-path compile break.
- **Mutation probe** (temporary edit, restored, tree verified clean): replacing the `MessageKind::GateFailure` arm in `NotificationBridge::on_message` with a no-op → `failed_gate_creates_gate_failure_notification_via_bridge` **FAILS** (`left: 0, right: 1 — bridge must create the p3 notification`). The end-to-end test is genuinely anchored to the bridge's production behavior, not incidental. Restored and re-ran the full message_dispatcher suite green.
- Mechanical checks → all exit 0: `check-arch.sh`, `check-dead-message-kinds.sh`, `check-scope-literal-defaults.sh`, `check-fabricated-scope-defaults.sh`, `check-inert-enforcement.sh`, `check-task-commit-attribution.sh`.
- Host-tooling note (not a candidate defect): `check-notification-audience.sh` / `check-notification-priority.sh` exit 2 with mawk awk parse errors before scanning any file; `git diff ed57c723..aba93603 -- scripts/` is empty, so the failures are byte-identical at pristine base — mawk-vs-gawk host issue, same class as the `check-notification-test-coverage.sh` note in the shipped record.

Verified working (no findings):

- **Dispatcher is single-owner and real.** `AppState` now holds the receiver as `Arc<Mutex<Option<Receiver>>>`; `take_message_dispatch_rx()` atomically takes it, so a second `spawn_message_consumer` call is a logged no-op — asserted by `spawn_message_consumer_end_to_end` (`rx.is_none()` after the second spawn). The old no-op drain loop in `build_state`/`mem::test_state_inner` is gone.
- **All three send paths clone into the channel** exactly as the spec's mechanism paragraph requires: `emit_event` (lib.rs:531), REST `api/messages.rs:306`, MCP `mcp.rs:2008`. `try_send` errors are handled per spec: `Full` → warn + drop ("consumer drops do not affect delivery guarantees"), `Closed` → debug. The send hot path never blocks.
- **NotificationBridge is a genuine consumer with real lookups.** GateFailure resolves MR → author agent → `spawned_by`, with tenant resolved from the workspace record and skip-on-unknown (no fabricated `"default"` scope — notably *better* than the removed synchronous path, which passed literal `tenant_id: "default"`). BudgetWarning/ReconciliationCompleted fan out to Admin/Developer/Owner members (viewer exclusion asserted); AgentError → p5 AgentEscalation for the spawning user. All repository errors are warned and skipped, never panic.
- **The GateFailure rewiring is a single creation path, not a duplicate.** The gate executor's synchronous p3 block and `notify_gate_failure` are fully removed (grep: no remaining callers); the event emitted in `run_gate` (pre-existing) is now the only trigger, and the HSI §8 p3 amendment is recorded in HSI's amendment table, consistent with message-bus.md §Relationship to Notifications ("when it receives a `GateFailure` message, it creates a notification"). Legitimate spec amendment, not a contradiction.
- **TTL expiry job matches the existing background-job style.** Hourly `tokio::time::interval` (first tick immediate — one expiry pass at startup), `MissedTickBehavior::Delay`, `mark_scheduled("message_expiry")` + `record_cycle` per cycle, wired in `main.rs` alongside the other `spawn_*` jobs. Registry `register()` only installs the definition/handler (no loop), so `POST /admin/jobs/message_expiry/run` triggers on demand with **no double scheduling** — the pattern used by mirror_sync/speculative_merge et al.
- **Env vars are respected with fail-safe defaults.** `run_message_expiry` reads `GYRE_EVENT_TTL_SECS`/`GYRE_DEAD_INBOX_TTL_SECS` per cycle via `env_u64` (default 604800, warn-on-invalid fallback), `saturating_sub` for the cutoff. `message_expiry_env_ttl_respected` is a hard test: with TTL=1s only the >1s-old event is deleted — if the env var were ignored, the default 7-day TTL would delete 0 and the `events_deleted == 1` assertion would fail.
- **Mem adapter now enforces the same dead-inbox contract as SQLite.** Verified line-by-line: `expire_events` retains agent-targeted or fresh (SQLite: `to_type != 'agent' AND created_at < cutoff`); `expire_acked_inboxes` deletes only agent-targeted messages whose ack reason is `agent_completed`/`agent_orphaned` (SQLite: same filter on `ack_reason`); `acknowledge` is idempotent-only-if-unacked and records `"explicit"`; `acknowledge_all` records the passed reason. Lock ordering (store → ack_reasons) is consistent across all three methods. The TTL retention test pins the boundaries: unacked Directed and explicitly-acked messages survive both expiries.
- **Coverage scope respected.** Diff vs base touches only the 13 listed files (server sources + spec/coverage/task bookkeeping); no scripts, no exemption files, no gates weakened, no tests deleted. Coverage rows 12/15 are marked `implemented` (not `verified`) with notes that accurately describe the code; SUMMARY.md's message-bus row updated 85% → 100% to match. The merge-repair resolution of the `specs/coverage/system/message-bus.md` conflict preserves both sides (main's row 7 `verified` + task-162's rows 12/15 `implemented`), header 14/14 (3 n/a) consistent with the table.

Findings:

- (none blocking)

Non-blocking observations, all pre-existing at base `ed57c723` and tracked in coverage files outside this task's scope (recorded so they are not lost):

1. `acknowledge_all(agent_completed/agent_orphaned)` still has no production callers (only tests) — message-bus.md coverage row 16 already tracks this as a separate gap ("Consider splitting"). Until that wiring lands, `expire_acked_inboxes` cleans nothing in production; the job and the adapter contract (this task's scope) are real and tested regardless.
2. `emit_reconciliation_completed` itself has no production callers (pre-existing; meta-spec-reconciliation coverage is 36% — upstream flow not yet built). The bridge correctly consumes the kind when emitted.
3. `BudgetWarning`/`AgentError` have no server-side emitters (HSI coverage row 43 tracks the BudgetWarning emitter as task-191 overlap); both kinds can still arrive via agent-sent bus messages through the REST/MCP send paths, which the bridge handles.
