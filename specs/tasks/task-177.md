---
title: "Rendering Technology — SVG canvas, layout engines, interaction events"
spec_ref: "ui-layout.md §10"
depends_on: [task-170]
progress: complete
coverage_sections:
  - "ui-layout.md §10. Rendering Technology"
  - "ui-layout.md §Canvas Rendering"
  - "ui-layout.md §Layout Engines"
  - "ui-layout.md §Interaction Events"
commits: ["01f680c70a25ead24ba39a5529624529ffe662e2", "79744b712975e1d1ba5eae7ab1ee4c6800bd1985", "4aec0fd467b2b76a6f7e7d38d5131eb9171c004f", "41e31138e5f8b42d3e55d43866297551c81afd3a", "e1a8686b47ea112f3639e6dabe015dd0a813f060", "825b4145e0f856d30b577bfe76d4247f1fc151dc", "5ae26dec4f1cc68769ec6d53ca0c85271e495d84"]
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

Rendering technology per ui-layout.md §10. Components and tests landed in
the earlier checkpoint commits (825b4145, e1a8686b, 41e31138, 4aec0fd4);
the rebase onto base fefde294 (task-089 generated-views lineage) landed as
c4620c63f; this round (01f680c7) repaired the three review findings from
the ff311d2f review round: the layout registry had no production consumer
after that rebase, its labelKeys pointed at the deleted `moldable_view`
i18n section, and the d3-force test was self-confirming against the
columnLayout fallback.

### Registry production wiring (commit 01f680c7)

- **View spec `layout` selects the renderer** (Implementation Plan item 4):
  `applyGeneratedViewSpec` (ExplorerView.svelte) resolves the generated
  ViewSpec's `layout` through `getLayout()` into `activeLayout`
  (unregistered names fall back to `graph`). The Boundary branch dispatches
  through the registry: `hierarchical`/`layered` render
  `SvgGraphCanvas` with the registry's engine selector (real ELK layered
  DOWN / RIGHT), `side-by-side` renders the CSS-Grid `SideBySideView`,
  `diff` renders `DiffView`, `list` renders the shell `VirtualNodeList`;
  `graph`/`flow`/`timeline` stay on the unified `ExplorerCanvas` (Canvas
  2D + TimelineScrubber, per the task-065 cutover). A chip labeled from
  the registry's `labelKey` (i18n) dismisses the generated layout back to
  the default canvas.
- **Scope projection for registry renderers** (`web/src/lib/view-query-scope.js`,
  pure): resolves the ViewQuery scope shapes a generated ViewSpec produces
  (`all`, `filter` node_types/name_pattern, computed `$governed_by('…')`,
  `concept` seed BFS with expand_edges/depth/direction) so the SVG/Grid/diff
  renderers receive the same node set the unified canvas would show;
  unsupported or unmatched scopes yield the full graph, mirroring the
  canvas's unmatched-scope behavior.
- **labelKeys**: moved from the deleted `moldable_view.*` section to new
  `explorer_view.layout_*` keys in `web/src/locales/en.json`; the layout
  chip renders them, and a registry test asserts every labelKey resolves
  in the locale (no raw-key UI).
- **d3-force test hardened**: the old assertion (`some(!'-26px')`) passed
  with the engine replaced by columnLayout. New fixture (4 same-type
  nodes, 2 disjoint edges) asserts all nodes resolve and land on 4 distinct
  x positions — the column fallback stacks same-type nodes in one column
  (identical x). Mutation-verified: mocking `computeLayout('graph')` to
  return `columnLayout` makes the exact new assertion fail.

### Behavior (unchanged from c4620c63f)

