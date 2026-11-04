# Review — task-163 (Dependency graph — persistent storage for breaking changes and policies)

Spec: `specs/system/dependency-graph.md` §Enforcement Policies, §Cascade Testing.
Candidate: `a5417644` (base `58deeb75`). Round 4 independent review (fresh model; prior round failed on `InferenceUnavailable`, not on findings).
Verdict: **approved**.

## What was reviewed

Diff base→candidate is 12 files, all task-163 surface: SQLite adapters (`sqlite/breaking_change.rs`, `sqlite/dependency_policy.rs`), Postgres mirrors, `schema.rs`, migration `2026-10-08-000056_dep_breaking_policies`, `lib.rs` `store!` wiring (two fields), the `task163_dependency_persistence.rs` integration test, and this task's file. `web/` is byte-identical to the assigned base (top commit `a5417644` resolved inherited merge-conflict markers in `web/dist` by taking base artifacts verbatim). Zero `scripts/` changes; `git diff --check` clean.

## Independent evidence (all run at candidate `a5417644`)

- `cargo test -p gyre-adapters --lib breaking_change` → **7 passed**; `--lib dependency_policy` → **4 passed**. Both include fresh-storage-instance-over-same-DB-file persistence proofs.
- `cargo test -p gyre-server --test task163_dependency_persistence` → **2 passed** (records/policies observed by a second `build_state` over the same DB file).
- **Mutation probe**: reverting the `store!` wiring to direct `Mem*Repository` construction makes both persistence tests fail (0 passed, 2 failed) — the tests are not self-confirming; they kill mem-only wiring. Tree restored to clean at `a5417644` afterwards (log: `/tmp/stage/review-evidence/task163-mutation-mem-wiring.log`).
- `cargo test -p gyre-server --lib merge_processor::tests` → **45 passed** (12 cascade tests: task per dependent, policy-disabled skip, dependent-workspace opt-out, default-policy enable, member notification, result routing); `--lib api::dependencies::tests` → **64 passed** (Block-policy merge rejection, Warn non-blocking, proceed-after-acknowledgment).
- Gates: `check-migration-versions`, `check-migration-sql-portability`, `check-mem-port-contracts`, `check-in-memory-state-stores`, `check-task-commit-attribution` — all exit 0. `check-rustfmt-diff.py 58deeb75` — clean (9 files).

## Verification of each acceptance criterion

1. **`BreakingChangeRepository` port + SQLite adapter** — real Diesel implementation (insert/find/filter/update with `spawn_blocking`, `.optional()`, error contexts; acknowledge returns existence as `bool`; unknown behavior strings rejected, fail-closed). Port traits are byte-identical to base (`git diff base..candidate -- crates/gyre-ports/` empty).
2. **`DependencyPolicyRepository` port + SQLite adapter** — upsert on `workspace_id` PK, all five policy columns round-trip; default policy for unset workspaces matches `DependencyPolicy::default()`.
3. **Migration 000056** — next unused sequence number; portable SQL (no dialect-only constructs; portability gate green); indexes on edge/source-repo/(acknowledged, detected_at); down migration present; `schema.rs` registers both tables and the sqlite mod.rs table-completeness test was extended.
4. **AppState wiring** — both fields go through the `store!` macro (SqliteStorage/PgStorage in DB-backed mode, mem only in pure in-memory mode), identical to every sibling store; proven by the cross-`build_state` persistence test plus the mutation probe above.
5. **Cascade test tasks** — `trigger_cascade_tests` (pre-existing at base, unchanged) creates High-priority `cascade-test` labeled tasks in each dependent repo, reading the policy through the now-persistently-wired store; per-dependent-workspace opt-out respected; 12 focused tests green.
6. **`cargo test --all`** — honestly left unchecked; owned by the verification stage.

## Notes (non-blocking)

- The task plan text names `list_by_dependency` / `get` / `set` / `acknowledge(id, by)`, but those ports already existed at base with `list_by_source_repo` / `get_for_workspace` / `set_for_workspace` / `acknowledge(id, by, at)` and base callers depend on those signatures. Implementing the port that exists is the correct resolution; redefining it would have broken base callers and violated the hexagonal contract. Disclosed in the task's Shipped section.
- Running `cargo test -p gyre-server` without `SKIP_WEB_BUILD=1` dirties `web/dist` via build.rs (rebuild drift vs. committed bundles). Pre-existing repo-wide condition, present at base; not a candidate defect. Reviewer restored the tree after the probe.
- TCP listener probe is supported in this sandbox, but the candidate adds no HTTP surface; storage persistence is fully proven by the focused suites and the mutation probe. Exact-head full workspace suite remains the verification stage's check.

Evidence: `/tmp/stage/review-evidence/task163-review-round4.json` and the `task163-*.log` files alongside it.
