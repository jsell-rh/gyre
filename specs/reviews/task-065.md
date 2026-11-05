# Review — task-065 (ExplorerCanvas — Unified Canvas with Semantic Zoom Treemap)

Spec: `explorer-canvas.md` §1–3, §8; `explorer-implementation.md` §1–2, §16, §17, §25.
Candidate: `9a24d46f1d8fa1baee9196d5da342e3d951953be` (base `d81b0fe4`, merge head `9502ab9f` + process commit).

## Round 1 (rebase-repair round review) — approved, no findings

Independent evidence (sandbox, /tmp/stage/review-evidence/):

- **`cd web && npm test`**: 62 files, **1602 passed | 41 skipped, 0 failed** (exit 0, run twice). Matches the shipped claim exactly.
- **explorer-visual.spec.js: 15/15 passed** against the committed baselines, run in-sandbox with a freshly built dist (`vite build`, 45.95s) served by a static bootstrap server on :2222 answering `/api/v1/admin/seed` + `/api/v1/workspaces` (the seeded fixture's contract), route interception + committed woff2 forced-font fixtures, chromium `--no-zygote` (sandbox EPERM on zygote forks — rendering unaffected). After the fresh build, `git status --porcelain` was **empty**: the committed dist is byte-identical to a fresh build of the merged sources.
- **Mutation probe (F1 class)**: temporarily reverted `nodeFilterOpacity`'s 'dependencies' case to unconditional dimming → **2 tests fail** (canvas-filters.test.js unit mapping + ExplorerCanvas.test.js component draw-path globalAlpha ratios), 166 others still pass. The dimming tests kill the real regression; restored, tree clean.
- **Merge integrity**: for all 24 files upstream changed between merge-base `2f215b1e` and base `d81b0fe4`, the candidate preserves them byte-identically except the six intentional ones (regenerated dist, ExplorerCanvas.svelte unified-cutover changes, ExplorerCanvas.test.js additions, re-pinned view-query-container baseline). No Rust/CI/upstream-specs changes vs base; nothing silently dropped.

Acceptance criteria verified:

- Single Explorer surface: MoldableView (1095 ln), FlowCanvas, FlowRenderer, ExplorerFilterPanel, duplicate NodeBadge components deleted; ExplorerView hosts one ExplorerCanvas (`ExplorerView.svelte:1395`), no tab structure; no live references to the deleted components remain.
- Props per §17: `repoId`, `nodes`, `edges`, `activeQuery`, `filter` ($bindable), `lens` ($bindable) (`ExplorerCanvas.svelte:20-26`).
- Lens toggle: Structural (default), Evaluative (functional), Observable `aria-disabled` + grayed + "requires production telemetry integration" + ObservableBanner on click (`:5039-5041`); pinned by unit tests and toolbar visual baselines.
- Filter presets: All/Endpoints/Types/Calls/Dependencies toolbar (`FILTER_PRESETS`), node dimming (`nodeFilterOpacity`) + edge gating (`edgePassesFilter`) in canvas-filters.js; pinned by unit tests, mutation-killing draw-path tests, and 5 visual baselines.
- Semantic zoom treemap: path-tree hierarchy, LOD per zoom band, dblclick Contains-edge drill + breadcrumb; zoom-level 0/1/2 visual baselines pass.
- View query rendering: scope resolution (focus/concept/test_gaps/diff/all + `$clicked`/`$selected`), emphasis (highlight, dim, tiered_colors, heat, badges), groups as labeled clusters, callouts, narrative numbered steps, annotation title/description with `$name`/`{{var}}` substitution; component tests + 2 visual baselines.
- Interactions: mousedown pan, wheel zoom, dblclick drill, click select, right-click context menu, minimap click+drag (`:4623-4666`).
- Minimap: `drawMinimap` (`:3711`) + minimap canvas (`:5203`); unit test + visible in full-page baseline.

Non-blocking hygiene note: the committed root-level sandbox probe configs (`web/gyre-playwright-visual.config.js`, `gyre-pw-chrome-launch.cjs`, `gyre-dom-probe.*`) reference /tmp paths that only exist in pipeline sandboxes; they are inert for CI (`playwright.config.js` testDir is `./tests/e2e`) and document the reproducible in-sandbox visual method. Carried from prior approved rounds; no action required.

**Verdict: approved** — all nine acceptance criteria hold under independent probes; evidence in `/tmp/stage/review-evidence/` (review-log.txt, npm-test-summary.txt, visual-spec.txt, mutation-f1.txt, merge-integrity.txt).
