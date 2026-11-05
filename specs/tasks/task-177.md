---
title: "Rendering Technology — SVG canvas, layout engines, interaction events"
spec_ref: "ui-layout.md §10"
depends_on: [task-170]
progress: ready-for-review
coverage_sections:
  - "ui-layout.md §10. Rendering Technology"
  - "ui-layout.md §Canvas Rendering"
  - "ui-layout.md §Layout Engines"
  - "ui-layout.md §Interaction Events"
commits: ["41e31138e5f8b42d3e55d43866297551c81afd3a", "e1a8686b47ea112f3639e6dabe015dd0a813f060", "825b4145e0f856d30b577bfe76d4247f1fc151dc"]
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

## Shipped

Rendering technology per ui-layout.md §10, building on the 825b4145 checkpoint (components + tests) and finishing it: the checkpoint's `SvgGraphCanvas.test.js` failed 8/8 at assignment end; this assignment diagnosed and fixed the remaining defects so the full suite is green.

### Behavior

- **SVG canvas** (`web/src/lib/SvgGraphCanvas.svelte`): SVG rendering for `graph`/`hierarchical`/`layered` views with individually addressable node `<g>`s and edge `<line>`s, pan (background drag, window-level pointer tracking) and zoom (wheel around cursor + toolbar buttons + fit), CSS-transitioned hover/selection state, and keyboard access (tabindex 0 + Enter/Space activation on nodes and edges).
- **Density policy** (`web/src/lib/canvas-density.js`, pure): ≤500 nodes render fully; >500 auto-filter to public-only with a banner + [Show All]; >1000 even after filtering switch to the virtualized list (`VirtualNodeList.svelte`, windowed rows via scroll offset + overscan) with a warning.
- **Layout engines** (`web/src/lib/layout-engines.js`): `graph` → d3-force (sync 300-tick simulation), `hierarchical` → ELK layered DOWN, `layered` → ELK layered RIGHT (real elkjs `elk.bundled.js`, verified to produce genuine layered positions in plain JS — no mocks), `list` → Svelte `{#each}` + virtual scroll, `timeline` → d3-scale time axis (`MoldableView.svelte`), `side-by-side` → CSS Grid, `diff` → custom `DiffView.svelte`, all eight registered in `layoutRegistry.js` (view spec `layout` selects the renderer).
- **ViewEvents** (`web/src/lib/viewEvents.js`): every interactive element (SVG nodes, edges, list rows) emits the standardized `ViewEvent` (type/entity_type/entity_id/position); the Explorer shell (`ExplorerView.svelte`) handles all four uniformly — click → detail panel, dblclick → drill into the entity's repo graph, hover → positioned tooltip, context-menu → actions menu (Escape closes).

### Defects fixed in this assignment (checkpoint had left these failing)

1. `SvgGraphCanvas.svelte` `$effect` state loop — `++layoutVersion` read+wrote `$state` inside the same effect; Svelte 5 threw at the write site (the checkpoint's 8/8 failure). Fixed with a plain non-reactive invalidation token; reproduced via a minimal component before fixing.
2. List-fallback CPU waste — in `list` density mode the layout effect still ran a 300-tick d3-force simulation over 1200 nodes whose positions were never rendered (cause of cleanup-hook timeouts under load). Layout is now skipped when `density.mode === 'list'`.
3. `ExplorerViewViewEvents.test.js` `vi.mock` hoisting bug (`apiMocks` referenced before initialization) — fixed with `vi.hoisted()`; `afterEach(vi.restoreAllMocks)` replaced with `vi.clearAllMocks()` (restore would strip the `mockResolvedValue` implementations plain `vi.fn()` mocks rely on between tests).
4. Test-fixture bugs: duplicate node ids in the density fixture (`each_key_duplicate` crash) and a banner assertion not whitespace-normalized across template line breaks.
5. Strengthened the ELK test to assert real ELK layered output (3 distinct y layers for the c→a→b chain) instead of just node count; mutation-verified — forcing the column fallback makes it fail (`expected 1 to be 3`). Heavyweight density tests (450–1200 nodes) got explicit 60s test timeouts for slow environments; assertions unchanged.

### Test evidence

Logs and probe outputs in `/tmp/stage/review-evidence/` (README.md itemizes commands and exit codes):

- Task-177 suite (7 files): 43/43 pass, two consecutive runs under full batch load.
- Regression: 16 existing suites covering components touched by the checkpoint (ExplorerView/ExplorerCanvas/MoldableView/…) — 359 passed, 41 pre-existing skips.
- Real ELK probe: `node elk-probe.mjs` → genuine layered positions (`a 32 12`, `b 12 72`, `c 152 72`), exit 0.
- ELK mutation proof: forcing the column fallback fails the strengthened test, exit 1.

No exemptions added, no verifiers weakened, no coverage-matrix statuses changed (auditor-owned).
