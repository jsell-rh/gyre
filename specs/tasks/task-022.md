# TASK-022: Dep Graph — Cascade Testing

**Spec reference:** `dependency-graph.md` §Cascade Testing  
**Depends on:** TASK-020 (breaking change detection must be operational)  
**Progress:** `not-started`

## Spec Excerpt

From `dependency-graph.md` §Cascade Testing:

> When `require_cascade_tests` is enabled and Repo B merges a change:
>
> 1. Forge identifies all repos that depend on Repo B
> 2. For each dependent repo, the forge triggers a test run against Repo B's new version
> 3. Results are reported in the activity feed
> 4. If any dependent repo's tests fail, a task is auto-created with the failure details
> 5. The Workspace Orchestrator is notified of the cascade failure
>
> This is like a cross-repo CI matrix, but forge-native. No external CI configuration needed — the forge knows the graph and runs the tests.

## Current State

- **No cascade testing logic exists.** Merging a change in Repo B has no effect on dependent repos.
- **Dependency graph queries** (blast-radius BFS, dependents) exist and can identify affected repos.
- **Agent spawn infrastructure** exists and can run test tasks.
- **Activity feed** and **task creation** infrastructure exist.
- **Workspace dependency policy** is created in TASK-020 (`require_cascade_tests` flag).

## Implementation Plan

1. **Post-merge cascade trigger** — After a successful merge in Repo B:
   - Check workspace dependency policy for `require_cascade_tests`
   - If enabled, query dependency graph for all repos depending on Repo B
   - For each dependent repo, create a cascade test task

2. **Cascade test task** — A lightweight test-only agent task:
   - Title: "Cascade test: {dependent_repo} against {source_repo}@{new_sha}"
   - The agent updates the dependency to the new version, runs tests, reports results
   - Labels: `cascade-test`, `auto-created`
   - Priority: High (blocking if policy = `block`)

3. **Result reporting** — Cascade test results:
   - Reported as activity events (`cascade_test_passed`, `cascade_test_failed`)
   - Failed results create follow-up tasks in the dependent repo
   - Workspace orchestrator notified of cascade failures via directed message

4. **Cascade status tracking** — For each breaking change:
   - Track which dependent repos have been tested
   - Track pass/fail status per repo
   - Surface in the breaking change detail view

5. **Add tests:**
   - Post-merge triggers cascade test tasks when policy enabled
   - Cascade test results recorded as activity events
   - Failed cascade creates follow-up task
   - Disabled policy skips cascade testing
   - Multiple dependent repos each get their own test task

## Acceptance Criteria

- [ ] Post-merge hook triggers cascade test tasks for dependent repos
- [ ] Cascade test tasks created with correct title, labels, and priority
- [ ] Test results reported as activity events
- [ ] Failed cascade tests create follow-up tasks in dependent repos
- [ ] Workspace orchestrator notified of cascade failures
- [ ] `require_cascade_tests: false` skips cascade testing
- [ ] Tests cover trigger, result reporting, and policy flag
- [ ] `cargo test --all` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `specs/system/dependency-graph.md` §Cascade Testing
3. Read `crates/gyre-server/src/api/merge_queue.rs` for the merge processor (post-merge hook point)
4. Read `crates/gyre-server/src/api/dependencies.rs` for dependency graph queries (blast_radius BFS)
5. Read `crates/gyre-server/src/api/spawn.rs` for agent task creation pattern
6. Read `crates/gyre-server/src/api/activity.rs` for activity event recording
7. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

_(none yet)_
