# Review — task-162: Message bus — wire MessageConsumer dispatcher and event TTL expiry job

- **Candidate:** `bdb2e1876b4cb3035310f0daa84fcbae99ac7b70` (branch `pipeline/task-162/dd7cb8dfcdf243a687ac2a2c9305b462-1`)
- **Base:** `047d652495dde3b1ae6ddbea4f8cf1b077a3c18f`
- **Round:** 3 (review of the round-2 repairs in `55ac5b52` plus the carried dispatcher/TTL work)
- **Spec:** message-bus.md §Relationship to Notifications, §Implementation Notes
- **Verdict:** **Approved** — no blocking findings. Rust test execution deferred to the independent verifier (4 GiB build-wrapper limit; probes recorded in the review evidence directory).

## What was reviewed

Full diff base→candidate (17 files, +1454/−227): new `crates/gyre-server/src/message_dispatcher.rs` (dispatcher + NotificationBridge + TTL job + 11 tests), `lib.rs` single-owner receiver (`take_message_dispatch_rx`) replacing the no-op drain, drop-logging `try_send` on all three send paths (`emit_event`, `api/messages.rs`, `mcp.rs message.send`), removal of the synchronous p3/p6/spec-policy notification calls in favor of the single bridge path, `mem.rs` adapter alignment with the SQLite/Postgres `ack_reason` contract, `jobs.rs` `message_expiry` registry entry, `main.rs` wiring, and the HSI §8 p3 spec amendment.

## Acceptance criteria

- `spawn_message_consumer()` exists, spawns the background drain task, takes single ownership of the receiver (second spawn is a logged no-op), wired in `main.rs` after `build_state`. ✔
- NotificationBridge is the registered consumer and creates notifications from bus events: GateFailure → rank-3 for the MR author's spawning user (spec-policy `gate_type "spec_enforcement"` routed to `notify_spec_policy_violation`), AgentError → rank-2 AgentEscalation, ReconciliationCompleted → rank-6 MetaSpecDrift for Admin/Developer/Owner. Verified as the *only* remaining production caller of `notify_gate_failure`/`notify_spec_policy_violation` (single creation path). ✔
- Event TTL expiry job (`spawn_message_expiry`) runs hourly (3600 s, `MissedTickBehavior::Delay`), calls `expire_events(now − GYRE_EVENT_TTL_SECS·1000)`; also registered as `message_expiry` in the admin job registry. ✔
- Dead inbox cleanup: `expire_acked_inboxes(now − GYRE_DEAD_INBOX_TTL_SECS·1000)` in the same cycle; mem adapter now mirrors the SQLite/Postgres `ack_reason ∈ {agent_completed, agent_orphaned}` contract (previously it deleted *any* acked message — a real port-contract divergence, now fixed; `check-mem-port-contracts.sh` exit 0). ✔
- Env vars respected via `env_u64` (warn-and-fall-back on invalid); tested with distinct TTLs that fail if the env is ignored. ✔
- `cargo test --all`: blocked in this sandbox by the build wrapper (gyre-server codegen > 4 GiB). `cargo check -p gyre-server --lib` and `--lib --tests` both exit 0; `cargo fmt --check` exit 0. Required test probes listed in `review-evidence/probes.txt`.

## Round-2 repair verification (prior findings)

1. **failing_gate_notifies_author_with_gate_result_payload broken** — fixed: the test now spawns `spawn_message_consumer` and polls with the existing `eventually` helper; a second end-to-end test (`failed_gate_creates_gate_failure_notification_via_bridge`) pins the whole run_gate → event → bridge chain.
2. **Duplicate spec-policy GateFailure notifications** — fixed: `report_spec_policy_violation` emits the event only; the bridge routes `gate_type "spec_enforcement"` to `notify_spec_policy_violation` with the original title/body; both merge-processor tests spawn the dispatcher and poll. Exactly one notification per violation (asserted `notifs.len() == 1` in the dedicated bridge test).
3. **BudgetWarning bridge fan-out (duplicate + over-broad)** — removed; replaced by `notification_bridge_does_not_create_budget_warning_notifications`, which fails if any bridge budget fan-out returns. The synchronous budget-check path (HSI §8 p7, spawning user + workspace Admin, per-period dedupe) is unchanged.

