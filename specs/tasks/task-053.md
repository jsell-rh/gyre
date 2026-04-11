# TASK-053: Explorer Phase 4 — Large Graph Performance & Responsive Layout

**Spec reference:** `explorer-implementation.md` §Migration Plan > Phase 4: Polish  
**Depends on:** TASK-029 (lens toggle, complete), TASK-031 (drill-down, complete), TASK-036 (streaming, complete), TASK-038 (keyboard shortcuts, complete)  
**Progress:** `ready-for-review`
**Review:** [`specs/reviews/task-053.md`](../reviews/task-053.md)

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
