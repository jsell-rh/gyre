---
title: "Navigable Architecture — canvas layout, interaction model, lens selector"
spec_ref: "system-explorer.md §1"
depends_on: [task-170, task-177]
progress: ready-for-review
coverage_sections:
  - "system-explorer.md §1. The Navigable Architecture"
  - "ui-navigation.md §Tab: Architecture (Moldable Development Surface)"
commits: ["618f415278d033b796295fc49ad55947513c81bd", "197222ba96f83d640031cd04880e3f157deb3323", "06d94c7af10ba557393a88833c7c32866a6a9e89"]
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
/workspaces/:id/explorer-views/generate), and the View Spec Editor.

This round is the review repair of the four findings from checkpoint
16e5800 (commit 618f415):

### Boundaries filter reached tree-group containers (was a visible no-op)

At default zoom the boundary boxes on screen are the synthetic
package/module tree-group containers (buildPathTree/layoutPathTreeNode),
which the draw loop exempted from the hiddenNodeIds filter — unchecking
Boundaries changed nothing in the canonical default view. Tree-group
containers are now filtered through their own node
(`treeGroupFilterHidden` in ExplorerCanvas: `nodeCategory` for unchecked
categories, `nodeMatchesConcept` for concept focus — the same
filter-categories.js semantics as the leaf rules). A filtered group skips
its own box but keeps drawing children, so zoomed-in content of other
categories stays visible when only the container's category is unchecked;
the minimap applies the same rule. The redraw effect now tracks the filter
props by identity (swapping one unchecked category for another keeps the
hidden set's size).

### Canvas-draw assertions are mutation-proof (were vacuous)

The old snapshot-and-clear capture read empty whenever the canvas was idle
between redraws, so `not.toContain` passed with the production hiding code
deleted (reviewer's mutation probe: 11/11). The capture is now
frame-bucketed per canvas element (stable per-element ctx mock;
`drawFrame` begins with `ctx.setTransform`, delimiting frames): the latest
frame's labels persist, and the tests wait for a frame drawn after the
interaction (`labelsOfNextRedraw`) before asserting, with a settle wait
before the baseline capture. Verified by re-running the mutations:
removing the whole filter block, or only the tree-group branch, fails both
the Boundaries and the concept-focus tests (2/12 each); the restored code
passes 12/12.

### Saved views load via REST when the dropdown opens

The dropdown's saved-views section no longer depends solely on the explorer
chat WebSocket's `views` message. Opening the dropdown fetches
`GET /workspaces/:id/explorer-views` (api.explorerViews) and
`GET /repos/:id/views` (api.savedViews) — merged by id, mirroring the
socket's ListViews scope mix; workspace records carry a ViewSpec and are
translated via `viewSpecToViewQuery`, repo records carry the ViewQuery
directly. Socket updates merge over the REST-fetched list preserving
already-known query fields. New test covers the no-socket path end to end
(list → select → canvas annotation applied).

### Observable lens toggle is keyboard-operable (a11y)

The `<label onclick>` wrapper around the disabled Observable checkbox
emitted `a11y_click_events_have_key_events` and
`a11y_no_noninteractive_element_interactions` on every build/test run. It is
now a `<div role="checkbox" aria-checked="false" aria-disabled="true"
tabindex="0">` with onclick + onkeydown (Enter/Space surface the telemetry
notice) around the same disabled input — visually identical (all styling is
class-driven; no CSS targets the removed label element), and the new
warnings are gone from the build output.

### Test evidence

- `ExplorerViewNavigableArchitecture.test.js`: 12/12 (boundaries + concept
  tests now kill the production-code mutations; new REST saved-views and
  Observable keyboard tests).
- Mutation probes (evidence in /tmp/stage/review-evidence): filter block
  removed → 2 failures; tree-group branch only removed → 2 failures;
  restored → 12/12.
- Related suites green: ExplorerCanvas* (6 files) + ExplorerDefaultViews +
  ExplorerGeneratedViews + ExplorerViewLensUrl + filter-categories (265);
  ExplorerView-* (8 files) + ExplorerChat* (3) + GeneratedViewChat (186);
  App/Workspace/RepoMode suites importing these components (210 passed,
  41 skipped).
- E2E visual baselines not regenerated: the only DOM change (label→div with
  identical class styling) is pixel-neutral, and exact-head GitHub checks
  verify after independent review.
