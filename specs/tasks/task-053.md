---
title: "Explorer Phase 4 — Large Graph Performance & Responsive Layout"
spec_ref: "explorer-implementation.md §Migration Plan > Phase 4: Polish"
depends_on: 
  - task-029
  - task-031
  - task-036
  - task-038
progress: ready-for-review
review: specs/reviews/task-053.md
coverage_sections: []
commits: ["b1abc22257e5652f582049b9449edcc1daff370b"]
---

## Spec Excerpt

From `explorer-implementation.md` §Migration Plan > Phase 4: Polish:

> - Mobile/responsive layout
> - Performance optimization for large graphs (>10k nodes)

These are the two remaining Phase 4 items not covered by existing tasks (TASK-036 covers streaming/status, TASK-038 covers keyboard shortcuts).

## Current State

- **Performance:** ExplorerCanvas renders all nodes and edges in a single canvas 2D pass. For repos with >10k nodes, this causes frame drops during pan/zoom. No virtualization, culling, or level-of-detail rendering exists.
- **Responsive layout:** The explorer view uses a fixed two-panel layout (canvas + chat). On narrow viewports (<768px), the chat panel overflows or is inaccessible. No breakpoint-based layout adjustment exists.

## Implementation Plan

### Part A: Large Graph Performance

1. **Viewport culling** — Only render nodes and edges within the visible viewport (plus a buffer zone). Skip draw calls for off-screen nodes entirely. The node positions are already computed; add a viewport intersection test before rendering each node.

2. **Level-of-detail rendering** — At low zoom levels (zoomed out), render nodes as simple colored rectangles without text labels. Switch to full rendering (name, type, badges) when zoom level is high enough that labels would be readable.

3. **Edge bundling/culling** — For graphs with >5k edges, skip rendering edges between nodes that are both off-screen. Consider rendering only edges connected to visible or selected nodes at very low zoom levels.

4. **Incremental layout** — If ELK/dagre layout computation is slow for >10k nodes, cache the layout and only recompute when the graph data changes (not on zoom/pan).

5. **Benchmark** — Add a performance test that loads a mock graph with 10k nodes and 20k edges, measures initial render time and pan/zoom frame rate. Target: <100ms initial render, >30fps during interaction.

### Part B: Responsive Layout

1. **Breakpoint-based layout** — At viewport widths <768px:
   - Stack canvas and chat vertically (canvas on top, chat below)
   - Or: hide chat panel by default, show a floating "Chat" button that opens chat as an overlay
   - Canvas should use full viewport width

2. **Touch interaction** — Ensure pan/zoom works with touch gestures (pinch-to-zoom, two-finger drag). The canvas 2D rendering should already support this via pointer events, but verify.

3. **Chat panel collapse** — On medium viewports (768px–1024px), make the chat panel collapsible to give more space to the canvas. Toggle via a button in the toolbar.

## Acceptance Criteria

- [ ] Explorer canvas renders 10k+ node graphs without dropping below 30fps during pan/zoom
- [ ] Off-screen nodes are culled from rendering
- [ ] Low-zoom levels use simplified node rendering (no text labels)
- [ ] On viewports <768px, layout adapts (stacked or overlay chat)
- [ ] Chat panel collapsible on medium viewports
- [ ] Touch gestures (pinch-to-zoom, drag) work on the canvas
- [ ] Performance benchmark with 10k nodes passes targets
- [ ] `npm test` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `web/src/lib/ExplorerCanvas.svelte` for the current rendering loop (canvas 2D)
3. Read `web/src/lib/ExplorerChat.svelte` for the chat panel layout
4. Read the container component that holds both panels (ExplorerView or equivalent)
5. Implement Part A (performance) first — it has higher impact
6. Implement Part B (responsive) second
7. Create a mock graph with 10k+ nodes for testing (generate programmatically in the test)
8. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- `feat(ui): add responsive layout and large graph performance (TASK-053)`
- `fix(process): add JS/TS mirrored-logic and tautological assertion checks (TASK-053)`
- `fix(ui): address R1 review findings — viewport sync, real tests (TASK-053)`
- `fix(ui): address R2 review findings — comparative tests, dead code, edge assertions (TASK-053)`
- `fix(ui): mutation-sensitive explorer perf tests, pinch anchor fix (TASK-053)` (`b1abc2225`, this branch)

## Shipped

The TASK-053 implementation (production code, responsive/benchmark tests,
and the R1–R3 review fixes) was recovered into this branch's history by the
pipeline checkpoint commits. An independent mutation-analysis review (R4,
`specs/reviews/task-053.md`) then found that the recovered performance
tests were not mutation-sensitive — every Part A mechanism (viewport
culling, LOD, edge bundling) could be individually disabled with the full
suite still passing, and no test ever moved the camera. Commit `b1abc2225`
on this branch repairs exactly that, plus one real production bug the new
tests exposed.

### What `b1abc2225` changed

- **Test infrastructure**: the synchronous rAF mock wedged the component's
  redraw loop (a stale truthy `animFrame` id made every later
  `scheduleRedraw()` a no-op), which is why camera interaction could not
  be simulated. Replaced with an explicit rAF queue + `flushFrames()`; the
  camera pose is read back from the minimap viewport rectangle (stroked in
  world coordinates once per frame).
