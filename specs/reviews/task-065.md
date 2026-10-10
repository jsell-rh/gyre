# Review: task-065 — ExplorerCanvas: Unified Canvas with Semantic Zoom Treemap

Spec: `explorer-canvas.md` §1–3, §8; `explorer-implementation.md` §1–2, §16, §17, §25.
Candidate: `3690d7a3` (assigned base `a1751da1`). Task commits:
`3fb05db2`, `6110fe43`, `58dddc24`, `d82a5a56`.

## Round 1 (this review — prerequisite-adoption candidate)

Contract and baseline:

- Task file's contract region (frontmatter through Agent Instructions) matches the
  assigned excerpt; only `depends_on`, `progress`, `commits:` and appended `## Shipped`
  history changed, which is the sanctioned lifecycle bookkeeping surface.
- Baseline gate repaired: `bash scripts/check-task-commit-attribution.sh` at candidate
  HEAD → **OK, exit 0** (evidence: `/tmp/stage/review-evidence/task-065-review-evidence.txt`
  §1). The prior attempt's failure (a781ede2 task-210 unattributed) is resolved by the
  merged prerequisite a1751da1; the merge introduces no web/ or explorer-spec changes
  (`git diff --stat 418745ab 475e6082 -- web/` is empty).

Diff audit (`git diff a1751da1 3690d7a3 -- web/`, 17 files, +698/−3876):

- Dead-code deletion sanctioned by spec: `explorer-implementation.md` line 561 lists
  "Current MoldableView Graph/Flow tabs, FlowRenderer, FlowCanvas, ExplorerFilterPanel"
  under **Replaces**. At base, `MoldableView.svelte` was already unimported by
  production code (only `MoldableViewNodeTypeFilter.test.js` referenced it; the live
  path is RepoMode → ExplorerView → ExplorerCanvas). Deleting the orphans plus their
  orphaned tests (`FlowRenderer.test.js`, `MoldableViewNodeTypeFilter.test.js`) is
  clean cutover, not scope creep. `locales/en.json` diff is dead-key removal
  (`moldable_view.*` block) plus `\u2014`-escape → literal-character normalization;
  both sides parse as valid JSON, all live keys (e.g. `explorer_treemap.filter_presets`)
  intact.
- **§17 props** verified at `ExplorerCanvas.svelte:19-44`: `repoId`, `nodes`, `edges`,
  `activeQuery`, `filter` ($bindable), `lens` ($bindable) — exact names, exact unions.
  The legacy `filters` prop (ExplorerFilterPanel category filtering) is removed with
  the panel, per the spec's Replaces list.
- **Three lenses** (`ExplorerCanvas.svelte:4969-4972`): Structural (active default),
  Evaluative (functional: EvaluativeOverlay + PlaybackControls, `:4988-4994`),
  Observable `aria-disabled="true"` with the spec's exact label "requires production
  telemetry integration" (§89-99) and ObservableBanner on click. Base had
  `disabled` + "(coming soon)"; candidate fixes it to the spec text and makes the
  explanatory banner reachable — an accessibility improvement over a native-disabled
  button, still enforced-disabled for the lens itself.
- **Five filter presets** (`FILTER_PRESETS` at `:1536-1542`, toolbar at `:4975-4986`):
  All/Endpoints/Types/Calls/Dependencies with `aria-pressed` + `setFilter` wiring.
  Node dimming (`nodeFilterOpacity`, canvas-filters.js:32-43) and edge gating
  (`edgePassesFilter`, :47-57) both cover all five values and stay in agreement —
  the exact inconsistency class task-062 R8 F4 flagged is now structurally pinned.
- **Semantic zoom treemap**: `buildPathTree` (:1005) with Contains-edge hierarchy and
  file-path fallback, `squarify` (:1111), LOD via summary mode, double-click drill
  (`onDblClick` :4186 → `drillInto` :4148 via `parentToChildren` from Contains edges),
  breadcrumb navigation (`navigateBreadcrumb` :4375), eased camera + fade transition.
- **View query rendering**: scope resolution incl. `$clicked`/`$selected` interactive
  templates (:53-63), emphasis — `tiered_colors` (:2102-2106), `highlight.matched.color`,
  `dim_unmatched` (:2040), heat palette (:2049+), badges metric lookup (:3196-3227
  preferring server `node_metrics`); groups as labeled dashed clusters (:2353-2357);
  callouts as annotated nodes (:2500); narrative as numbered steps (:2606); annotation
  title/description with `$name` and `{{var}}` substitution (:4920-4922).
- **Interactions**: `onMouseDown` pan (:3843), `onWheel` zoom (:3908), click select,
  dblclick drill, right-click context menu, minimap click+drag (`minimapToWorld` :4554,
  `onMinimapMouseDown` :4591).
- **Minimap** (`drawMinimap` :3657, element :5133-5135): full-graph render + viewport
  rect + navigation.
- Ghost-animation gate now skips rAF churn in structural lens where `drawGhostOverlays`
  hardcodes `basePulse=0` (:2643) — performance-only, draw output identical.
- Performance tests switched wall-clock → `process.cpuUsage()` with explicit timeouts:
  makes the jsdom regression guard deterministic under parallel-worker contention
  (documented 16s-vs-3s wall inflation). Legitimate hardening, not test-weakening:
  thresholds unchanged (15s).

Test-quality verification (mutation probes, all against pristine tree, restored +
md5-verified after each — `/tmp/stage/review-evidence/task-065-review-evidence.txt` §4):

- M1: `dependencies` case reverted to unconditional `0.1` → **2 tests fail** (unit
  + component draw-path). Killed.
- M2: `collectEdgeParticipants` drops target endpoint → **6 tests fail**. Killed.
- M3: dependencies lit-opacity `1.0` → `0.5` → **2 tests fail** (exact-ratio
  assertions). Killed.

The component-level tests capture `ctx.globalAlpha` at leaf-label `fillText` and
assert participant/non-participant opacity ratios — they exercise the real
production draw path, not mirrored logic.

Test runs (this sandbox, `npm ci` locked versions first):

- Focused `canvas-filters.test.js` + `ExplorerCanvas.test.js`: **157 passed, 0 failed**.
- Full `npm test`: 56 files — **1 failed | 55 passed; 1512 passed | 41 skipped**.
  The single failure (`DetailPanelChat.test.js` "InlineChat recipient indicator")
  is a parallel-contention flake, not a candidate regression: it passes isolated
  (9/9, 11.85s), its files (`DetailPanel.svelte`, `DetailPanelChat.test.js`) are
  untouched by any commit in `a1751da1..3690d7a3` (last touched by task-092/091
  commits), and it passed in the prior round's full runs. Note: one full-suite run
  in this sandbox was contaminated by an accidentally-concurrent mutation probe
  (my tool-scheduling error, recorded in the evidence file); the flake reproduced
  identically in a clean sequential run, and no task-065 file failed in any run.

Transport restriction: TCP listener probe unsupported in this sandbox (errno 95,
`/tmp/stage/capabilities.json`), so no live dev-server browser check. Host
verification checklist recorded in the evidence file §5: exact-head GitHub CI plus
manual localhost:3000 checks (toolbar, drill/breadcrumb, minimap, preset dimming,
trace query). The jsdom draw-path tests stand in as the behavioral substitute
in-sandbox.

All eight acceptance criteria verified against code at the candidate head with
file:line evidence above. No structural defects, no fake implementations, no
missing enforcement, no unsafe scope handling, no weak tests found.

**Verdict: approved** (`/tmp/stage/verdict.json`, empty findings).
