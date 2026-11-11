---
title: "Navigable Architecture — canvas layout, interaction model, lens selector"
spec_ref: "system-explorer.md §1"
depends_on: [task-170, task-177]
progress: complete
coverage_sections:
  - "system-explorer.md §1. The Navigable Architecture"
  - "ui-navigation.md §Tab: Architecture (Moldable Development Surface)"
commits: ["618f415278d033b796295fc49ad55947513c81bd", "197222ba96f83d640031cd04880e3f157deb3323", "6f5650c790080b3c73cab488306b00b181343cc6"]
---

## Spec Excerpt

system-explorer.md §1 defines the Navigable Architecture view:

**Layout**: Zoomable, pannable canvas. Default zoom shows boundaries and interfaces. Colors indicate spec coverage (green=governed, amber=suggested, red=no spec). Size indicates complexity/churn. Lens toggle: Structural (default) / Evaluative / Observable (future).

**In-view filter panel** (200px, collapsible left panel): Boundaries, Interfaces, Data, Specs categories. Toggle via filter icon in control bar. NOT part of the app sidebar.

**Interaction Model**:
- Click → detail panel shows entity with moldable view
- Double-click → drill into next C4 level
- Right-click → context menu (View spec, View provenance, View history, Open in code)
- Drag timeline scrubber → canvas shows architecture at that point in time
- Search (`/`) → find entity by name/type/spec within canvas

From ui-navigation.md §3 Architecture tab:
- **Lens selector**: Checkbox group (Structural always on + Evaluative toggle + Observable toggle)
- **View selector**: Boundary View, Spec Realization, Change View, saved views, LLM-generated views
- **Ask input**: Natural language → view generation
- **Saved Views**: CRUD via `GET/POST/PUT/DELETE /workspaces/:workspace_id/explorer-views`
- **View Spec Editor**: "Edit view spec" button opens JSON editor panel

## Implementation Plan

1. **Enhance ExplorerView canvas**:
   - Verify zoomable/pannable SVG canvas
   - Implement spec coverage coloring (green/amber/red based on `spec_confidence`)
   - Size nodes by complexity or churn metrics
   - Default zoom to show boundaries and interfaces

2. **Lens selector** (checkbox group):
   - Structural (always on) — base layer
   - Evaluative (toggle) — overlay test status, gate outcomes, spec coverage, risk
   - Observable (toggle, future) — placeholder for production telemetry
   - When Evaluative active, fetch data from `GET /repos/:id/graph/risks` and gate endpoints

3. **View selector**:
   - Dropdown: Boundary View (default), Spec Realization, Change View
   - Saved views section (from CRUD endpoints)
   - "Generate view..." option (triggers Ask input)
   - View Spec Editor: collapsible JSON editor alongside canvas

4. **In-view filter panel**:
   - 200px collapsible left panel inside Architecture content area
   - Category filters: Boundaries, Interfaces, Data, Specs, Concepts
   - Toggle via filter icon in control bar

5. **Right-click context menu**:
   - View spec, View provenance, View history, Open in code options
   - Standard context menu component

6. **Ask input** (natural language):
   - Text input that sends to `POST /workspaces/:id/explorer-views/generate`
   - Renders generated view on canvas
   - Ephemeral views, user can save explicitly

7. **Tests**:
   - Lens toggle changes data enrichment
   - View selector renders different layouts
   - Filter panel toggles node visibility
   - Right-click context menu appears

## Acceptance Criteria

- [ ] Canvas shows boundaries and interfaces at default zoom
- [ ] Spec coverage coloring (green/amber/red) applied to nodes
- [ ] Lens selector as checkbox group with Structural/Evaluative/Observable
- [ ] Evaluative lens fetches and overlays risk data
- [ ] View selector with built-in views, saved views, generate option
- [ ] In-view filter panel (200px, collapsible) with category filters
- [ ] Right-click context menu with View spec/provenance/history/code
- [ ] Ask input sends to generate endpoint and renders result
- [ ] View Spec Editor opens collapsible JSON editor
- [ ] Tests pass