- **SVG canvas** (`web/src/lib/SvgGraphCanvas.svelte`): SVG rendering for `graph`/`hierarchical`/`layered` views with individually addressable node `<g>`s and edge `<line>`s, pan (background drag, window-level pointer tracking) and zoom (wheel around cursor + toolbar buttons + fit), CSS-transitioned hover/selection state, and keyboard access (tabindex 0 + Enter/Space activation on nodes and edges).
- **Density policy** (`web/src/lib/canvas-density.js`, pure): <=500 nodes render fully; >500 auto-filter to public-only with a banner + [Show All] (applied inside `ExplorerCanvas.svelte`); >1000 even after filtering switch to the virtualized list (`VirtualNodeList.svelte`, windowed rows via scroll offset + overscan) with a warning. The shell-owned list fallback nests inside the upstream `explorerViewMode === 'boundary'` branch (Boundary view = the canvas), so the other default views (spec-realization/change/system-trace) are unaffected by canvas density.
- **Layout engines** (`web/src/lib/layout-engines.js`): `graph` -> d3-force (sync 300-tick simulation), `hierarchical` -> ELK layered DOWN, `layered` -> ELK layered RIGHT (real elkjs `elk.bundled.js`, verified to produce genuine layered positions in plain JS — no mocks), `list` -> virtualized node list, `timeline` -> d3-scale time axis (in `TimelineScrubber.svelte`), `side-by-side` -> CSS Grid (`SideBySideView.svelte`), `diff` -> custom `DiffView.svelte`; all eight registered in `layoutRegistry.js` (`flow` -> unified `ExplorerCanvas` per the upstream task-065 cutover) and — since 01f680c7 — consumed by the Explorer shell's generated-view dispatch.
- **ViewEvents** (`web/src/lib/viewEvents.js`): every interactive element (SVG nodes, edges, list rows, insight cards) emits the standardized `ViewEvent` (type/entity_type/entity_id/position); the Explorer shell (`ExplorerView.svelte`) handles all four uniformly — click -> detail panel, dblclick -> drill into the entity's repo graph, hover -> positioned tooltip, context-menu -> actions menu (Escape closes). The uniform handler coexists with upstream's `active_query` canvas-state effect and the `view_spec`/generated-views flow (`openNodeDetail` is shared by the canvas callback, the ViewEvent dispatch, and the registry renderers' `onSelectNode`).

### Rebase resolution (commit c4620c63f, retained context)

The base deleted `MoldableView.svelte` (task-065/088/089 replaced the
multi-layout tab shell with the default-views architecture: the canvas IS
the Boundary view, flow renders through the canvas's evaluative lens, and
timeline scrubbing lives in the canvas's `TimelineScrubber`). The candidate
had built list/timeline rendering inside `MoldableView.svelte`. Resolution:

1. Followed the upstream deletion of `MoldableView.svelte` (nothing in the
   merged tree imports it; its tests were deleted upstream with it).
2. **Ported the d3-scale time axis** (ui-layout.md §10 `timeline` engine)
   into the live `TimelineScrubber.svelte`: `scaleTime()` domain over the
   delta timestamps, key-moment markers positioned by *timestamp* (not
   array index — spacing reflects real time), scale-generated tick axis
   with date labels under the scrubber, and an index-spacing fallback for
   degenerate (single-timestamp) domains. Two new focused tests pin the
   time-proportional positioning (33.3%/83.3% on an uneven 30-day fixture,
   where index spacing would give 25%/75%) and the degenerate fallback.
3. `layoutRegistry.js`: union — kept upstream's `flow` -> `ExplorerCanvas`
   cutover (no `FlowRenderer` import; file deleted upstream) plus the
   task-177 registrations (`hierarchical`/`layered` -> `SvgGraphCanvas` with
   engine selectors, `side-by-side`, `diff`); updated the stale
   "renders inline in MoldableView" comments to name the live renderers.
4. `ExplorerView.svelte`: union — kept upstream's default-views dispatch,
   lens URL handling, liveness wiring, and `active_query` effect, plus the
   task-177 ViewEvent shell (tooltip/menu/Escape cascade), insight-card
   ViewEvent emission, and the >1000-node virtualized-list fallback nested
   inside the boundary branch; `onNodeDetail` extracted (shared), upstream's
   `pushInteraction` on interactive queries preserved.
5. `ExplorerCanvas.svelte`: import union only — the task-177 density policy
   and ViewEvent emission blocks auto-merged cleanly into the upstream
   (liveness/causal-trace/lens) canvas.
6. `en.json`: took the base's version (the `moldable_view` i18n section was
   removed upstream together with the component); since 01f680c7 the
   registry's replacement `explorer_view.layout_*` keys live there.

### Test evidence

- Full web unit suite on the final tree: **98 files, 2000 passed, 41
  skipped (pre-existing skips), 0 failures** (`npx vitest run`, exit 0).
  New this round: `ExplorerViewLayoutRenderers.test.js` (6 tests: SVG
  dispatch for hierarchical, scope filtering of the rendered node set,
  list dispatch, canvas retained for graph/flow/timeline, chip dismissal,
  unregistered-layout fallback — driven through the production
  `onApplyView` -> `applyGeneratedViewSpec` path) and
  `view-query-scope.test.js` (11 tests: scope shapes, $governed_by,
  concept BFS, unsupported-scope null semantics, edge accessors), plus a
  labelKey-resolves-in-locale registry test and the hardened d3-force test.
- Mutation check: replacing `computeLayout('graph')` with `columnLayout`
  makes the new d3-force assertion fail (killed regression); engine probe
  output retained in
  `/tmp/stage/review-evidence/task177-force-vs-column-probe.txt`.
- One pre-existing fixture updated (ExplorerGeneratedViews.test.js
  AUTH_SPEC `layout: 'hierarchical'` -> `'graph'`): it asserted the *canvas*
  query annotation, which is only correct for canvas-rendered layouts now
  that `layout` selects the renderer — the hierarchical dispatch is covered
  by ExplorerViewLayoutRenderers.test.js.
- JS verifiers pass on the new tests (assertionless, conditional guards,
  mirrored logic, self-confirming, tautological, dead test variables,
  phantom APIs, comparative claims, stale mechanism claims,
  self-contradicting comments). check-dead-components reports only the 4
  pre-existing violations on files untouched here.
- Rust compile/clippy could not be run in this sandbox (crates.io
  unreachable — `cargo check --offline` fails on dependency resolution,
  same restriction as prior rounds) — recorded for remote verification.
  No Rust source was touched by the 01f680c7 repair.
- `web/dist` is left untouched per the assignment contract (final
  integration rebuilds and publishes those assets remotely).

No exemptions added, no verifiers weakened, no coverage-matrix statuses
changed (auditor-owned).