## Additional verification performed

- **No other duplication paths:** enumerated every `MessageKind::GateFailure` emitter (gate_executor Workspace event, merge_processor spec_enforcement Workspace event — both consumed once), every `ReconciliationCompleted` emitter (`maybe_emit_reconciliation_completed` only; `run_conformance_sweep` and stale-pin detection create MetaSpecDrift notifications directly without emitting a consumed kind), and every `AgentError` emitter (none in production; kind is reachable only via the server). ralph_loop's agent-directed copies are `MessageKind::GateResult`, which the bridge ignores by kind. `AtomicGroupFailed`, `StaleSpecWarning`, `MergeQueuePaused/Resumed`, `MrReverted`, `Escalation`, `SpeculativeConflict` are not consumed. No production path both emits a consumed kind and creates the same notification directly.
- **No forgery path:** GateFailure, ReconciliationCompleted, AgentError are all `server_only` kinds — rejected on user/agent-origin sends, so a client cannot mint a bus message that triggers notification creation.
- **No fabricated scopes:** bridge resolves tenant via workspace lookup and skips (warns) on failure; `mr_id`/`agent_id` are resolved from storage, and unresolvable authors are skipped, not defaulted.
- **Job wiring matches house style:** `spawn_message_expiry` mirrors `spawn_budget_daily_reset` (`mark_scheduled` + own loop + `record_cycle`); `start_job_registry` explicitly does not spawn schedulers, so no double-run.
- **Test strength:** the TTL tests use boundary fixtures (unacked Directed retained, explicit-ack retained, dead-inbox deleted, fresh event retained) that fail under the old mem behavior or an ignored env var; e2e tests go through the real `emit_event` send path and the real dispatcher.
- **Spec amendment legitimacy:** HSI §8 p3 amended to "Via MessageConsumer consuming GateFailure events" — this reconciles HSI with message-bus.md §Relationship to Notifications (the authoritative spec for this task, which already specified the notification system as a GateFailure consumer), recorded in the HSI amendment ledger row. Not a contradiction of the contract.
- **Mechanical checks** (all exit 0 in this sandbox): check-arch, check-dead-message-kinds, check-mem-port-contracts, check-inert-enforcement, check-fabricated-scope-defaults, check-scope-literal-defaults, check-lossy-secret-conversion, check-forwarded-header-trust, check-unbounded-external-http, check-relative-path-defaults, check-byte-slice-truncation, check-notification-toggle-coverage, check-migration-versions, check-abac-route-registry, check-mcp-write-tools, check-task-commit-attribution (against the committed candidate tree), cargo fmt --check. check-notification-test-coverage and check-event-emission-coverage exit 2 identically on the assigned base (verified in a detached worktree) — pre-existing, not candidate-introduced.

## Non-blocking observations

1. **Comment/matrix imprecision (documentation only):** `notification_bridge_ignores_agent_directed_gate_failure_copy` and coverage row 12 describe ralph_loop::on_gates_failed as emitting an "Agent-directed GateFailure copy"; it actually emits `MessageKind::GateResult` (Agent-directed), which the bridge ignores by kind match. The Destination guard in `on_gate_failure` is therefore defensive-but-currently-unreachable in production. Behavior is correct; only the stated rationale is imprecise. No action required for approval.
2. `spawn_message_consumer_end_to_end` relies on `test_state()`'s mem repositories; acceptable for a unit-level e2e of the dispatch chain, and the gate_executor/merge_processor tests cover the same chain against seeded MRs.

## Verifier must run

See `/tmp/stage/review-evidence/probes.txt` (reproduced in the shipped notes of the task file): the message_dispatcher test module, the two gate_executor notification tests, both merge-processor spec-policy tests, the reconciliation wave-completion test, the merge_processor/gate_executor/budget/reconciliation suites, and `cargo test --all`.
