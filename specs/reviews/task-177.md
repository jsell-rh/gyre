# Review: TASK-177 — Rendering Technology (SVG canvas, layout engines, interaction events)

**Reviewer:** Independent review (pi runtime)
**Date:** 2026-10-09
**Comparison base:** fefde2946f3a0af1f212588f54591516fee0f7b2
**Candidate (HEAD):** 9de24eeb99e1b739dc4916fee046140ce7b08e42
**Verdict:** NOT approved (needs revision)

---

## Summary

The engines and components are real — elkjs produces genuine layered positions,
d3-force produces genuine force-directed positions (verified by direct node
probe, no mocks), the density policy (500/1000 thresholds) is live in the
reachable ExplorerCanvas + shell VirtualNodeList fallback, and the ViewEvent
system (interface, uniform shell handling of click/dblclick/hover/context-menu,
emission from canvas/cards/list rows) is real, reachable, and well tested.

However, the headline deliverable of §10 — SVG rendering for
graph/hierarchical/layered layouts positioned by ELK/d3-force, plus the
side-by-side and diff layouts — is **dead code in the merged tree**. The rebase
onto base fefde294 followed the upstream deletion of `MoldableView.svelte`
(the registry's only consumer) without replacing the consumer, and the task's
own Implementation Plan item 4 ("View spec `layout` field selects the renderer")
was not implemented: `applyGeneratedViewSpec` → `viewSpecToViewQuery` projects
every generated ViewSpec to a canvas ViewQuery regardless of its `layout` field
(the only layout dispatch is upstream's pre-existing `'flow'` special case).
Approving would let the §10 Canvas Rendering / Layout Engines coverage sections
be marked implemented while the shipped product renders every view through the
Canvas-2D explorer and never positions anything via ELK or d3-force.

## Findings

### 1. (blocking, code) Layout registry renderers unreachable — no production consumer

Evidence (see `/tmp/stage/review-evidence/task177-reachability-probe.md`):

- `layoutRegistry.js` is imported only by its own test. The production render
  chain is `RepoMode → ExplorerView → ExplorerCanvas` (Canvas 2D) with
  `VirtualNodeList` for the >1000-node fallback.
- `SvgGraphCanvas`, `SideBySideView`, `DiffView` are imported only by the
  registry (and by each other). No dynamic component dispatch
  (`<svelte:component>` / `getLayout(...)`) exists anywhere in `web/src`.
- The ViewSpec `layout` field never selects a renderer
  (`ExplorerView.svelte:140,142` — log string and the upstream 'flow' case).
- The candidate's original checkpoint 825b4145 wired the registry into
  `lib/MoldableView.svelte`; the rebase dropped that wiring.

Consequence: spec §10 "The Explorer canvas uses **SVG** for
graph/hierarchical/layered layouts … positioned by ELK or dagre" is not
performed by any production code path in the merged tree; the CSS-Grid
side-by-side and diff card layouts are equally unreachable. This is shipped
dead code, not shipped behavior.

Fix direction: wire the registry into the live shell — e.g.
`applyGeneratedViewSpec` dispatches on `spec.layout` to the registered renderer
component (SvgGraphCanvas for hierarchical/layered, SideBySideView, DiffView,
…), falling back to the canvas ViewQuery projection for layouts without a
dedicated renderer — so the layout field actually selects the renderer, per
the task plan.

### 2. (minor, code) Registry labelKeys reference deleted i18n keys

All eight registrations use `labelKey: 'moldable_view.tab_*'`, but the base
removed the `moldable_view` section from `web/src/locales/en.json`
(`grep -c "moldable_view" web/src/locales/en.json` → 0). Any consumer of the
registry would render raw keys. Symptom of finding 1; fix together with it.

### 3. (minor, test) d3-force component test is self-confirming against the fallback

`SvgGraphCanvas.test.js` "positions nodes via the d3-force engine (graph)"
only asserts some node transform ≠ the pending-column position
(`!t.includes('-26px')`). The `columnLayout` fallback (translate x = 14px)
also satisfies this, so the test passes with d3-force disabled and replaced by
the fallback — it does not pin the engine. (The engine itself is genuine:
direct node probe verified real force output. The sibling ELK hierarchical
test IS strong — it requires 3 distinct y layers, which the column fallback
cannot produce.)

## What was verified as real and working

- **Density policy** (`canvas-density.js`): pure, thresholds 500/1000 pinned by
  tests; applied live in `ExplorerCanvas.svelte` (public-only + banner +
  [Show All]) and by the shell for the >1000 virtualized-list fallback
  (`ExplorerView.svelte` boundary branch → `VirtualNodeList.svelte`, windowed
  rows + overscan). ExplorerViewViewEvents/ExplorerViewLargeGraph tests pass.
- **ELK + d3-force engines** (`layout-engines.js`): real `elkjs/lib/elk.bundled.js`
  and `d3-force`; direct node probe produced genuine layered positions
  (DOWN: 3 distinct y layers; RIGHT: 3 distinct x layers for chain c→a→b) and
  real force coordinates — no mocks anywhere in the test setup.
- **ViewEvents** (`viewEvents.js`): standardized interface
  (type/entity_type/entity_id/position), global listener bus + DOM bubbling;
  uniform shell handling of all four event types (click→panel, dblclick→drill,
  hover→tooltip, context-menu→actions menu, Escape cascade), `handled` flag
  prevents double-servicing by the integrated Canvas-2D emitter. Tested.
- **d3-scale timeline** (`TimelineScrubber.svelte`): time-proportional marker
  positioning via `scaleTime()` with index-spacing fallback for degenerate
  domains; reachable through the live canvas. Two new focused tests pin
  time-proportional vs index spacing.
- **Invariant gates**: check-task-commit-attribution, check-arch,
  check-in-memory-state-stores, check-relative-path-defaults — all pass.
- Focused test runs (evidence: `/tmp/stage/review-evidence/task177-test-runs.md`):
  canvas-density + SvgGraphCanvas + layoutRegistry-engines (23 passed),
  ExplorerViewViewEvents + VirtualNodeList + DiffView + SideBySideView +
  TimelineScrubber (44 passed), ExplorerViewLargeGraph + ExplorerCanvas
  (160 passed) — all exit 0.

## Verdict

**Not approved.** Findings 1 must be resolved before the §10 coverage sections
can be called implemented: the SVG canvas / ELK / d3-force / side-by-side /
diff rendering technology must be reachable from the product (the ViewSpec
`layout` field selecting the registered renderer is the task's own plan), or
the sections stay open. Findings 2–3 should be fixed alongside.
