# TASK-045: Spec Links — Interactive DAG Visualization

**Spec reference:** `spec-links.md` §UI (lines 186–189)

**Progress:** `needs-revision`

**Review:** [`specs/reviews/task-045.md`](../reviews/task-045.md)

**Depends on:** TASK-016 (staleness data), TASK-019 (query endpoints, conflict data)

---

## Spec Excerpt

> **Spec graph visualization:** interactive DAG at workspace or tenant level. Nodes colored by approval status. Stale links highlighted in yellow. Conflicts highlighted in red. Click to navigate.

---

## Context

The SpecDashboard currently has a "Graph" view toggle (`web/src/components/SpecDashboard.svelte:348`) and loads data from `GET /api/v1/specs/graph` (`crates/gyre-server/src/api/specs.rs:1017`). However, the current rendering is a flat card grid (`.spec-graph-grid` with `.spec-graph-card`), not an interactive DAG as specified.

The backend endpoint already returns `{ nodes, edges }` with spec relationship data. The frontend needs to render this as a proper directed acyclic graph.

---

## Scope

Replace the card-grid rendering in SpecDashboard's graph view with an interactive DAG visualization that satisfies the spec requirements.

### Requirements

1. **DAG layout:** Render spec nodes and edges as a directed graph using SVG (consistent with Explorer canvas rendering approach — ELK/dagre for layout). Nodes are specs; edges are link types (`implements`, `supersedes`, `depends_on`, `conflicts_with`, `extends`).

2. **Node coloring by approval status:**
   - Approved → green
   - Pending → amber/yellow
   - Rejected → red
   - Deprecated → gray

3. **Edge styling by link type:**
   - `depends_on` → solid arrow
   - `implements` → dashed arrow
   - `supersedes` → dotted arrow with strikethrough target
   - `conflicts_with` → red bidirectional line
   - `extends` → solid arrow, visually distinct from `depends_on`

4. **Staleness highlighting:** Stale links (SHA mismatch) highlighted in yellow. Data source: link staleness from TASK-016.

5. **Conflict highlighting:** Active conflicts highlighted in red. Data source: conflict detection from TASK-019.

6. **Click to navigate:** Clicking a node opens the spec detail (via `goToEntityDetail('spec', path, data)` — same as current card click handler).

7. **Scope selector:** Toggle between workspace-level and tenant-level graph views.

### Implementation Notes

- Reuse the SVG rendering infrastructure from ExplorerCanvas where possible (layout engine, zoom/pan, node rendering patterns).
- The existing `specGraph` state variable and `loadSpecGraph()` function in SpecDashboard.svelte should be retained — only the rendering changes.
- Keep the card grid as a fallback (`viewMode` toggle could offer `list`, `grid`, `dag`), or replace the grid entirely if the DAG is strictly superior.

### Existing Endpoints

- `GET /api/v1/specs/graph` → `{ nodes: [...], edges: [...] }` (registered at `mod.rs:345`)
- `GET /api/v1/specs/:path/dependents` (from TASK-019)
- `GET /api/v1/specs/:path/dependencies` (from TASK-019)
- `GET /api/v1/specs/stale-links` (from TASK-019)
- `GET /api/v1/specs/conflicts` (from TASK-019)

---

## Acceptance Criteria

- [ ] SpecDashboard graph view renders an interactive DAG (not a flat card grid)
- [ ] Nodes colored by approval status (green/amber/red/gray)
- [ ] Edges styled by link type with visual differentiation
- [ ] Stale links highlighted in yellow
- [ ] Conflicts highlighted in red
- [ ] Click a node to navigate to spec detail
- [ ] Workspace-level and tenant-level scope toggle works
- [ ] Component test verifies DAG rendering with mock graph data

---

## Git Commits

- `cc256081` feat(web): interactive DAG visualization for spec graph (TASK-045)

---

## Agent Instructions

When working on this task:
1. Update the **Progress** field above to `in-progress` when you begin.
2. Read `web/src/components/SpecDashboard.svelte` (lines 405–455) for the current graph rendering.
3. Read `web/src/components/ExplorerCanvas.svelte` for SVG rendering patterns to reuse.
4. Implement the DAG visualization, replacing the card grid in the graph view section.
5. Add a component test in `web/src/__tests__/`.
6. List each commit SHA under **Git Commits** above.
7. When done, update **Progress** to `ready-for-review`.
