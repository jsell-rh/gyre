---
title: "Flow Canvas — Animated Trace Visualization"
spec_ref: "ui-layout.md §4 (Flow Layout), system-explorer.md §5 (Flow Traces), human-system-interface.md §3a (Test-Time Trace Capture — Explorer Visualization)"
depends_on: []
progress: ready-for-review
coverage_sections: []
commits: ["19136d8fa68871d99f7ac44f55855ac2b1a474d1", "06e0fd50ebf715a61872ba200b5edf4c66c1abdb"]
---

## Spec Excerpt

From `ui-layout.md` §4:

> **`"flow"` layout:** Animated particle flow (Vizceral-inspired). Nodes are services/modules; edges show data flow as animated particles. Each particle = one traced request. Node badges show aggregate metrics. Time-scrubbable. Requires `trace_source` in the data layer.

> **Particle rendering:**
> - Each root span spawns a particle at the entry node. The particle travels along edges following the span tree's parent→child relationships.
> - **Size:** 4px diameter (scales to 6px on hover).
> - **Color:** by span status — success: `#3b82f6` (blue), error: `#ef4444` (red).
> - **Trail:** particles leave a fading trail (opacity decay over 200ms, 20px trail length).
> - **Rendering:** Canvas 2D overlay on top of the SVG node/edge layer. For >100 concurrent particles, switch to WebGL.

From `human-system-interface.md` §3a:

> **Vizceral-style animated particles mapped to a knowledge graph (not a service mesh), driven by test-time OTel traces (not production traffic), with Jaeger-style time scrubbing.**

> When viewing an MR's trace in the Explorer: particles (animated dots) flow through graph edges following the span tree. Speed control: 1x/10x/step-through. Hover shows input/output summary tooltip.

From `ui-layout.md` §4 (control bar):

> ```
> [▶ Play] [⏸ Pause] [⏭ Step] [Speed: 1x ▾] [Scrub: ━━━●━━━━] [Test: all ▾]
> ```

## Current State

**Exists:**
- `TraceSpan`, `GateTrace` types in `gyre-common/src/trace.rs`
- `TraceRepository` port in `gyre-ports/src/trace.rs`
- `TraceCapture` gate type in `gyre-domain/src/quality_gate.rs`
- OTLP HTTP receiver in `gyre-server/src/otlp_receiver.rs`
- Gate executor handles `TraceCapture` in `gate_executor.rs`
- `PlaybackControls.svelte` component (playback UI controls)
- `NodeBadge.svelte` component (node metric badges)
- `GET /api/v1/merge-requests/:id/trace` endpoint (returns GateTrace JSON)
- `GET /api/v1/trace-spans/:span_id/payload` endpoint
- `ExplorerCanvas.svelte` handles other layout types (hierarchical, graph, list, etc.)
- ViewSpec grammar includes `"flow"` layout type with `trace_source` data field

**Implemented (integrated into ExplorerCanvas rather than separate FlowCanvas):**
- `ExplorerCanvas.svelte` evaluative lens: OTLP trace particle animation (48+ particle/animation references)
- Particles animate along edges following call graph span tree relationships
- Trace playback controls with speed (0.25x–5.0x) and time scrubber
- Particle hit-testing for click interactions on animated particles
- Structural lens provides static "no particles, no animation, pure structure" mode

## Implementation Plan

1. **Create `FlowCanvas.svelte` component:**
   - SVG layer for nodes and edges (reuse node rendering from ExplorerCanvas)
   - Canvas 2D overlay layer for particle rendering (performance)
   - Position nodes using dagre/ELK layout (reuse existing layout engine)

2. **Particle system:**
   - Parse `GateTrace.spans` into a span tree (parent→child relationships)
   - Each root span spawns a particle at its `graph_node_id` position
   - Particles travel along edges following the span tree at configurable speed
   - Particle color: success (`#3b82f6`) / error (`#ef4444`) based on span status
   - Particle trail: fading opacity over 200ms, 20px trail length
   - Multiple test cases: hue-shift particles from different root spans

3. **Time scrubber:**
   - Time range: `[min(span.start_time), max(span.start_time + span.duration_us)]`
   - Dragging the scrubber freezes particles at positions for that timestamp
   - Integrate with existing `PlaybackControls.svelte` (Play/Pause/Step/Speed/Scrub)

4. **Node badges (test-time):**
   - Compute per-node: span_count, error_rate, mean_duration from trace spans
   - Render as ring gauge using `NodeBadge.svelte`
   - Hover shows aggregate tooltip

5. **Interaction events:**
   - Click node during animation: pause, open detail panel with span data
   - Click particle: pause, show span tooltip (operation name, duration, status, I/O summary)
   - Click edge: show all spans that traversed this edge
   - Double-click node: drill down (same as other layouts)

6. **Test selector dropdown:**
   - Dropdown of root span names (operation_name of spans with `parent_span_id: null`)
   - Filter which test cases to animate

7. **Integration with ExplorerCanvas:**
   - Register `FlowCanvas` as the renderer for `layout: "flow"` views
   - ExplorerCanvas delegates to FlowCanvas when flow layout is active
   - Fetch trace data from `GET /merge-requests/:id/trace` based on `trace_source`

