# TASK-046: Cross-Repo Dependency Graph — Visualization UI

**Spec reference:** `dependency-graph.md` §UI (lines 228–232)

**Progress:** `ready-for-review`

**Review:** [`specs/reviews/task-046.md`](../reviews/task-046.md)

**Depends on:** TASK-020 (breaking change detection), TASK-021 (version drift tracking)

---

## Spec Excerpt

> **Dependency graph visualization:** interactive DAG at workspace and tenant level. Nodes = repos, edges = dependencies colored by type. Stale edges in yellow, breaking in red. Click a node to see impact analysis.
>
> **Workspace dashboard:** aggregate dependency health. "3 repos have stale dependencies. 1 breaking change unacknowledged."

---

## Context

The backend already provides cross-repo dependency data via endpoints registered in `gyre-server/src/api/mod.rs`:

- `GET /api/v1/repos/:id/dependencies` (list dependencies for a repo)
- `POST /api/v1/repos/:id/dependencies` (add a dependency)
- `DELETE /api/v1/repos/:id/dependencies/:dependency_id` (remove a dependency)
- `GET /api/v1/repos/:id/dependents` (list repos depending on a given repo)
- `GET /api/v1/repos/:id/blast-radius` (impact analysis)

No frontend component currently renders this data as a graph visualization. The workspace dashboard does not show aggregate dependency health.

---

## Scope

Create a new dependency graph visualization component and integrate it into the workspace view. Two deliverables:

### Part A: Dependency Graph DAG Component

1. **New component:** `web/src/components/DependencyGraph.svelte` — renders cross-repo dependency data as an interactive directed graph.

2. **Node representation:** Each node is a repo. Display repo name, and optionally dependency health indicator (healthy/stale/breaking).

3. **Edge styling by dependency type:**
   - Code dependency (import/require) → solid line
   - Spec dependency (spec-links) → dashed line
   - API dependency → dotted line
   - Stale edges → yellow
   - Breaking changes → red with exclamation badge

4. **Interaction:**
   - Click a node → navigate to repo detail OR open impact analysis panel showing blast radius
   - Hover a node → highlight its direct dependencies and dependents
   - Zoom/pan for large graphs

5. **Scope selector:** Toggle between workspace-level (repos within one workspace) and tenant-level (all repos across workspaces) views.

6. **Data fetching:** The component needs a workspace-level or tenant-level aggregation endpoint. If the existing per-repo endpoints are insufficient for a workspace-wide graph, add a `GET /api/v1/workspaces/:id/dependency-graph` endpoint that returns `{ nodes: [...], edges: [...] }` by aggregating all repo dependencies within the workspace.

### Part B: Workspace Dashboard Health Summary

1. **Add to workspace home:** A compact "Dependency Health" card showing aggregate stats:
   - Total repos with dependencies
   - Repos with stale dependencies (count, yellow)
   - Unacknowledged breaking changes (count, red)
   - Link to full graph view

2. **Placement:** Add to the workspace home view alongside existing summary cards (Architecture, Metrics, etc.).

---

## Implementation Notes

- Reuse SVG rendering infrastructure from ExplorerCanvas (ELK/dagre layout, zoom/pan, node rendering).
- For the workspace-level graph, consider a new aggregation endpoint if iterating over per-repo endpoints is too slow. Check if `crates/gyre-server/src/api/dependencies.rs` already has workspace-level queries.
- The health summary card can query the same data as the graph but render only counts.

### Existing Endpoints

- `GET /api/v1/repos/:id/dependencies` (registered at `mod.rs:522–524`)
- `GET /api/v1/repos/:id/dependents` (registered at `mod.rs:530–532`)
- `GET /api/v1/repos/:id/blast-radius` (registered at `mod.rs:534`)

---

## Acceptance Criteria

- [ ] `DependencyGraph.svelte` renders an interactive DAG with repos as nodes and dependencies as edges
- [ ] Edges colored by dependency type; stale edges yellow, breaking edges red
- [ ] Click a node to navigate to repo detail or view impact analysis
- [ ] Hover highlights direct dependencies and dependents
- [ ] Workspace-level and tenant-level scope toggle
- [ ] Workspace home shows "Dependency Health" summary card with aggregate stats
- [ ] Component test verifies DAG rendering with mock data
- [ ] Component test verifies health summary card rendering

---

## Git Commits

- `c3853572` feat(web): add cross-repo dependency graph visualization (TASK-046)
- `7ebb0131` fix(web): scope breaking count to workspace and handle external node clicks (TASK-046)

---

## Agent Instructions

When working on this task:
1. Update the **Progress** field above to `in-progress` when you begin.
2. Read the existing dependency API at `crates/gyre-server/src/api/dependencies.rs` to understand available data.
3. Read `web/src/components/ExplorerCanvas.svelte` for SVG rendering patterns.
4. Read `web/src/components/WorkspaceHome.svelte` for the dashboard layout to add the health card.
5. Implement Part A (graph component) and Part B (health card).
6. Add component tests in `web/src/__tests__/`.
7. If a new aggregation endpoint is needed, implement it in `crates/gyre-server/src/api/dependencies.rs` and register in `mod.rs`.
8. List each commit SHA under **Git Commits** above.
9. When done, update **Progress** to `ready-for-review`.
