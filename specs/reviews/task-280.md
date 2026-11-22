# Review — task-280 (Cascade testing: real agent-executed test runs, durable result records)

Spec: `specs/system/dependency-graph.md` §Cascade Testing (EXECUTION + DURABLE RESULTS links).
Candidate under review: `ca1ffaf8e7ef5d7097d71a797f791043b80e0a1a` (branch `pipeline/task-280/0db5874371894a988fc1730acc5a1bb9-1`, base `2b5189b9`; product commits `3d88b9c0`/`15bae5c0`/`935f4084`, rebase merge `6882bd3d`).
Verdict: **approved** — no findings. Evidence: `/tmp/stage/review-evidence/`.

## What was verified

### EXECUTION (agent spawned with no human/orchestrator action)

- `trigger_cascade_tests` (merge_processor.rs) now sets
  `task.task_type = Some(TaskType::Implementation)` before `state.tasks.create(&task)` —
  the discriminator `spawn_implementation_workers` requires (previously unset →
  "untyped → never spawned"). The pre-fix `&merge_commit_sha[..8]` byte-slice was also
  converted to `chars().take(8)`.
- The scheduler path is unchanged production code that already spawns Implementation
  tasks; `spawn_agent_core` accepts Implementation tasks with no spec-binding gate,
  and `format_context_prompt` renders the task description into the spawned agent's
  `system://context` prompt (`GYRE_PROMPT_CONTEXT`), so the new description —
  dependent repo, merged repo + id, merge SHA, "update the dependency reference to
  the merged commit (or its tag), run the repo's full test suite, complete on pass /
  fail with the failing output" — is the agent's instruction set.
- Completion/failure flow through the existing `report_cascade_test_result` calls in
  api/spawn.rs (pass on complete, fail with details on agent failure) — unchanged.
- The new probe test `signal_chain::tests::scheduler_spawns_agent_for_cascade_test_task`
  asserts the kill-capable outcomes (task InProgress, `assigned_to` agent record,
  worktree in the dependent repo) that only the worker-spawn claim path can produce;
  with task_type unset the scheduler's `continue` makes every one of them fail. It
  mirrors the already-proven fixture pattern (`chain_state` + `scheduler_run_once`,
  same assertions as the sibling delegation-decomposition test). Type-checked here
  (`cargo check -p gyre-server --tests` exit 0); execution deferred to the verifier
  per the 4 GiB budget rule.

### DURABLE RESULTS (survive restart)

- New port `CascadeTestResultRepository` with the one-row-per-task upsert contract;
  mem (code-enforced retain+push), Diesel SQLite, and Postgres
  (`ON CONFLICT (cascade_task_id) DO UPDATE`) implementations — all three compiled
  (gyre-adapters test build) with identical replace semantics.
- Migration `2026-11-21-000070_cascade_test_results` uses the next unused sequence
  number and plain portable SQL (TEXT/BOOLEAN/BIGINT, UNIQUE, CREATE INDEX) in the
  shared embedded dir both backends run; check-migration-versions and
  check-migration-sql-portability both pass; the SQLite adapter tests construct
  `SqliteStorage` (running pending migrations) and pass, proving it executes on SQLite.
- Wired via the `store!` macro (`AppState.cascade_test_results`) — DB-backed in
  SQLite/PG modes; check-in-memory-state-stores passes.
- Executed probe: `cargo test -p gyre-adapters cascade` — **5/5 passed**, including
  `result_survives_fresh_storage_instance` (fresh `SqliteStorage` over the same DB
  file recovers the record — restart durability) and `record_replaces_row_for_same_task`
  (upsert contract). `report_cascade_test_result` persists the record on every report
  (pass and fail, failure details on fail) before the unchanged
  cascade_test_passed/failed events, member notifications, and auto-created
  cascade-test-failure follow-up task (all verified present in the fail branch).
- Trigger-time context (merged repo id, merge SHA) persisted in the `store!`-wired KV
  store keyed by cascade task id; the missing-context path degrades with a warning
  rather than skipping the record.

### Mechanical checks (all exit 0)

check-mem-port-contracts, check-migration-sql-portability, check-in-memory-state-stores,
check-migration-versions, check-byte-slice-truncation, check-arch,
check-scope-literal-defaults, check-fabricated-scope-defaults,
check-lossy-secret-conversion, check-inert-enforcement, check-relative-path-defaults,
check-forwarded-header-trust, check-task-commit-attribution (all three frontmatter
commits are ancestors of the candidate), `cargo fmt --check` on the four touched
crates, `cargo check -p gyre-server`, `cargo check -p gyre-server --tests`.

## Verifier must run (deferred per budget rule)

- `cargo test -p gyre-server signal_chain::tests::scheduler_spawns_agent_for_cascade_test_task`
- `cargo test -p gyre-server merge_processor::tests::report_cascade_test_result_persists_durable_pass_record ..._persists_durable_failure_record ..._rereport_replaces_record ..._creates_tasks_for_dependents`
- existing guards: `trigger_cascade_tests_creates_task_per_dependent`,
  `trigger_cascade_tests_skips_when_policy_disabled`,
  `trigger_cascade_tests_respects_dependent_workspace_opt_out`,
  `report_cascade_test_result_failed_creates_follow_up_task`,
  `report_cascade_test_failure_notifies_workspace_members`
- PG-instance smoke of migration 000070 + the upsert contract if PostgreSQL is available.

## Non-blocking observations

- The SQL upsert's DO UPDATE set-list does not refresh the row `id` on re-report (mem
  replaces the whole record). Unobservable through the port — no find-by-record-id
  consumer exists; `id` is a per-report UUID.
- The degraded-context path records empty `merged_repo_id`/`merge_commit_sha` with a
  warning instead of skipping — intentional ("outcome never lost") and logged.
- The original discovery's dead-UI-trigger / missing cascade-test query API note is
  outside this task's required behavior (which scoped EXECUTION + DURABLE RESULTS).
