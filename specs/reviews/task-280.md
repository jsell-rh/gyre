# Review — task-280 (Cascade testing: real agent-executed test runs, durable result records)

Spec: `specs/system/dependency-graph.md` §Cascade Testing (gap `7014f11632cdabb887a52f2b`, requirement 2: "For each dependent repo, the forge triggers a test run against Repo B's new version").
Candidate under review: `b14fd9049f56c0a040f8ae72dc99710d07f35b4e` (branch `pipeline/task-280/89ce43e27a1846d7b3108aab0d5d231e-1`, base `f611524162da62dc67ba84ef8cb0db04bea4b9df`; rebase repair of saved source `44c7ecfb` onto the task-284 AIBOM base).
Verdict: **approved** — no findings. The EXECUTION and DURABLE RESULTS links are real production code, independently probed where the sandbox budget permits; the rebase preserved the implementation byte-for-byte.

## Round 2 (rebase repair review)

### Rebase fidelity

`git diff 44c7ecfb..b14fd904 -- crates/ specs/coverage/` touches only the incoming base's task-284 files (`api/aibom.rs`, `api/meta_specs.rs`, `gate_executor.rs`) and the coverage matrix conflict resolution — every cascade file (merge_processor.rs, signal_chain.rs, mem.rs, ports, adapters, migration) is unchanged from the saved source. The `specs/coverage/system/dependency-graph.md` conflict was resolved correctly: row 12 (Cascade Testing) carries this task's `implemented` status and row 13 (Version Drift) keeps the incoming task-284 AIBOM notes — both branches' claims survive. Evidence: `/tmp/stage/review-evidence/rebase-fidelity.diffstat.txt`.

### EXECUTION link (verified by inspection + type check)

- `trigger_cascade_tests` (merge_processor.rs) now sets `task.task_type = Some(TaskType::Implementation)` at creation — the exact discriminator `spawn_implementation_workers` (signal_chain.rs:803) requires; pre-fix the loop skipped untyped tasks ("untyped → never spawned"). Read the full spawn path: Backlog scan → re-read/claim (Backlog→InProgress, `assigned_to`) → `try_spawn_worker` → `spawn_agent_core` (agent record, worktree, task assignment). No human or orchestrator action is involved beyond the `require_cascade_tests` policy gates (checked in both merged and dependent workspaces, unchanged).
- The task description is the spawned agent's instruction set and actually reaches the agent: `context_prompt_for_spawn` → `format_context_prompt` renders `### Description / Acceptance Criteria` into the `system://context` prompt (api/prompts.rs:241), injected as `GYRE_PROMPT_CONTEXT` at spawn (api/spawn.rs:2377). The description carries dependent repo, merged repo + id, merge SHA, and the procedure (update the dependency reference to the merged commit/tag, run the full test suite, complete on pass / fail with output).
- Completion/failure flow through the pre-existing `report_cascade_test_result` calls in api/spawn.rs (complete → passed, fail → failed with details) — that file is untouched by this diff.
- Trigger-time context (merged repo id, merge SHA) is persisted in the KV store keyed by cascade task id (`CASCADE_TEST_CTX_NS`), so the durable record carries the merge identity. The KV store is port-backed (`store!(dyn KvJsonStore, ...)`) — durable in DB mode like the result store.
- The new test `scheduler_spawns_agent_for_cascade_test_task` mirrors the established, already-passing `scheduler_spawns_worker_for_implementation_task` (same `chain_state()` fixture — repos r-1/r-2 exist so `resolve_task_repo` resolves; same assertions: task → InProgress, `assigned_to`, agent record with `current_task_id`, worktree in the DEPENDENT repo). Every helper and API it references exists with matching signatures (`DependencyEdge::new` 8-arg form, `WorktreeRepository::find_by_agent`, `AgentWorktree.repository_id`). Pre-fix (task_type unset) this test fails at the InProgress assertion — it is a real kill test.

### DURABLE RESULTS link (verified by probe)

