# Review: TASK-065 — ExplorerCanvas: Unified Canvas with Semantic Zoom Treemap

**Reviewer:** Independent review (pi runtime)
**Date:** 2026-11-05
**Comparison base:** 2f215b1e58d633e666852bb1fc42f842d89ae68c
**Candidate:** fac1d104f4417328b9455c4a86d05dfe60263f9e
**Verdict:** approved

---

## Scope of this round

The candidate is the previously approved task-065 lineage (unified Explorer
cutover, F1 filter-dimming repair, font-truth visual baselines) merged with
upstream task-180 (Executable Spec Assertions), plus the font-route CI repair
(3ad887b9) that resolves the durable CI finding from run 41905243500
(toolbar-default, 1858 px / 3.0% > 2%). Relative to the assigned base, the
non-web production surface is unchanged: only `docs/ui.md` and
`specs/tasks/task-065.md` differ outside `web/`.

## Findings

None. The merge resolution, the font-route fix, and the task contract surface
were all verified with independent evidence.

### Merge resolution (7fb0736a)

- `web/src/locales/en.json` resolved as a union: key-set comparison of both
  parents against the merged file shows the only dropped ours-only key is
  `meta_specs.toast.preview_unavailable`, which is a stale orphan — no code in
  the merged tree references it (ours' MetaSpecs preview usage was refactored
  away by upstream task-180; `MetaSpecs.svelte`/`EditorSplit.svelte` are
  byte-identical to the base, and ours had zero unique changes to those files
  vs the merge-base, so taking upstream wholesale is correct).
- All other dropped keys are intentional task-065 deletions
  (`moldable_view.*`, `explorer_filter.*`, `explorer_chat.*`,
  `explorer_treemap.empty_filtered`).
- Systematic scan: every `$t('...')` literal in `web/src` resolves to an
  existing en.json key — zero dangling i18n references.
- `web/dist` rebuilt from merged sources: no conflict markers; a fresh
  `npm run build` in this sandbox reproduced the committed dist byte-identically
  (clean `git status` after build).

### Font-route repair (3ad887b9)

- The one-line fix (serve woff2 fixtures via `new URL(name, FONTS_DIR)`
  instead of double-prefixing `fonts/`) is load-bearing: a mutation reverting
  it fails 14 of 15 explorer-visual tests, including the exact CI failure
  (`toolbar_renders_with_lens_toggle_and_stats`). The committed baselines
  genuinely encode the Red Hat font binaries, not per-machine fallbacks.
- Staged CI artifacts cross-checked: run 41905243500's "expected" png is
  byte-identical (md5) to the pre-repair baseline at 47f40c00, and the CI
  "actual" (fallback-font render) differs from both the old and the
  regenerated baseline — consistent with the root-cause narrative
  (mono-font advance mismatch between sandbox and CI fontconfig fallbacks).
- The residual CI-parity assumption (CI chromium rev 1208 rasterizes the
  committed woff2 identically to this sandbox's rev-1208 chrome-headless-shell)
  is sound in kind and is owned by exact-head GitHub CI / remote verification.

### Task contract (acceptance criteria)

- **Single Explorer tab**: `MoldableView.svelte`, `FlowCanvas.svelte`,
  `FlowRenderer.svelte`, `ExplorerFilterPanel.svelte`, and the duplicate
  `NodeBadge` are deleted; ExplorerView hosts one ExplorerCanvas with no tab
  switcher; `layoutRegistry` maps both `graph` and `flow` ViewSpec layouts to
  the same unified component.
- **Lens toggle**: three toolbar buttons — Structural (default, aria-pressed),
  Evaluative (functional), Observable (`aria-disabled`, grayed, "requires
  production telemetry integration" label, banner on click).
- **Filter presets**: `FILTER_PRESETS` (all/endpoints/types/calls/dependencies)
  drive `nodeFilterOpacity` node dimming + `edgePassesFilter` edge gating in
  the unit-tested `canvas-filters.js` consumed by the component.
- **Props**: `repoId`, `nodes`, `edges`, `activeQuery`, `filter`
  ($bindable), `lens` ($bindable) — matches explorer-implementation.md §17.
- **Minimap**: `drawMinimap` + viewport rect + `minimapToWorld` click/drag
  navigation; present in the full-page visual baseline.
- **View query rendering / semantic zoom / interactions**: covered by the
  visual suite (zoom levels 0–2, view-query groups/callouts/narrative/
  annotation, blast radius) and DOM probes.

### Probes run (evidence: /tmp/stage/review-evidence/probes.log)

Static bootstrap server (node http on :2222) + sandbox chromium rev 1208,
dist rebuilt from candidate sources:

1. Focused vitest (canvas-filters, ExplorerCanvas, ExplorerCanvas-performance):
   **176 passed**.
2. `explorer-visual.spec.js` via `gyre-playwright-visual.config.js`:
   **15 passed** — including the CI-failing toolbar-default.
3. Mutation (font-route fix reverted): **14 failed / 1 passed** — fix proven
   load-bearing; mutation restored, tree clean afterward.
4. `gyre-dom-probe.spec.js`: **7 passed** (lens toggle, five filter presets,
   view-query annotation bar, blast radius, localized strings, chat status).
5. Full frontend suite `npm test`: **62 files, 1581 passed | 41 skipped,
   0 failed** — matches the executor's claim.
6. en.json merge-union key-set analysis: no referenced key lost (script in
   evidence log).

### Notes for verification (not defects)

- app.spec.js (49 tests) requires the real release gyre-server and is owned by
  exact-head GitHub CI (e2e.yml); not runnable in this sandbox (python TCP
  listener probes unsupported; node http servers and chromium reach
  localhost:2222 fine — both verified live here).
- `layoutRegistry.js` retains `moldable_view.tab_*` labelKey strings whose
  en.json keys were deleted; they are data never resolved at runtime (the
  view-switcher no longer exists) and date from the previously approved
  lineage. Cosmetic only.
