# Review — task-065 (ExplorerCanvas — Unified Canvas with Semantic Zoom Treemap)

Spec: `explorer-canvas.md` §1–3, §8; `explorer-implementation.md` §1–2, §16, §17, §25.
Candidate: `418745abbc581c45fc1ddfcbd7c582e3eb4315b0` (base `8c2d1775`).
Commits under review: `3fb05db2`, `6110fe43`, `58dddc24`, `d82a5a56` (all verified ancestors of the candidate).
Verdict: **approved**.

## Round 1 — independent review (2026-10-09)

Evidence: `/tmp/stage/review-evidence/task-065-independent-review.txt`.
Working tree clean at candidate throughout; all temporary mutations/probes
restored and verified (`git status` clean after each).

### Test runs

- `cd web && npm ci` → 169 packages (locked versions).
- `cd web && npm test` → **56 files passed, 1513 passed | 41 skipped, 0 failed**.
- Focused `canvas-filters.test.js` + `ExplorerCanvas.test.js` → **157 passed**.

### Mutation probes (each reverted after; tree verified clean)

| Mutation | Result |
|---|---|
| `dependencies` case → unconditional `0.1` | 2 tests fail — killed |
| `depends_on` participant term dropped | 2 tests fail — killed |
| dim constant `0.1`→`0.5` | 8 tests fail — killed |
| Observable banner onclick removed | 1 test fails — killed |
| `setFilter` no-op | 1 test fails — killed |
| groups rendering disabled | passes — not pinned |
| narrative rendering disabled | passes — not pinned |
| minimap viewport rect removed | passes — not pinned |

The first three replicate the shipped note's mutation re-proof and were
independently confirmed. The last three survive because no test pins those draw
paths — but all three behaviors are byte-identical pre-existing code at the
assigned base (verified by grepping base's ExplorerCanvas.svelte), and the task
explicitly excludes "new tests for already-working behavior." This review ran a
positive draw-path probe instead (temporary test, removed after): groups issue
`setLineDash([6,4])` rounded-rect strokes + label fillText; narrative issues
numbered-step arcs + digit text; callouts draw annotation text; minimap strokes
the `#60a5fa` viewport rect. All 4 probe tests passed — the behaviors are real,
not hollow.

### Contract-region integrity

Diff of `specs/tasks/task-065.md` base→candidate contains only `progress:` and
`commits:` frontmatter updates plus the appended `## Shipped` section; no
deletions inside the assigned contract. All four listed commits are ancestors.

### Acceptance criteria — all verified with source evidence

1. **Unified cutover** — MoldableView, FlowCanvas, FlowRenderer,
   ExplorerFilterPanel, both NodeBadge copies deleted (−3852 lines); zero live
   references remain; ExplorerView hosts the single ExplorerCanvas
   (ExplorerView.svelte:1316); RepoMode has one 'architecture' tab.
2. **Three lenses** — Structural (default), Evaluative (real OTLP span→particle
   construction `:823-880`, `drawTraceParticles :3352`, PlaybackControls under
   `lens === 'evaluative'`), Observable (`aria-disabled="true"`, 0.35 opacity,
   "requires production telemetry integration" label, ObservableBanner on
   click — mutation-killed).
3. **Five filter presets** — FILTER_PRESETS toolbar + `setFilter` (mutation-
   killed); node dimming via `nodeFilterOpacity` and edge gating via
   `edgePassesFilter` in canvas-filters.js are the real draw path
   (filterOpacity `:1565`, filterEdge `:1574`); mutations 1–3 killed.
4. **Semantic zoom treemap** — path-tree hierarchy, `drillInto :4148` on
   dblclick, breadcrumb update, eased camera transition (`lerpCam` drill branch).
5. **View query rendering** — scope resolution (`:1893+`, incl. `$clicked`/
   `$selected`), emphasis (dim/heat/tiered_colors/highlight/badges), groups as
   labeled dashed clusters (`:2315-2360`), callouts, narrative numbered steps
   (`:2606-2629`), annotation `$name`/`{{var}}` substitution (`:4898-4928`) —
   confirmed by the positive draw-path probe.
6. **Interactions** — pan/zoom/dblclick/click/right-click/minimap-drag all
   present as real handlers.
7. **Minimap** — `drawMinimap :3657` + `minimapToWorld` inversion; viewport
   rect probe passed.
8. **§17 props** — `repoId`, `nodes`, `edges`, `activeQuery`, `filter`
   ($bindable), `lens` ($bindable) at ExplorerCanvas.svelte:20-27; ExplorerView
   binds both (`:1321-1322`). §8's NodeBadge: both component copies were
   dead/test-only at base (zero `<NodeBadge>` markup usage anywhere); badge
   drawing is real canvas code (`:3196-3270`), so deletion is a clean cutover.
9. **`cd web && npm test` passes** — verified.

### Noted, pre-existing at base (not findings)

- `scripts/check-task-commit-attribution.sh` fails on task-210 commit
  `a781ede2`, an ancestor of the assigned base (verified via merge-base) —
  identical failure at the base checkout; not introduced by this candidate.
- Svelte a11y warnings in ExplorerCanvas.svelte — unchanged from base.

### Sandbox restriction

TCP listener probes unsupported (errno 95 per capabilities.json), so no live
dev-server browser check; jsdom component tests with recorded canvas draw calls
(including this review's probe) are the behavioral evidence. Host verification
should run `cd web && npm test` at the candidate (expected: 56 files / 1513
passed / 0 failed).
