# Review: task-065 — ExplorerCanvas — Unified Canvas with Semantic Zoom Treemap

Spec: `explorer-canvas.md` §1–3, §8; `explorer-implementation.md` §1–2, §16, §17, §25.
Candidate: `1bee3537` (branch `pipeline/task-065/097ec125528841008e916b2573b85b9d-1`), assigned base
`a884329d`. Repair category: **requirements_changed** (reconciliation round).

## Round: requirements-changed reconciliation (approved)

The controller re-ran task-065 because the contract advanced on main after the lineage was
published: task-065 landed on main (feat `0e889626f`), progress flipped to `complete`, and the
coverage matrix advanced around it (explorer-canvas.md row 5 → implemented via task-067;
explorer-implementation.md §16 gained the call_graph member under task-076).

**Candidate delta vs base:** `git diff a884329d..1bee3537` touches exactly one file —
`specs/tasks/task-065.md` (progress → ready-for-review, shipped notes re-recorded for the
reconciliation round, commits list re-scoped). `git diff … -- web crates` is **empty (0 lines)**:
the candidate's product content is tree-identical to the base, i.e. the published,
independently approved task-065 implementation plus main's subsequent advances (task-067
two-phase drill-down, synthetic tree-group drilling, breadcrumb rootLabel; task-076 LSP call
graph backend). The two new branch commits (`090f8ef3`, `1bee3537`) touch only the task record —
no product surface, so no attribution obligation; `scripts/check-task-commit-attribution.sh`
passes at the head.

The frontmatter's commits list drops checkpoint commit `3fb05db2` ("checkpoint: recover
interrupted pipeline source" — not a task-labeled product-surface commit) and matches the
controller's assignment scoping exactly (all 8 listed commits exist in history).

### Independent verification (sandbox, at candidate head)

- **Props (§17):** `ExplorerCanvas.svelte:20–47` — `repoId`, `nodes`, `edges`, `activeQuery`,
  `filter` ($bindable), `lens` ($bindable), plus documented extras (canvasState, callbacks,
  trace/timeline data). Matches the spec interface.
- **Single Explorer tab (§1):** `ExplorerView.svelte:39` — tabs removed per spec; one
  `ExplorerCanvas` mount at :1406. No live MoldableView with split Graph/Flow tabs;
  `layoutRegistry.js` keeps a `'flow'` ViewSpec entry as a compatibility shim resolving to the
  same unified `ExplorerCanvas` (documented at :53–57), not a second tab surface.
- **Three lenses (§2–3):** toolbar at `ExplorerCanvas.svelte:5236–5241` — Structural/Evaluative
  active buttons (aria-pressed), Observable `aria-disabled`, 0.35 opacity, "requires production
  telemetry integration" label, ObservableBanner on click (:5685). Structural is the default
  (`lens = $bindable('structural')`).
- **Filter presets (§25):** `FILTER_PRESETS` :1584–1590 (All/Endpoints/Types/Calls/Dependencies);
  dimming delegated to `nodeFilterOpacity`/`edgePassesFilter` in `canvas-filters.js`.
  **Mutation-proven:** disabling the endpoints dimming in `canvas-filters.js` makes
  `canvas-filters.test.js` fail (1 failed); restored, 18/18 pass; tree verified clean after.
- **Semantic zoom treemap:** `buildPathTree` (:1052), squarified treemap layout (:1157+), LOD
  bands per zoom, dblclick drill (:4399, wired :5313) with two-phase eased transition
  (`drillTransitioning` :242, :3826–3853) and `resolveTreeGroupScopeNode` (:4294+) for synthetic
  tree-group cells.
- **View query rendering:** emphasis — `dim_unmatched` (:2126), `heat` (:2135),
  `tiered_colors` (:2188), `highlight` (:2193); groups as labeled clusters (:2403); callouts
  (:2091, drawn :2585); narrative numbered steps (:2693); annotation `$name`/`{{var}}`
  substitution.
- **Interactions:** canvas element :5304–5320 wires mousedown/mousemove/mouseup/wheel/click/
  dblclick/contextmenu/keyboard/touch handlers; minimap click+drag navigation.
- **Minimap:** `drawMinimap` (:3758) renders full graph + viewport rect; `minimapToWorld`
  (:4798), `onMinimapMouseDown` (:4835), minimap canvas element (:5403).
- **Test suite:** `cd web && npm ci` (exit 0) then `npm test` → **64 files, 1626 passed |
  41 skipped, 0 failed** — matches the shipped claim exactly.
- **Mechanical gates:** `scripts/check-task-commit-attribution.sh` OK;
  `scripts/check-sandbox-sweep-artifacts.sh` OK.
- Evidence: `/tmp/stage/review-evidence/task-065-review.txt` (plus full npm-test log
  `task-065-review-npmtest.txt`).

### Deferred to remote/exact-head CI (not code defects)

- `explorer-visual.spec.js` (15 baselines): requires the release gyre-server + web/dist rebuild.
  The candidate's `web/` tree is byte-identical to base `a884329d` (0-line diff), whose CI
  verified the baselines at the same sources; re-running in-sandbox has no discriminative power
  for a candidate whose only delta is the task record.
- Rust workspace gates and `app.spec.js`: zero `crates/` delta vs base; owned by exact-head CI.

### Findings

None. The reconciliation round makes no product-code change; its record accurately describes the
state of the tree, and the tree at the candidate head independently satisfies every acceptance
criterion of the task contract (verified in source and by the passing, mutation-sensitive test
suite).

Verdict: **approved** — no blocking findings.
