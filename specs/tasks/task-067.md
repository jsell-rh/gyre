---
title: "Progressive Drill-Down & Breadcrumb Navigation"
spec_ref: "explorer-canvas.md §5"
depends_on:
  - task-065
progress: complete
coverage_sections:
  - "explorer-canvas.md §5 Progressive Drill-Down"
commits: ["7519d04cec17d1e9f82d965e2b87cef98a0aa241"]
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

## Shipped

Real progressive drill-down over the knowledge graph's `Contains` edges in
`web/src/lib/ExplorerCanvas.svelte`, driven end-to-end by real mouse events
and verified through the component's real hit-testing (node positions
discovered via hover tooltips, not internals).

### Behavior

- Double-click on a container (package/module/type — a real graph node, or a
  synthetic treemap group resolved to its `Contains` scope node via
  `resolveTreeGroupScopeNode`) drills the canvas to exactly that node's
  children; siblings and the rest of the graph are filtered out.
- Breadcrumb shows `root > package > module > type` (root segment labeled
  with the repo name via the new `rootLabel` prop wired from ExplorerView);
  every segment, including the root, navigates back up and restores that
  level's topology (reverse transition).
- Two-phase animated zoom transition (rAF, ease-out cubic, 300ms per phase):
  phase 1 zooms into the clicked node while unrelated nodes fade out
  (`drillFadeAlpha`, floor 0.12, fade target = the clicked layout cell so the
  cell and its children stay bright); the scope swap (breadcrumb push →
  layout rebuild) is deferred to phase-1 completion (`pendingDrill`) so the
  fade-out plays on the old layout instead of an abrupt content jump; phase
  2 smoothly settles the camera on the new level's fitted view.
- Leaf nodes (function/field) never drill: double-click opens the detail
  panel (`onNodeDetail`) and zooms to the node.
- Lens toggle is orthogonal to drill level: evaluative overlay renders at
  any level; trace particles are scoped to `layoutNodeMap` (current level)
  and per-node span stats surface only for current-level nodes.

### Defects found and fixed in the drill substrate

- NaN camera: drill transitions stored their start pose on the `cam` object
  (`cam._drillStartX`), but the layout `$effect` replaces `cam` wholesale on
  every breadcrumb change, so `lerpCam` interpolated from `undefined` →
  `cam.x/y` became NaN → every hit-test rect "contained" every point (after
  any drill, hovering anywhere returned one arbitrary node). Fixed by keeping
  the start pose in a separate `drillStartCam` plus a non-finite-camera guard
  in `lerpCam`.
- Jump-cut scope swap: the breadcrumb push happened in the same tick as the
  double-click, so "fade out unrelated nodes" never actually played. Fixed
  with the deferred `pendingDrill` application at phase-1 completion.

### Test evidence

`web/src/__tests__/ExplorerCanvasDrillDown.test.js` (replaces the checkpoint's
diagnostic `probe-drill.test.js`, deleted together with the follow-up
probes):

- Full §5 chain via real double-clicks: L0 packages → L1 modules (lib,
  greeting; full-sweep absence check for other-service/User) → L2 types
  (GreetingService, User) → L3 members (greet, config); breadcrumb text
  asserted at each level; segment + root navigation restores each level;
  single- and double-click on the greet leaf call `onNodeDetail` and never
  change the breadcrumb; lens toggle switches at a drilled level.
- Recorded-2d-context instrumentation: during a drill the camera passes
  through ≥3 distinct poses (kills jump-cut and the NaN-camera regression —
  NaN collapses all frames to one Set entry), node draw opacity falls below
  0.1 (fade-out), and recovers after settling (p75 after > p75 during,
  max ≥ 0.9).
- Deep-linked L3 render with evaluative lens + trace data: only
  current-level nodes present/hoverable, evaluative UI renders, hovered
  nodes show their own span stats (p50/error-rate); spans on out-of-level
  nodes do not leak into tooltips.

Results (focused runs, sandbox-local via `/tmp/stage/bin/npx vitest`,
re-run on the recovered source after lease expiry):

- `ExplorerCanvasDrillDown.test.js` → 3/3 passed (exit 0).
- `ExplorerCanvas.test.js` + `ExplorerViewDrillUrl.test.js` → 154/154
  passed (exit 0).
- Pre-recovery full runs on the identical source tree: ExplorerCanvas +
  performance + ExplorerViewDrillUrl + ExplorerViewScope + NodeDetailPanel
  + DrillDown → 237/237; ExplorerView-responsive + RepoMode +
  WorkspaceHomeArchitecture + canvas-state → 61/61.

### Source recovery

The original final commit `34a9d4a1` was lost to a worker lease expiry;
the source was recovered verbatim as checkpoint `7519d04c` (its `web/src`
tree is byte-identical to the current branch head) and merged with the
base `9bef21a4` at `af05cc66`. The `commits:` frontmatter was remapped
from the dead SHA to `7519d04c` so review scoping sees the commit that
actually carries the product surface. Post-recovery focused runs above
confirm the recovered source behaves identically.

Full `cd web && npm test`, architecture checks and CI are owned by the
verification stage. Evidence: `/tmp/stage/review-evidence/task-067-implementation.md`.
