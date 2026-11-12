# Review — task-181 (Risk Map — heat map overlay on architecture canvas)

Spec: `specs/system/system-explorer.md` §7 "Risk Map".
Candidate under review: `abedaaa3f` (base `4a203a06`); product commit `69b388d0`
(attributed in the task frontmatter at the candidate commit).
Verdict: **approved** — every acceptance criterion holds with independent
probe evidence; no findings.

## Probes (evidence: `/tmp/stage/review-evidence/`)

All run against the exact candidate checkout with `npm ci` (locked versions)
and `/tmp/stage/bin/npx`:

- `npx vitest run src/__tests__/RiskMap.test.js` → **25/25 passed** (`riskmap-focused.log`).
- `npx vitest run src/__tests__/ExplorerCanvas.test.js src/__tests__/ExplorerCanvas.evaluative-lens.test.js` → **162/162 passed** (`canvas-focused.log`).
- `npx vitest run` (full web suite) → **107 files, 2164 passed | 41 skipped** (`web-full-suite.log`).
- Mutation probes (temporary, restored — `git diff --quiet` confirmed clean after each):
  - `buildRiskHeatMap` metric-ignore mutation (def forced to churn) → **4 tests fail** (`mutation-metric.log`) — the metric selector genuinely drives coloring.
  - `detectRiskAnomalies` no-op mutation (`return []`) → **11 tests fail across both files** (`mutation-anomalies.log`) — the anomaly tests exercise the real production function, not mirror logic.
  - `evaluativeNodeColor` risk-branch disable in the canvas render path → **3 component tests fail** — heat colors flow through the real draw loop.
- `scripts/check-task-commit-attribution.sh` against the committed candidate tree → **exit 0** (`attribution-gate-committed.log`). Note: the gate fails against the *working tree* only because the executor restored the assigned contract (`commits: []`, `progress: not-started`) into the working tree after the checkpoint — the known process artifact from the durable finding, not a candidate defect; the committed candidate carries `commits: ["69b388d0e…"]`.

## Verified against acceptance criteria

- **Activation via Evaluative lens** — controls render when
  `lens === 'evaluative' && evaluativeMetric === 'risk' && riskEntries.length > 0`;
  the metric auto-switches to `risk` when no trace data exists and risk
  entries do (ExplorerCanvas ~L498), and `risk` is selectable in the
  EvaluativeOverlay metric bar (`data-testid="evaluative-metric-risk"`).
  Component test pins activation.
- **Metric selector** — `RISK_MAP_METRICS` = churn, coupling, complexity,
  spec_coverage, test_coverage; test asserts the exact five option labels.
- **Heat scale coloring** — `riskHeatColor` maps t∈[0,1] to
  hsl(210,70%,42%) blue → hsl(55,80%,50%) yellow → hsl(0,90%,42%) red;
  per-metric tests pin the endpoints and the coverage-metric inversion
  (red = uncovered). Zero is a valid cool-blue value; nodes without data
  for the metric get no heat fill instead of a fabricated zero. Layout is
  untouched — only the fill changes (`evaluativeNodeColor`).
- **Legend** — `RiskMapOverlay.svelte` renders the gradient bar + low/high
  labels in the canvas corner; test asserts presence and labels.
- **Anomaly callouts** — `detectRiskAnomalies()` (utility, per task
  instruction) covers all four spec patterns plus heavily-depended-on and
  dead-code extras; consumes `/graph/risks` entries so server
  `spec_covered`/complexity override stale node props (test pins that
  precedence). Panel is dismissible (`anomalyPanelOpen` close button).
- **Clickable callouts** — click runs `zoomToNode(n.id)` + `onNodeDetail(n)`;
  test fires a real click and asserts the detail callback with the node.
- **Clean deactivation** — test unchecks the Evaluative lens and asserts
  controls + legend removed and no heat fills redrawn.
- **Data source contract** — `RiskMetricsResponse` in
  `crates/gyre-server/src/api/graph.rs` (node_id, churn_rate, fan_in,
  fan_out, complexity, spec_covered) matches exactly what `risk-map.js`
  consumes; complexity/test_coverage additionally come from graph node
  properties per the task plan.

## Test-quality assessment

The revision *replaced* the previous mirror-logic anomaly tests (which
re-implemented the checks beside the code) with direct tests of the shipped
`detectRiskAnomalies` — an anti-inflation improvement. All three mutation
probes were killed, so the suite discriminates real regressions in the
metric mapping, the anomaly detection, and the canvas render path.

## Notes for the verifier (non-blocking)

- No Rust surface changed — no server tests required beyond the web suites
  above; the verifier rebuilds `web/dist/` and runs the full web suite +
  repo check scripts (attribution gate against the *committed* tree).
- Minor observations, not defects: (a) in `detectRiskAnomalies` the
  complexity-percentile base filters on `n.complexity != null` while mapping
  with server-first `complexityOf(n)`, so server-only complexity values sit
  outside the distribution base; (b) `riskMetricRawValue` churn falls back
  `?? 0` for an entry missing `churn_rate` — unreachable given the server's
  non-optional `u32`; (c) the task instruction's "SVG nodes / CSS custom
  properties" phrasing maps to canvas 2D `fillStyle` here since the
  architecture canvas is a `<canvas>` element, not SVG — the reactive-binding
  intent is honored via `$derived` + redraw scheduling.
- The executor's working-tree restoration of `specs/tasks/task-181.md`
  (assigned contract, `commits: []`) differs from the committed candidate's
  ready-for-review frontmatter; the committed tree is authoritative and
  passes the attribution gate.
