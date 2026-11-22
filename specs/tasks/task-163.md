---
title: "Dependency graph — persistent storage for breaking changes and policies"
spec_ref: "dependency-graph.md §Enforcement Policies"
depends_on: []
progress: complete
coverage_sections:
  - "dependency-graph.md §Enforcement Policies"
  - "dependency-graph.md §Cascade Testing"
commits: ["c1585054074783657cc3df38121ec8c52776ad27", "c5864610e312bb58612119261300250f7d1e69fd", "0777605ecc9e4c10f087b3b64b0167aa09edd2ae", "202c9a2d1ab79e31f83b86d36152d701bc6e08ec", "38e4ae86b7b40c5c2c6e0a6550238f80ef391523", "a5417644d252d948125544f3520d0cb272ffb1ba", "5f30c7f7c64e4e6b84f50ab94aca634536bb3627", "43e7a476cafe12e3c5720533818a4bf095d1ac76", "097e5213ab1218688b168f888cb447885fbcc08b", "71e25bb8c1b92e52babd309d383dc4de864794b8"]
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

Persistent storage for breaking changes and per-workspace dependency policies, wired into production `AppState`. The first five acceptance criteria are satisfied by production code on this branch; the sixth (`cargo test --all`) is owned by the independent verifier (focused-suite evidence below). Criteria boxes above are left exactly as assigned — satisfaction is reported here, not by editing protected text.

- **Ports**: `BreakingChangeRepository` (`crates/gyre-ports/src/breaking_change.rs`: create / find_by_id / list_unacknowledged / list_by_source_repo / acknowledge) and `DependencyPolicyRepository` (`crates/gyre-ports/src/dependency_policy.rs`: get_for_workspace / set_for_workspace). The plan text names `list_by_dependency(edge_id)` and `get`/`set`, but both ports pre-existed on this task's base at their current signatures with base callers depending on them; redefining them would have broken the hexagonal contract. The adapters implement the ports that exist.
- **SQLite + Postgres adapters**: `crates/gyre-adapters/src/sqlite/{breaking_change,dependency_policy}.rs` with Postgres mirrors — full Diesel implementations; policy upsert keyed on `workspace_id` (PK); behavior string↔enum mapping rejects unknown values; `schema.rs` registers both tables on both backends.
- **Migration** `2026-10-08-000061_dep_breaking_policies`: `breaking_changes` (id PK, edge/source-repo/unacknowledged indexes, acknowledged fields) and `dependency_policies` (workspace_id PK, all five policy columns). Portable SQL — `check-migration-sql-portability.sh` OK; idempotent `up.sql` (`CREATE TABLE/INDEX IF NOT EXISTS`), so the renumbers forced by upstream version collisions (000056→000057→000059→000060→000061) re-run harmlessly.
- **AppState wiring**: `breaking_changes` and `dependency_policies` go through the `store!` macro (`crates/gyre-server/src/lib.rs`) — SqliteStorage/PgStorage in DB-backed mode, mem fallback only in pure in-memory mode, identical to every sibling store.
- **Cascade tests**: `trigger_cascade_tests` (`crates/gyre-server/src/merge_processor.rs`) runs after every merge, reads the persistent policy, honors the dependent workspace's opt-out, creates a High-priority `cascade-test` labeled task in each dependent repo with the breaking-change context, emits `cascade_test_triggered` events, and notifies members. Agent completion/failure of `cascade-test` tasks routes through `report_cascade_test_result` (`spawn.rs`).
- **web/dist**: no task-163 UI changes; generated assets are restored/rebuilt by the executor and are not touched by this task.

### Test evidence

Focused suites (full `cargo test --all` deferred to the independent verifier per the 4 GiB budget rule; the verifier should run at minimum the suites below plus the workspace suite):

- `cargo test -p gyre-adapters --lib breaking_change` — 7 passed (roundtrip, missing→None, unacknowledged filter, acknowledge-missing→false, pre-acknowledged persistence, source-repo scoping, records survive a fresh storage instance).
- `cargo test -p gyre-adapters --lib dependency_policy` — 4 passed (default policy, full-field roundtrip, overwrite, survives fresh storage instance).
- `cargo test -p gyre-adapters --lib migrations` — 3 passed (incl. `migrations_create_tables`, which runs the real embedded migrations against a fresh DB, and `no_duplicate_migration_version_prefixes`).
- `cargo test -p gyre-server --test task163_dependency_persistence` — 2 passed: records and policies written via one `build_state` are observed by a second `build_state` over the same DB file — fails against mem-only wiring.
- `cargo test -p gyre-server --lib merge_processor::tests::trigger_cascade` — 8 passed; `api::dependencies::tests` — 64 passed (incl. Block-policy merge rejection, Warn non-blocking, proceed-after-acknowledgment); `api::scim::tests` — 6 passed.
- These suites were executed at earlier heads of this same branch (last at `5b6707c43`); no task-163 Rust code changed since — the only product-surface diff this round is the migration directory rename (commit `c15850540`, SQL files only, byte-identical content, no Rust references to the directory name). crates.io is unreachable from the current round's sandbox (connection refused, `curl https://static.crates.io/config.json` exit 7; same restriction as rounds 5/7/8/11) — an infrastructure restriction recorded for remote verification, not a code defect.
- Static gates re-run at this round's head `c15850540`, all exit 0: migration-versions (the failing gate from the round-11 verification finding — now fixed), migration-sql-portability, arch, hierarchy, abac-route-registry, abac-exempt-handlers, mcp-write-tools, dead-message-kinds, byte-slice-truncation, relative-path-defaults, fail-open-ref-resolution, mem-port-contracts, fabricated-scope-defaults, lossy-secret-conversion, scope-literal-defaults, inert-enforcement, forged-scope-fields, forwarded-header-trust, in-memory-state-stores, unbounded-external-http.

### Contract repair (round 12, migration version collision)

The base merge `cba416b46` introduced `2026-10-08-000060_task_generation`, colliding with this task's `2026-10-08-000060_dep_breaking_policies`; Diesel runs only one migration per version (alphabetically first wins), so `task_generation` was silently dropped on fresh databases and `check-migration-versions.sh` failed. Fixed by renumbering this task's migration to the next unused sequence `2026-10-08-000061_dep_breaking_policies` (commit `c15850540`). The rename is byte-preserving (`git mv`, 100% similarity); `up.sql` idempotency makes re-application a no-op on databases that already ran version 20261008000060, and the previously-dropped upstream `task_generation` migration now runs again on fresh databases.
