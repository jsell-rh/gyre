---
title: "Progressive Drill-Down & Breadcrumb Navigation"
spec_ref: "explorer-canvas.md §5"
depends_on:
  - task-065
progress: not-started
coverage_sections:
  - "explorer-canvas.md §5 Progressive Drill-Down"
commits: []
---

## Spec Excerpt

**§5 Progressive Drill-Down:**

The canvas supports hierarchical navigation following the knowledge graph's `Contains` edges:

```
Level 0: All packages in the repo
  double-click "greeting-service" package
Level 1: Modules inside greeting-service (lib, greeting)
  double-click "greeting" module
Level 2: Types/functions inside greeting (GreetingService, GreetingConfig, User, ApiResponse)
  double-click "GreetingService" type
Level 3: Fields and methods of GreetingService
  click "greet" method → detail panel with code, spec linkage, provenance
```

**Breadcrumb bar** shows the drill-down path: `e2e-repo > greeting-service > greeting > GreetingService`. Each segment is clickable to navigate back up.

**Zoom transition**: When drilling down, the canvas smoothly zooms into the clicked node, fades out unrelated nodes, and renders the children in the same layout style. When navigating back up, the reverse transition plays.

At every level, the lens toggle works the same way — structural shows static topology of that level, evaluative overlays trace data scoped to spans touching nodes at that level.

## Implementation Plan

### Existing Code

- `web/src/lib/ExplorerCanvas.svelte` — already imports `Breadcrumb` component and has `canvasState.breadcrumb` in the bindable state.
- `web/src/components/Breadcrumb.svelte` — breadcrumb navigation component.

### Work Required

1. **Double-click drill-down**: Verify double-clicking a container node (package, module, type) filters the canvas to show only its children (nodes connected via outgoing `Contains` edges). The parent node and siblings should fade out.

2. **Breadcrumb bar**: Verify the breadcrumb shows the full drill-down path (e.g., `repo > package > module > type`). Each segment must be clickable to navigate back up to that level.

3. **Zoom transition animation**: When drilling down, the canvas should:
   - Smoothly zoom into the area of the clicked node
   - Fade out unrelated nodes (not abrupt disappearance)
   - Render children in the same treemap layout style
   - When navigating back up (clicking breadcrumb), reverse the transition

4. **Level-scoped lenses**: Verify that at each drill-down level, the lens toggle still works. Evaluative overlay should scope trace data to spans touching nodes at the current level.

5. **Edge case: leaf nodes**: Double-clicking a leaf node (function, field) should NOT drill down (no children). Instead, it should open the detail panel.

## Acceptance Criteria

- [ ] Double-click a container node → canvas shows only its children via `Contains` edges
- [ ] Breadcrumb shows full path: `repo > package > module > type`
- [ ] Each breadcrumb segment is clickable → navigates back to that level
- [ ] Smooth zoom transition when drilling down (not abrupt)
- [ ] Fade-out of unrelated nodes during drill-down
- [ ] Reverse transition when navigating back up via breadcrumb
- [ ] Lens toggle works at every drill-down level
- [ ] Double-click on leaf node opens detail panel instead of drilling down
- [ ] `cd web && npm test` passes

## Agent Instructions

Read `specs/system/explorer-canvas.md` §5 (Progressive Drill-Down). Then audit:
- `web/src/lib/ExplorerCanvas.svelte` — look for double-click handling, `Contains` edge filtering, drill-down state
- `web/src/components/Breadcrumb.svelte` — breadcrumb rendering and click handlers

Key implementation detail: The drill-down works by filtering the displayed graph to children of the clicked node. The ExplorerCanvas likely already tracks a "current parent" or breadcrumb path. Verify the zoom transition is animated (CSS transition or requestAnimationFrame), not a jump-cut.
