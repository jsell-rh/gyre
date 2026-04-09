# TASK-020: Dep Graph — Breaking Change Detection & Enforcement Policies

**Spec reference:** `dependency-graph.md` §Breaking Change Detection, §Enforcement Policies  
**Depends on:** None (builds on M22.4 dependency graph infrastructure)  
**Progress:** `needs-revision`  
**Review:** [specs/reviews/task-020.md](../reviews/task-020.md) — R4 needs-revision (2 findings)

## Spec Excerpt

From `dependency-graph.md` §Breaking Change Detection:

> When a push to Repo B is detected as a **breaking change** (semver major bump via conventional commit, or API contract change):
>
> 1. Forge detects breaking change in Repo B
> 2. Query dependency graph: "What repos depend on Repo B?"
> 3. For each dependent repo:
>    a. Create a task (title: "Breaking change in {repo_b}: {change_description}", labels: dependency-update, breaking-change, auto-created, priority: High)
>    b. Notify the dependent repo's orchestrator via MCP
>    c. If policy requires: BLOCK Repo B's merge until dependent repos acknowledge the breaking change

From `dependency-graph.md` §Enforcement Policies:

> Configurable per-workspace:
> ```yaml
> dependency_policy:
>   breaking_change_behavior: block | warn | notify
>   max_version_drift: 3
>   stale_dependency_alert_days: 30
>   require_cascade_tests: true
>   auto_create_update_tasks: true
> ```
>
> | Policy | Effect |
> |---|---|
> | `block` | Breaking change cannot merge until all dependent repos acknowledge |
> | `warn` | Breaking change merges with warnings. Tasks auto-created. |
> | `notify` | Breaking change merges silently. Dependent repo orchestrators notified. |

## Current State

- **Domain types:** `DependencyStatus::Breaking` exists in `gyre-domain/src/dependency.rs` but nothing sets it. `DependencyEdge.version_drift` field exists but is never computed.
- **Push detection:** The existing push-time dependency extraction (`Cargo.toml` parsing) detects new/changed dependencies but does NOT detect breaking changes (semver major bumps, `feat!:`/`BREAKING CHANGE:` conventional commits).
- **Enforcement:** No workspace dependency policy configuration exists. No breaking-change-specific endpoints exist.
- **Missing API endpoints** (following existing pattern in `mod.rs`):
  - `GET /api/v1/dependencies/breaking` — list unacknowledged breaking changes (tenant-wide)
  - `POST /api/v1/dependencies/breaking/:id/acknowledge` — acknowledge a breaking change

## Implementation Plan

1. **Breaking change detection on push** — In the push handler (or post-receive hook):
   - Parse pushed commits for conventional commit markers: `feat!:`, `BREAKING CHANGE:` footer, semver major bump in `Cargo.toml`/`package.json`
   - When detected, query the dependency graph for all repos depending on the changed repo
   - Update the `DependencyEdge.status` to `Breaking` for affected edges
   - Record the breaking change details (change description, commit SHA, affected repos)

2. **Workspace dependency policy** — Add `dependency_policy` to workspace configuration:
   - `breaking_change_behavior: block | warn | notify` (default: `warn`)
   - Store in workspace config (or a new `dependency_policies` table)
   - Add `GET/PUT /api/v1/workspaces/:id/dependency-policy` endpoints

3. **Enforcement at merge time** — When merging an MR that contains a breaking change:
   - If policy = `block`: reject merge until all dependent repos acknowledge
   - If policy = `warn`: merge with warnings, auto-create tasks in dependent repos
   - If policy = `notify`: merge silently, notify dependent repo orchestrators

4. **Auto-task creation** — For each dependent repo affected by a breaking change:
   - Create a high-priority task: "Breaking change in {repo}: {description}"
   - Add labels: `dependency-update`, `breaking-change`, `auto-created`

5. **Breaking change acknowledgment** — New endpoints:
   - `GET /api/v1/dependencies/breaking` — list all unacknowledged breaking changes
   - `POST /api/v1/dependencies/breaking/:id/acknowledge` — mark as acknowledged
   - Acknowledgment clears the merge block (if policy = `block`)

6. **MCP notification** — Send directed message to dependent repo orchestrators via message bus when a breaking change is detected

7. **Add tests:**
   - Conventional commit breaking change detection (feat!, BREAKING CHANGE footer)
   - Policy enforcement at merge time (block, warn, notify)
   - Auto-task creation in dependent repos
   - Acknowledgment clears merge block
   - MCP notification delivery

## Acceptance Criteria

- [ ] Breaking changes detected from conventional commits on push
- [ ] Dependent repos identified via dependency graph queries
- [ ] Workspace dependency policy configurable (block/warn/notify)
- [ ] Merge blocked when policy = `block` and unacknowledged breaking changes exist
- [ ] Tasks auto-created in dependent repos with correct priority and labels
- [ ] `GET /api/v1/dependencies/breaking` lists unacknowledged breaking changes
- [ ] `POST /api/v1/dependencies/breaking/:id/acknowledge` marks change as acknowledged
- [ ] Dependent orchestrators notified via message bus
- [ ] Tests cover detection, enforcement, and acknowledgment
- [ ] `cargo test --all` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `specs/system/dependency-graph.md` §Breaking Change Detection and §Enforcement Policies
3. Read `crates/gyre-server/src/api/dependencies.rs` for existing dependency graph endpoints
4. Read `crates/gyre-domain/src/dependency.rs` for domain types (DependencyStatus::Breaking)
5. Read `crates/gyre-server/src/git_http.rs` for the push handler
6. Read `crates/gyre-server/src/api/merge_queue.rs` for the merge processor
7. Verify new endpoint URLs against `crates/gyre-server/src/api/mod.rs` route registration
8. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- `feat(deps): implement breaking change detection and enforcement (TASK-020)`
- `2e39a167 fix(deps): address TASK-020 R2 findings — require_cascade_tests, %B format, production test`
