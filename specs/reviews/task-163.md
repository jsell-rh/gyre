# Review — task-163 (Dependency graph — persistent storage for breaking changes and policies)

Spec: `specs/system/dependency-graph.md` §Enforcement Policies, §Cascade Testing.
Base `a4ebdf6f` → candidate `4dc0da73b`. Net-new surface: 13 files — SQLite + Postgres adapters for `BreakingChangeRepository` / `DependencyPolicyRepository`, schema.rs table registrations, migration `2026-10-08-000057_dep_breaking_policies`, `lib.rs` `store!` wiring, `api/scim.rs` test-race fix (round 4), the `task163_dependency_persistence` integration test, and the task file. The cascade-trigger logic itself (`merge_processor.rs::trigger_cascade_tests`, `spawn.rs` routing) is already on base from an earlier published round and is unchanged here; this candidate's job is making the stores it reads persistent — which is what was verified.
Verdict: **approved**.

## Probes (all at candidate `4dc0da73b`, clean tree, `git status` empty)

Focused suites (`/tmp/stage/bin/cargo`, cold target — no shared-target contamination):

- `cargo test -p gyre-adapters --lib breaking_change` → **7 passed** (roundtrip, missing→None, unacknowledged filter, acknowledge-missing→false, pre-acknowledged persistence, source-repo scoping, `records_survive_fresh_storage_instance`).
- `cargo test -p gyre-adapters --lib dependency_policy` → **4 passed** (default policy, full-field roundtrip, overwrite, `policy_survives_fresh_storage_instance`).
- `cargo test -p gyre-adapters --lib migrations` → **3 passed** (both new tables created on a fresh DB).
- `cargo test -p gyre-server --test task163_dependency_persistence` → **2 passed** (records and policies observed by a second `build_state` over the same DB file; acknowledgment durable across rebuilds).
- `cargo test -p gyre-server --lib merge_processor::tests::trigger_cascade` → **8 passed**.
- `cargo test -p gyre-server --lib api::dependencies::tests` → **64 passed** (Block-policy merge rejection, Warn non-blocking, proceed-after-acknowledgment, auto-task creation).
- `cargo test -p gyre-server --lib api::scim::tests` → **6 passed × 5 consecutive runs** (round-4 race fix stable under parallel lib tests).

**Mutation probe (the hollow-store killer):** temporarily reverted the `lib.rs` hunk back to `Arc::new(mem::MemBreakingChangeRepository::default())` / `MemDependencyPolicyRepository` and re-ran `--test task163_dependency_persistence` → **exit 101, both tests FAILED** ("breaking change must survive an AppState rebuild"; policy assertion mismatch). The wiring test genuinely kills the mem-only regression — it is not self-confirming. Source restored via `git checkout --`; HEAD unchanged (`4dc0da73b`), tree clean, test re-run green at the restored state.

Static gates: migration-versions OK (no duplicate Diesel versions; `000057` unique, round-6 renumber resolved the `000056` collision with `spec_assertion_results`), migration-sql-portability OK, arch OK, mem-port-contracts OK, in-memory-state-stores OK, task-commit-attribution OK (all 7 frontmatter commits resolve), `check-rustfmt-diff.py a4ebdf6f` changed lines clean (10 files), `git diff --check a4ebdf6f..HEAD` clean.

## Verified

- **Real Diesel adapters, both backends.** SQLite and Postgres `BreakingChangeRepository` / `DependencyPolicyRepository` do real queries (insert / find / filter / update with row counts), `spawn_blocking` over the pool, `optional()` for missing rows. Policy upsert uses `ON CONFLICT (workspace_id) DO UPDATE`. Behavior string↔enum mapping **rejects** unknown values (`Err` on anything but block/warn/notify) — no fail-open. Postgres mirrors compile in the crate build.
- **Migration is real and safe.** `breaking_changes` (id PK, edge/source-repo/unacknowledged indexes, acknowledgment columns) + `dependency_policies` (workspace_id PK, all five policy columns). Idempotent (`IF NOT EXISTS`), portable SQL (gate + `migrations` test confirm), correct `down.sql`.
- **AppState wiring is persistent in DB mode.** `store!` selects PgStorage/SqliteStorage when a DB is configured; mem only in pure in-memory mode — identical to every sibling store. Proven behaviorally by the mutation probe above, not just by reading the diff.
- **Cascade path reads persistent state.** `trigger_cascade_tests` calls `state.dependency_policies.get_for_workspace` (now DB-backed) for both the merged and dependent workspaces, creates a real High-priority `cascade-test` task per dependent via `state.tasks.create`, emits `cascade_test_triggered`, notifies members. 8 focused tests cover enable/disable, per-dependent tasks, opt-out, no-dependents noop.
- **Contract integrity.** Acceptance Criteria section is byte-identical to the assigned contract (criterion 6 honestly unchecked — full-workspace `cargo test --all` is the verifier's domain). No changes to `web/`, `scripts/`, exemption files, or unrelated specs; diff touches only task-163-owned surface plus the disclosed scim test fix.
- **SCIM race fix** (`38e4ae86`) is test-only and correct: the divergent `GYRE_SCIM_TOKEN` write raced parallel SCIM handlers; pointing it at the shared value keeps the missing-Authorization rejection intact.

## Non-blocking notes

- `trigger_cascade_tests` uses `.unwrap_or_default()` on `get_for_workspace` — on a storage error the default policy applies (`require_cascade_tests: true`), i.e. the failure direction enables cascade tests rather than suppressing them; and this call site predates the candidate (on base, unchanged). Not a defect of this diff.
- No Postgres integration tests for the new adapters — consistent with the repo-wide pattern (Postgres is never integration-tested; SQL portability covered by gate); same scope note as task-207.
- `detected_at`/`acknowledged_at` u64↔i64 casts mirror the codebase-wide timestamp convention.

Evidence: `/tmp/stage/review-evidence/` (static-gates logs, all suite logs, mutation probe log, pre-mutation HEAD record).
