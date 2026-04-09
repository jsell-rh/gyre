# TASK-049: Merge Queue Dependency Graph UI

**Spec reference:** `merge-dependencies.md` §Merge Queue Integration > Visualization
**Depends on:** None (backend endpoint `GET /api/v1/merge-queue/graph` already exists)
**Progress:** `not-started`

## Spec Excerpt

From `merge-dependencies.md` §Merge Queue Integration > Visualization:

> The forge exposes the dependency graph via API and renders it in the web UI:
>
> ```
> GET /api/v1/merge-queue/graph
> ```
>
> Returns a DAG of queued MRs with dependency edges, gate status per node, and atomic group boundaries. The dashboard renders this as a visual pipeline showing what's blocked on what.

## Current State

The backend endpoint `GET /api/v1/merge-queue/graph` exists in `crates/gyre-server/src/api/merge_deps.rs` and returns:

```json
{
  "nodes": [
    {
      "mr_id": "...",
      "title": "...",
      "status": "open|approved|merged|closed|reverted",
      "priority": 0,
      "depends_on": ["..."],
      "atomic_group": "group-name" | null
    }
  ]
}
```

The UI (`web/src/components/PipelineOverview.svelte`) shows merge queue entries as a flat list. It does not render dependency edges, show what's blocked on what, or visualize atomic group boundaries.

## Implementation Plan

1. **Fetch graph data:**
   - Call `GET /api/v1/merge-queue/graph` from the existing merge queue API client
   - The response already includes `depends_on` edges and `atomic_group` membership

2. **DAG layout:**
   - Use ELK or dagre (already available in the codebase for Explorer canvas) to compute DAG layout from the `nodes` and their `depends_on` edges
   - Nodes are MRs; directed edges point from dependency → dependent (A → B means "B depends on A, A must merge first")
   - Atomic group members are visually grouped (dashed boundary box or shared background color)

3. **Node rendering:**
   - Each node shows: MR title (truncated), status badge (color-coded), priority indicator
   - Blocked nodes (deps not merged) have a distinct visual treatment (grayed out, lock icon, or "blocked" badge)
   - Merged nodes shown as green/completed

4. **Edge rendering:**
   - Solid arrows for dependency edges
   - Color: green if dependency is satisfied (merged), amber if pending, red if failed/closed
   - Animate or pulse edges for actively processing entries

5. **Interaction:**
   - Click a node to navigate to MR detail
   - Hover shows full title, status, and blocking dependencies
   - Toggle between DAG view and flat list view (preserve existing list for users who prefer it)

6. **Integration:**
   - Add the DAG view to the existing merge queue panel in `PipelineOverview.svelte` or as a new `MergeQueueGraph.svelte` component
   - Wire into the existing data refresh cycle (merge queue auto-refreshes)

7. **Tests:**
   - Component test: renders DAG with mock graph data (2+ nodes, dependency edge)
   - Component test: empty graph renders empty state
   - Component test: atomic group boundary renders correctly

## Acceptance Criteria

- [ ] Merge queue view shows a DAG visualization of queued MRs with dependency edges
- [ ] Dependency edges clearly show what's blocked on what (direction, color coding)
- [ ] Blocked MRs are visually distinct from ready-to-merge MRs
- [ ] Atomic group members are visually grouped
- [ ] Click a node to navigate to MR detail
- [ ] Hover shows MR summary and blocking dependencies
- [ ] Flat list view is still accessible (toggle or tab)
- [ ] Component test verifies DAG rendering with mock data
- [ ] `npm test` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `web/src/components/PipelineOverview.svelte` for the current merge queue UI
3. Read `web/src/lib/api.js` for the merge queue graph API call
4. Read `web/src/components/ExplorerCanvas.svelte` for existing DAG rendering patterns (ELK layout, SVG rendering)
5. Create the graph component, reusing layout/rendering patterns from ExplorerCanvas
6. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

_(none yet)_
