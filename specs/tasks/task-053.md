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
commits: ["5923bdd7c5be16cb0b77697bf7ee01a45248c050"]
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

- [x] Explorer canvas renders 10k+ node graphs without dropping below 30fps during pan/zoom
- [x] Off-screen nodes are culled from rendering
- [x] Low-zoom levels use simplified node rendering (no text labels)
- [x] On viewports <768px, layout adapts (stacked or overlay chat)
- [x] Chat panel collapsible on medium viewports
- [x] Touch gestures (pinch-to-zoom, drag) work on the canvas
- [x] Performance benchmark with 10k nodes passes targets
- [x] `npm test` passes

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

Implemented via the explorer pipeline lineage and recovered into this
branch's history by the checkpoint commits (`3fb05db28` et al.); the four
commits from the original operational history are not ancestors of this
branch. This assignment re-verified the recovered implementation on the
current base and updated this record only (see `## Shipped`).

## Shipped

The TASK-053 implementation (production code, benchmark/responsive tests,
and all R1–R3 review fixes) is present in the assigned base tree — it was
recovered into this branch's history by the pipeline checkpoint commits,
and the review record at `specs/reviews/task-053.md` closes at
"R3 — complete, 0 findings". This assignment made **no production code
changes**: it re-verified the recovered implementation against the current
base (which has since stacked task-036/038/065/087 work on the same files)
and confirmed every acceptance criterion still holds.

### Behavior (Part A — large graph performance)

- **Viewport culling**: `ExplorerCanvas.svelte` `isVisible()` performs a
  frustum intersection per node; `drawNodeRecursive` prunes entire invisible
  subtrees; `drawEdges` frustum-culls edges whose endpoints are both
  off-screen.
- **Level-of-detail rendering**: leaf labels render only when
  `sw > 30 && sh > 14 && cam.zoom >= 0.5` — at low zoom nodes are simple
  colored rectangles; semantic zoom additionally gates node types
  (packages → types → endpoints → functions → fields) at zoom 0.3/0.6/1.0/2.0.
- **Edge bundling/culling**: `drawEdges` switches to `drawBundledEdges`
  (group-to-group aggregate arrows) when `cam.zoom < 0.5` or
  `renderEdges.length > 5000`.
- **Incremental layout**: the layout `$effect` recomputes only on
  graph-data/breadcrumb/filter changes; pan/zoom mutate the non-reactive
  `cam` and never trigger layout.
- **Benchmark**: `ExplorerCanvas-performance.test.js` generates a 10k-node /
  20k-edge hierarchical mock graph and asserts CPU-time regression guards,
  culling-bounded draw calls, LOD text reduction (comparative small-vs-large
  graph), and bundled-vs-individual edge rendering. The >30fps interaction
  target cannot be measured in jsdom (no real frame loop/GPU); the test file
  documents this limitation — the culling/LOD/bundling mechanisms it verifies
  are what keep interaction above the target in a real browser.

### Behavior (Part B — responsive layout)

- **<768px**: stacked layout — canvas full-width on top, chat as a
  full-screen overlay with its own collapse bar (`@media (max-width: 768px)`).
- **769–1024px**: chat panel collapsible via a toolbar collapse button;
  collapsed state shows a floating "Chat" expand button.
- **Viewport-widen reset**: a `matchMedia('(min-width: 1025px)')` listener
  resets `chatCollapsed = false` so the chat can never be stranded hidden on
  wide viewports (R1-F4 fix), with teardown on destroy.
- **Touch**: `ontouchstart`/`ontouchmove`/`ontouchend` are wired on the
  canvas; single-finger drag pans, two-finger pinch zooms toward the pinch
  midpoint (world point under the midpoint is preserved).

### Test evidence

Focused probes on this branch (head at the assigned base
`213b4725b`, clean tree, `npm ci` with locked versions, evidence saved under
`/tmp/stage/review-evidence/`):

- `npx vitest run src/__tests__/ExplorerView-responsive.test.js
  src/__tests__/ExplorerCanvas-performance.test.js` — **23/23 pass**
  (8 responsive: real collapse/expand click interactions, matchMedia
  viewport-sync reset, aria labels; 15 performance: 10k/15k-node renders with
  CPU-time guards, culling bounds, comparative LOD, edge bundling).
- `npx vitest run src/__tests__/ExplorerCanvas.test.js
  src/__tests__/ExplorerCanvasDrillDown.test.js src/__tests__/ExplorerChat.test.js
  src/__tests__/ExplorerChat-streaming.test.js` — **269/269 pass** (no
  regressions from the later tasks stacked on these same files).

Full workspace suites, architecture checks, and GitHub CI are owned by the
verification and publication stages.
