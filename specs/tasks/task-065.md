---
title: "ExplorerCanvas — Unified Canvas with Semantic Zoom Treemap"
spec_ref: "explorer-canvas.md §1–3, §8; explorer-implementation.md §1–2, §17, §25"
depends_on:
  - task-062
progress: ready-for-review
coverage_sections:
  - "explorer-canvas.md §1 Problem"
  - "explorer-canvas.md §2 Design"
  - "explorer-canvas.md §3 One Canvas, Three Lenses"
  - "explorer-canvas.md §8 Component Architecture"
  - "explorer-implementation.md §1 Overview"
  - "explorer-implementation.md §2 Architecture"
  - "explorer-implementation.md §16 Frontend Components"
  - "explorer-implementation.md §17 ExplorerCanvas (Svelte)"
  - "explorer-implementation.md §25 Phase 1: Canvas + Filters"
commits: ["58dddc244728efcb163cf07c3f0109a6937d93fc", "d82a5a560e2d6e9b1420da503951b75a97763a16", "6110fe43dbbf19aee4ef9dbf5cab751d89007e90"]
---

## Spec Excerpt

**explorer-canvas.md §1 Problem:** The current implementation has two separate tabs (Graph, Flow) that are the same canvas rendered twice with different overlays. The spec says these should be one view with lens toggles — the topology is constant, the overlay changes.

**explorer-canvas.md §2–3 Design:** The Explorer canvas is a single interactive surface. The knowledge graph topology (nodes + edges) is always visible. A lens toggle in the toolbar controls what data is overlaid. Three lenses: Structural (default), Evaluative, Observable (future, grayed out).

**explorer-canvas.md §8 Component Architecture:**
```
MoldableView
  ├── Explorer tab (merged Graph + Flow)
  │     ├── LensToggle (Structural / Evaluative / Observable)
  │     ├── ExplorerCanvas (SVG graph layer — always rendered)
  │     ├── EvaluativeOverlay (Canvas 2D particles — rendered when lens=evaluative)
  │     │     └── PlaybackControls (play/pause, scrubber, speed)
  │     ├── NodeBadge (SVG overlay — metrics per node)
  │     └── Breadcrumb (drill-down path)
  ├── List tab (unchanged)
  └── Timeline tab (unchanged)
```

**explorer-implementation.md §17 ExplorerCanvas (Svelte) Props:**
```typescript
{
  repoId: string;
  nodes: GraphNode[];
  edges: GraphEdge[];
  activeQuery: ViewQuery | null;
  filter: 'all' | 'endpoints' | 'types' | 'calls' | 'dependencies';
  lens: 'structural' | 'evaluative' | 'observable';
}
```

Responsibilities: Semantic zoom treemap rendering (canvas 2D), pan/zoom/click/drag interaction, view query resolution and rendering, filter preset application, lens overlay, minimap.

**explorer-implementation.md §25 Phase 1: Canvas + Filters:**
- Build ExplorerCanvas from the prototype (`explore3.html`)
- Semantic zoom treemap with path tree hierarchy
- Filter presets (All, Endpoints, Types, Calls, Dependencies)
- Three lenses (structural, evaluative, observable)
- View query renderer (scope, emphasis, groups, callouts, narrative)
- Replace MoldableView's Graph + Flow tabs with single component

## Implementation Plan

### Existing Code

- `web/src/lib/ExplorerCanvas.svelte` (6049 lines) — already implements semantic zoom treemap with lens toggle, filters, evaluative overlay, breadcrumb, etc.
- `web/src/components/ExplorerView.svelte` (3729 lines) — container component
- `web/src/components/ExplorerFilterPanel.svelte` — filter panel
- Various supporting components: `PlaybackControls`, `TimelineScrubber`, `NodeBadge`, `Breadcrumb`, `EvaluativeOverlay`, `ObservableBanner`

### Work Required

1. **Audit ExplorerCanvas.svelte** against the spec's prop interface. Verify it accepts: `repoId`, `nodes`, `edges`, `activeQuery`, `filter`, `lens`. Check that all filter presets work: `all`, `endpoints`, `types`, `calls`, `dependencies`.

2. **Verify semantic zoom treemap**: The canvas should render nodes as a treemap grouped by package/module hierarchy. Double-click zooms into children. Pan/zoom via mouse/trackpad.

3. **Verify lens toggle**: Three buttons in toolbar — Structural (active by default), Evaluative (available), Observable (grayed out with "Requires production telemetry integration" label).

4. **Verify view query rendering**: When `activeQuery` is set, the canvas should:
   - Resolve scope → result set of nodes
   - Apply emphasis (highlight, dim, tiered_colors, heat, badges)
   - Render groups as labeled clusters
   - Render callouts as annotated nodes
   - Render narrative as numbered sequence
   - Show annotation title/description