- New port `CascadeTestResultRepository` (gyre-ports) with domain type `CascadeTestResult` (dependent repo, merged repo, merge SHA, cascade task id, pass/fail, failure details, timestamp). Wired via `store!` as `AppState.cascade_test_results` (lib.rs) — not an `Arc<Mutex<HashMap>>` alias (check-in-memory-state-stores green); mem fallback lives in mem.rs as the standard mem-adapter struct.
- All three adapters implement the identical one-row-per-task upsert contract: mem (`retain` + push = replace), Diesel SQLite and PostgreSQL (both `ON CONFLICT (cascade_task_id) DO UPDATE`, structurally identical code; both compile in the default feature set — verified by the adapters test build).
- Migration `2026-11-21-000070_cascade_test_results` is the next unused sequence after `000069` and uses plain portable SQL (TEXT/BOOLEAN/BIGINT, CREATE TABLE/INDEX) — check-migration-versions and check-migration-sql-portability green; the SQLite run is proven by the adapter tests (SqliteStorage::new executes the embedded migrations).
- **Probe run independently (exit 0, 10m22s build):** `cargo test -p gyre-adapters cascade_test` → 5/5 passed, including `result_survives_fresh_storage_instance` (a FRESH `SqliteStorage` over the same DB file retrieves the record — the restart-survival proof), `record_replaces_row_for_same_task` (upsert contract, no duplicates), `failure_details_persisted`, `list_by_dependent_and_merged_repo`. Log: `/tmp/stage/review-evidence/adapters-cascade-test.log`.
- `report_cascade_test_result` writes the record through the port before the (unchanged) event/notification/follow-up links; a missing trigger-time context degrades to empty identity fields with a warning — the record is still written, never skipped (no fail-open on the outcome itself). The mem-level contract is additionally covered by the server-side tests below (deferred).

### Type check

`cargo check -p gyre-server` → **exit 0** (6m25s, `/tmp/stage/bin/cargo`), run independently on the merged tree. Log: `/tmp/stage/review-evidence/server-check.log`.

### Mechanical gates (all exit 0, logs in `/tmp/stage/review-evidence/mechanical-checks.log`)

check-arch, check-mem-port-contracts, check-migration-versions, check-migration-sql-portability, check-in-memory-state-stores, check-task-commit-attribution, check-byte-slice-truncation (the `sha_short` fixed-index slice was also repaired to a char-boundary-safe `chars().take(8)`), check-dead-message-kinds, check-lossy-secret-conversion, check-fabricated-scope-defaults.

### Test-inflation check

None found. Each new assertion kills a real defect mode: task_type unset → spawn test fails; persistence removed → `find_by_task` expect fails; upsert replaced with blind insert → replace test fails; KV hand-off removed → non-empty `merged_repo_id`/SHA assertions fail; description gutted → prompt-content assertions fail. No assertion weakened in existing tests (full diff read — existing tests only gained assertions).

### Non-blocking observations (recorded, not findings)

- The PostgreSQL adapter has no automated test in this sandbox (no PG instance); its code is structurally identical to the SQLite adapter and compiles, and the migration SQL is portable, but the PG run itself is only exercised where a PG instance exists.
- Degraded-context records (task predating the hand-off, or a failed KV write) carry empty `merged_repo_id`/`merge_commit_sha` — logged, and the outcome row is never dropped; acceptable given the hand-off is written in the same transaction window as the task creation.

### Verifier must run (gyre-server test-target compilation exceeds this sandbox's 4 GiB budget)

- `cargo test -p gyre-server --lib signal_chain` — includes `scheduler_spawns_agent_for_cascade_test_task` (task → InProgress, agent record tied to the dependent repo r-1 via its worktree).
- `cargo test -p gyre-server --lib merge_processor` — includes `report_cascade_test_result_persists_durable_pass_record`, `report_cascade_test_result_persists_durable_failure_record`, `report_cascade_test_result_rereport_replaces_record`, plus the pre-existing cascade tests extended with the task_type/prompt-context/KV-hand-off assertions.
- If a PostgreSQL instance is available: run the embedded migrations against PG to confirm migration 000070 executes on both backends.
