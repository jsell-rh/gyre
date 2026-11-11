---
title: "Dependency graph — persistent storage for breaking changes and policies"
spec_ref: "dependency-graph.md §Enforcement Policies"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "dependency-graph.md §Enforcement Policies"
  - "dependency-graph.md §Cascade Testing"
commits: ["c5864610e312bb58612119261300250f7d1e69fd", "0777605ecc9e4c10f087b3b64b0167aa09edd2ae", "202c9a2d1ab79e31f83b86d36152d701bc6e08ec", "38e4ae86b7b40c5c2c6e0a6550238f80ef391523", "a5417644d252d948125544f3520d0cb272ffb1ba", "5f30c7f7c64e4e6b84f50ab94aca634536bb3627", "43e7a476cafe12e3c5720533818a4bf095d1ac76", "097e5213ab1218688b168f888cb447885fbcc08b", "71e25bb8c1b92e52babd309d383dc4de864794b8"]
---

## Spec Excerpt

### §Enforcement Policies (dependency-graph.md)

Per-workspace dependency enforcement policy controls how the forge responds to breaking changes, version drift, and stale dependencies:

```rust
pub struct DependencyPolicy {
    pub breaking_change_behavior: BreakingChangeBehavior, // Block, Warn, Notify
    pub max_version_drift: u32,
    pub stale_dependency_alert_days: u32,
    pub require_cascade_tests: bool,
    pub auto_create_update_tasks: bool,
}
```

### §Cascade Testing (dependency-graph.md)

When `require_cascade_tests` is true, a breaking change in repo A triggers test execution in all dependent repos before the change can merge.

## Implementation Plan

1. **BreakingChangeRepository port trait**: Create in `gyre-ports/src/breaking_change.rs`:
   - `create(change: &BreakingChange) -> Result<()>`
   - `find_by_id(id: &Id) -> Result<Option<BreakingChange>>`
   - `list_by_dependency(edge_id: &Id) -> Result<Vec<BreakingChange>>`
   - `list_unacknowledged(tenant_id: &Id) -> Result<Vec<BreakingChange>>`
   - `acknowledge(id: &Id, by: &str) -> Result<()>`

2. **DependencyPolicyRepository port trait**: Create in `gyre-ports/src/dependency_policy.rs`:
   - `get(workspace_id: &Id) -> Result<Option<DependencyPolicy>>`
   - `set(workspace_id: &Id, policy: &DependencyPolicy) -> Result<()>`

3. **SQLite adapters**: Implement both port traits. Create DB migration for `breaking_changes` and `dependency_policies` tables.

4. **Replace in-memory adapters**: Update `AppState` to use the persistent adapters instead of `MemBreakingChangeRepository` and `MemDependencyPolicyRepository`.

5. **Cascade test stub**: When `require_cascade_tests` is true and a breaking change is detected, create tasks in dependent repos labeled `cascade-test` with the breaking change context. The actual test execution is handled by agents — this task just ensures the tasks are created.

## Acceptance Criteria

