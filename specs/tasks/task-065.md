---
title: "ExplorerCanvas — Unified Canvas with Semantic Zoom Treemap"
spec_ref: "explorer-canvas.md §1–3, §8; explorer-implementation.md §1–2, §17, §25"
depends_on:
  - task-062
progress: needs-revision
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
commits: ["3566b64a8060ce07d0b987ef3ded683a7ab092d5", "4689d4cc8f36adb5775689dee5fd5d13711135f6", "90159da1475e997efebd6b083382af1d73e68613", "52c93ac3d0abdef4aa680d2d7f005b077ed97482"]
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

- [x] Single Explorer tab replaces separate Graph + Flow tabs
- [x] Lens toggle shows three options: Structural (default), Evaluative, Observable (disabled)
- [x] Filter presets work: All, Endpoints, Types, Calls, Dependencies
- [x] Semantic zoom treemap renders nodes grouped by hierarchy
- [x] View query rendering applies scope, emphasis, groups, callouts, narrative, annotation
- [x] Pan/zoom/click/drag interactions work
- [x] Minimap is present
- [x] Props match spec: `repoId`, `nodes`, `edges`, `activeQuery`, `filter`, `lens`
- [x] `cd web && npm test` passes

## Agent Instructions

Read `specs/system/explorer-canvas.md` §1–3, §8 and `specs/system/explorer-implementation.md` §1–2, §17, §25. Then read the existing implementation:
- `web/src/lib/ExplorerCanvas.svelte` — the main canvas component
- `web/src/components/ExplorerView.svelte` — the container
- `web/src/components/ExplorerFilterPanel.svelte` — filter panel

The existing code is 6000+ lines and likely implements most of this spec. Your job is to **audit and complete**, not rewrite. Check each acceptance criterion against the code. Fix gaps. Run `cd web && npm test` to verify.

Key question: Does the current ExplorerView still have separate Graph/Flow tabs, or has it already been unified? If separate tabs remain, merge them into a single Explorer view.


## Implementation Record (2026-10-09)

**Audit result:** the unified canvas was already substantially complete on this
branch (prior rounds deleted MoldableView/FlowCanvas/FlowRenderer/
ExplorerFilterPanel and their tests; ExplorerView is the single Architecture
tab surface). This round completed the cutover and hardened verification:

- **Props** (explorer-implementation.md §17): ExplorerCanvas.svelte:20-45 accepts
  exactly `repoId`, `nodes`, `edges`, `activeQuery`, `filter` ($bindable),
  `lens` ($bindable).
- **Lens toggle** (§3): toolbar `lens-group` — Structural active by default,
  Evaluative functional, Observable `aria-disabled` with "requires production
  telemetry integration" label + ObservableBanner on click.
- **Filter presets** (§25): all five (`all`/`endpoints`/`types`/`calls`/
  `dependencies`) via FILTER_PRESETS → filterOpacity (node level) +
  canvas-filters.js edgePassesFilter (edge level).
- **Semantic zoom treemap**: path-tree hierarchy with tree-group containment,
  5-level LOD rules (packages/modules at overview → fields/constants at full
  zoom), double-click drill-down via Contains edges with breadcrumb update
  and eased zoom transition (drillInto, onDblClick).
- **View query rendering**: scope resolution (focus/filter/concept/test_gaps/
  diff/all + $clicked/$selected interactive), emphasis (highlight/dim/
  tiered_colors/heat/badges), groups as labeled dashed clusters (bounding-box
  + label), callouts as node labels, narrative as numbered step markers,
  annotation title/description with $name substitution.
- **Interactions**: mousedown pan, wheel zoom, dblclick drill, click select,
  context menu, minimap click+drag navigation.
- **Minimap**: drawMinimap renders full graph + blue viewport rect; click and
  drag navigation (minimapToWorld, onMinimapMouseDown).
- **NodeBadge cleanup**: badge behavior (span count / error rate / mean
  duration, spec §Evaluative) is drawn on the canvas 2d layer
  (ExplorerCanvas.svelte:3211-3290). The two orphaned Svelte components
  (`components/NodeBadge.svelte` — dead import; `lib/NodeBadge.svelte` —
  left over from deleted FlowRenderer) were removed as dead code from the
  Flow cutover. docs/ui.md component list updated.
- **Perf guard flake fixed**: "renders 10k+ nodes within 100ms" failed in the
  full suite (16.2s wall > 15s budget) while passing isolated (~2.7s). Root
  cause was wall-clock measurement under 8-worker vitest parallelism, not a
  rendering regression. The guard now measures `process.cpuUsage()` (CPU
  time), which is load-insensitive — same 15s threshold, deterministic under
  parallel load. 15k guard given the same treatment.