## Agent Instructions

Read `system-explorer.md` §1 "The Navigable Architecture" and `ui-navigation.md` §3 "Tab: Architecture" for the full feature set. The existing `ExplorerView.svelte`, `ExplorerFilterPanel.svelte`, and `EvaluativeOverlay.svelte` components are starting points. Check what's already built in these components before adding features. The saved views CRUD endpoints may already be registered — check `mod.rs` for `/explorer-views`. The lens selector should use checkboxes (not a dropdown) per the spec. The Evaluative lens requires data from `GET /repos/:id/graph/risks` — verify this endpoint exists.

## Shipped

The Navigable Architecture surface (system-explorer.md §1 + ui-navigation.md
§3 "Tab: Architecture") on the task-065/170/177 canvas substrate: zoomable/
pannable canvas with semantic zoom (boundaries at default zoom, interfaces on
drill-in), spec-coverage green/amber/red coloring, complexity/churn node
sizing, click → detail panel, double-click → C4 drill-down, `/` search,
timeline scrubber, right-click context menu (View spec / provenance /
history / code), lens checkbox group (Structural locked on, Evaluative
additive overlay fetching `GET /repos/:id/graph/risks`, Observable future),
view-selector dropdown (built-in → saved → Generate view…), 200px collapsible
in-view filter panel, Ask input (GeneratedViewChat → POST
/workspaces/:id/explorer-views/generate), and the View Spec Editor. The
review-repair round (filter reaches tree-group containers, mutation-proof
draw tests, REST saved views, Observable a11y) is commit 618f415.

### This round: rebase onto base cfeefbda (repair 71d93b0c)

Rebased the task branch onto the updated base (task-091 head), which
introduced the flow-view / workspace-graph line of work in
ExplorerCanvas.svelte and ExplorerView.svelte. Resolved both conflicts:

- `web/src/lib/ExplorerCanvas.svelte` — both sides added import blocks at
  the same site; kept both (flow-particles/particle-layer from the base,
  filter-categories from task-178). Verified every symbol from both blocks
  is used in the merged draw loop: the particle overlay (ParticleLayer,
  particlesAtTime, trailSegments, …) and the tree-group filter
  classification (nodeCategory, nodeMatchesConcept) coexist on the same
  canvas.
- `docs/ui.md` — merged the task-178 dropdown view-selector description
  with the base's richer ui-layout.md §5 Level-1 (workspace-scope graph)
  wording; the "view-selector tabs" phrasing from the old base was dropped
  because it contradicts the dropdown the code ships.

Merge commit: 177990c31.

### Test evidence

Focused web probes only (npm ci with locked versions; full suites, server
builds, and web/dist rebuilds are owned by independent verification):

- Batch 1 — 12 files, 274 tests passed: ExplorerCanvas, ExplorerCanvas.flow,
  ExplorerCanvas.evaluative-lens, filter-categories, ExplorerDefaultViews,
  ExplorerViewLensUrl, ExplorerViewNavigableArchitecture, ExplorerViewScope,
  ExplorerViewWorkspaceGraph, FlowCanvas, flow-particles, particle-layer.
- Batch 2 — 12 files, 119 tests passed: canvas-filters, ExplorerViewDrillUrl,
  ExplorerCanvasDrillDown, ExplorerCanvasTrace, ExplorerCanvasLiveness,
  ExplorerViewLivenessWiring, ExplorerGeneratedViews, ExplorerViewViewEvents,
  ExplorerViewShortcuts, ExplorerViewLayoutRenderers, flow-layout,
  ExplorerCanvas-performance.

Total 393/393 passing, covering both sides of the merge. Evidence and
command output retained in /tmp/stage/review-evidence (controller artifact
path, not a repo path).
