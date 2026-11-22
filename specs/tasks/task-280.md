---
title: "Cascade testing: real agent-executed test runs against the merged version, with durable result records"
spec_ref: "Spec gap spec-cab97078efffe0fbfb7339be / cascade-test-execution / ed9f34316476"
depends_on: []
progress: complete
commits: ["3d88b9c0b70ac9433ef890ae3e74361f4ea18b7f", "15bae5c0fb9d06aa98ac05b08fd46a0264845c23", "935f408420804f6df661657ddca7e6e554e5b8cd"]
---

## Required behavior

The identification, task-filing, event, notification, and failure follow-up links are real and unchanged; this task closes the missing EXECUTION link so "the forge knows the graph and runs the tests" is true. (1) EXECUTION: the cascade-test task created by trigger_cascade_tests must actually run without human or orchestrator intervention beyond the require_cascade_tests policy gates: set task_type = Implementation (the discriminator the worker scheduler spawns — signal_chain.rs currently skips untyped tasks: "untyped → never spawned") or an equivalent dedicated spawn path in the scheduler; the spawned agent's prompt carries the full context — dependent repo, merged repo, merge SHA — and instructs it to run the dependent repo's test suite against Repo B's new version (update the dependency reference to the merged commit/tag, run the repo's tests), so completion/failure is a real test outcome flowing through the existing report_cascade_test_result path (agent complete → passed; agent failed → failed with details). (2) DURABLE RESULTS: report_cascade_test_result persists a CascadeTestResult record (dependent repo id, merged repo id, merge SHA, cascade task id, outcome pass/fail, failure details, timestamp) through a new gyre-ports trait implemented by the mem, Diesel SQLite, AND Postgres adapters (contract enforced identically in all three — check-mem-port-contracts), with a migration using the next unused 6-digit sequence number and SQL portable across both backends (check-migration-sql-portability), wired via the store! macro — NOT an Arc<Mutex<HashMap>> in AppState (check-in-memory-state-stores; a restart must not orphan results). The existing one-shot workspace events and notifications remain.

## Acceptance Criteria

- [ ] With require_cascade_tests enabled in both the merged and dependent workspaces, merging a change in repo B that repo A depends on results in an agent actually spawned for the cascade-test task (an agent record tied to repo A and the task exists) with no human action, and the task carries a spawnable task_type at creation.
- [ ] On agent completion, a durable CascadeTestResult (passed) is retrievable from the store after a server restart; on agent failure, the record carries the failure details and the existing follow-up task and cascade_test_failed event fire.
- [ ] The new port has mem, SQLite, and Postgres implementations with the failure constraints enforced identically, and the migration runs on both storage backends.

## Verification intent

Enable require_cascade_tests in both workspaces and merge a change in repo B on which repo A depends: pre-fix the created cascade-test task has task_type unset and signal_chain's scheduler loop explicitly skips untyped tasks ("untyped → never spawned"), so no agent is ever spawned, no test of the dependent repo ever runs, and no durable result record exists anywhere — cascade outcomes live only as one-shot workspace events. The probe (an agent spawned for the task plus a CascadeTestResult that survives a server restart) must fail until execution and persistence land.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/dependency-graph.md`. Source: `678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce`; plan: `1745ed1044f3de3976ac34455f356a3300421816ea7a6e7574e2e5d9ec4ad3a4`.

[
  {
    "id": "7014f11632cdabb887a52f2b",
    "decision": "gap",
    "requirement_quote": "2. For each dependent repo, the forge triggers a test run against Repo B's new version",
    "detail": "The identification, task-filing, event, notification, failure follow-up, and orchestrator-notification links are real: on merge, trigger_cascade_tests (gated on require_cascade_tests in both the merged and dependent workspaces) lists dependents from the graph, creates a High-priority 'cascade-test' task per dependent, emits cascade_test_triggered to the dependent workspace, and notifies members; when an agent running a cascade-test task completes or fails, report_cascade_test_result emits cascade_test_passed/failed, auto-creates a failure follow-up task, and notifies the workspace. The missing link is execution: the cascade-test task is created with task_type unset, and the worker scheduler explicitly never spawns untyped tasks — nothing in the forge dispatches an agent to actually run the dependent repo's tests, and there is no API endpoint to trigger or query cascade tests (the UI's trigger call is a dead reference, see §Impact Analysis/§UI). The spec's claim 'the forge knows the graph and runs the tests' is therefore only partially true: the forge files paperwork for a test run that nothing automatically executes.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/merge_processor.rs",
        "quote": "            format!(\"Cascade test: {dep_repo_name} against {merged_repo_name}@{sha_short}\"),",
        "detail": "trigger_cascade_tests creates the per-dependent cascade-test task (High priority, labels [cascade-test, auto-created]) — but the task is created with no task_type set."
      },
      {
        "file": "crates/gyre-server/src/signal_chain.rs",
        "quote": "            continue; // Delegation/Coordination → orchestrators; untyped → never spawned",
        "detail": "The worker-spawn cycle skips every task whose task_type is not Implementation (and the orchestrator claim path only takes Delegation/Coordination), so cascade-test tasks sit in the Backlog unless a human manually spawns an agent — the forge never runs the tests itself."
      },
      {
        "file": "crates/gyre-server/src/api/spawn.rs",
        "quote": "                crate::merge_processor::report_cascade_test_result(&state, &task, true, None).await;",
        "detail": "Result reporting is wired to agent completion (and to the fail path with passed=false), proving the intended execution model — but only an agent that happens to be manually spawned on the cascade-test task ever reaches this code."
      }
    ]
  }
]

