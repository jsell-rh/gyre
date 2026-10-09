---
title: "Dependency graph — persistent storage for breaking changes and policies"
spec_ref: "dependency-graph.md §Enforcement Policies"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "dependency-graph.md §Enforcement Policies"
  - "dependency-graph.md §Cascade Testing"
commits: ["e13b780a1ccbed1c66fe0751ad452dc4dffb5f7b", "594559c034a68ac3a33012fa691b4665bfd7220a", "77ffea10149755337d976a8a4c690f391e15dddb"]
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

- [x] `BreakingChangeRepository` port trait exists with SQLite adapter
- [x] `DependencyPolicyRepository` port trait exists with SQLite adapter
- [x] DB migration creates `breaking_changes` and `dependency_policies` tables
- [x] In-memory adapters replaced with persistent ones in AppState
- [x] Cascade test tasks auto-created when `require_cascade_tests` is true
- [ ] `cargo test --all` passes

## Shipped

Real persistent storage for breaking changes and per-workspace dependency
policies, wired into `AppState` via the standard `store!` dispatch (SQLite
when `GYRE_DATABASE_URL` is a SQLite file, Postgres for a Postgres URL, mem
fallback only in pure in-memory mode):

- **Ports** (`gyre-ports`): `BreakingChangeRepository` (`breaking_change.rs`)
  and `DependencyPolicyRepository` (`dependency_policy.rs`). Method shapes
  follow the actual domain model rather than the task-plan sketch: breaking
  changes are keyed by `dependency_edge_id`/`source_repo_id` (the domain has
  no per-tenant scoping on `BreakingChange`), and `get_for_workspace`
  returns the default policy rather than `Option` (every workspace has an
  effective policy; `DependencyPolicy::default()` is the spec's yaml default:
  warn / drift 3 / 30 days / cascade on / auto-tasks on).
- **Adapters** (`gyre-adapters`): Diesel-backed SQLite implementations
  (`sqlite/breaking_change.rs`, `sqlite/dependency_policy.rs`) plus
  Postgres counterparts (`postgres/breaking_change.rs`,
  `postgres/dependency_policy.rs`), both implementing the shared ports.
- **Migration** `2026-10-08-000056_dep_breaking_policies` creates
  `breaking_changes` and `dependency_policies` (portable SQL, runs on both
  backends per the shared embedded `migrations/` invariant).
- **AppState wiring**: `breaking_changes` and `dependency_policies` now use
  `store!`, so a server restart over the same DB observes every record. The
  mem adapters remain only as the no-DB fallback.
- **Cascade testing** (`gyre-server/src/merge_processor.rs`,
  `trigger_cascade_tests` / `report_cascade_test_result`): on merge, when the
  workspace policy enables `require_cascade_tests`, a `cascade-test` +
  `auto-created` labeled High-priority task is created in each dependent
  repo (resolved to the dependent repo's own workspace, honoring that
  workspace's opt-out), a `cascade_test_triggered` event is emitted, and
  members are notified. Test failure reporting creates a follow-up
  `cascade-test-failure` task and notifies the workspace. `create` preserves
  pre-acknowledged records so Block-policy merges stay unblocked.

Behavior verified in this sandbox (repair round re-ran all focused probes
against the upstream-merged base `8c2d1775`; evidence under
`/tmp/stage/review-evidence/`: `task163-repair-probes.log`,
`task163-repair-probes-server.log`, `task163-repair-invariants.log`):

- `cargo test -p gyre-adapters --lib sqlite::breaking_change
  sqlite::dependency_policy migrations_create_tables` — adapter CRUD,
  round-trip, upsert-overwrite (`set_for_workspace` now also updates
  `breaking_change_behavior` on conflict), pre-acknowledged-field
  preservation, and migration table creation.
- `cargo test -p gyre-server --test task163_dependency_persistence` —
  records survive a full `build_state` rebuild over the same SQLite file
  (the mem-fallback regression this task closes).
- `cargo test -p gyre-server --lib -- merge_processor::tests::trigger_cascade
  merge_processor::tests::report_cascade` — task creation per dependent,
  policy-disabled skip, no-dependents noop, dependent-workspace resolution
  and opt-out, member notification, follow-up task on failure, default-policy
  enable.
- `cargo test --all` is owned by the controller's integrated gate — this
  sandbox cannot run the loopback-listener server suite (TCP accept
  unsupported, errno 95; see `/tmp/stage/capabilities.json`).

Frontmatter note: the commit list records the three retained wip commits
(77ffea10, 594559c0, e13b780a) carrying the implementation; this round added
no product-surface code — it re-verified the retained work against the
upstream-merged base (upstream changes touch `api/admin.rs` and pipeline
scripts only, no interaction with this task's surface) and repaired commit
attribution drift (`a781ede2` recorded in task-210's frontmatter; that
commit arrived via the upstream merge and is not part of this task's
surface).
