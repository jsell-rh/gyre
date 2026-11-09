---
title: "Navigable Architecture — canvas layout, interaction model, lens selector"
spec_ref: "system-explorer.md §1"
depends_on: [task-170, task-177]
progress: ready-for-review
coverage_sections:
  - "system-explorer.md §1. The Navigable Architecture"
  - "ui-navigation.md §Tab: Architecture (Moldable Development Surface)"
commits: ["197222ba96f83d640031cd04880e3f157deb3323", "af179daa1"]
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

The Navigable Architecture surface (system-explorer.md §1 + ui-navigation.md §3
"Tab: Architecture") on top of the task-065/170/177 canvas substrate. The
pre-existing pieces verified as already real (not re-implemented): zoomable/
pannable canvas with semantic zoom showing boundaries then interfaces by zoom
level, spec-coverage green/amber/red coloring (`lens-rendering.js`
coverageDecoration, pre-existing), complexity/churn node sizing, click → detail
panel, double-click → C4 drill-down, `/` search, timeline scrubber overlay,
right-click context menu with View spec / View provenance / View history /
Open in code, and the Ask input (GeneratedViewChat → POST
/workspaces/:id/explorer-views/generate → rendered view).

### Lens selector — checkbox group (was exclusive buttons)

`ExplorerCanvas.svelte` toolbar: Structural is a checked+locked checkbox
(always-on base layer), Evaluative is an additive-overlay checkbox toggle
(checked = `lens === 'evaluative'`, the rendering path where the evaluative
overlay composes on the structural base), Observable is a locked-off future
toggle whose label surfaces the telemetry-integration notice banner. Per
ui-navigation.md §3 "Checkbox group (not a dropdown — checkboxes allow
multi-select for composition)".

### Evaluative lens fetches and overlays risk data

`ExplorerView.svelte` fetches `GET /repos/:id/graph/risks` when the Evaluative
lens is active (best-effort, repo-change-safe, retry on failure) and passes a
`riskByNode` map (churn × complexity, mirroring the canvas's risk-score
definition but sourced from the server's stored metrics) to the canvas. The
canvas gains a `risk` evaluative metric: `EvaluativeOverlay` renders a Risk
heat button when risk data is present (also without trace data), and the
metric colors nodes through the existing evaluative heat pipeline
(`getNodeMetricValue`/`evaluativeNodeColor`).

### View selector — dropdown

Replaces the Boundary|Spec Realization|Change|System Trace tab row with a
dropdown (`.explorer-view-select`, keeps `data-testid="explorer-view-selector"`)
listing built-in views, then the workspace's saved views (delivered by the
explorer chat WebSocket `views` message, applied via their stored view query —
works with or without the socket), then a "Generate view…" option that focuses
the Ask/generate chat column. Selecting a saved view shows its name on the
collapsed button and applies its query to the canvas.

### In-view filter panel (200px, collapsible)

New `ExplorerFilterPanel.svelte` inside the Architecture content area (left of
the canvas, `NOT` the app sidebar), toggled by a filter icon in the control
bar. Category checkboxes — Boundaries, Interfaces, Data, Specs — with
per-category counts, plus a Concepts section listing predefined concepts
discovered from the knowledge graph (`filter-categories.js`: cross-cutting
name tokens shared by ≥2 nodes, stopword-filtered; membership uses the same
case-insensitive substring semantics as `GET /repos/:id/graph/concept/:name`).
The panel state reduces to a hidden-node-id set (`hiddenNodeIds` prop); the
canvas skips hidden nodes and edges touching them in the draw loop, and a
redraw effect reacts to filter/risk changes (the round-trip test caught that
this redraw was initially missing — a real defect fixed in this task).

### View Spec Editor

An "Edit view spec" button (code icon) in the control bar opens a collapsible
JSON editor panel for the view spec grammar (data/layout/encoding/highlight).
It prefills from the active generated ViewSpec or one synthesized from the
current view state; Apply validates JSON, translates via `viewSpecToViewQuery`,
and re-renders the canvas live (annotation title proves the application);
invalid JSON shows an inline `role=alert` error and applies nothing.

### Tests

- `filter-categories.test.js` (new, 14): classification, concept discovery,
  concept matching, hidden-set computation incl. category×concept
  intersection, counts.
- `ExplorerViewNavigableArchitecture.test.js` (new, 11): lens checkbox group
  semantics; Evaluative checkbox → repoGraphRisks fetch + Risk metric
  appearance/disappearance; filter panel toggle/collapse with counts;
  category-uncheck and concept-select hide nodes in the actual canvas draw
  output (fillText capture) with round-trip restore (non-vacuous); saved views
  in the dropdown load and apply; "Generate view…" focuses the generator;
  view spec editor prefill/apply/error paths.
- Updated for the checkbox group / dropdown: ExplorerCanvas.test.js,
  ExplorerCanvasDrillDown, ExplorerCanvasLiveness, ExplorerViewLensUrl,
  ExplorerDefaultViews, ExplorerGeneratedViews (assertion strength kept —
  tabs→dropdown options, active-class→checked/disabled state).
- E2E: `explorer-visual.spec.js` evaluative-lens selector updated to the
  checkbox; the three chrome-affected baselines regenerated (toolbar-default,
  toolbar-evaluative, view-query-container — 2px taller from the checkbox
  chrome) against a transient local build; web/dist itself restored to the
  assignment base for remote integration rebuild.

Evidence: full web suite 100 files / 2025 tests green; focused suites 215
green; explorer-visual E2E 16/16 green (see /tmp/stage/review-evidence).
