# TASK-046 Review — R1

**Reviewer:** Verifier  
**Commit:** `c3853572`  
**Result:** `needs-revision` (2 findings)

---

## Findings

- [ ] [process-revision-complete] **F1: Breaking changes count is tenant-wide, not workspace-scoped.** In `web/src/components/WorkspaceHome.svelte`, `loadDepHealth()` calls `api.breakingChanges()` which fetches ALL unacknowledged breaking changes tenant-wide via `GET /api/v1/dependencies/breaking`. The `breakingCount` is computed as `(breakingList ?? []).filter(b => !b.acknowledged).length` — a tenant-wide count. In contrast, `staleDependencies(workspace.id)` correctly passes a workspace filter. The task scope (Part B §1) says the workspace dashboard health summary should show "Unacknowledged breaking changes (count, red)" — this is a workspace dashboard, so the count should be scoped to repos in the current workspace. A tenant with 3 workspaces and 10 breaking changes across all workspaces would show "10 breaking changes unacknowledged" on each workspace's dashboard, even if only 2 affect repos in the current workspace. **Fix:** Filter the `breakingList` results against `depGraphNodes` repo IDs (which are already workspace-scoped) before counting, e.g.: `breakingList.filter(b => wsRepoIds.has(b.source_repo_id) && !b.acknowledged).length`, where `wsRepoIds` is derived from the workspace graph nodes or the `repos` array.

- [ ] [process-revision-complete] **F2: External repo node clicks are silently swallowed.** In `web/src/components/WorkspaceHome.svelte:1278-1281`, the `onNodeClick` handler does `repos.find(r => r.id === node.repo_id)`. The `repos` array contains only repos in the current workspace (loaded from `api.workspaceRepos`). The dependency graph's workspace scope includes nodes for repos from other workspaces when an edge crosses the workspace boundary (the endpoint filters for "at least one endpoint in the workspace"). Clicking a node for an external repo returns `undefined` from `repos.find()`, and the `if (repo)` guard silently skips the navigation. The acceptance criterion says "Click a node to navigate to repo detail or view impact analysis" — external repo nodes are unclickable with no user feedback. **Fix:** Either (a) handle external nodes by navigating to their repo detail page directly via repo ID (if cross-workspace navigation is supported), or (b) show a tooltip/message indicating the node is from another workspace, or (c) as a minimal fix, use the graph node's name for a fallback action (e.g., open blast-radius analysis).
