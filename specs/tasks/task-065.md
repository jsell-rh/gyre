---
title: "ExplorerCanvas — Unified Canvas with Semantic Zoom Treemap"
spec_ref: "explorer-canvas.md §1–3, §8; explorer-implementation.md §1–2, §17, §25"
depends_on: [task-062, task-212]
progress: complete
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
commits: ["b5fd081f626e0f47b06d6b520c9b2049fb54047a", "d4bbe5769a504cb177aa8bf5f7aae0dd0b4ed22f", "db9f944bd9631a742f93a0512e87f4eb450931dd", "240aab15ab9b5b67a013a204be74b2864dc68000", "3fb05db28561b78c30f12bbc9e8450fe79a9f89d", "6110fe43dbbf19aee4ef9dbf5cab751d89007e90", "58dddc244728efcb163cf07c3f0109a6937d93fc", "d82a5a560e2d6e9b1420da503951b75a97763a16"]
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

### Shipped behavior (task-065 lineage, carried at the current head)

- **Unified cutover**: MoldableView with its separate Graph/Flow tabs,
  FlowCanvas, FlowRenderer, ExplorerFilterPanel, and the duplicate NodeBadge
  components deleted. The live surface is ExplorerView's Architecture tab
  hosting a single ExplorerCanvas.
- **Props per explorer-implementation.md §17**: `repoId`, `nodes`, `edges`,
  `activeQuery`, `filter` ($bindable), `lens` ($bindable).
- **Three lenses**: Structural (default/active), Evaluative (functional —
  OTLP trace particles, heat, badges, PlaybackControls), Observable
  (`aria-disabled`, grayed at 0.35 opacity, "requires production telemetry
  integration" label, ObservableBanner on click).
- **Five filter presets**: All/Endpoints/Types/Calls/Dependencies via
  FILTER_PRESETS, node dimming (`nodeFilterOpacity`) + edge gating
  (`edgePassesFilter`) in canvas-filters.js, pinned by unit tests and
  component-level draw-path tests (task-065 F1 repair).
- **Semantic zoom treemap**: path-tree hierarchy, LOD rules per zoom band,
  double-click drill-down via Contains edges with breadcrumb update and
  eased camera transition; workspace-scope `onScopeDrill` on repo_id leaves
  without Contains children (adopted from upstream task-173 in the rebase
  repair below).
- **View query rendering**: scope resolution (focus/filter/concept/
  test_gaps/diff/all + $clicked/$selected), emphasis (highlight, dim,
  tiered_colors, heat, badges), groups as labeled dashed clusters, callouts
  as annotated nodes, narrative as numbered steps, annotation title/
  description with $name and {{var}} substitution.
- **Interactions**: mousedown pan, wheel zoom, dblclick drill, click select,
  right-click context menu, minimap click+drag navigation.
- **Minimap**: drawMinimap renders full graph + viewport rect;
  minimapToWorld + onMinimapMouseDown/drag navigate.
- **Deterministic visual baselines**: committed woff2 font fixtures +
  forced-font stylesheet so sandbox and CI rasterize identical glyph
  outlines; 15 explorer-visual.spec.js baselines.

### Rebase-repair round (2026-10-10, finding acdb29ed) — current head

Resolved the merge of upstream base e608ac78 (task-173/task-207 lineage)
into candidate 8af68f80 (merge commit 3c22aeaf, merge-base db37fd02):

- **ExplorerCanvas.test.js**: both sides appended different describe blocks
  at EOF — kept both (ours: task-065 F1 filter-dimming draw-path tests;
  theirs: upstream task-173 scope drill-down tests). Upstream's
  ghost-overlay rAF-suspension hunk auto-merged.
- **ExplorerCanvas-performance.test.js**: took the candidate side for all
  three conflict blocks — the CPU-time timing guard (upstream's side still
  referenced the pre-refactor `elapsed` variable, undefined in the merged
  context) and the single text-width-cache test at the candidate's
  position; upstream's `, 30000` timeout additions outside the conflicts
  are preserved.
- **MoldableView.svelte** (deleted by us / modified by them): resolved
  deleted — this branch's reviewed cutover removed it and nothing in app
  code imports it even on upstream. Upstream's MoldableViewListView.test.js
  (renders the deleted component) removed with it; the registry-driven tab
  refactor of that dead component does not apply to this branch's surface.
- **layoutRegistry.js** (upstream-new): repaired its import of the deleted
  FlowRenderer — the 'flow' ViewSpec layout now maps to ExplorerCanvas (the
  unified canvas renders flow/trace data via its evaluative lens). The
  registry mechanism and view-spec.test.js extensibility coverage are
  preserved unchanged.
- **web/dist** (rename/rename elk bundle + index.html): rebuilt from the
  merged sources (vite build); the d14fa446 `.gitattributes`
  `web/dist/** -whitespace` covers regenerated bundles for the whitespace
  gate.

### Test evidence (rebase-repair round, sandbox, at merge head 3c22aeaf)

- `cd web && npm ci` → locked versions; `cd web && npm test` → **62 files,
  1564 passed | 41 skipped, 0 failed** (suite grew from 56/1513 pre-merge:
  the merge adopts upstream's new suites and both sides' ExplorerCanvas
  test blocks are kept).
- explorer-visual.spec.js against the rebuilt dist (static bootstrap server
  on :2222, forced-font fixtures, committed baselines): **15 passed** —
  the merged Explorer surface is pixel-identical to the baselines;
  upstream's ExplorerView/ExplorerCanvas changes shift no pinned pixels.
- Controller gates at the merge head: `git diff --check HEAD^1 HEAD` exit 0;
  check-task-commit-attribution.sh OK; all 18 fast static tree-scan gates
  (arch, abac route registry, mcp write tools, migrations, byte-slice,
  relative-path, fail-open refs, mem port contracts, fabricated scopes,
  lossy secrets, scope literals, inert enforcement, forwarded headers,
  in-memory stores, unbounded http, exempt handlers, sql portability,
  dead message kinds) pass.
- Evidence: /tmp/stage/review-evidence/task-065-rebase-repair.txt.

### Verification notes for the reviewer

- app.spec.js (49 tests) requires the real release gyre-server — owned by
  exact-head GitHub CI (e2e.yml), not runnable in this sandbox (python TCP
  listener probe errno 95; node http servers bind fine on localhost:2222).
- Prior rounds' durable findings remain addressed in the lineage: font-truth
  visual repair (b5fd081f), whitespace-gate exemption for generated dist
  (d14fa446), F1 filter-dimming repair with mutation-proof evidence
  (6110fe43).

Progress: ready-for-review at merge head 3c22aeaf.
