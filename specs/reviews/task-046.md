# TASK-046 Review — R1

**Reviewer:** Verifier  
**Commit:** `c3853572`  
**Result:** `needs-revision` (2 findings)

---

## Findings

- [-] [process-revision-complete] **F1 (resolved R2): Breaking changes count is tenant-wide, not workspace-scoped.** In `web/src/components/WorkspaceHome.svelte`, `loadDepHealth()` calls `api.breakingChanges()` which fetches ALL unacknowledged breaking changes tenant-wide via `GET /api/v1/dependencies/breaking`. The `breakingCount` is computed as `(breakingList ?? []).filter(b => !b.acknowledged).length` — a tenant-wide count. In contrast, `staleDependencies(workspace.id)` correctly passes a workspace filter. The task scope (Part B §1) says the workspace dashboard health summary should show "Unacknowledged breaking changes (count, red)" — this is a workspace dashboard, so the count should be scoped to repos in the current workspace. A tenant with 3 workspaces and 10 breaking changes across all workspaces would show "10 breaking changes unacknowledged" on each workspace's dashboard, even if only 2 affect repos in the current workspace. **Fix:** Filter the `breakingList` results against `depGraphNodes` repo IDs (which are already workspace-scoped) before counting, e.g.: `breakingList.filter(b => wsRepoIds.has(b.source_repo_id) && !b.acknowledged).length`, where `wsRepoIds` is derived from the workspace graph nodes or the `repos` array.

- [-] [process-revision-complete] **F2 (resolved R2): External repo node clicks are silently swallowed.** In `web/src/components/WorkspaceHome.svelte:1278-1281`, the `onNodeClick` handler does `repos.find(r => r.id === node.repo_id)`. The `repos` array contains only repos in the current workspace (loaded from `api.workspaceRepos`). The dependency graph's workspace scope includes nodes for repos from other workspaces when an edge crosses the workspace boundary (the endpoint filters for "at least one endpoint in the workspace"). Clicking a node for an external repo returns `undefined` from `repos.find()`, and the `if (repo)` guard silently skips the navigation. The acceptance criterion says "Click a node to navigate to repo detail or view impact analysis" — external repo nodes are unclickable with no user feedback. **Fix:** Either (a) handle external nodes by navigating to their repo detail page directly via repo ID (if cross-workspace navigation is supported), or (b) show a tooltip/message indicating the node is from another workspace, or (c) as a minimal fix, use the graph node's name for a fallback action (e.g., open blast-radius analysis).

---

# TASK-046 Review — R2

**Reviewer:** Verifier  
**Commit:** `7ebb0131`  
**Result:** `complete` (0 findings)

---

## R1 Fix Verification

**F1 resolved:** `WorkspaceHome.svelte:410-414` now builds `wsRepoIds` from workspace graph nodes and filters `breakingList` by `wsRepoIds.has(b.source_repo_id)`. Two new tests in `WorkspaceHomeSections.test.js` verify: (a) only workspace-repo breaking changes are counted (mixed in/out scenario), (b) zero count when all breaking changes are outside workspace. Both tests pass.

**F2 resolved:** `WorkspaceHome.svelte:1284-1288` adds `else { toastError(...) }` for external repo clicks. Two new tests verify: (a) toast error shown when clicking external node, (b) normal `onSelectRepo` navigation for local nodes with no toast. Both tests pass.

## Acceptance Criteria Verification

All 8 acceptance criteria satisfied:
1. `DependencyGraph.svelte` renders interactive DAG — 20 component tests ✓
2. Edge styling by type (code=solid blue, spec=dashed purple, api=dotted green); stale=yellow, breaking=red ✓
3. Node click navigates to repo detail; external nodes show toast error ✓
4. Hover highlights adjacency (deps + rdeps), dims non-adjacent nodes ✓
5. Workspace/tenant scope toggle with data refetch ✓
6. `DependencyHealthCard.svelte` on workspace home with totalWithDeps, staleCount, breakingCount ✓
7. DAG component tests with mock ELK layout ✓
8. Health card component tests (10 tests) ✓

## New Finding Sweep — R2

No new findings. The fix changes are minimal and correct. Test coverage for both fixes is adequate with positive and negative cases.
