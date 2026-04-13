---
title: "ExplorerCanvas — Lens Definitions & Evaluative Overlay"
spec_ref: "explorer-canvas.md §4"
depends_on:
  - task-065
progress: not-started
coverage_sections:
  - "explorer-canvas.md §4 Lens Definitions"
commits: []
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