5. **Verify no separate Graph/Flow tabs**: MoldableView should have a single Explorer tab, not separate Graph and Flow tabs.

6. **Minimap**: Verify the canvas has a minimap showing the full graph with viewport indicator.

## Acceptance Criteria

- [ ] Single Explorer tab replaces separate Graph + Flow tabs
- [ ] Lens toggle shows three options: Structural (default), Evaluative, Observable (disabled)
- [ ] Filter presets work: All, Endpoints, Types, Calls, Dependencies
- [ ] Semantic zoom treemap renders nodes grouped by hierarchy
- [ ] View query rendering applies scope, emphasis, groups, callouts, narrative, annotation
- [ ] Pan/zoom/click/drag interactions work
- [ ] Minimap is present
- [ ] Props match spec: `repoId`, `nodes`, `edges`, `activeQuery`, `filter`, `lens`
- [ ] `cd web && npm test` passes

## Agent Instructions

Read `specs/system/explorer-canvas.md` §1–3, §8 and `specs/system/explorer-implementation.md` §1–2, §17, §25. Then read the existing implementation:
- `web/src/lib/ExplorerCanvas.svelte` — the main canvas component
- `web/src/components/ExplorerView.svelte` — the container
- `web/src/components/ExplorerFilterPanel.svelte` — filter panel

The existing code is 6000+ lines and likely implements most of this spec. Your job is to **audit and complete**, not rewrite. Check each acceptance criterion against the code. Fix gaps. Run `cd web && npm test` to verify.


Key question: Does the current ExplorerView still have separate Graph/Flow tabs, or has it already been unified? If separate tabs remain, merge them into a single Explorer view.

## Shipped

The unified-canvas cutover itself landed in the earlier branch commits
(d82a5a56 / the 3566b64a lineage): MoldableView and its separate Graph/Flow
tabs, FlowCanvas/FlowRenderer, ExplorerFilterPanel, and the duplicate
NodeBadge were deleted (−3852 lines); the live surface is ExplorerView's
Architecture tab hosting the single ExplorerCanvas with lens toggle
(Structural active, Evaluative, Observable disabled w/ "coming soon"),
five filter presets, semantic-zoom treemap, view-query rendering
(scope/emphasis/groups/callouts/narrative/annotation), drill-down with
breadcrumb, and minimap.

This assignment repaired the remaining review finding **F1**: node-side
filter dimming was unpinned. `filterOpacity` in ExplorerCanvas.svelte had
no test coverage — the prior mutation proof showed reverting the
`dependencies` case to unconditional `0.1` dimming passed the whole suite
(Dependencies preset would render a fully dimmed canvas).

Shipped in 6110fe43:

- **Extraction** (repo's established task-062 F3/F4 pattern):
  `web/src/lib/canvas-filters.js` gains `collectEdgeParticipants(edges,
  type)` and `nodeFilterOpacity(filter, ln, participants)`;
  ExplorerCanvas's `filterOpacity`/`edgeParticipants` delegate to them —
  behavior-identical refactor verified by draw-path probing before/after.
- **Unit tests** (`canvas-filters.test.js`): per-preset lit/dimmed split
  for all five presets (`all`/`endpoints`/`types`/`calls`/`dependencies`),
  including the depends_on-only participant that distinguishes
  `dependencies` from `calls`, participant-set alias/casing semantics.
- **Component-level killing tests** (`ExplorerCanvas.test.js`): render the
  component per filter and capture `ctx.globalAlpha` at each leaf-label
  `fillText`; participant/non-participant alpha ratios pin the composed
  draw opacity exactly (zoom fade cancels in the ratio). Fixture uses
  multi-segment qualified names (`app.svc.*`) so graph nodes land as
  visible leaf cells, with endpoint/type nodes as lit references for
  those presets.
- **Mutation proof** (all killed at both levels; evidence retained at
  /tmp/stage/review-evidence/task-065-f1-mutation-proof.txt):
  1. revert `dependencies` to unconditional `0.1` → 2 tests fail
  2. drop the depends_on participant term → 2 tests fail
  3. swap dim constant `0.1`→`0.5` → 2 tests fail

Test evidence: `cd web && npm test` → **56 files passed, 1513 passed |
41 skipped, 0 failed**. Focused files: `canvas-filters.test.js` +
`ExplorerCanvas.test.js` → 157 passed.

Sandbox note (host verification only): mid-session host load spike
(loadavg 46 on 8 shared CPUs) caused vitest fork-worker startup
timeouts (>60s worker start); runs at loadavg <15 complete in ~2.5s.
No code defect — recorded in the evidence file.
