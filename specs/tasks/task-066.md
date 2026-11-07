---
title: "ExplorerCanvas — Lens Definitions & Evaluative Overlay"
spec_ref: "explorer-canvas.md §4"
depends_on:
  - task-065
progress: complete
coverage_sections:
  - "explorer-canvas.md §4 Lens Definitions"
commits: ["7a3d96f01f6846c6184db0d5b82855f2a4c30c34", "9fcf9f28120915dae7feb267fe4bce9a83974279", "3b37be901ab245b5dd82a94e611fac2c066e2b99", "96cd52a977b7b2f79dab55b5af9e7dcb60c92117", "1b09c7e020035a48822527681d2b3659cbf53905"]
---

## Spec Excerpt

**§4 Lens Definitions:**

**Structural (default):**
- Data source: Knowledge graph nodes + edges (especially `Calls`, `RoutesTo`, `Contains`)
- Shows: static topology, causal flow paths, spec linkage (colored by spec coverage: green=governed, amber=suggested, red=no spec), complexity/churn indicators (node size=complexity, border thickness=churn)
- Interactions: click node → detail panel, double-click → drill into children, click edge → relationship detail, right-click → context menu ("View spec", "View provenance", "Trace from here")
- No particles. No animation. Pure structure.

**Evaluative:**
- Data source: OTLP trace spans from TraceCapture gates (`GET /merge-requests/:id/trace`)
- Overlays on structural view: particles animated along edges, node heat (blue=fast, red=slow), node badges (span count, error rate, mean duration), edge thickness (proportional to call frequency), failed spans (red glow, error particles), playback controls (play/pause, scrubber, speed 0.25x–5x)
- Interactions: everything from Structural + click particle → span detail panel, click node → "Evaluative" tab with all spans, hover → timing stats (p50, p95, error rate)
- The structural topology is always visible underneath.

**Observable (future):**
- Data source: Production OpenTelemetry collector (not yet built)
- Grayed out in UI with label: "Requires production telemetry integration."

## Implementation Plan

### Existing Code

- `web/src/lib/ExplorerCanvas.svelte` — already has `lens` prop and conditional rendering
- `web/src/components/EvaluativeOverlay.svelte` — particle animation layer
- `web/src/components/PlaybackControls.svelte` — play/pause/speed controls
- `web/src/components/ObservableBanner.svelte` — likely the "grayed out" banner
- `web/src/components/NodeBadge.svelte` — metrics badges on nodes

### Work Required

1. **Structural lens audit**: Verify spec linkage coloring (green/amber/red based on `GovernedBy` edges), complexity indicators (node size), churn indicators (border thickness). Verify right-click context menu with "View spec", "View provenance", "Trace from here" actions.

2. **Evaluative lens audit**: Verify particle animation, node heat coloring by duration, node badges (span count, error rate, mean duration), edge thickness by call frequency, failed span glow. Verify playback controls (play/pause, scrubber, speed slider 0.25x–5x). Verify click-particle → span detail, hover → tooltip with p50/p95/error rate.

3. **Observable lens**: Verify it's grayed out with the correct label text. Clicking it should show the banner, not switch to a broken view.

4. **Lens switching**: Switching lenses should preserve the structural topology and viewport. Only the overlay changes.

## Acceptance Criteria

- [ ] Structural lens: nodes colored by spec coverage (green=governed, amber=suggested, red=no spec)
- [ ] Structural lens: node size reflects complexity, border thickness reflects churn
- [ ] Structural lens: right-click context menu with "View spec", "View provenance", "Trace from here"
- [ ] Evaluative lens: particles animate along edges following trace execution paths
- [ ] Evaluative lens: node heat coloring (blue=fast, red=slow) from trace span durations
- [ ] Evaluative lens: node badges show span count, error rate, mean duration
- [ ] Evaluative lens: edge thickness proportional to call frequency
- [ ] Evaluative lens: playback controls (play/pause, scrubber, speed 0.25x–5x)
- [ ] Evaluative lens: click particle → span detail panel
- [ ] Evaluative lens: hover node → tooltip with p50, p95, error rate
- [ ] Observable lens: grayed out with "Requires production telemetry integration" label
- [ ] Switching lenses preserves structural topology and viewport position
- [ ] `cd web && npm test` passes

## Agent Instructions

