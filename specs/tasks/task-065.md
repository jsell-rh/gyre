---
title: "ExplorerCanvas — Unified Canvas with Semantic Zoom Treemap"
spec_ref: "explorer-canvas.md §1–3, §8; explorer-implementation.md §1–2, §17, §25"
depends_on: [task-062, task-212]
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
commits: ["3ad887b9849093fdcfc9e3708b1b407c43e03f8c", "b5fd081f626e0f47b06d6b520c9b2049fb54047a", "d4bbe5769a504cb177aa8bf5f7aae0dd0b4ed22f", "db9f944bd9631a742f93a0512e87f4eb450931dd", "240aab15ab9b5b67a013a204be74b2864dc68000", "3fb05db28561b78c30f12bbc9e8450fe79a9f89d", "6110fe43dbbf19aee4ef9dbf5cab751d89007e90", "58dddc244728efcb163cf07c3f0109a6937d93fc", "d82a5a560e2d6e9b1420da503951b75a97763a16", "bcfe713b9ecdafc64e5938090b8d2e413bcfb272"]
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
  without Contains children (adopted from upstream task-173).
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

### Merge-repair round (main 2115ae23 — current head bcfe713b)

Merged current main (2115ae23 — task-064 View Query Grammar §10
Interaction Context + fix/verifier-script-overlay) into the published
candidate 968d4005 (merge commit bcfe713b). Conflict resolutions:

1. **ExplorerCanvas.svelte** (single conflict block, the import list):
   resolved as the union of both sides — ours' canvas-filters.js imports
   (`collectEdgeParticipants`, `nodeFilterOpacity`: the unit-tested F1
   filter dimming) plus main's `ZoomInteractionTracker` from the new
   canvas-state.js. Main's §10 zoom-interaction tracking (arm/disarm
   camera-control wiring across drill/breadcrumb/query-fit/auto-fit,
   `trackZoomChange()` in the animation loop) is adopted wholesale
   alongside our filter implementation. Main's unused `NodeBadge` import
   stays dropped with our deleted duplicate component (verified zero
   `<NodeBadge` usages in main's version).

2. **web/dist** (rename/rename on `elk.bundled-*.js`, both-modified
   `index.html`): resolved by rebuilding dist from the merged sources
   (vite build) — the same method as every prior merge round; dist is
   generated output and the conflicted index.html referenced divergent
   bundle hashes. New bundles: index-DtVmBKEH.js /
   elk.bundled-BeoNa-iK.js; CSS hash unchanged (index-GeLbLKhg.css).

Retained from our side (auto-merge verified in the merged tree):
`$bindable` filter/lens props, FILTER_PRESETS toolbar,
nodeFilterOpacity/edgePassesFilter delegation to canvas-filters.js,
structural-lens ghost-animation gating. Adopted from main alongside the
zoom tracker: canvas-state.js wire format (`toWireCanvasState` in
ExplorerChat), ExplorerView §10 interaction records (structured
`{action, detail}` entries, `active_query` in canvas state, `onLoadView`
interaction, zoom_level/visible_tree_groups owned by reactive sync
effects instead of stale syncCanvasState snapshots).

### Test evidence (merge-repair round, sandbox, at merge head bcfe713b)

- `cd web && npm ci` → locked versions; `npm run build` → dist rebuilt
  cleanly from merged sources.
- `cd web && npm test`: **63 files, 1623 passed | 41 skipped, 0 failed**
  (grew from 62/1602: the merge adopts main's canvas-state.test.js).
- explorer-visual.spec.js: **15 passed** via the sandbox Playwright
  config (chromium `--no-zygote`, static bootstrap server on :2222 + the
  spec's route interception) — every baseline compares clean against the
  merged tree, no re-pinning needed (main's task-064 canvas changes are
  non-visual: interaction tracking only).
- `scripts/check-task-commit-attribution.sh` and
  `scripts/check-sandbox-sweep-artifacts.sh`: OK at the merge head.
- Evidence: /tmp/stage/review-evidence/task-065-merge-repair.txt.

### Verification notes for the reviewer

- Rust-side merge content (gyre-common view_query.rs, gyre-server
  explorer_views/saved_views/explorer_ws, gyre-domain
  view_query_composition.rs) is pure upstream code from main; our
  candidate made zero crates/ changes relative to the merge base
  (701cb1ad), so the textual auto-merge carries no semantic-merge risk
  from our side. Exact-head gates remain owned by GitHub CI.
- app.spec.js requires the real release gyre-server — owned by exact-head
  GitHub CI (e2e.yml), not runnable in this sandbox.
- Glyphs outside the forced fonts' latin subsets (canvas arrow/check
  badges, 🔒) still resolve through system fallback; they matched across
  environments in every baseline generation to date and sit well under
  the 2% budget.
- Prior rounds' durable findings remain addressed in the lineage:
  font-truth visual repair (b5fd081f), font-route fix (3ad887b9),
  whitespace-gate exemption for generated dist (d14fa446), F1
  filter-dimming repair with mutation-proof evidence (6110fe43),
  task-180 merge resolution (2f215b1e), task-063/task-092 rebase repair
  with re-pinned view-query baselines (9502ab9f lineage).

Progress: ready-for-review at head bcfe713b.
