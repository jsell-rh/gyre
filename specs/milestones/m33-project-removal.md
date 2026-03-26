# M33: Project Removal — Workspace as Primary Grouping Entity

**Status:** Done
**Milestone:** M33

---

## Summary

M33 removes the `Project` entity from Gyre's domain model. `Workspace` becomes the primary grouping entity for repos, tasks, and agents. All APIs that previously required `project_id` are migrated to use `workspace_id`. The UI is updated to show workspace-scoped repo lists in place of the project grid.

---

## M33.1 — Server: Remove Project Entity (PR #371)

The `Project` domain type, adapters, and API routes are removed. `workspace_id` is now required (non-nullable) on all entity creation endpoints.

**Removed:**
- `POST/GET/DELETE /api/v1/projects` — project CRUD endpoints removed
- `gyre_domain::project`, `gyre_adapters::project` modules removed
- `project_id` columns/foreign keys removed from `repos`, `tasks`, `agents` tables

**Changed:**
- `workspace_id` is now required on repo, task, and agent create requests (previously optional)
- All list endpoints (`GET /api/v1/repos`, `/tasks`, `/agents`) filter by `workspace_id` when provided

---

## M33.2 — UI: Workspace-Scoped Repo List (merged via HSI wave)

`ProjectList.svelte` (project grid with nested repo expansion) is replaced by `RepoList.svelte` — a workspace-scoped flat list of repos.

**Changed:**
- `web/src/components/RepoList.svelte` — workspace-scoped repo list replacing project grid
- `ComposeView` drops `project_id` field from spawn form
- Clone URLs updated to use `repo.id` directly
- E2E tests updated to match new repository-centric navigation

---

## Acceptance Criteria

- [x] `Project` entity removed from domain, adapters, and API
- [x] `workspace_id` required on repo/task/agent creation
- [x] All repo/task/agent list endpoints accept `?workspace_id=` filter
- [x] `RepoList.svelte` renders workspace-scoped repos replacing project grid
- [x] E2E tests updated for repository-centric navigation
- [x] No references to `project_id` in active API paths

---

## Implementation Notes

- `crates/gyre-domain/src/project.rs` — removed
- `crates/gyre-adapters/src/sqlite/project.rs` and `postgres/project.rs` — removed
- `crates/gyre-server/src/api/projects.rs` — removed
- `web/src/components/RepoList.svelte` — new workspace-scoped repo list
- Migration: `project_id` columns dropped; `workspace_id` made non-nullable
