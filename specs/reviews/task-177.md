# Review — task-177 (Rendering Technology — SVG canvas, layout engines, interaction events)

Spec: `specs/system/ui-layout.md` §10 — SVG canvas for graph/hierarchical/layered (DOM addressability, Svelte reactivity, CSS transitions), auto-filter >500 nodes to public-only with banner + [Show All], list fallback with virtual scrolling >1000 nodes even after filtering, the eight-entry layout-engine table (d3-force / ELK / d3-scale / CSS Grid / custom), and the standardized `ViewEvent` interface handled uniformly by the Explorer shell (click → detail panel, dblclick → drill-down, hover → tooltip, context-menu → actions).

Commits under review: `825b4145` (checkpoint: components + tests, 8/8 SvgGraphCanvas failures at assignment end) and `e1a8686b` (fixes: effect state loop, list-mode layout waste, test reliability), plus process commit `12d6f30f` (frontmatter attribution only). Cumulative diff `git diff 63aa1486..12d6f30f` is web-only (+2771/−36 across 21 files); no Rust, scripts, or coverage-matrix changes.

Verdict: **complete** (approved).

## Evidence (commands, exit codes, outputs in `/tmp/stage/review-evidence/`)

`npm ci` in `web/` before every vitest invocation; all probes on the exact candidate checkout; working tree verified clean (`git status` empty) at HEAD `12d6f30f` after every mutation.

- **Task-177 suite** (SvgGraphCanvas, canvas-density, VirtualNodeList, layoutRegistry-engines, DiffView, SideBySideView, ExplorerViewViewEvents): 43/43 passed, two consecutive runs, exit 0.
- **Full web suite** (regression): 67 files, 1595 passed | 41 pre-existing skips, 0 failed.
- **Mutation 1 — ELK disabled** (forced `throw` before the elkjs import in `elkLayout`, restored): `positions nodes via the ELK engine (hierarchical)` FAILED (1/8). The strengthened test (3 distinct y layers for the c→a→b chain) kills the column-fallback path — the checkpoint's version asserting only node count did not.
- **Mutation 2 — public-only filter disabled** (`if (true)` bypass in `applyDensityPolicy`, restored): 6 tests failed across canvas-density + SvgGraphCanvas (banner text, hidden-count, rendered-node count, list fallback). The thresholds are enforced, not decorative.
- **Mutation 3 — shell click handling removed** (drop `openNodeDetail` from `handleViewEvent`'s click case, restored): `registers the handler on mount: click opens the detail panel` FAILED.
- **Real-ELK probe** (plain node, no mocks): `elkjs/lib/elk.bundled.js` lays out the c→a→b chain DOWN with genuine layered positions (y = 12/72/132), exit 0.
- **Live-surface density probe** (temporary test, deleted after run): ExplorerCanvas (the Canvas-2D production surface) with 450 public + 80 private nodes shows the `.density-banner` ("80 private nodes hidden"), renders 450 nodes, and [Show All] restores 530 with the banner gone; 500 public nodes → no banner. This closed the gap that no committed test exercises the ExplorerCanvas density wiring.
- `scripts/check-task-commit-attribution.sh` → OK (both implement commits recorded in `commits:` frontmatter).

## Acceptance criteria — verified

- **SVG canvas with pan/zoom/click** — `SvgGraphCanvas.svelte`: addressable `<g>`/`<line>` elements, background-drag pan with window-level pointer tracking and unmount cleanup, cursor-anchored wheel zoom clamped [0.1, 8] plus toolbar zoom/fit, `onSelectNode` on click with drag-suppression, keyboard activation (Enter/Space). Tested.
- **Auto-filter >500** — pure `canvas-density.js` policy applied in three places: SvgGraphCanvas (full cascade), ExplorerCanvas (live 2D surface, banner + Show All, effect re-runs on the Show All toggle because `densityShowAll` is read), ExplorerView (list-fallback ownership). Banner text matches the spec wording. Boundary semantics (>500, not ≥) pinned by tests.
- **List fallback >1000** — shell swaps the canvas for `VirtualNodeList` when public count alone exceeds 1000; genuine windowing (scrollTop/rowHeight + overscan; spacer = full height; only ~27 of 5000 rows mounted; scroll advances the window; end-clamped). Layout computation is correctly skipped in list mode (the checkpoint's CPU-waste defect).
- **ELK** — real `elkjs` WASM engine for hierarchical (DOWN) and layered (RIGHT) via the pre-existing `layout-engines.js`, now rendered by the SVG canvas; mutation-verified test plus an independent plain-node probe.
- **d3-force** — sync 300-tick simulation (pre-existing engine, no RAF loop), tested through SvgGraphCanvas positions.
- **ViewEvent interface** — `viewEvents.js` (pre-existing) extended with the `handled` marker; every interactive element (SVG nodes, SVG edges, virtual-list rows, Moldable list/timeline rows, insight cards, DiffView cards) emits the standardized shape. Tested from nodes, edges, and rows.
- **Shell handles all four types** — `ExplorerView.handleViewEvent`: click → detail panel, dblclick → repo drill-down (verified: loads the entity's repo graph), hover → positioned tooltip, context-menu → actions menu with data-conditioned entries; Escape closes via the overlay cascade. ExplorerCanvas's own interactions emit `handled: true` events so the shell observes without double-performing — the canvas services them through its existing onNodeDetail/drill/tooltip/menu plumbing.
- **Registry** — all eight specced layouts registered (`graph`→ExplorerCanvas, `hierarchical`/`layered`→SvgGraphCanvas with engine selectors, `side-by-side`→CSS-Grid SplitView with 50/50 and 60/40 toggles, `diff`→DiffView with real added/modified/removed classification against node timestamps and soft-deletes, `list`/`timeline` inline in MoldableView, `flow`→FlowRenderer); `engine` prop plumbed through the renderer contract; re-registration rejected (grammar closed). d3-scale added to package.json/lock and drives the timeline time axis + ticks.

## Observations (not blocking, for the auditor's context)

- The layout-registry consumer `MoldableView` is test-only (zero production imports) — a pre-existing substrate property documented in `specs/coverage/system/explorer-canvas.md` rows 1-2, not introduced by this task. The task's contract was to register the engines in the task-170 registry and integrate density/ViewEvents into the live ExplorerView/ExplorerCanvas surface, both of which are genuinely done and reachable in production.
- The ExplorerCanvas density wiring has no committed test (the SvgGraphCanvas and pure-policy tests cover the same logic); my temporary probe verified it live. Worth a follow-up test in a polish task, not a gap in this task's contract.
- `SvgGraphCanvas`'s wheel handler calls `preventDefault()` on a delegated Svelte listener; browsers may log a passive-listener warning without breaking the zoom. Cosmetic.