Read `specs/system/explorer-canvas.md` §4 (Lens Definitions). Then audit:
- `web/src/lib/ExplorerCanvas.svelte` — look for lens-conditional rendering
- `web/src/components/EvaluativeOverlay.svelte` — particle animation
- `web/src/components/PlaybackControls.svelte` — playback UI
- `web/src/components/ObservableBanner.svelte` — observable lens disabled state
- `web/src/components/NodeBadge.svelte` — metric badges

For each lens, walk through every bullet point in the spec and verify the code implements it. The structural lens has the most interaction requirements (right-click menu, spec linkage coloring). The evaluative lens has the most rendering requirements (particles, heat, badges, thickness).

## Shipped

### Lens audit result (explorer-canvas.md §4)

All §4 behaviors are implemented in production code; two evaluative-lens
defects found by the audit were repaired (see below).

- **Structural**: spec-coverage coloring in `specBorderColor` (green =
  GovernedBy edge / high confidence, amber = heuristic `spec_path` only,
  red = no spec) with a precomputed `governedByIndex`; node size = complexity
  (`nodeWeight`), border thickness = churn (`churnThickness`, capped 4px);
  click → detail panel, double-click → drill into children, edge click →
  relationship detail, right-click context menu with "View spec",
  "View provenance", "Trace from here". Ghost overlays hardcode `basePulse=0`
  in the structural lens — no particles, no animation.
- **Evaluative**: particles animate along trace execution edges
  (`evalParticles` + `tickParticles`, advanced only while `evalPlaying`);
  node heat blue=fast → red=slow by span duration; node badges show span
  count, error rate, AND mean duration (repaired); edge thickness scales
  with trace call frequency (1.5–6px); failed spans get a red glow
  (`nodeHasErrors` + shadowBlur); PlaybackControls (play/pause, scrubber,
  speed 0.25x–5x); click particle → span detail via `onNodeDetail`; hover
  tooltip shows p50, p95, error rate. Structural topology stays visible
  underneath (overlay-only rendering).
- **Observable**: button `aria-disabled` with "(requires production telemetry
  integration)" label; clicking shows the ObservableBanner instead of
  switching lenses.
- **Lens switching**: `onLensChange` only syncs canvas state and redraws —
  topology and viewport are preserved; only the overlay changes.

### Repairs in 7a3d96f0

The base merge (876ca6ec) took main's `ExplorerCanvas.svelte` wholesale,
dropping the earlier checkpoints' evaluative fixes while keeping the tests
that pin them; the interrupted attempts also left two debug scaffold test
files. Shipped:

1. **Evaluative badge chips**: the badge drew only the heat metric selected
   in the metric picker, hiding two of the three §4-required metrics. Now a
   chip row renders span count, error rate, and mean duration on every
   traced node, independent of the heat metric.
2. **Render-loop survival under synchronous rAF** (jsdom mocks, sync-rAF
   webview polyfills): `animFrame = requestAnimationFrame(animLoop)`
   re-enters synchronously, and the outer assignment restores a stale
   handle after the nested loop stops — every later `scheduleRedraw()`
   bailed and the canvas froze permanently. Scheduling is now guarded by an
   `animScheduled` flag that stays true for the whole frame (mid-frame
   `scheduleRedraw` calls coalesce into the tail decision), and the
   synchronous re-frame chain is depth-capped: conditions not bounded by
   frame count (drill transitions keyed on wall-clock time, evaluative
   playback) never settle when a sync rAF advances no time between frames,
   so the chain stops cleanly at the cap and a later `scheduleRedraw()`
   starts a fresh chain instead of exhausting the stack. Async rAF (real
   browsers) never accumulates depth and is unaffected.
3. Removed the assertionless debug scaffolds (`tmp-repro.test.js`,
   `zz-debug.test.js`) left by the interrupted attempts.

### Test evidence

- `ExplorerCanvas.evaluative-lens.test.js`: 4/4 at the fix head — was 1/4
  at the assignment head (3 failures: badge chips ×2, redraw-after-prop
  change ×1; recorded under `/tmp/stage/review-evidence/pre-fix-evaluative-lens-failure.txt`).
- ExplorerCanvas suites (`ExplorerCanvas.test.js`,
  `ExplorerCanvas.evaluative-lens.test.js`, `ExplorerCanvasDrillDown.test.js`,
  `ExplorerCanvas-performance.test.js`): 172/172 with zero unhandled
  errors. A naive re-apply of the checkpoint fix produced 3 unhandled
  `RangeError: Maximum call stack size exceeded` from the unbounded sync
  re-frame chain; the depth-capped scheduler eliminates them.
- Full web suite (`cd web && npm test`): 65 files, 1630 passed, 41 skipped,
  exit 0.
