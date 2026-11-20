---
title: "Cascade testing: real agent-executed test runs against the merged version, with durable result records"
spec_ref: "Spec gap spec-cab97078efffe0fbfb7339be / cascade-test-execution / ed9f34316476"
depends_on: []
progress: not-started
commits: []
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
