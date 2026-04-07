# TASK-015: Flow Canvas — Animated Trace Visualization

**Spec reference:** `ui-layout.md` §4 (Flow Layout), `system-explorer.md` §5 (Flow Traces), `human-system-interface.md` §3a (Test-Time Trace Capture — Explorer Visualization)
**Depends on:** None (TraceCapture gate, OTLP receiver, trace types, and PlaybackControls already exist)
**Progress:** `complete`

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

- [ ] `FlowCanvas.svelte` component renders nodes/edges as SVG with Canvas 2D particle overlay
- [ ] Particles animate along edges following span tree relationships
- [ ] Particle color reflects span status (blue=success, red=error)
- [ ] Particle trail fades over 200ms
- [ ] Time scrubber freezes particles at the selected timestamp
- [ ] PlaybackControls (Play/Pause/Step/Speed) control animation
- [ ] Node badges show span_count, error_rate, mean_duration
- [ ] Click node pauses animation and opens detail panel
- [ ] Test selector dropdown filters which root spans to animate
- [ ] ExplorerCanvas delegates to FlowCanvas for `layout: "flow"` views
- [ ] Component tests with mock trace data

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

## Git Commits

Implemented as part of HSI milestone PRs (pre-task-creation), integrated into ExplorerCanvas.svelte rather than as separate FlowCanvas.svelte component. Status updated to `complete` by project manager after code verification.
