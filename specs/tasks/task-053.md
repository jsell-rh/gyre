---
title: "Explorer Phase 4 — Large Graph Performance & Responsive Layout"
spec_ref: "explorer-implementation.md §Migration Plan > Phase 4: Polish"
depends_on: 
  - task-029
  - task-031
  - task-036
  - task-038
progress: complete
review: specs/reviews/task-053.md
coverage_sections: []
commits: ["b1abc22257e5652f582049b9449edcc1daff370b", "2bd4b013b4f4f3d52f3db964db817578cf7fb8a7"]
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

## Shipped

The TASK-053 production implementation, benchmark/responsive tests, and
R1-R3 review fixes are in this branch's history (recovered by the pipeline
checkpoint commits). The R4 mutation-analysis review
(`specs/reviews/task-053.md`) then found the recovered performance tests
were not mutation-sensitive: every Part A mechanism could be individually
disabled with the suite still passing, and no test ever moved the camera.
Commit `b1abc2225` (recorded in the frontmatter above) repaired exactly
that, plus the real pinch-anchoring production bug the new tests exposed.

This round is the contract-repair round: the executor restored the assigned
task document, and this assignment preserved the production fix unchanged
and re-verified it on the merged base (task-065 stacked on the same
files). No production code changed in this round.

### Behavior (Part A - large graph performance)

- **Viewport culling**: `ExplorerCanvas.svelte` `isVisible()` performs a
  frustum intersection per node; `drawNodeRecursive` prunes entire invisible
  subtrees; `drawEdges` frustum-culls edges whose endpoints are both
  off-screen.
- **Level-of-detail rendering**: leaf labels render only when
  `sw > 30 && sh > 14 && cam.zoom >= 0.5`; at low zoom nodes are simple
  colored rectangles. Semantic zoom additionally gates node types
  (packages, then types, endpoints, functions, fields) at zoom
  0.3/0.6/1.0/2.0.
- **Edge bundling/culling**: `drawEdges` switches to `drawBundledEdges`
  (group-to-group aggregate arrows with "N edges" count labels) when
  `cam.zoom < 0.5` or `renderEdges.length > 5000`.
- **Incremental layout**: the layout `$effect` recomputes only on
  graph-data/breadcrumb/filter changes; pan/zoom mutate the non-reactive
  `cam` and never trigger layout.
- **Benchmark**: `ExplorerCanvas-performance.test.js` generates a 10k-node /
  20k-edge hierarchical mock graph and asserts CPU-time regression guards
  and total-work bounds. The R4 revision made the mechanism tests
  mutation-sensitive (each verified to fail under its disabling mutation):
  culling under a deep-zoomed camera, LOD label suppression at mid zoom
  with labels reappearing at high zoom, semantic-zoom node-type gates, and
  edge bundling asserted by its unique observable (the "N edges" bundle
  labels) on both the >5000-edge and low-zoom branches. Interaction tests
  simulate real wheel/drag/pinch input and assert the world point under
  the cursor or pinch midpoint is preserved. The >30fps target cannot be
  measured in jsdom (no real frame loop/GPU); the per-frame draw-work
  bounds under a moving camera are the enforceable proxy for what keeps
  interaction above the target in a real browser.

### Behavior (Part B - responsive layout)

- **<768px**: stacked layout; canvas full-width on top, chat as a
  full-screen overlay with its own collapse bar (`@media (max-width: 768px)`
  in `components/ExplorerView.svelte`).
- **769-1024px**: chat panel collapsible via a toolbar collapse button;
  collapsed state shows a floating "Chat" expand button.
- **Viewport-widen reset**: a `matchMedia('(min-width: 1025px)')` listener
  resets `chatCollapsed = false` so the chat can never be stranded hidden on
  wide viewports (R1-F4 fix), with teardown on destroy.
- **Touch**: `ontouchstart`/`ontouchmove`/`ontouchend` are wired on the
  canvas; single-finger drag pans, two-finger pinch zooms toward the pinch
  midpoint (world point under the midpoint is preserved, via the
  `targetCam`-based anchor fix in `b1abc2225`).

### Test evidence (contract-repair round, merged tree)

Focused probes on this branch (clean tree, `npm ci` with locked versions,
evidence saved under `/tmp/stage/review-evidence/task-053-contract-repair/`):

- `npx vitest run src/__tests__/ExplorerCanvas-performance.test.js
  src/__tests__/ExplorerView-responsive.test.js`: **27/27 pass**
  (19 performance: 10k/15k-node renders with CPU-time guards, camera
  interaction tests, culling/LOD/semantic-zoom/bundling under real moved
  cameras; 8 responsive: collapse/expand click interactions, matchMedia
  viewport-sync reset, aria labels).
- `npx vitest run` on the seven other explorer test files (ExplorerCanvas,
  ExplorerCanvasDrillDown, ExplorerChat, ExplorerChat-streaming,
  ExplorerViewDrillUrl, ExplorerViewScope, ExplorerViewShortcuts):
  **284/284 pass** (no regressions from the merge with the task-065 base).
- **Mutation spot-check** (independent re-verification that the R4 tests
  stayed sensitive after the merge): with `isVisible()` mutated to an
  early `return true` in `ExplorerCanvas.svelte`, the deep-zoom culling
  test fails (node draws per frame far exceed the bounded expectation);
  source restored afterwards, tree clean.

Full workspace suites, architecture checks, and GitHub CI are owned by the
verification and publication stages.
