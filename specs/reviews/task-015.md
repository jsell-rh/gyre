# Review: TASK-015 — Flow Canvas (Animated Trace Visualization)

**Reviewer:** Independent review agent
**Date:** 2026-11-10 (this round)
**Candidate:** `a87cf1ce68b9160211478aab86fcfedb02c93c7a` (base `8b93f242c313cc18d626855f14562f4548d68aba`)
**Verdict:** approved — real implementation, no findings

---

## Scope reviewed

Diff base→candidate: `web/src/lib/FlowCanvas.svelte` (new, 1029 lines),
`web/src/lib/flow-particles.js` (new, pure span→particle logic),
`web/src/lib/particle-layer.js` (new, Canvas 2D / WebGL2 switch),
`web/src/lib/flow-layout.js` (new, layered layout), ExplorerCanvas.svelte
(delegation + shared particle path), PlaybackControls.svelte (step button,
test selector, legend, 10x speed), ExplorerView.svelte (`layout` prop
wiring), `specs/system/explorer-canvas.md` (amendment), and five new test
files (65 focused tests).

## Acceptance criteria — evidence

- **FlowCanvas renders nodes/edges as SVG with Canvas 2D particle overlay** —
  SVG `g[data-flow-node][role=button]` / `path[data-flow-edge]` layer plus a
  `.flow-particles` host holding the overlay canvas
  (`data-particle-mode="2d"`, `pointer-events: none`). Verified by
  FlowCanvas.test.js "renders nodes/edges as accessible SVG…".
- **Particles animate along edges following span tree relationships** —
  `particlesAtTime` maps each span's `parent_span_id`→`graph_node_id` pair
  onto edge endpoints; progress interpolates along the laid-out node
  positions. Unit tests pin progress values (0.5, 1/3) and edge identity.
- **Color blue/red by span status** — `SUCCESS_PARTICLE_COLOR = '#3b82f6'`,
  `ERROR_PARTICLE_COLOR = '#ef4444'`, asserted in
  flow-particles.test.js (also fixes the pre-existing `#60a5fa` deviation on
  the ExplorerCanvas path, which now shares the module).
- **Trail fades over 200ms** — `trailSegments` (TRAIL_FADE_MS=200,
  TRAIL_LENGTH_PX=20) with age-based alpha decay, in-place pruning, length
  cap, and no segments for stationary (paused/scrubbed) particles; unit and
  component tests cover decay, cap, and the rendered stroke alphas.
- **Time scrubber freezes particles at the timestamp** — the paused-scrub
  `$effect` resolves `particlesAtTime` at `minTime + scrubber × total`;
  test asserts active-span counts at 300/700/950‰ scrub positions.
- **PlaybackControls (Play/Pause/Step/Speed)** — shared component extended
  with Step (next filtered span start, pause) and 0.25x–10x speed (union of
  explorer-canvas §4 and HSI §3a 1x/10x); spec amended accordingly.
- **Node badges span_count / error_rate / mean_duration** — `nodeStats`
  computed from GateTrace spans; rendered as SVG chips; tests assert
  "2 spans / 0% err / 3.5ms" and the error case "1 spans / 100% err".
- **Click node pauses and opens detail panel** — click handler pauses
  playback when the node has stats and enriches the payload with
  `_activeSpan` from `nearestParticleToNode`; tested end-to-end via
  `onNodeDetail` mock.
- **Test selector filters root spans** — `rootSpanCases` (root spans in
  start order) drive a `Test: all` dropdown + hue-shift legend; filter
  threads through `particlesAtTime`/`nextSpanStart`; tested on both
  FlowCanvas and the ExplorerCanvas evaluative path.
- **ExplorerCanvas delegates for `layout: "flow"`** — new `layout` prop
  (wired from ExplorerView's active layout) gates an `isFlowLayout` branch
  that renders FlowCanvas instead of the 2D canvas and suppresses
  ExplorerCanvas's own playback bar; delegation asserted in tests.
- **Component tests with mock trace data** — 65 focused tests across
  FlowCanvas / ExplorerCanvas.flow / flow-particles / particle-layer /
  flow-layout.

## Probes run (evidence in /tmp/stage/review-evidence)

1. `npm ci` (exit 0), then focused vitest run of the five new test files:
   **5 files, 65 tests, all passed** (`task-015-focused-vitest.txt`).
2. **Mutation probe** (`task-015-mutation-probe.txt`): temporarily disabled
   the test-case filter and the span-expiry window in `flow-particles.js`
   → **6 tests failed** (scrub repositioning, filter, WebGL threshold,
   window boundaries). Tests are behavior-pinning, not self-confirming.
   File restored afterward (`git status` clean apart from the executor's
   pre-existing task-015.md working-tree change).
3. **Regression probe**: ExplorerCanvas.evaluative-lens / .test /
   Trace / lens-rendering / Liveness — **5 files, 229 tests, all passed**
   (`task-015-regression-vitest.txt`).

## Spec amendment check

`specs/system/explorer-canvas.md` previously mandated "FlowCanvas merges
into EvaluativeOverlay", contradicting ui-layout.md §4 ("Canvas 2D overlay
on top of the SVG node/edge layer"). The candidate amends explorer-canvas.md
to make FlowCanvas the flow-layout renderer (SVG layer + particle overlay,
WebGL2 above 100 concurrent particles) and unifies the speed range — a
legitimate spec-lifecycle reconciliation of an internal contradiction
rather than code contradicting a spec. Both renderers share the same
particle modules, so the >100-particle WebGL rule holds on both paths.

## Notes (non-blocking)

- Particle "4px / 6px on hover" is interpreted as canvas radius, matching
  the pre-existing ExplorerCanvas convention (`4 + cam.zoom·0.5`); the
  hover growth delta matches the spec. Cosmetic only.
- A duplicated `.treemap-canvas` CSS rule was left in ExplorerCanvas.svelte
  (harmless, last rule wins identically).

## Verifier follow-ups

No server-side (Rust) changes are in scope; frontend verification is
covered by the vitest suites above. If the verifier re-runs, use:
`cd web && npx vitest run src/__tests__/FlowCanvas.test.js
src/__tests__/flow-layout.test.js src/__tests__/flow-particles.test.js
src/__tests__/particle-layer.test.js src/__tests__/ExplorerCanvas.flow.test.js`.