- **New mutation-sensitive tests** (each verified to fail under its
  disabling mutation, logs under `/tmp/stage/review-evidence/task-053-r4/`):
  culling under a deep-zoomed camera, LOD label suppression at mid zoom
  (0.3–0.5) with labels reappearing at high zoom, semantic-zoom node-type
  gates, edge bundling on both the >5000-edge and low-zoom branches.
- **New interaction tests**: wheel zoom-in (world point under cursor
  preserved), wheel zoom-out, mouse-drag pan, single-finger touch pan,
  two-finger pinch.
- **Pinch-zoom bug fix**: `onTouchMove` computed the midpoint correction
  via `screenToWorld()`, which reads the lagging display camera — the
  correction was identically zero, so pinch zoom anchored at the screen
  center instead of the pinch midpoint. Now solved against `targetCam`,
  the same math as `applyZoomAtMouse`.
- **Removed superseded weak tests** (initial-fit draw-count bounds, the
  small-vs-large fillText comparison); kept the honest total-work bounds
  and CPU-time regression guards.

### Behavior (Part A — large graph performance)

- **Viewport culling**: `ExplorerCanvas.svelte` `isVisible()` performs a
  frustum intersection per node; `drawNodeRecursive` prunes entire invisible
  subtrees; `drawEdges` frustum-culls edges whose endpoints are both
  off-screen.
- **Level-of-detail rendering**: leaf labels render only when
  `sw > 30 && sh > 14 && cam.zoom >= 0.5`; semantic zoom additionally gates
  node types at zoom 0.3/0.6/1.0/2.0.
- **Edge bundling/culling**: `drawEdges` switches to `drawBundledEdges`
  (group-to-group aggregate arrows with "N edges" count labels) when
  `cam.zoom < 0.5` or `renderEdges.length > 5000`.
- **Incremental layout**: the layout `$effect` recomputes only on
  graph-data/breadcrumb/filter changes; pan/zoom mutate the non-reactive
  `cam` and never trigger layout.
- **Benchmark**: 10k/15k-node renders with CPU-time regression guards. The
  >30fps interaction target cannot be measured in jsdom (no real frame
  loop/GPU); the interaction tests simulate real wheel/drag/pinch input
  through the component's handlers and the culling/LOD/bundling tests
  assert per-frame draw work stays bounded while the camera moves — the
  mechanisms that keep interaction above the target in a real browser.

### Behavior (Part B — responsive layout)

- **<768px**: stacked layout — canvas full-width on top, chat as a
  full-screen overlay with its own collapse bar (`@media (max-width: 768px)`
  in `components/ExplorerView.svelte`).
- **769–1024px**: chat panel collapsible via a toolbar collapse button;
  collapsed state shows a floating "Chat" expand button.
- **Viewport-widen reset**: a `matchMedia('(min-width: 1025px)')` listener
  resets `chatCollapsed = false` so the chat can never be stranded hidden on
  wide viewports (R1-F4 fix), with teardown on destroy.
- **Touch**: `ontouchstart`/`ontouchmove`/`ontouchend` are wired on the
  canvas; single-finger drag pans, two-finger pinch zooms toward the pinch
  midpoint (world point under the midpoint preserved — now actually true
  after the `b1abc2225` anchor fix).

### Test evidence

Focused probes on this branch (clean tree, `npm ci` with locked versions,
evidence saved under `/tmp/stage/review-evidence/task-053-r4/`):

- `npx vitest run src/__tests__/ExplorerCanvas-performance.test.js` —
  **19/19 pass** (10k/15k CPU guards and total-work bounds; 5 camera
  interaction tests; culling/LOD/semantic-zoom/bundling under real moved
  cameras; individual-edge small-graph test).
- **Mutation verification** (each mutation applied to
  `ExplorerCanvas.svelte`, targeted test run, source restored):
  - `isVisible()` → `return true`: culling test fails (4502 node fills per
    frame at deep zoom vs ≤60 expected).
  - leaf label guard → `if (true)`: LOD test fails (3471 leaf labels at mid
    zoom vs 0 expected).
  - semantic-zoom gates removed: semantic test fails (3473 fills for the
    function-leaf graph vs ≤50 expected).
  - edge bundling condition → `if (false)`: both bundling tests fail
    (0 "N edges" bundle labels vs >0 expected).
  - pinch midpoint anchoring removed: pinch test fails (19.5 world-unit
    drift vs <0.5 expected).
- `npx vitest run src/__tests__/ExplorerCanvas.test.js
  src/__tests__/ExplorerCanvasDrillDown.test.js src/__tests__/ExplorerChat.test.js
  src/__tests__/ExplorerChat-streaming.test.js src/__tests__/ExplorerViewDrillUrl.test.js
  src/__tests__/ExplorerViewScope.test.js src/__tests__/ExplorerViewShortcuts.test.js
  src/__tests__/ExplorerView-responsive.test.js` — **292/292 pass** (no
  regressions from this repair or the later tasks stacked on these files).
- Mechanical checks pass: `check-mirrored-logic-tests-js.sh`,
  `check-tautological-assertions-js.sh`, `check-assertionless-tests.sh`,
  `check-task-commit-attribution.sh`.

Full workspace suites, architecture checks, and GitHub CI are owned by the
verification and publication stages.
