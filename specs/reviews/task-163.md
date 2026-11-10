# Review — task-163 (Dependency graph — persistent storage for breaking changes and policies)

Spec: `specs/system/dependency-graph.md` §Enforcement Policies, §Cascade Testing.
Candidate under review: `d13aba9b450798992697cad4283a1aabed42c021` (assigned base `18adceb684926b31138c6d05be1403b1621d3b02`).
Verdict: **approved** — no findings. Evidence: `/tmp/stage/review-evidence/` (probe commands, outputs, exit codes).

## Scope of the diff (base → candidate)

13 files, all task-163-owned surface: SQLite adapters `sqlite/{breaking_change,dependency_policy}.rs`, Postgres mirrors `postgres/{breaking_change,dependency_policy}.rs`, `schema.rs` (both tables), migration `2026-10-08-000060_dep_breaking_policies` (up/down), `sqlite/mod.rs` + `postgres/mod.rs` module registrations and migration-table test list, `gyre-server/src/lib.rs` AppState wiring (mem hardcode → `store!`), integration test `tests/task163_dependency_persistence.rs`, the SCIM test race fix in `api/scim.rs` (round 4), and this task's own file. Zero `web/` or `scripts/` changes; `git diff --check` clean.

## What was verified (all probes at the exact candidate, exit codes recorded)

- **Ports and adapters are real.** Both SQLite and Postgres adapters are full Diesel implementations (insert/find/filter/update, `spawn_blocking`, pool connections) of the pre-existing `BreakingChangeRepository` / `DependencyPolicyRepository` ports — no stubs, no `bail!`, no hardcoded lookups. Policy upsert keys on `workspace_id` PK; behavior string↔enum mapping rejects unknown values; `create` preserves pre-acknowledged fields (Block-policy merges stay blocked on seeded records).
- **Migration is sound and un-shadowing.** `2026-10-08-000060` is unique among all 68 migrations by 14-digit diesel version; portable SQL (`CREATE TABLE/INDEX IF NOT EXISTS` — idempotent, harmless re-run after the round-6/7/8 renumbers). Deterministic proof: all 68 `up.sql` applied in version order to a fresh sqlite DB → `breaking_changes`, `dependency_policies`, and the three collision-round upstream tables (`spec_lifecycle_configs` with `repo_id`/`config`, `spec_assertion_results`, `spec_approval_events` with `attestation_level`/`stack_hash`) all present.
- **AppState wiring is persistent, and the test that pins it kills the bug.** `cargo test -p gyre-server --test task163_dependency_persistence` → 2/2 passed. Negative control: reverting `lib.rs` to the mem hardcode (`Arc::new(mem::Mem*)`) makes both tests FAIL (exit 101) — the tests are not self-confirming. Source restored; tree clean.
- **Focused Rust suites** (crates.io reachable this round; `/tmp/stage/bin/cargo`): adapters `breaking_change` 7/7, `dependency_policy` 4/4, `migrations` 3/3; server `trigger_cascade` 8/8, `api::dependencies::tests` 64/64, `api::scim::tests` 9/9. All exit 0.
- **Cascade Testing coverage section is enforced through the persistent store.** `trigger_cascade_tests` (merge_processor.rs:3158) reads the policy via `state.dependency_policies` (now persistent), respects the dependent workspace's opt-out, creates a High-priority `cascade-test` task per dependent repo, and notifies members; the merge-time Block gate (merge_processor.rs:2021-2045) reads unacknowledged changes and policy through the now-persistent stores. 8 cascade tests + 64 dependencies-API tests (Block rejection, Warn non-blocking, proceed-after-acknowledgment) green.
- **SCIM race fix (round 4) verified:** `scim_unauthorized_without_token` now writes the shared `test-scim-token` value; the 403 assertion on the missing `Authorization` header is unchanged in strength; 9/9 SCIM tests pass.
- **Contract integrity:** the task file is byte-identical to the assigned contract body (python byte-compare); acceptance criterion 6 (`cargo test --all` passes) remains verbatim and unchecked — correctly deferred to the verification stage per the harness rule that workspace-wide suites belong to verification.
- **Static gates:** migration-versions, migration-sql-portability, arch, mem-port-contracts, in-memory-state-stores, task-commit-attribution — all exit 0; `check-rustfmt-diff.py 18adceb68` changed lines clean (10 Rust files).

## Documented deviations (accepted, not defects)

- **Port signatures vs plan text:** the plan named `list_by_dependency(edge_id)` / `get` / `set` / `acknowledge(id, by)`, but the ports pre-exist at base with `list_by_source_repo` / `get_for_workspace` / `set_for_workspace` / `acknowledge(id, by, at)` and base callers depend on those signatures (round-3 contract repair). The adapters implement the port that exists; redefining it would have broken the hexagonal contract.
- **Coverage rows 11/12 remain `task-assigned`:** promotion to `verified` is the controller's auditor call (task-199 round-1 precedent), not the implementer's.

## Non-blocking observation (pre-existing base code, not in this diff)

merge_processor.rs:2021-2034 reads `list_by_source_repo(...).await.unwrap_or_default()` at the Block-policy merge gate — a storage-layer error would read as "no unacknowledged changes". This pattern exists at base and is untouched by this candidate; recorded for a process owner (the fail-open invariant class in AGENTS.md currently targets `resolve_ref` only).

## Deferred to verification

`cargo test --all` (criterion 6) — the review wrapper rejects workspace-wide suites; all focused suites relevant to the diff were executed here and are green.
