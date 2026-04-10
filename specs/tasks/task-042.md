# TASK-042: Evaluative Node Detail Tab — Per-Node Span List

**Spec reference:** `explorer-canvas.md` §Evaluative (Interaction)  
**Depends on:** TASK-029 (lens toggle, complete), TASK-033 (evaluative heat map, complete)  
**Progress:** `needs-revision`  
**Review:** [specs/reviews/task-042.md](../reviews/task-042.md)

## Spec Excerpt

From `explorer-canvas.md` §Evaluative:

> **Interaction**:
> - Click a node → detail panel adds an "Evaluative" tab showing all spans that touched this node, sorted by duration
> - Click a particle → detail panel shows the span: operation name, duration, status, attributes, input/output summary

## Current State

- `NodeDetailPanel.svelte` shows node details (type view, trait view, endpoint view) but has NO evaluative tab.
- Evaluative stats (p50, p95, error rate) are shown in **hover tooltips** on the canvas (`ExplorerCanvas.svelte` lines 5188-5202), not in the detail panel.
- Aggregate metrics (span count, error rate, mean duration) appear as **node badges** on the canvas (`ExplorerCanvas.svelte` lines 3332-3365).
- The trace span data is loaded and available client-side, but there is no UI to list individual spans per node.

The spec requires a dedicated **tab** in the detail panel that shows **all individual spans** touching a node, sorted by duration — not just aggregate stats.

## Implementation Plan

1. In `NodeDetailPanel.svelte`, add an "Evaluative" tab that appears when the evaluative lens is active:
   - Tab label: "Evaluative" or "Spans"
   - Tab should be conditionally visible: only when `lens === 'evaluative'` AND trace data exists

2. Tab content — per-node span list:
   - Filter loaded trace spans to those whose `node_id` or code location matches the selected node
   - Sort spans by duration (descending — slowest first)
   - Each row shows: operation name, duration (formatted), status (OK/ERROR), timestamp
   - Clicking a row selects that span and highlights it on the canvas

3. Aggregate summary at the top of the tab:
   - Total span count, mean duration, p50, p95, error rate
   - Reuse the stats already computed for hover tooltips (`nodeSpanStats`)

4. Span detail expansion:
   - Clicking a span row expands to show: attributes, input/output summary (if available)
   - This maps to the spec's "Click a particle → detail panel shows the span"

5. Add component test:
   - Mock trace data with multiple spans for a node
   - Verify tab appears when evaluative lens active
   - Verify spans are sorted by duration
   - Verify clicking a span selects it

## Acceptance Criteria

- [ ] "Evaluative" tab appears in NodeDetailPanel when evaluative lens is active
- [ ] Tab shows all spans that touched the selected node
- [ ] Spans are sorted by duration (descending)
- [ ] Each span row shows operation name, duration, status
- [ ] Clicking a span row highlights it on the canvas
- [ ] Aggregate stats (p50, p95, error rate) shown at top of tab
- [ ] Tab is hidden when structural or observable lens is active
- [ ] Component test verifies tab behavior

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `web/src/lib/NodeDetailPanel.svelte` for the existing tab structure
3. Read `web/src/lib/ExplorerCanvas.svelte` lines 5188-5202 for the existing `nodeSpanStats` computation (reuse it)
4. Read `web/src/lib/ExplorerCanvas.svelte` lines 746-759 for how trace spans are mapped to nodes (`traceEdgeFrequency`)
5. The trace data structure comes from `GET /api/v1/merge-requests/:id/trace` — check `GateTrace` type in `gyre-common`
6. Follow the existing tab pattern in NodeDetailPanel (Svelte 5, $state, design system CSS)
7. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- feat(web): add evaluative tab to NodeDetailPanel (TASK-042)
