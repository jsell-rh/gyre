# M28: UI Cohesion — Workspace Selector, Deep Linking, Cross-Entity Navigation

**Status:** Done
**Milestone:** M28

---

## Summary

M28 delivers three cohesion improvements to the Svelte SPA that were missing from earlier frontend milestones: a persistent global workspace context selector, entity-scoped deep-link URLs, and zero-prop-drill cross-entity navigation via Svelte context.

---

## M28.1 — Global Workspace Selector

A workspace dropdown appears in the **topbar** (right of the breadcrumb area). On load, it fetches all workspaces via `GET /api/v1/workspaces` and renders a `<select>` with an **"All Workspaces"** option plus one entry per workspace.

- Selection is persisted to `localStorage` (`gyre_selected_workspace_id`) and restored on page load.
- The selected workspace ID is passed as a `workspaceId` prop to `AgentList`, `TaskBoard`, and `ProjectList` — these filter their `GET` calls accordingly (e.g. `GET /api/v1/agents?workspace_id=<id>`).
- Selecting "All Workspaces" clears the filter and removes the key from `localStorage`.
- The topbar workspace scope **chip** (M22.5) is superseded by this selector but preserved for backwards compatibility where applicable.

---

## M28.2 — Entity Deep-Link URLs

`App.svelte`'s `navigate()` function now generates parameterized entity URLs:

| Entity | URL pattern | Notes |
|---|---|---|
| Repo detail | `/repos/:id` | Fetches `GET /api/v1/repos/{id}` on mount |
| Task detail | `/tasks/:id` | Fetches `GET /api/v1/tasks/{id}` on mount |
| MR detail | `/merge-requests/:id` | Fetches `GET /api/v1/merge-requests/{id}` on mount |
| Workspace detail | `/workspaces/:id` | Fetches `GET /api/v1/workspaces/{id}` on mount |

**Deep-link restoration:** on `onMount`, `App.svelte` parses `window.location.pathname` and, for parameterized paths, fetches the entity by ID and sets the correct view + selected entity. Browser back/forward continues to work via `popstate`.

**Distinction from sidebar view paths:** the existing static view paths (e.g. `/agents`, `/tasks`, `/dashboard`) remain unchanged. Entity deep links are parameterized (`/tasks/:id`) and load a specific entity rather than a list view.

---

## M28.3 — Cross-Entity Navigation via Svelte Context

`App.svelte` calls `setContext('navigate', navigate)` so child components can navigate to related entities without prop drilling. Components call `getContext('navigate')(path)`.

Navigation wired up in M28:

| Source component | Clickable element | Destination |
|---|---|---|
| Agent detail panel | `current_task_id` link | `/tasks/:id` |
| Task detail view | `assigned_to` agent link | `/agents` (filtered) |
| Task detail view | `parent_task_id` link | `/tasks/:id` |
| MR detail panel | `author_agent_id` link | `/agents` (filtered) |
| Workspace detail repos tab | repo name | `/repos/:id` |

---

## M28.4 — SpecDashboard Repo Filter

`SpecDashboard` receives an optional `repoId` prop and renders a **repo dropdown** next to the existing status filter pills. When a repo is selected, the spec list is filtered by `repo_name` field match. This prepares the component for embedding in the future `RepoDetail` Specs tab.

---

## Acceptance Criteria

- [x] Workspace selector in topbar — persists to localStorage, passes `workspaceId` to list components
- [x] `/repos/:id`, `/tasks/:id`, `/merge-requests/:id`, `/workspaces/:id` URLs restore correct view on direct navigation and page reload
- [x] Browser back/forward still works after entity deep-link navigation
- [x] Agent detail `current_task_id` navigates to task detail
- [x] Task detail `parent_task_id` navigates to parent task
- [x] MR detail `author_agent_id` navigates to agents list
- [x] Workspace detail repo name navigates to repo detail
- [x] SpecDashboard repo filter dropdown filters spec list by repo
- [x] `npm run build` clean, `npm test` 95/95 vitest passing

---

## Implementation Notes

- `setContext`/`getContext` are called at component initialization (not inside reactive blocks) per Svelte rules.
- `localStorage` keys: `gyre_selected_workspace_id`.
- Deep-link path parsing uses `window.location.pathname.split('/')` — no client-side router dependency added.
- Entity fetch on mount uses the existing `api.js` helpers (`getRepo`, `getTask`, `getMergeRequest`, `getWorkspace`).
