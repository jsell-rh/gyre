---
title: "Risk Map — heat map overlay on architecture canvas"
spec_ref: "system-explorer.md §7"
depends_on: [task-178]
progress: complete
coverage_sections:
  - "system-explorer.md §7. Risk Map"
commits: ["69b388d0e504dc8bb374af836675c142c651e310", "40dc0d1d06d5410969b2d30512800f4041c3a0de"]
---

## Spec Excerpt

system-explorer.md §7 defines a Risk Map overlay on the architecture canvas:

**Heat map mode** — color nodes by one of several metrics:
- Churn rate (cool blue = stable, hot red = high churn)
- Coupling score
- Spec coverage
- Complexity
- Test coverage

The user selects which metric drives the heat map coloring. This is an overlay mode on the existing canvas — nodes change color based on the selected metric while maintaining their position.

**Anomaly callouts** — automated warnings surfaced alongside the heat map:
- "Module X has high complexity (42) but low test coverage (23%)"
- "These 3 modules always change together but have no shared spec"
- "Type Y was created by an agent but has no governing spec"
- "This trait has 5 implementations but only 2 are tested"

The risk map helps the human identify where to direct attention — where understanding is most needed and where problems are likely to emerge.

**Data source:** `GET /api/v1/repos/:id/graph/risks` (already exists) provides `fan_out`, `fan_in`, `churn_rate`, `spec_covered` per node. Additional metrics (complexity, test_coverage) come from the knowledge graph node properties.

## Implementation Plan

1. **Risk Map overlay component** (`RiskMapOverlay.svelte`):
   - Activated when the Evaluative lens is enabled (from task-178's lens selector)
   - Metric selector dropdown: Churn, Coupling, Complexity, Spec Coverage, Test Coverage
   - When active, node fill colors change to heat map scale (blue→yellow→red)
   - Color scale legend shown in corner of canvas

2. **Heat map coloring logic**:
   - Fetch risk data from `GET /repos/:id/graph/risks`
   - Map the selected metric to a continuous color scale
   - Apply colors to existing SVG nodes (CSS variables or inline style)
   - Preserve node positions and layout — only color changes

3. **Anomaly detection and callouts**:
   - Analyze risk data for anomaly patterns:
     - High complexity + low test coverage
     - Co-changing modules without shared spec (from churn correlation)
     - Unspecified nodes (no `spec_path`)
     - Traits with partial implementation testing
   - Show anomaly callouts as dismissible cards at the top of the canvas or in a collapsible panel
   - Each callout links to the relevant nodes (click → navigate on canvas)

4. **Integration with Evaluative lens**:
   - Risk Map is one aspect of the Evaluative lens overlay
   - When Evaluative lens is toggled on, Risk Map controls appear
   - When toggled off, nodes return to structural coloring

5. **Tests**:
   - Heat map colors applied correctly for each metric
   - Anomaly detection identifies known patterns from mock data
   - Metric selector switches coloring
   - Overlay activates/deactivates with Evaluative lens toggle

## Acceptance Criteria

- [ ] Risk Map overlay activates via Evaluative lens toggle
- [ ] Metric selector with Churn, Coupling, Complexity, Spec Coverage, Test Coverage
- [ ] Nodes colored on heat map scale (blue→yellow→red) based on selected metric
- [ ] Color scale legend displayed on canvas
- [ ] Anomaly callouts generated from risk data patterns
- [ ] Callout nodes are clickable (navigate on canvas)
- [ ] Overlay deactivates cleanly when lens toggled off
- [ ] Tests pass

## Agent Instructions

Read `system-explorer.md` §7 "Risk Map" for the full specification. The risk data endpoint `GET /repos/:id/graph/risks` is already registered in `crates/gyre-server/src/api/mod.rs` — check its response shape in `graph.rs`. The Evaluative lens toggle is part of task-178 — check if `ExplorerView.svelte` or `EvaluativeOverlay.svelte` already handle lens switching. For heat map coloring, use CSS custom properties or Svelte reactive style bindings on the SVG nodes. The anomaly detection logic should be in a utility function, not inline in the component. Check the design system colors in `web/src/lib/design-system.css` for the brand palette.

## Shipped

The Risk Map overlay (system-explorer.md §7) on the task-178 canvas, in
commit 69b388d0e:

- **Heat map mode** — `web/src/lib/risk-map.js` (pure, unit-tested) maps
  each of the five spec metrics to a continuous blue → yellow → red scale
  (`riskHeatColor`, `buildRiskHeatMap`). Churn/coupling/spec coverage come
  from the `GET /repos/:id/graph/risks` entries; complexity and test
  coverage come from graph node properties (complexity falls back to the
  risk entry). Zero is a valid cool-blue value (stable/covered), coverage
  metrics invert so red = uncovered, and nodes with no data for the metric
  get no heat fill instead of a fabricated zero. Node positions and layout
  are untouched — the canvas only swaps the fill (`evaluativeNodeColor`).
- **Metric selector + legend** — `RiskMapOverlay.svelte` renders the
  Churn/Coupling/Complexity/Spec Coverage/Test Coverage dropdown and a
  color-scale legend in the canvas corner whenever the Evaluative lens runs
  the Risk metric; `ExplorerView` passes the full `RiskMetricsResponse`
  records (`riskEntries`) alongside the task-178 churn × complexity
  aggregate. Toggling the lens (or metric) off removes the overlay cleanly.
- **Anomaly callouts** — the detection patterns live in
  `detectRiskAnomalies()` (per the task instruction: utility, not inline),
  consuming the risk entries so server-computed `spec_covered`/complexity
  override stale node props. Callout clicks pan/zoom the canvas onto the
  offending node (`zoomToNode`) and open its detail panel.
- **Test quality** — the anomaly tests call the real
  `detectRiskAnomalies`; no mirror-logic test helpers.

### Round 2 (post base-merge 267b7a2c9)

The executor restored the assigned task document after the previous
checkpoint; the production fix (69b388d0e) was already on the branch and
the base merge touched none of its web files (only `docs/dev-pipeline.md`
differs under web/ + docs/ since that commit). No source change was
needed. Re-verified with `npm ci` (locked versions) plus the focused web
probes, all green (evidence: `/tmp/stage/review-evidence/web-tests.log`):

- `npx vitest run src/__tests__/RiskMap.test.js` — 25 passed.
- `npx vitest run src/__tests__/ExplorerCanvas.test.js
  src/__tests__/ExplorerCanvas.evaluative-lens.test.js
  src/__tests__/ExplorerViewNavigableArchitecture.test.js
  src/__tests__/ExplorerViewLensUrl.test.js` — 180 passed.
- `npx vitest run src/__tests__/` (full web suite) — 107 files,
  2164 passed | 41 skipped.

No Rust surface changed — no server tests required for the verifier
beyond the web suites above.
