# M29: Workspace Integration

**Status:** In Progress
**Milestone:** M29

---

## Summary

M29 delivers full workspace-aware behavior across the Gyre SPA and API — making the workspace context selector (introduced in M28) actually filter entities in all views.

---

## M29.4 — Workspace API Wiring

Wire the global workspace selector through to all major list API calls so the UI filters by workspace context.

- `GET /api/v1/agents?workspace_id=<id>` — `AgentList` re-fetches on `workspaceId` prop change
- `GET /api/v1/tasks?workspace_id=<id>` — `TaskBoard` re-fetches on `workspaceId` prop change; post-create reload preserves filter
- `GET /api/v1/projects?workspace_id=<id>` — `ProjectList` re-fetches on `workspaceId` prop change; post-create reload preserves filter

All calls without `workspaceId` are backwards compatible (param omitted when unset). Clearing the selector to "All Workspaces" removes the filter and shows all entities.

---

## Acceptance Criteria

- [x] Select a workspace → Agents/Tasks/Projects views refresh filtered to that workspace
- [x] Clear workspace → all entities shown (no `workspace_id` param sent)
- [x] Spawn agent while workspace selected → refreshed agent list still filtered correctly
- [x] Create task/project while workspace selected → post-create reload preserves workspace filter

---

## Implementation Notes

- `api.js`: `getAgents({ workspaceId })`, `getTasks({ workspaceId })`, `getProjects({ workspaceId })` — param appended as `?workspace_id=<id>` when truthy
- Components use `$effect` to re-run load functions when `workspaceId` prop changes
- Post-create reload bug fixed in follow-up (PR #322): `loadTasks(workspaceId)` / `loadProjects(workspaceId)` must pass `workspaceId` explicitly

---

## M29.1 — Diesel Tables for Platform Entities

Migration  creates 10 Diesel-managed tables persisting the 9 platform entities introduced in M22 (previously in-memory only):

| Table | Entity |
|---|---|
| `workspaces` | Workspace |
| `personas` | Persona |
| `teams` | Team |
| `workspace_memberships` | WorkspaceMembership |
| `notifications` | Notification |
| `policies` | Policy |
| `policy_decisions` | PolicyDecision |
| `spec_approvals` | SpecApproval |
| `dependency_edges` | DependencyEdge |
| `budget_configs` | BudgetConfig |

**New port traits added:** ,  (previously no Diesel-backed port traits existed for these).

**Design decisions:**
- Complex fields (, enums, , ) → JSON TEXT columns
-  fields → NULLABLE columns
-  uses composite  ( or )
- Both SQLite and Postgres adapters implemented

---

## Sub-milestones M29.2–M29.3

Not yet defined.
