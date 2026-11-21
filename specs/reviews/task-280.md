# Review — task-280 (Cascade testing: real agent-executed test runs, durable result records)

Spec: `specs/system/dependency-graph.md` §Cascade Testing (gap `7014f11632cdabb887a52f2b` — the missing EXECUTION link).
Candidate under review: `3fcff50086d49d4ad6dc347020a50e69f88f0d8a` (branch `pipeline/task-280/f90e687f5426402e82db604f9848a0e7-2`, base `9e7cf62e4b3318b9e5aba9d71ab2a2fb64dcc6ce`; product commits `935f4084` + `15bae5c0`, both attributed in the task frontmatter).
Verdict: **approved** — no findings. Both required links (EXECUTION, DURABLE RESULTS) verified by inspection, mechanical gates, focused adapter tests (5/5 green, including restart survival), and a clean `cargo check -p gyre-server --tests`. The gyre-server test-binary run is deferred to the verifier (4 GiB budget) with an exact list (`/tmp/stage/review-evidence/verifier-test-list.md`).

## Round 1

Probes run (checkout at candidate `3fcff500`; working tree carried only the executor's protected-task-text restoration of `specs/tasks/task-280.md`, not part of the product diff):

- `/tmp/stage/bin/cargo test -p gyre-adapters cascade_test` → **exit 0**, 5/5 passed (`result_survives_fresh_storage_instance` is the restart-survival kill test: a fresh `SqliteStorage` over the same DB file retrieves the record).
- `/tmp/stage/bin/cargo check -p gyre-server --tests` → **exit 0** (7m24s) — all new server tests compile.
- Mechanical gates all exit 0: check-arch, check-migration-versions, check-migration-sql-portability, check-mem-port-contracts, check-in-memory-state-stores, check-byte-slice-truncation.
- Full diff read (18 files, +1147/−22); grep sweeps of the spawn/report/scheduler/prompt surfaces.
- Evidence under `/tmp/stage/review-evidence/` (probe-summary.md, adapters-cascade-test.log, server-check-tests.log, mechanical-gates-summary.txt, verifier-test-list.md).

Verified (no findings):

- **EXECUTION.** `trigger_cascade_tests` (merge_processor.rs) now sets `task_type = Some(TaskType::Implementation)` at task creation — the discriminator `spawn_implementation_workers` (signal_chain.rs:782) spawns; pre-fix the task was untyped and explicitly skipped ("untyped → never spawned"). The spawn path is the real production machinery: claim → `try_spawn_worker` → `spawn_agent_core` with scheduler system auth, creating the agent record, worktree (repository_id = dependent repo), and task assignment — proven at runtime by the pre-existing `scheduler_decomposes_delegation_into_ordered_subtasks` test using the same `chain_state()` fixture and asserting the identical outcome for Implementation tasks; the new `scheduler_spawns_agent_for_cascade_test_task` test differs only in creating the task through `trigger_cascade_tests` and asserting the type at creation, so it fails pre-fix on both the type assertion and the (absent) claim. No human or orchestrator action is involved beyond the policy gates; the merge-time trigger call site is unchanged.
- **Agent prompt carries the full context.** The task description (dependent repo, merged repo + id, merge SHA, "update the dependency reference to the merged commit … run this repo's full test suite … complete on pass / fail with the failing output") is rendered into the spawned agent's `system://context` prompt: `format_context_prompt` (api/prompts.rs:213) emits it under "Description / Acceptance Criteria", and `spawn_agent_core` (api/spawn.rs:2195) injects `context_prompt_for_spawn` into the container environment. Completion/failure flows through the unchanged `report_cascade_test_result` call sites in api/spawn.rs (complete → passed; fail → failed with details).
- **DURABLE RESULTS.** `report_cascade_test_result` persists a `CascadeTestResult` (dependent repo, merged repo, merge SHA, cascade task id, pass/fail, failure details, timestamp) through the new `CascadeTestResultRepository` port on every report, in addition to the unchanged one-shot events, notifications, and failure follow-up task. Wired via the `store!` macro (`AppState.cascade_test_results`) — PgStorage/SqliteStorage when a DB backend is configured, mem otherwise; both test-support AppState constructors wired to the same store. Not an `Arc<Mutex<HashMap>>` (check-in-memory-state-stores green).
- **Port contract parity.** SQLite and PG adapters enforce the identical one-row-per-task upsert (`ON CONFLICT (cascade_task_id) DO UPDATE`); the mem adapter enforces the same replace semantics in code (retain + push). Adapter tests prove round-trip, failure-details persistence, replace-on-re-report, list ordering, and restart survival.
- **Migration.** `2026-11-21-000070_cascade_test_results` uses the next unused sequence number (000070) and plain portable SQL (TEXT/BOOLEAN/BIGINT, CREATE TABLE + CREATE INDEX — the established multi-statement pattern), embedded once and run by BOTH backends (`embed_migrations!("migrations")` + `run_pending_migrations` in SqliteStorage and PgStorage constructors). check-migration-versions and check-migration-sql-portability green.
- **Context hand-off.** The merged-repo identity and merge SHA are persisted at trigger time in the port-backed KV store (KvJsonStore has SQLite + PG impls) keyed by cascade task id, so the result record carries the full identity of the triggering merge; a missing context degrades with a warning while still writing the record (outcome never lost).
- **No collateral damage.** Only 4 removed lines in merge_processor.rs: the byte-slice `&sha[..8]` prefix replaced with a char-boundary-safe `chars().take(8)` (repo invariant), the old description text, and a moved `details` binding. Coverage matrix: only row 12 (Cascade Testing) changed, `task-assigned` → `implemented`, with accurate source evidence; count 11/16 → 12/16. Commit attribution correct (both product commits listed; the rest are process-only).

Test-inflation check: none found. The scheduler test asserts state produced only by production code (task type at creation, claim status, agent record, worktree repo) and fails pre-fix; the adapter restart test fails if persistence is process-memory-only; the replace test fails if the upsert duplicates rows; the merge_processor durable tests read back through the port after driving the real report path.

Verifier must run (gyre-server test-binary build exceeds this sandbox's 4 GiB budget; compilation verified via `cargo check --tests`):

- `cargo test -p gyre-server scheduler_spawns_agent_for_cascade_test_task` (signal_chain::tests).
- `cargo test -p gyre-server report_cascade_test_result` (merge_processor durable pass/fail/re-report tests).
- `cargo test -p gyre-adapters cascade_test` (re-run for the record).
- Full workspace suite + Clippy + frontend per standard verification.
