# Review — task-163 (Dependency graph — persistent storage for breaking changes and policies)

Spec: `specs/system/dependency-graph.md` §Enforcement Policies, §Cascade Testing.
Candidate: `564a70f3d2e39535a3ca401cf8c2b103737f4e15` (base `fefde2946f3a0af1f212588f54591516fee0f7b2`; round-8 head = round-7 repaired head `0777605ec` + two process commits touching only `specs/tasks/task-163.md`).
Verdict: **approved** (round 8).

## Round 8 (this review)

crates.io was reachable through cargo this round (`cargo fetch` succeeded), so the focused Rust suites were re-executed at the exact candidate HEAD — first executable re-verification since round 6.

Test runs (`/tmp/stage/bin/cargo`; evidence: `/tmp/stage/review-evidence/round8/`):

- `cargo test -p gyre-adapters --lib breaking_change` → **7 passed, 0 failed** (incl. `records_survive_fresh_storage_instance` — a fresh `SqliteStorage` over the same DB file observes the first instance's writes; cannot pass against a mem stand-in).
- `cargo test -p gyre-adapters --lib dependency_policy` → **4 passed, 0 failed** (incl. `policy_survives_fresh_storage_instance`).
- `cargo test -p gyre-adapters --lib migrations` → **3 passed, 0 failed** (incl. `migrations_create_previously_shadowed_columns`).
- `cargo test -p gyre-server --test task163_dependency_persistence` → **2 passed, 0 failed** — records/policies written via one `build_state` are observed by a second `build_state` over the same DB file; differential against mem-only wiring by construction (`.expect("...must survive an AppState rebuild")` on an independent instance).
- `cargo test -p gyre-server --lib merge_processor::tests::trigger_cascade` → **8 passed, 0 failed**.
- `cargo test -p gyre-server --lib api::dependencies::tests` → **64 passed, 0 failed** (Block-policy merge rejection, Warn non-blocking, proceed-after-acknowledgment).
- `cargo test -p gyre-server --lib api::scim::tests` → **9 passed, 0 failed in each of 5 consecutive runs** (round-4 env-token race fix stable at this head).

Static gates at the exact candidate (all exit 0): migration-versions, migration-sql-portability, arch, mem-port-contracts, in-memory-state-stores, task-commit-attribution, `check-rustfmt-diff.py fefde294`, `git diff --check fefde294..564a70f3d`. Zero `web/` or `scripts/` changes vs base (exemption files untouched).

Deterministic un-shadowing proof (no cargo): all 63 `up.sql` applied in Diesel name-sorted order to a fresh sqlite DB → `breaking_changes` (9 cols, 3 indexes), `dependency_policies` (7 cols), `spec_assertion_results`, `spec_approval_events` (with `attestation_level`/`stack_hash`), `meta_specs`, `rollout_policies` all present — the round-7 renumber to `2026-10-08-000059` no longer shadows upstream task-193's `000057` migration.

Verified against the task contract:

- **Ports + adapters**: `BreakingChangeRepository` and `DependencyPolicyRepository` (pre-existing on base at their current signatures — `list_by_source_repo`/`get_for_workspace`/`set_for_workspace`/`acknowledge(id, by, at)`; redefining to the stale plan names would have broken base callers) implemented for `SqliteStorage`, `PgStorage`; mem adapters remain as pure in-memory fallback. Behavior string↔enum mapping rejects unknown values; policy upsert keyed on `workspace_id` PK.
- **Migration**: `2026-10-08-000059_dep_breaking_policies` creates both tables with portable, idempotent SQL; version unique (gate + proof above).
- **AppState wiring**: both stores go through the `store!` macro (lib.rs) — Sqlite/Pg in DB-backed mode, mem only in pure in-memory mode, identical to every sibling store; `check-in-memory-state-stores.sh` clean.
- **Cascade testing**: `trigger_cascade_tests` (merge_processor.rs) reads the persistent policy, respects the dependent workspace's opt-out, creates a High-priority `cascade-test` task in each dependent repo, emits events, notifies members. Enforcement is real: `BreakingChangeBehavior::Block` fails the merge-queue entry and returns; Warn logs and proceeds; Notify is a no-op.
- **Contract integrity**: task-file changes vs base are bookkeeping only (progress, `commits:` frontmatter, five checked criteria, Shipped notes); acceptance criterion 6 remains verbatim `- [ ] cargo test --all passes`. Process commits `2e0d0f60d`/`564a70f3d` touch only the task file.
- **Scope**: diff vs base touches only task-163-owned surface (two SQLite + two Postgres adapters, schema.rs, migration 000059, lib.rs store wiring, the persistence integration test, the round-4 SCIM test race fix, task file).

No findings. The remaining unchecked criterion (`cargo test --all`) is owned by the verification stage per the standing assignment; this review's focused suites at the exact head are all green.
