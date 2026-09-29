---
title: "Rendering Technology — SVG canvas, layout engines, interaction events"
spec_ref: "ui-layout.md §10"
depends_on: [task-170]
progress: not-started
coverage_sections:
  - "ui-layout.md §10. Rendering Technology"
  - "ui-layout.md §Canvas Rendering"
  - "ui-layout.md §Layout Engines"
  - "ui-layout.md §Interaction Events"
commits: []
---

## Spec Excerpt

ui-layout.md §10 defines the rendering technology stack:

**Canvas Rendering**: SVG for graph/hierarchical/layered layouts, positioned by ELK or dagre. SVG chosen for DOM addressability, Svelte reactivity, CSS transitions. Node count typically <500 after C4 filtering. Auto-filter to public-only nodes at ~500; switch to list layout with virtual scrolling at ~1000.

**Layout Engines**:
- `graph`: d3-force (Svelte wrapper)
- `hierarchical`: ELK (elkjs, WASM)
- `layered`: ELK
- `list`: Native Svelte `{#each}` with virtual scroll
- `timeline`: d3-scale (time axis) + Svelte
- `side-by-side`: CSS Grid (50/50 or 60/40)
- `diff`: Custom Svelte component
- `flow`: Custom Svelte + Canvas 2D overlay for particles, WebGL fallback for >100 particles

**Interaction Events**: All interactive elements emit standardized `ViewEvent`:
```typescript
interface ViewEvent {
  type: 'click' | 'dblclick' | 'hover' | 'context-menu';
  entity_type: string;
  entity_id: string;
  position: { x: number, y: number };
}
```

Explorer shell handles all events uniformly: click → detail panel, dblclick → drill-down, hover → tooltip, context-menu → actions.

## Implementation Plan

1. **SVG canvas component** (`ExplorerCanvas.svelte`):
   - Verify/enhance SVG rendering with pan, zoom, click events
   - Auto-filter: >500 nodes → public-only with banner; >1000 → list fallback
   - CSS transitions for visual state changes

2. **Layout engine integration**:
   - Install/verify elkjs (WASM) for hierarchical and layered layouts
   - Install/verify d3-force for graph layout
   - d3-scale for timeline layout
   - Virtual scrolling for list layout
   - CSS Grid for side-by-side layout
   - Custom diff component for structural diffs

3. **ViewEvent interface**:
   - Define `ViewEvent` TypeScript interface
   - All graph nodes, edges, list rows, cards emit standardized events
   - Explorer shell dispatches: click→panel, dblclick→drill, hover→tooltip, context-menu→menu

4. **Layout registry integration**:
   - Connect to layout registry from task-170
   - Each layout engine registered as a renderer component
   - View spec `layout` field selects the renderer

5. **Tests**:
   - SVG rendering with mock graph data
   - Auto-filter behavior at node count thresholds
   - ViewEvent dispatching from different element types

## Acceptance Criteria

- [ ] SVG canvas renders graph with pan/zoom/click
- [ ] Auto-filter at >500 nodes (public-only + banner)
- [ ] List fallback with virtual scroll at >1000 nodes
- [ ] ELK integration for hierarchical/layered layouts
- [ ] d3-force for graph layout
- [ ] ViewEvent interface defined and emitted uniformly
- [ ] Explorer shell handles all four event types
- [ ] Layout engines registered in layout registry
- [ ] Tests pass

## Agent Instructions

Read `ui-layout.md` §10 for the full rendering technology stack. The existing `ExplorerView.svelte` and `ExplorerCanvas.svelte` components are the starting points. Check what layout engines are already installed in `web/package.json`. The `ViewEvent` interface is foundational — every interactive element across all layouts must emit it. The auto-filter thresholds (500/1000) prevent SVG performance degradation. For the flow layout's Canvas 2D particle rendering, that's handled in a separate task focused on Flow Traces.