## Shipped

The EXECUTION and DURABLE RESULTS links of dependency-graph.md §Cascade
Testing are implemented in production code; the identification, task-filing,
event, notification, and failure follow-up links are unchanged from before
this task.

### This round (rebase repair against current main)

The published task-280 head (d21b6c6) conflicted with current main
(2b5189b9, task-278 every-push detection work). The merge was resolved in
commit 6882bd3d: the sole conflict, specs/coverage/system/dependency-graph.md,
was resolved by keeping this branch's row 12 (Cascade Testing, task-280
implemented) and main's task-278 row-5/6/8 re-evidence plus its header note,
with the combined coverage count (12/16) recomputed from the merged table.
No production-code conflicts: main's incoming changes touch git_http.rs and
api/dependencies.rs, none of the task-280 cascade files. Evidence on the
merged tree: `cargo check -p gyre-server` passes (exit 0); `cargo test -p
gyre-adapters cascade` passes 5/5; `cargo fmt --check` clean for
gyre-ports, gyre-domain, gyre-adapters, and gyre-server; check-mem-port-contracts,
check-migration-sql-portability, check-in-memory-state-stores, and
check-migration-versions all pass.

### EXECUTION (agent actually spawned, no human action)

- `trigger_cascade_tests` (crates/gyre-server/src/merge_processor.rs) creates
each per-dependent cascade-test task with
`task_type = Some(TaskType::Implementation)` — the discriminator the worker
scheduler spawns (previously unset, so signal_chain.rs skipped the task:
"untyped → never spawned").
- The task description is the spawned agent's instruction set, rendered into
its `system://context` prompt: full context (dependent repo, merged repo +
id, merge SHA) and the procedure — update the dependency reference to the
merged commit (or its tag), run the repo's full test suite, complete the
agent run on pass / fail the run with the failing test output on failure.
Completion and failure flow through the existing `report_cascade_test_result`
path (api/spawn.rs), unchanged.
- The trigger-time context (merged repo id, merge SHA) is persisted in the KV
store keyed by cascade task id so the durable result record carries the full
identity of the triggering merge.

### DURABLE RESULTS (survive restart)

- New port `CascadeTestResultRepository` (gyre-ports) with a
  `CascadeTestResult` domain record (dependent repo id, merged repo id,
  merge SHA, cascade task id, pass/fail, failure details, timestamp).
- Implemented by all three adapters with the identical one-row-per-task
  upsert contract (record replaces the row for the same cascade_task_id):
  mem (MemCascadeTestResultRepository), Diesel SQLite, and Postgres
  (ON CONFLICT (cascade_task_id) DO UPDATE).
- Migration 2026-11-21-000070_cascade_test_results uses plain portable SQL
  (TEXT/BOOLEAN/BIGINT, no dialect-only constructs) so it runs on both
  backends; sequence number 000070 verified unused.
- Wired via the `store!` macro as `AppState.cascade_test_results` (lib.rs) —
  no Arc<Mutex<HashMap>> state alias; a restart does not orphan results.
- `report_cascade_test_result` persists the record on every report (pass and
  fail; failure details carried on fail), in addition to the unchanged
  one-shot cascade_test_passed/failed events, member notifications, and
  auto-created failure follow-up task.

### Test evidence

- Passed here (smaller explicit packages): `cargo test -p gyre-adapters
cascade` — 5/5, including `result_survives_fresh_storage_instance` (a fresh
SqliteStorage instance over the same database file recovers the record,
proving restart durability) and `record_replaces_row_for_same_task` (the
upsert contract).
- gyre-server test compilation is deferred to the independent verifier per
the 4 GiB budget rule. The verifier must execute the focused server tests:
  - `signal_chain::tests::scheduler_spawns_agent_for_cascade_test_task`
    (the EXECUTION probe: merge in repo B → cascade-test task claimed by
    the worker-spawn cycle → agent record tied to the dependent repo)
  - `merge_processor::tests::report_cascade_test_result_persists_durable_pass_record`,
    `..._persists_durable_failure_record`, `..._rereport_replaces_record`
  - `merge_processor::tests::trigger_cascade_tests_creates_tasks_for_dependents`
    (asserts the task carries task_type = Implementation at creation and
    that the description is the agent instruction set)
  - existing trigger/report behavioral tests:
    `trigger_cascade_tests_creates_task_per_dependent`,
    `trigger_cascade_tests_skips_when_policy_disabled`,
    `trigger_cascade_tests_respects_dependent_workspace_opt_out`,
    `report_cascade_test_result_failed_creates_follow_up_task`,
    `report_cascade_test_failure_notifies_workspace_members`.