- [ ] `BreakingChangeRepository` port trait exists with SQLite adapter
- [ ] `DependencyPolicyRepository` port trait exists with SQLite adapter
- [ ] DB migration creates `breaking_changes` and `dependency_policies` tables
- [ ] In-memory adapters replaced with persistent ones in AppState
- [ ] Cascade test tasks auto-created when `require_cascade_tests` is true
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/dependency-graph.md` §"Breaking Change Detection", §"Enforcement Policies", and §"Cascade Testing". The domain types (BreakingChange, DependencyPolicy, BreakingChangeBehavior) already exist in `gyre-domain/src/dependency.rs`. The in-memory adapters are in `gyre-server/src/mem.rs` — search for `MemBreakingChangeRepository` and `MemDependencyPolicyRepository`. Follow the existing adapter patterns in `crates/gyre-adapters/src/sqlite/` (e.g., `task.rs`, `agent.rs`).

## Shipped

Persistent storage for breaking changes and per-workspace dependency policies, wired into production `AppState`. The first five acceptance criteria are satisfied by production code at this branch head; the sixth (`cargo test --all`) is owned by the independent verifier (focused-suite evidence below). Criteria boxes above are left exactly as assigned — satisfaction is reported here, not by editing protected text.

- **Ports**: `BreakingChangeRepository` (`gyre-ports/src/breaking_change.rs`: create / find_by_id / list_unacknowledged / list_by_source_repo / acknowledge) and `DependencyPolicyRepository` (`gyre-ports/src/dependency_policy.rs`: get_for_workspace / set_for_workspace). The plan text names `list_by_dependency(edge_id)` and `get`/`set`, but both ports pre-existed on this task's base at their current signatures with base callers depending on them; redefining them would have broken the hexagonal contract. The adapters implement the ports that exist.
- **SQLite + Postgres adapters**: `crates/gyre-adapters/src/sqlite/{breaking_change,dependency_policy}.rs` with Postgres mirrors — full Diesel implementations; policy upsert keyed on `workspace_id` (PK); behavior string↔enum mapping rejects unknown values; `schema.rs` registers both tables on both backends.
- **Migration** `2026-10-08-000060_dep_breaking_policies`: `breaking_changes` (id PK, edge/source-repo indexes, acknowledged fields) and `dependency_policies` (workspace_id PK, all five policy columns). Portable SQL — `check-migration-sql-portability.sh` OK; idempotent `up.sql` (`CREATE TABLE/INDEX IF NOT EXISTS`), so the renumbers forced by upstream version collisions (000056→000057→000059→000060) re-run harmlessly.
- **AppState wiring**: `breaking_changes` and `dependency_policies` go through the `store!` macro (`gyre-server/src/lib.rs`) — SqliteStorage/PgStorage in DB-backed mode, mem fallback only in pure in-memory mode, identical to every sibling store.
- **Cascade tests**: `trigger_cascade_tests` (`merge_processor.rs`) runs after every merge, reads the persistent policy, honors the dependent workspace's opt-out, creates a High-priority `cascade-test` labeled task in each dependent repo with the breaking-change context, emits `cascade_test_triggered` events, and notifies members. Agent completion/failure of `cascade-test` tasks routes through `report_cascade_test_result` (`spawn.rs`).
- **web/dist**: no task-163 UI changes; generated assets are restored/rebuilt by the executor and are not touched by this task.

### Test evidence

Focused suites (full `cargo test --all` deferred to the independent verifier per the 4 GiB budget rule; the verifier should run at minimum the four suites below plus the workspace suite):

- `cargo test -p gyre-adapters --lib breaking_change` — 7 passed (roundtrip, missing→None, unacknowledged filter, acknowledge-missing→false, pre-acknowledged persistence, source-repo scoping, records survive a fresh storage instance).
- `cargo test -p gyre-adapters --lib dependency_policy` — 4 passed (default policy, full-field roundtrip, overwrite, survives fresh storage instance).
- `cargo test -p gyre-adapters --lib migrations` — 3 passed (incl. `migrations_create_tables`, which runs the real embedded migrations against a fresh DB).
- `cargo test -p gyre-server --test task163_dependency_persistence` — 2 passed: records and policies written via one `build_state` are observed by a second `build_state` over the same DB file — fails against mem-only wiring.
- `cargo test -p gyre-server --lib merge_processor::tests::trigger_cascade` — 8 passed; `api::dependencies::tests` — 64 passed (incl. Block-policy merge rejection, Warn non-blocking, proceed-after-acknowledgment); `api::scim::tests` — 6 passed.
- These were last executed at heads `202c9a2d1` and `5b6707c43` (rounds 6 and 10); `cargo check -p gyre-adapters -p gyre-server` exit 0 at `5b6707c43`. crates.io is unreachable from the current round's sandbox (connection refused; same restriction as rounds 5/7/8) — an infrastructure restriction recorded for remote verification, not a code defect. No task-163 code path changed since: this round's only diff vs `01dd6dd7c` is this task file.
- Static gates re-run at this round's head, all exit 0: migration-versions, migration-sql-portability, arch, mem-port-contracts, in-memory-state-stores, task-commit-attribution (with the frontmatter commit list), `check-rustfmt-diff.py cfeefbda975211a6ac24a17e5920be6f8b23008b` (changed lines clean, 10 Rust files), `git diff --check cfeefbda..HEAD` clean.

### Contract repair (this round, finding be72aa9ca7ef49658cdb847d92e1e386)

The executor restored the assigned task document in this checkout after the protected task text changed at candidate `01dd6dd7c` (checked acceptance boxes plus round-2/round-3 diagnostic sections outside `## Shipped`).

- **Protected text untouched**: frontmatter (title/spec_ref/depends_on/coverage_sections), Spec Excerpt, Implementation Plan, Acceptance Criteria, and Agent Instructions are byte-identical to the restored assigned document, including the unchecked boxes and the trailing newline. Only `progress:` and `commits:` were set (process bookkeeping, round-3/5 precedent), and all notes are confined to `## Shipped` with `###` subheadings — no heading renamed, no new top-level section.
- **Production fix preserved**: `git diff 01dd6dd7c -- . ':(exclude)specs'` was empty at round start — the round-10 merged head `5b6707c43` and all nine task-163 product-surface commits remain intact; this round changes only this task file.
- **Frontmatter**: `commits:` re-recorded with the nine task-labeled product-surface commits — `check-task-commit-attribution.sh` fails (exit 1) at the restored empty list and passes (exit 0) with them present.
- Evidence: `/tmp/stage/review-evidence/contract-repair-be72aa9c/`.

### Repair history (rounds 2-10)

- **Round 2 (rustfmt)**: changed lines in the new adapter/test files reformatted to rustfmt-canonical; rustfmt-diff gate clean thereafter.
- **Round 4 (SCIM test race, `38e4ae86`)**: `scim_unauthorized_without_token` pointed `GYRE_SCIM_TOKEN` at a divergent value, racing parallel SCIM tests (403 → missing `id` in a sibling test's create response); it now configures the shared test token, keeping the 403-from-missing-header assertion. Post-fix: full `gyre-server` lib suite 1242 passed / 0 failed; SCIM suite green across 10 consecutive runs.
- **Rounds 6-8 (migration version collisions, `202c9a2d1` / `0777605ec` / `c5864610e`)**: three upstream merges each brought a migration colliding with this task's Diesel version (same date+sequence), which would silently drop one side's tables on fresh databases; renumbered to the next unused sequence each time (now `2026-10-08-000060`). Un-shadowing proofs: all `up.sql` files applied in Diesel version order to a fresh sqlite DB show both sides' tables present.
- **Round 9 (frontend heap, merge `6370cb7fc`)**: the failing vitest worker crash was upstream test-heap exhaustion fixed by upstream PR #764/#765 (not contained in the failing candidate; this branch has zero `web/` changes); after merging the fix, `npm test -- --no-file-parallelism` → 99 files / 2010 tests passed, exit 0.
- **Round 10 (rebase, merge `5b6707c43`)**: `schema.rs` conflict with upstream `commit_signatures` resolved as the union of both sides; no version collision that round (different dates); focused adapter suites and `cargo check -p gyre-adapters -p gyre-server` green at the merged head.