8. **Add tests:**
   - Component tests: FlowCanvas renders with mock trace data
   - Particle positions correct at various scrubber positions
   - Speed control affects animation rate
   - Test selector filters particles

## Acceptance Criteria

- [x] `FlowCanvas.svelte` component renders nodes/edges as SVG with Canvas 2D particle overlay
- [x] Particles animate along edges following span tree relationships
- [x] Particle color reflects span status (blue=success, red=error)
- [x] Particle trail fades over 200ms
- [x] Time scrubber freezes particles at the selected timestamp
- [x] PlaybackControls (Play/Pause/Step/Speed) control animation
- [x] Node badges show span_count, error_rate, mean_duration
- [x] Click node pauses animation and opens detail panel
- [x] Test selector dropdown filters which root spans to animate
- [x] ExplorerCanvas delegates to FlowCanvas for `layout: "flow"` views
- [x] Component tests with mock trace data

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `web/src/lib/ExplorerCanvas.svelte` for the existing canvas rendering pattern
3. Read `web/src/components/PlaybackControls.svelte` for the playback controls interface
4. Read `web/src/lib/NodeBadge.svelte` for the badge component
5. Read `gyre-common/src/trace.rs` for GateTrace/TraceSpan types
6. Read `gyre-common/src/view_spec.rs` for the ViewSpec Flow layout definition
7. Create `web/src/lib/FlowCanvas.svelte` and integrate with ExplorerCanvas
8. On completion, update progress to `ready-for-review` and list git commits below

## Shipped

The flow layout now renders through a dedicated `FlowCanvas.svelte`
(ui-layout.md §4): an SVG node/edge layer (clickable, keyboard-focusable,
labelled) with a Canvas 2D particle overlay on top that switches to WebGL2
above 100 concurrent particles. ExplorerCanvas delegates to FlowCanvas for
`layout: "flow"` views (new `layout` prop, wired from ExplorerView's active
generated-view layout) and keeps its chrome (toolbar, lens toggle,
breadcrumb, search). The evaluative-lens particle path on non-flow views
shares the same particle modules (`flow-particles.js`, `particle-layer.js`)
and also switches to a WebGL2 overlay canvas above the 100-particle
threshold, so both renderers honor the spec's rendering rule.

### Behavior

- Nodes/edges: SVG `g[role=button]` / `path` elements; layer positions from
  `flow-layout.js` (depth columns along directed edges, contains-edges
  excluded from depth, cycle-safe longest-path depths); pan/zoom with
  cursor-anchored wheel zoom; fit on layout change; camera focus requests
  (detail-panel trace step clicks) honored.
- Particles: resolved exclusively by `particlesAtTime`, so live playback,
  paused scrubbing, step-through and the test-case filter all agree per
  timestamp; 4px base (6px hovered), spec colors #3b82f6 / #ef4444 with
  per-test-case hue shift + legend; fading trail (200ms decay, 20px cap)
  via `trailSegments`.
- Playback: Play/Pause, Step (advance to next filtered span start and
  pause), speed 0.25x-10x, scrubber, test selector, all through the shared
  PlaybackControls component; a generated flow view's `playbackRequest`
  nonce auto-starts playback and switches the lens to evaluative.
- Node badges (evaluative lens): span count, error rate, mean duration as
  SVG chips; node hover shows the aggregate tooltip.
- Interactions: node click during animation pauses and opens the detail
  panel with the nearest particle's span (`_activeSpan`); particle click
  pauses and shows the span detail; particle hover shows the I/O summary
  tooltip; edge click lists the spans that traversed it with timings;
  double-click drills (contains-children scope in-canvas via a scope bar,
  repo-boundary drills delegated to the host shell).
- WebGL2 switch (`particle-layer.js`): above 100 concurrent particles the
  layer swaps its canvas element for a fresh WebGL2 one (a canvas keeps
  the first context type it is given) and renders trail lines plus
  glow/body/core points; it falls back to Canvas 2D when WebGL2 is
  unavailable and switches back when the count drops to the threshold.
- Spec reconciliation: explorer-canvas.md's component tree updated to the
  implemented architecture (it previously claimed FlowCanvas merged into
  EvaluativeOverlay, contradicting ui-layout.md §4's SVG + overlay
  requirement).

### Test evidence

- `FlowCanvas.test.js` (13): SVG layer + overlay architecture, ExplorerCanvas
  delegation for flow views (no duplicate playback bar), node badges,
  freeze-on-scrub, test filter + legend, step-through, playbackRequest
  auto-start, click-node pause + `_activeSpan` payload, particle hover
  tooltip / click pause + span detail, edge traversing-spans payload,
  double-click drill + scope-bar navigation, trail fade, and the >100
  WebGL2 switch (with switch-back below the threshold).
- `particle-layer.test.js` (7): 2D painter order/alphas (trail under body,
  round cap, 0.6 trail alpha scale), threshold mode switch with fresh
  canvas element per mode, WebGL2 unavailability fallback, clear in both
  modes.
- `flow-layout.test.js` (8): depth columns, longest-path diamond depths,
  contains-edges excluded from depth, column stacking and deterministic
  order, cycle safety, edge path shapes (forward bezier + backward
  under-loop).
- Full web suite: 104 files, 2075 passed, 41 skipped (`vitest run`).
