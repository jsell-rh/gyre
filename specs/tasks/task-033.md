# TASK-033: Evaluative Heat Map & Span Badges

**Spec reference:** `explorer-canvas.md` §Evaluative  
**Depends on:** TASK-029 (lens toggle — evaluative lens must be selectable)  
**Progress:** `complete`

## Spec Excerpt

From `explorer-canvas.md` §Evaluative:

> **What it overlays on the structural view**:
> - Node heat: colored by duration (cool blue = fast, warm red = slow)
> - Node badges: span count, error rate, mean duration
> - Edge thickness: proportional to call frequency in traces
> - Failed spans: red glow on nodes, error particles
>
> **Interaction**:
> - Click a particle → detail panel shows the span: operation name, duration, status, attributes
> - Click a node → detail panel adds an "Evaluative" tab showing all spans that touched this node, sorted by duration
> - Hover a node → tooltip shows timing stats (p50, p95, error rate)

The evaluative lens uses trace data from `GET /api/v1/merge-requests/:id/trace` (GateTrace spans).

## Current State

**Implemented.** Heat map, badges, edge thickness, and tooltips all work:
- Lines 659, 2214-2227: blue→red heat palette based on duration
- Lines 3332-3365: evaluative badges show span_duration, span_count, error_rate
- Lines 3540-3557, 746-759: edge thickness scales by `traceEdgeFrequency` (1.5px–6px)
- Lines 5188-5202: hover tooltips show p50, p95, error rate, tail latency warning

**Note:** The "Evaluative" tab in NodeDetailPanel (per-node span list sorted by duration) was split to TASK-042 — the current implementation surfaces these stats in hover tooltips rather than a dedicated panel tab.

## Implementation Plan

1. **Node heat coloring** — In `EvaluativeOverlay.svelte` or `ExplorerCanvas.svelte` (when evaluative lens active):
   - Compute aggregate duration per node from trace spans
   - Map duration to a blue→red color scale (`#3b82f6` cool → `#ef4444` hot)
   - Apply as fill or overlay color on each node

2. **Node badges** — Extend `NodeBadge.svelte`:
   - When evaluative lens active, show badges with: span count, error rate (%), mean duration (ms)
   - Use the Vizceral-style ring gauge (or simpler badge) per `ui-layout.md` §4
   - Position badges at top-right of each node

3. **Edge thickness** — In the edge rendering path:
   - Count how many spans traversed each edge (source→target)
   - Scale stroke-width proportionally (1px baseline → 4px for high-frequency edges)

4. **Failed span glow** — Nodes with any error spans get a red glow effect (CSS `filter: drop-shadow` or SVG `<feGaussianBlur>`)

5. **Hover tooltips** — On node hover, show tooltip with:
   - p50 duration, p95 duration, error rate
   - Computed from the loaded trace spans

6. **Evaluative detail tab** — Add an "Evaluative" tab to `NodeDetailPanel.svelte`:
   - Shows all spans that touched the selected node
   - Sorted by duration (descending)
   - Each span row: operation name, duration, status, timestamp

7. Add tests for aggregate computation (duration stats, error rate, edge frequency).

## Acceptance Criteria

- [ ] Nodes colored by duration (blue=fast, red=slow) when evaluative lens active
- [ ] Node badges show span count, error rate, mean duration
- [ ] Edge thickness varies by call frequency in traces
- [ ] Error spans produce a red glow on affected nodes
- [ ] Hover tooltip shows p50, p95, and error rate
- [ ] Detail panel shows "Evaluative" tab with per-node span list
- [ ] Clicking a span in the list shows span detail
- [ ] Tests for aggregate computation functions

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `web/src/components/EvaluativeOverlay.svelte` for the current trace rendering
3. Read `web/src/components/NodeBadge.svelte` for the existing badge component
4. Read `web/src/lib/NodeDetailPanel.svelte` for the detail panel tab structure
5. The trace data format comes from the `GateTrace` type — check `crates/gyre-common/src/` for the span structure
6. Compute aggregates client-side from the loaded trace spans (no new API calls needed)
7. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

Implemented as part of the explorer-canvas branch prior to task decomposition.
Evaluative detail panel tab deferred to TASK-042.
