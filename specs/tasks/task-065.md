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
commits: ["3ad887b9849093fdcfc9e3708b1b407c43e03f8c", "b5fd081f626e0f47b06d6b520c9b2049fb54047a", "d4bbe5769a504cb177aa8bf5f7aae0dd0b4ed22f", "db9f944bd9631a742f93a0512e87f4eb450931dd", "240aab15ab9b5b67a013a204be74b2864dc68000", "3fb05db28561b78c30f12bbc9e8450fe79a9f89d", "6110fe43dbbf19aee4ef9dbf5cab751d89007e90", "58dddc244728efcb163cf07c3f0109a6937d93fc", "d82a5a560e2d6e9b1420da503951b75a97763a16"]
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

### Rebase-repair round (2026-11-05, upstream d81b0fe4 — current head)

Merged upstream task-092 (HSI Conflict Prevention) + task-063 (View
Query Grammar) lineage into the branch (merge commit 9502ab9f).
Conflict resolutions:

1. **web/dist** (rename/rename on `elk.bundled-*.js`, both-modified
   `index.html`): resolved by rebuilding dist from the merged sources
   (vite build) — the same method as the prior task-180 merge round;
   dist is generated output and the conflicted index.html referenced
   divergent bundle hashes from both sides.

2. **`view-query-container-linux.png`** (binary, both-modified): neither
   side's pin matches the merged tree — ours (3ad887b9) has the
   forced-font environment but not task-063's canvas changes; theirs
   (32291080) has task-063's rendering but was pinned in a different
   font environment. Re-pinned to the merged tree's actual rendering
   under the committed forced-font environment: the merged scope-`all`
   client-side resolution (upstream task-063; `VIEW_QUERY_WITH_ANNOTATIONS`
   uses `scope: {type: 'all'}` + `highlight.matched.label` + `zoom: 'fit'`)
   deterministically changes the canvas region — pixelmatch 2865 px
   (1.05%, under the 2% threshold but real and settled; verified
   byte-identical at +1s/+3s/+6s captures, so not animation).
   `view-query-annotated-linux.png` re-pinned for the same reason (same
   fixture, same change); both re-pins verified pixel-exact against the
   merged rendering at a 0.0001 threshold. The other 13 baselines are
   byte-identical under the merged tree.

### Test evidence (rebase-repair round, sandbox, at merge head 9502ab9f)

- `cd web && npm ci` → locked versions; `npm run build` → dist rebuilt
  cleanly from merged sources (index-BTVoRmpO.js / elk.bundled-B1BOOF3L.js).
- explorer-visual.spec.js: **15 passed** (clean compare against the
  re-pinned baselines), via a static bootstrap server on :2222 + the
  spec's route interception; chromium launched with `--no-zygote`
  (this sandbox forbids chromium zygote forks — rendering unaffected).
- `cd web && npm test`: **62 files, 1602 passed | 41 skipped, 0 failed**
  (grew from 1581: the merge adopts upstream task-063/task-092 suites).
- `scripts/check-task-commit-attribution.sh` and
  `scripts/check-sandbox-sweep-artifacts.sh`: OK at the merge head.
- Evidence: /tmp/stage/review-evidence/task-065-rebase-repair.txt.

### Verification notes for the reviewer

- app.spec.js requires the real release gyre-server — owned by exact-head
  GitHub CI (e2e.yml), not runnable in this sandbox. The Rust-side merge
  content (gyre-server lib.rs/ws.rs presence features) is pure upstream
  code that auto-merged without conflict and was verified on main.
- Glyphs outside the forced fonts' latin subsets (canvas arrow/check
  badges, 🔒) still resolve through system fallback; they matched across
  environments in every baseline generation to date and sit well under the
  2% budget.
- Prior rounds' durable findings remain addressed in the lineage: font-truth
  visual repair (b5fd081f), font-route fix (3ad887b9), whitespace-gate
  exemption for generated dist (d14fa446), F1 filter-dimming repair with
  mutation-proof evidence (6110fe43), task-180 merge resolution (2f215b1e),
  rebase repair of upstream e608ac78 (3c22aeaf).

Progress: ready-for-review at head 9502ab9f.
