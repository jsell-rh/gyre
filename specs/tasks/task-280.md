---
title: "Cascade testing: real agent-executed test runs against the merged version, with durable result records"
spec_ref: "Spec gap spec-cab97078efffe0fbfb7339be / cascade-test-execution / ed9f34316476"
depends_on: []
progress: ready-for-review
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

The EXECUTION and DURABLE RESULTS links of dependency-graph.md §Cascade Testing
are implemented in production code; the identification, task-filing, event,
notification, and failure follow-up links are unchanged. This round finalizes
the checkpoint: formatting verified, mechanical gates re-run green, evidence
re-collected under /tmp/stage/review-evidence, and attribution restored.

### EXECUTION (agent actually spawned, no human action)

- `trigger_cascade_tests` (crates/gyre-server/src/merge_processor.rs) creates
each per-dependent cascade-test task with
`task_type = Some(TaskType::Implementation)` — the discriminator the worker
scheduler spawns (previously unset, so signal_chain.rs skipped the task:
"untyped → never spawned").
- The task description is the spawned agent's instruction set, rendered into
its `system://context` prompt: full context (dependent repo, merged repo + id,
merge SHA) and the procedure — update the dependency reference to the merged
commit (or its tag), run the repo's full test suite, complete the agent run on
pass / fail the run with the failing test output on failure. Completion and
failure flow through the existing `report_cascade_test_result` path
(api/spawn.rs), unchanged.
- The trigger-time context (merged repo id, merge SHA) is persisted in the KV
store keyed by cascade task id so the durable result record carries the full
identity of the triggering merge.

### DURABLE RESULTS (survive restart)

- New port `CascadeTestResultRepository`
(crates/gyre-ports/src/cascade_test_result.rs) with domain type
`gyre_domain::CascadeTestResult` (dependent repo id, merged repo id, merge
SHA, cascade task id, pass/fail, failure details, timestamp).
- Implemented by all three adapters with the identical contract — one result
row per cascade task, re-reporting REPLACES the row (upsert on
`cascade_task_id`): mem (code guard), Diesel SQLite, and PostgreSQL
(ON CONFLICT DO UPDATE).
- Migration `2026-11-21-000070_cascade_test_results` uses the next unused
sequence number and plain portable SQL (TEXT/BOOLEAN/BIGINT), runnable by
both the SQLite and PostgreSQL embedded runners.
- Wired via the `store!` macro as `AppState.cascade_test_results`
(crates/gyre-server/src/lib.rs) — not an `Arc<Mutex<HashMap>>`; the
test-support AppState constructors are wired to the same store.
- `report_cascade_test_result` persists the record on every report (pass and
fail) in addition to the existing one-shot workspace events, notifications,
and failure follow-up task, which remain unchanged.

### Test evidence (this round, /tmp/stage/review-evidence)

- `cargo test -p gyre-adapters --lib cascade_test_result`: 5/5 passed in this
sandbox (adapters-cascade-test.log) — including
`result_survives_fresh_storage_instance` (a fresh storage instance over the
same DB file retrieves the record — restart survival) and
`record_replaces_row_for_same_task` (port upsert contract).
- `cargo check -p gyre-server -p gyre-adapters -p gyre-ports -p gyre-domain`:
clean, and `cargo check -p gyre-server --tests`: clean (server-check-tests.log)
— the server-side tests compile; their full test-binary run is deferred to the
independent verifier under the 4 GiB build budget (exact commands and
acceptance-criteria mapping in verifier-test-list.md:
`scheduler_spawns_agent_for_cascade_test_task` proves AC-1; the
`report_cascade_test_result_persists_durable_*` and `..._rereport_replaces_record`
tests prove AC-2).
- `cargo fmt --all -- --check`: clean — the cascade-test Rust files are
formatted.
- Mechanical gates re-run green this round (logs in review-evidence):
check-arch, check-migration-versions, check-migration-sql-portability,
check-mem-port-contracts, check-in-memory-state-stores.
check-task-commit-attribution needs full history and is left to CI; the three
`implement(task-280)` product-surface commits are recorded in the frontmatter.
- Coverage row 12 (Cascade Testing) in
specs/coverage/system/dependency-graph.md is `implemented` with source
evidence; no other rows touched.
