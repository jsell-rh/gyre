---
title: "Dependency graph — persistent storage for breaking changes and policies"
spec_ref: "dependency-graph.md §Enforcement Policies"
depends_on: []
progress: not-started
coverage_sections:
  - "dependency-graph.md §Enforcement Policies"
  - "dependency-graph.md §Cascade Testing"
commits: ["3a59b024fd907dc7aecdefab648c319e8363acdd", "e13b780a1ccbed1c66fe0751ad452dc4dffb5f7b", "594559c034a68ac3a33012fa691b4665bfd7220a", "77ffea10149755337d976a8a4c690f391e15dddb"]
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
