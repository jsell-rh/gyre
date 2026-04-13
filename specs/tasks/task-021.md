---
title: "Dep Graph — Version Drift Tracking & Stale Dependencies"
spec_ref: "dependency-graph.md §Version Drift Tracking, API (GET /api/v1/deps/stale)"
depends_on: []
progress: complete
review: specs/reviews/task-021.md
coverage_sections: []
commits: 
  - e36336eb
  - a17b5201
  - 11966ac4
---

## Spec Excerpt

From `dependency-graph.md` §Version Drift Tracking:

> The forge continuously monitors version drift:
>
> ```
> Repo A pins repo-b-crate = "1.2.3"
> Repo B is currently at v1.5.0
>
> Drift = 3 minor versions (1.2.3 -> 1.5.0)
> ```
>
> When drift exceeds `max_version_drift`:
> - Task auto-created: "Update repo-b dependency from 1.2.3 to 1.5.0"
> - The Repo Orchestrator can dispatch an agent to update the dependency, run tests, and open an MR
> - The AIBOM flags stale dependencies in release reports

From `dependency-graph.md` §Enforcement Policies:

> `max_version_drift: 3` — flag repos >3 versions behind
> `stale_dependency_alert_days: 30` — flag deps not updated in 30 days
> `auto_create_update_tasks: true` — auto-file tasks for dependency updates

## Current State

- **Domain types:** `DependencyEdge.version_drift: Option<u32>` and `DependencyEdge.version_pinned: Option<String>` exist but are never populated during detection.
- **Push-time detection:** `Cargo.toml` parsing extracts dependency names but does NOT resolve current versions of target repos or compute drift.
- **DependencyStatus::Stale** exists but nothing sets it.
- **Missing endpoint:** `GET /api/v1/dependencies/stale` for tenant-wide stale dependency listing is not registered in `mod.rs`.

## Implementation Plan

1. **Version resolution on push** — When dependency detection runs on push:
   - For each detected dependency edge, resolve the target repo's current version (from `Cargo.toml` in the target repo's default branch, or from git tags)
   - Compute `version_drift` (number of minor/major versions behind)
   - Update `DependencyEdge.version_pinned` and `DependencyEdge.target_version_current`
   - If drift exceeds workspace policy `max_version_drift`, set `DependencyEdge.status = Stale`

2. **Staleness check job** — Add a background job (or extend existing job infrastructure):
   - Periodically scan all dependency edges
   - Check if `last_verified_at` is older than `stale_dependency_alert_days`
   - Update status to `Stale` if threshold exceeded

3. **Auto-task creation** — When a dependency goes stale (drift exceeds threshold):
   - If workspace policy `auto_create_update_tasks` is enabled
   - Create task: "Update {target} dependency from {pinned} to {current}"
   - Labels: `dependency-update`, `auto-created`

4. **Stale dependencies endpoint** — `GET /api/v1/dependencies/stale`:
   - Returns all dependency edges with `status = Stale` (tenant-wide)
   - Filterable by `workspace_id`
   - Response: list of `DependencyEdgeResponse` with drift details

5. **AIBOM integration** — Extend AIBOM generation:
   - Include dependency health snapshot (stale count, max drift)
   - Flag stale dependencies as supply chain risk

6. **Add tests:**
   - Version drift computation (semver parsing)
   - Staleness threshold detection
   - Auto-task creation when drift exceeds policy
   - Stale endpoint returns correct results
   - AIBOM includes dependency health

## Acceptance Criteria

- [ ] Version drift computed on push (compare pinned vs current version)
- [ ] `DependencyEdge.version_drift` and `version_pinned` populated
- [ ] Dependencies exceeding `max_version_drift` set to `Stale` status
- [ ] Auto-tasks created when `auto_create_update_tasks` enabled
- [ ] `GET /api/v1/dependencies/stale` returns stale dependencies
- [ ] Staleness check job detects time-based staleness
- [ ] Tests cover drift computation, staleness detection, and auto-task creation
- [ ] `cargo test --all` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `specs/system/dependency-graph.md` §Version Drift Tracking
3. Read `crates/gyre-server/src/api/dependencies.rs` for existing dependency endpoints
4. Read `crates/gyre-domain/src/dependency.rs` for DependencyEdge (version_drift, version_pinned, status)
5. Read `crates/gyre-server/src/spec_registry.rs` for push-time detection hook pattern
6. Read `crates/gyre-server/src/jobs.rs` for background job pattern
7. Verify new endpoint URL against `crates/gyre-server/src/api/mod.rs` route registration
8. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- `e36336eb` feat(deps): implement version drift tracking and stale dependency detection (TASK-021)
- `a17b5201` fix(deps): push-time staleness detection and multi-cycle persistence (TASK-021 R1)
- `11966ac4` fix(deps): injectable version resolver and time-based staleness recovery (TASK-021 R2 F1)
