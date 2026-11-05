# Review — task-177: Rendering Technology (SVG canvas, layout engines, interaction events)

- **Candidate:** `d68630dafcdf1a6acdbd38d2e614f55ddea25642` (branch `pipeline/task-177/e9ef24136f644befbd7e3ef611e91230-3`)
- **Base:** `d81b0fe4ab93abff06089432e4a8fca5be6e5d31`
- **Spec:** ui-layout.md §10 (Canvas Rendering, Layout Engines, Interaction Events)
- **Verdict:** **Approved** — independent evidence supports the task contract.

## What was verified

Acceptance criteria, each with independent probe evidence (logs under
`/tmp/stage/review-evidence/`, itemized in its README.md):

1. **SVG canvas with pan/zoom/click** — `SvgGraphCanvas.svelte` renders
   addressable node `<g>`/edge `<line>` elements, background-drag pan with
   window-level pointer tracking, wheel/button zoom, CSS-transitioned
   hover/selection, keyboard activation. Tests assert real DOM structure,
   zoom-label state transitions, and ViewEvent emission from nodes and edges.
2. **Auto-filter >500 nodes** — `canvas-density.js` (pure) + banners in
   `ExplorerCanvas` (production graph tab) and `SvgGraphCanvas`, with
   [Show All] override. Mutation A (filters disabled) fails 6 tests.
3. **List fallback >1000 nodes with virtual scroll** — shell-level swap in
   `ExplorerView` (canvas removed, `VirtualNodeList` windowed rows mounted);
   tested end-to-end through a rendered ExplorerView with a 1200-node graph.
   Covered by Mutation A as well.
4. **ELK for hierarchical/layered** — real elkjs via
   `layout-engines.js::elkLayout`; the hierarchical test asserts three
   distinct y-layers for the c→a→b chain. Mutation C (ELK forced to the
   column fallback) fails that test — the assertion demands genuine ELK
   output, not the fallback.
5. **d3-force for graph layout** — `forceLayout` (sync 300-tick simulation)
   exercised by the SvgGraphCanvas `engine: 'graph'` test asserting real
   force positions.
6. **ViewEvent interface + uniform emission** — `viewEvents.js` defines the
   specced shape; SVG nodes/edges, list rows, timeline rows, and Explorer
   insight cards all emit it; ExplorerCanvas emits `handled: true` events for
   its integrated interactions (no double-handling — tested).
7. **Shell handles all four types** — `ExplorerView.handleViewEvent`:
   click→detail panel, dblclick→repo drill-down (verified to load the target
   repo's graph), hover→positioned tooltip, context-menu→actions menu with
   data-conditioned entries; Escape cascade closes menu/tooltip. Mutation B
   (handler disabled) fails 5 of 8 shell tests.
8. **Layout engines registered** — all eight specced layouts in
   `layoutRegistry.js` with engine selectors for the SVG renderers; registry
   test pins the mapping and the closed-for-modification semantics.
9. **Tests pass** — 286/286 across the task suites and every touched-surface
   suite (17 files) on the candidate tree.

Repair-round items re-verified independently: the clippy `useless_conversion`
`.into()` is gone (no clippy warnings on any candidate-changed Rust line;
`cargo check -p gyre-server` exit 0), and the committed `web/dist` is
byte-identical to a fresh `npm ci` + `vite build` (`git diff -- web/dist`
empty after rebuild).

Mechanical gates: rustfmt-diff, arch, commit attribution, and the JS
test-quality checks all pass. No exemptions added, no verifier or coverage
changes (`git diff base..candidate -- scripts/ specs/coverage/` empty).

## Observations (non-blocking, recorded for follow-up)

- The registry maps layout name `graph` to the pre-existing Canvas-2D
  treemap `ExplorerCanvas` (a task-170/base decision), so the production
  "graph" tab is treemap-positioned; the d3-force SVG path is reachable via
  `SvgGraphCanvas`'s `engine: 'graph'` (tested) but not routed by any
  registry entry. Replacing the treemap would regress the specced
  system-explorer drill-down/spans/particles machinery, so keeping it as the
  `graph` renderer is a defensible reading of "existing components are the
  starting points" — but the tension with the §10 engine table is worth an
  explicit spec reconciliation if it matters downstream.
- `MoldableView` (the layout registry's consumer, and therefore the route to
  the hierarchical/layered/side-by-side/diff renderers) is not mounted
  anywhere in the production UI. This is identical at base (task-170 state)
  and outside task-177's stated scope ("connect to the layout registry from
  task-170"), but the product-integration step remains open work.
- Keyboard-activated ViewEvents (Enter/Space on nodes/rows/cards) carry
  `position: { x: undefined, y: undefined }` since KeyboardEvents have no
  clientX/Y; the shell defaults these to 0. Cosmetic; no misbehavior.
- `crates/gyre-domain/src/rust_extractor.rs:1071` trips a clippy 1.99
  `never_loop` **error** in this sandbox; the file is untouched by the
  candidate (pre-existing toolchain drift) — remote verification with the
  CI-pinned toolchain should confirm.