**Verification:** `cd web && npm test` — 55 files, 1509 passed, 51 skipped,
0 failed (after fix; before, 1 load-flaky failure in
ExplorerCanvas-performance.test.js).

**Known non-blocking gaps (outside task-065 scope):**
- ExplorerChat.svelte references 16 `explorer_chat.*` i18n keys missing from
  en.json (e.g. `status_connecting`, `no_saved_views`). Pre-existing at
  baseline 2bf5eb2 — belongs to task-071 (ExplorerChat), not this task.
## Review Round 1 (2026-10-09)

**Verdict: needs-revision** — one material gap (unpinned filter behavior); all other acceptance criteria verified against code.

### Verified (no action needed)

- **Props (§17)**: ExplorerCanvas.svelte accepts exactly repoId/nodes/edges/activeQuery/filter($bindable)/lens($bindable); dead `filters` prop removed. Both callers (ExplorerView bind:filter/bind:lens; WorkspaceHome defaulted) verified.
- **Lens toggle (§3)**: three buttons, Structural default, Evaluative functional, Observable aria-disabled + grayed CSS (.tb-btn-observable opacity 0.35) + click shows ObservableBanner naming GYRE_OTLP_ENDPOINT/OpenTelemetry. Spec text "requires production telemetry integration" present.
- **Filter preset UI**: five buttons (All/Endpoints/Types/Calls/Dependencies) in .filter-preset-group, aria-pressed switching pinned by ExplorerCanvas.test.js:98-115, 808-831.
- **Cutover completeness**: zero live references to MoldableView/FlowCanvas/FlowRenderer/ExplorerFilterPanel/NodeBadge anywhere in web/src (verified 2026-10-09, post-deletion). MoldableView was test-only at review base 66422bd (no imports outside web/src/__tests__), so deletion is dead-code removal, not functionality loss; its List/Timeline functionality survives in ExplorerView (timeline scrubber + delta stats + ghost overlays, graph_too_large list fallback) and RepoMode tabs. 63 removed i18n keys all dead (moldable_view.*, explorer_filter.*, explorer_treemap.empty_filtered — zero $t references remain); zero keys added, zero values changed.
- **Badge behavior (§Evaluative)**: drawn on canvas 2d layer (ExplorerCanvas.svelte:3210-3293: emphasis.badges with server node_metrics, evaluative span_duration/span_count/error_rate, TEST-node badge); Svelte NodeBadge components were dead (one an unimported file, one left from deleted FlowRenderer). docs/ui.md updated consistently.
- **Attribution gate**: scripts/check-task-commit-attribution.sh OK; all 4 frontmatter commits exist on the branch and the two product-surface commits (3566b64, plus wip commits 52c93ac/90159da/4689d4c containing the deletions) are recorded.
- **Perf guards**: CPU-time measurement (process.cpuUsage) with same 15s/20s thresholds is a legitimate de-flake, not gate weakening — the assertion semantics (catch ~10x algorithmic regression) are preserved; per-test 30s timeouts added. Full-suite flake claim independently confirmed: solo full suite at HEAD passed (exit 0, 55 files, 1509 passed — /tmp/stage/review-evidence/task065-perf-full-suite.log, Run C) while concurrent-load runs hit vitest 30s test timeouts, not the cpuMs assertion. Evidence revision e9edc5a differs from HEAD only in scripts/docs, not web/src, so the pass applies to HEAD.

### Finding F1 (material) — Dependencies preset node-dimming is unpinned; the shipped fix has no killing test

ExplorerCanvas.svelte:1580 fixed the at-base stub `case 'dependencies': return 0.1` (dim every node) to highlight depends_on/calls edge participants. But no test renders the canvas with `filter` set to any preset, so nothing pins node-side filterOpacity at all.

**Mutation proof** (/tmp/stage/review-evidence/task065-mutation-run.log): reverting line 1580 to the at-base stub in an isolated worktree — full suite passes: 55 files, 1509 tests, exit 0. The exact regression class that task-062 F4 caught on the edge side (canvas-filters.test.js) is unguarded on the node side, and this repo has already silently lost this case once.

**Repair path** (implementation role): add a focused test that renders ExplorerCanvas with `filter: 'dependencies'` on a fixture with both depends_on-participant and non-participant nodes and asserts the dimming is selective — e.g. capture ctx.globalAlpha values passed to drawLeafNode draws (mock ctx already exposes globalAlpha; assign via a setter or record values in the existing mock) and assert participants draw at 1.0 while non-participants draw at 0.1. Cover at least `dependencies` (the fixed stub) and ideally `endpoints`/`calls` node cases the same way. The test must fail on the reverted stub.
