# Review — task-065 (ExplorerCanvas — Unified Canvas with Semantic Zoom Treemap)

**Candidate:** 781d69b4e09b2f4b3a8241e3540df28030259337 (base 2115ae23)
**Round:** merge-repair (bcfe713b merged main 2115ae23 / task-064 §10
interaction context into the published candidate 968d4005; 8c840af4 +
781d69b4 are specs/tasks-only process commits)
**Verdict:** approved — no open findings.
**Evidence:** /tmp/stage/review-evidence/task-065-review.txt

## Merge resolution verified live

The single ExplorerCanvas.svelte conflict (import list) was resolved as the
union of both sides, and both sides are functional in the merged tree:

- ours: canvas-filters.js (`collectEdgeParticipants` :1588-1589,
  `nodeFilterOpacity` :1598, `edgePassesFilter` :1607) — the unit-tested F1
  filter dimming;
- main's: `ZoomInteractionTracker` (:18, :260) with `trackZoomChange()` in
  the rAF loop (:3831) and the full arm/disarm camera-control wiring (arm
  :3999/:4623/:4630/:4743; disarm :680/:693/:1488/:4268/:4309/:4319/:4494/
  :4794/:4818).

`git diff 2115ae23 781d69b4 -- crates/` is empty and canvas-state.js is
unchanged from main — the Rust side and main's new module carried through
unmodified; the candidate's own changes are web-only, so no semantic-merge
risk from this side.

`npm run build` on the merged sources reproduces the committed web/dist
byte-identically (0 dirty files; bundle hashes match the merge commit's
claim), so dist is genuinely regenerated output, not a hand-resolved hybrid.

## Independent probes (sandbox, at candidate head)

- `cd web && npm ci && npm test` → **63 files, 1623 passed | 41 skipped,
  0 failed** — matches the shipped claim.
- explorer-visual.spec.js → **15/15 passed** via the repo's sandbox visual
  config (headless chromium, static bootstrap server on :2222 serving
  web/dist, spec route interception for all graph data). Semantic zoom
  levels 0/1/2, view-query groups/callouts/narrative/annotation, filter
  presets, blast radius, toolbar lens toggle, full page all compare clean.
- **Mutation probe**: prepending `return 1.0;` in
  canvas-filters.js `nodeFilterOpacity` (disables node dimming) fails
  **9 tests** across canvas-filters.test.js and ExplorerCanvas.test.js —
  the F1 filter tests are mutation-proof, not self-confirming. Mutation
  restored; tree verified clean afterwards.
- scripts/check-task-commit-attribution.sh → OK.

## Acceptance criteria

All nine criteria verified in code and/or tests: single Explorer tab
(MoldableView/FlowCanvas/FlowRenderer/ExplorerFilterPanel/duplicate NodeBadge
deleted, zero live references; legacy graph/flow ViewSpec layout names both
resolve to the unified ExplorerCanvas); three-lens toolbar with Structural
default, functional Evaluative, and Observable `aria-disabled` with the
"requires production telemetry integration" label; five filter presets
pinned by mutation-proof unit/component tests; semantic zoom treemap with
Contains-edge drill + breadcrumb (visual baselines); view-query rendering
(scope/emphasis/groups/callouts/narrative/annotation — component tests +
visual baselines); pan/zoom/click/drag interactions; minimap with viewport
navigation; §17 prop interface (`repoId`, `nodes`, `edges`, `activeQuery`,
`$bindable` filter/lens); `npm test` passes.

## Non-blocking notes

- The visual "filter presets" tests exercise the view-query editor path
  (emphasis `dim_unmatched`), not the toolbar preset buttons; toolbar
  dimming is pinned by the unit/component tests instead. Coverage is real,
  just split across surfaces.
- layoutRegistry.js `labelKey` entries reference i18n keys removed with
  MoldableView (`moldable_view.tab_*`). Inert metadata — no renderer reads
  labelKey (only registerLayout's grammar side effect is consumed, by
  view-spec validation). Cosmetic dead reference; candidates for a later
  sweep, not a defect against this task's contract.
