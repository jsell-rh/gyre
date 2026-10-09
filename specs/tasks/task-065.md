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
commits: ["6110fe43dbbf19aee4ef9dbf5cab751d89007e90", "58dddc244728efcb163cf07c3f0109a6937d93fc", "d82a5a560e2d6e9b1420da503951b75a97763a16"]
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

**Contract-repair round (2026-10-09).** The prior candidate's task-file edit
strayed into the assigned contract region (a stray blank line inside the
Agent Instructions and a reordered `commits:` list), which the harness
flagged as a contract violation (9b96b72f). This round restored the task
contract byte-for-byte to the assigned baseline and re-verified the shipped
work without touching the contract region. No code changes were needed —
the lineage already carries the full implementation; this round is
verification plus lifecycle bookkeeping.

Shipped behavior (from the branch lineage):

- **Unified cutover** (d82a5a56 lineage): MoldableView with its separate
  Graph/Flow tabs, FlowCanvas, FlowRenderer, ExplorerFilterPanel, and the
  duplicate NodeBadge components deleted (−3852 lines). The live surface is
  ExplorerView's Architecture tab hosting a single ExplorerCanvas.
- **Props per §17**: `repoId`, `nodes`, `edges`, `activeQuery`,
  `filter` ($bindable), `lens` ($bindable) — verified at
  ExplorerCanvas.svelte:20-45.
- **Three lenses**: Structural (default/active), Evaluative (functional —
  OTLP trace particles, heat, badges, PlaybackControls), Observable
  (`aria-disabled`, grayed at 0.35 opacity, "requires production telemetry
  integration" label, ObservableBanner on click).
- **Five filter presets**: All/Endpoints/Types/Calls/Dependencies via
  FILTER_PRESETS, node dimming (nodeFilterOpacity) + edge gating
  (edgePassesFilter) in canvas-filters.js.
- **Semantic zoom treemap**: path-tree hierarchy, LOD rules per zoom band,
  double-click drill-down via Contains edges with breadcrumb update and
  eased camera transition (drillInto/onDblClick/navigateBreadcrumb).
- **View query rendering**: scope resolution (focus/filter/concept/
  test_gaps/diff/all + $clicked/$selected), emphasis (highlight, dim,
  tiered_colors, heat, badges), groups as labeled dashed clusters,
  callouts as annotated nodes, narrative as numbered steps, annotation
  title/description with $name and {{var}} substitution.
- **Interactions**: mousedown pan, wheel zoom, dblclick drill, click
  select, right-click context menu, minimap click+drag navigation.
- **Minimap**: drawMinimap renders full graph + viewport rect;
  minimapToWorld + onMinimapMouseDown/drag navigate.
- **F1 repair** (6110fe43, re-verified this round): node-side filter
  dimming extracted to canvas-filters.js (`collectEdgeParticipants`,
  `nodeFilterOpacity`) and pinned by 18 unit tests + 6 component-level
  draw-path tests that capture `ctx.globalAlpha` at leaf-label `fillText`
  and assert participant/non-participant ratios.

Test evidence (2026-10-09, this round, sandbox):

- `cd web && npm test` → **56 files passed, 1513 passed | 41 skipped,
  0 failed**.
- Focused: `canvas-filters.test.js` + `ExplorerCanvas.test.js` →
  157 passed.
- **Mutation re-proof** (evidence:
  /tmp/stage/review-evidence/task-065-repair-mutation-proof.txt): all
  three prior mutations re-applied against the pristine tree and killed —
  (1) `dependencies` reverted to unconditional 0.1 → 2 tests fail;
  (2) depends_on participant term dropped → 2 tests fail; (3) dim constant
  0.1→0.5 → 2 tests fail. Sources restored pristine after each mutation
  (verified by `git diff`).

Pre-existing, not this task's: check-task-commit-attribution.sh fails on
task-210 commit a781ede2, which is an ancestor of the assignment base
8c2d1775 (verified via merge-base; identical failure at base checkout).
Sandbox note: vitest fork workers are load-sensitive on 8 shared CPUs;
this round's runs completed at normal load.

---

**Checkpoint-recovery round (2026-10-09).** Recovered an interrupted
assignment (durable finding 301df7d1: "implementation must finish and
obtain fresh review"). The implementation was already complete in the
lineage (working tree clean at candidate 808a7825); this round re-ran the
full verification with a fresh node_modules install and regenerated the
evidence for review. No code changes — this round is verification and
evidence.

Fresh evidence (this round, sandbox):

- `npm ci` (locked versions) → 169 packages, 8s.
- `cd web && npm test` → **56 files passed, 1513 passed | 41 skipped,
  0 failed** (90s).
- Focused `canvas-filters.test.js` + `ExplorerCanvas.test.js` → **157
  passed**.
- Acceptance-criteria audit re-verified at head 808a7825 with file:line
  evidence for all eight criteria (unified cutover, three lenses, five
  presets, treemap drill, view-query rendering, interactions, minimap,
  §17 props).
- Mutation re-proof regenerated (evidence:
  /tmp/stage/review-evidence/task-065-checkpoint-recovery-evidence.txt):
  all three mutations (dependencies→unconditional dim; depends_on
  participant dropped; dim 0.1→0.5) re-applied against the pristine tree
  and killed; sources restored pristine after each (`git status` clean).
  The prior round's evidence file names a different path
  (task-065-repair-mutation-proof.txt) — this sandbox is fresh and only
  the new evidence file exists.
- Sandbox transport restriction recorded in the evidence file: TCP
  listener probe unsupported (errno 95) — live dev-server browser check
  deferred to host verification (exact-head GitHub CI plus manual UI
  check at localhost:3000); jsdom-mounted component tests with recorded
  canvas draw calls serve as the in-sandbox behavioral substitute.

Progress remains `ready-for-review` awaiting fresh review on this
candidate.
