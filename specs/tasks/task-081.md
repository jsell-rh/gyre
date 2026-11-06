---
title: "Explorer Canvas — Causal Flow Trace (Structural Lens Feature)"
spec_ref: "explorer-canvas.md §6"
depends_on:
  - task-072
  - task-065
progress: ready-for-review
coverage_sections:
  - "explorer-canvas.md §6 Causal Flow Trace (Structural Lens Feature)"
commits: ["67005b8905fa1c5da5d089b1215972f4effb0c86"]
---

## Spec Excerpt

When the user selects "Trace from here" on any node (right-click or toolbar button), the canvas enters **trace mode**:

1. The selected node highlights as the starting point
2. All nodes reachable via outgoing `Calls`/`RoutesTo` edges are highlighted in sequence
3. Unreachable nodes dim to 20% opacity
4. The trace path is numbered: ① → ② → ③ showing execution order
5. Each step is annotated with its governing spec (if any)
6. The detail panel shows the full trace as a list, each step clickable

This is `system-explorer.md` §5 — "What happens when an agent pushes code?" answered visually.

**Data requirement:** The knowledge graph must contain `Calls` edges. This requires the LSP call graph extraction pipeline (task-072).

## Implementation Plan

1. **Add "Trace from here" to the canvas context menu** in `ExplorerCanvas.svelte`:
   - Right-click on a node → context menu item "Trace from here"
   - Also add a toolbar button that activates trace mode for the selected node

2. **Implement BFS/DFS trace computation:**
   - From the starting node, follow outgoing `Calls` and `RoutesTo` edges
   - Build an ordered list of reachable nodes (BFS for breadth-first ordering, which better represents execution flow)
   - Track the path depth for each node
   - This can run client-side using the graph data already loaded, or server-side via a new MCP tool/API endpoint

3. **Render trace mode on the canvas:**
   - Starting node: bold border, numbered ①
   - Reachable nodes: highlighted with numbered labels ②③④... in BFS order
   - Trace edges: thickened and colored (e.g., orange) to show the flow path
   - Unreachable nodes: dim to 20% opacity
   - Unreachable edges: dim to 10% opacity

4. **Spec annotation on trace steps:**
   - For each node in the trace, check for `GovernedBy` edges
   - If a governing spec exists, show it as a small badge on the node (e.g., "platform-model.md")
   - In the detail panel, list the spec for each step

5. **Detail panel trace view:**
   - When trace mode is active, the detail panel shows a numbered list of trace steps
   - Each step shows: number, node name, node type, governing spec (if any)
   - Clicking a step pans/zooms the canvas to that node

6. **Exit trace mode:**
   - Click "Clear trace" button in toolbar
   - Press `Escape`
   - Right-click → "Clear trace"
   - Select a different node's "Trace from here" (replaces current trace)

## Acceptance Criteria

- [ ] Right-click context menu has "Trace from here" option
- [ ] Toolbar button activates trace for selected node
- [ ] BFS traversal via `Calls`/`RoutesTo` edges produces ordered trace
- [ ] Trace nodes numbered ①②③... in execution order
- [ ] Unreachable nodes dimmed to 20% opacity
- [ ] Trace edges highlighted (thickened, colored)
- [ ] Governing specs shown as badges on trace nodes
- [ ] Detail panel shows clickable trace step list
- [ ] Escape/Clear button exits trace mode
- [ ] Graceful handling when no `Calls` edges exist (show warning: "Call graph not available — run code sync to extract call edges")
- [ ] Frontend tests for trace mode activation and rendering
- [ ] `npm test` passes in `web/`

## Agent Instructions

Read `specs/system/explorer-canvas.md` §6 (Causal Flow Trace) for the full spec. This depends on task-072 (LSP call graph pipeline for `Calls` edges) and task-065 (ExplorerCanvas component). The trace computation should work with whatever edges are available — if `Calls` edges don't exist yet, show a warning but don't crash. The canvas component is at `web/src/lib/ExplorerCanvas.svelte`. Check how the existing graph data is structured (nodes and edges arrays) to understand how to traverse outgoing edges. Use `edge.edge_type === 'calls' || edge.edge_type === 'routes_to'` for filtering.

## Shipped

Causal Flow Trace (explorer-canvas.md §6) is implemented end-to-end on the real production surface (ExplorerCanvas via ExplorerView), in product commit 09580636 (recovered checkpoint; content verified identical at the current branch head after merge with base 7f8b5d9c). Coverage matrix row §6 is `implemented`. This round re-verified the behavior and repaired the task-document contract: the prior session had checked the acceptance boxes (protected task text); the executor restored the assigned contract and the boxes are left unchecked per that contract — completion is attested here, in the coverage matrix, and by the tests, not by editing protected checkboxes.

### Behavior

- **Traversal** — pure module `web/src/lib/causal-trace.js`: `computeTraceOrder` runs BFS over outgoing `calls`/`routes_to` edges from the start node (alphabetical sibling tie-break for determinism, cycle-safe via visited set, ignores non-causal edge types and dangling endpoints). The canvas `$effect` consumes it, restricted to the query-matched subgraph so step numbers agree with the dimming.
- **Activation** — right-click "Trace from here" is a primary context-menu item, and a toolbar **Trace** button activates the trace for the selected node (disabled with a hint tooltip when nothing is selected). Both go through one shared `startTraceFromNode`.
- **No call graph** — when the loaded graph has zero `calls`/`routes_to` edges, activating a trace shows a dismissible `role="alert"` banner with the exact spec text "Call graph not available — run code sync to extract call edges" instead of entering a degenerate empty trace.
- **Rendering** — numbered step badges (①②③…) in BFS order; thick red trace-path arrows with edge-type labels; unreachable nodes dim to 20% (`dim_unmatched: 0.2` on the trace query; unmatched edges get the query filter's ×0.1 factor). Also fixed a real bug found en route: the trace `$effect` never requested a redraw, so the overlay only appeared on the next unrelated pan/zoom — it now schedules one (untracked).
- **Spec annotation** — `governingSpecFor` resolves each step's governing spec from the `spec_path` field *or* its `GovernedBy` edge target; shown as a badge beside the step number on the canvas and per-step in the detail list.
- **Detail panel** — NodeDetailPanel renders the numbered, clickable step list (number, name, type, governing spec). Clicking a step pans/zooms the canvas to that node: `onTraceStepNavigate` → ExplorerView `focusRequest {nodeId, nonce}` → ExplorerCanvas `zoomToNode` (external camera-control prop; layout-tracked so early requests re-fire once layout settles).
- **Exit** — toolbar "Clear trace" button (replaces the Trace button while a trace is active), `Escape` anywhere on the page (window-level listener, inputs excluded), right-click "Clear trace", and starting a new trace replaces the current one.

### Test evidence

- `web/src/__tests__/causal-trace.test.js` — 21 tests on the pure traversal: BFS-vs-DFS ordering kill test (diamond graph a→b/c→d/e numbers b=2,c=3,d=4,e=5), `routes_to` traversal, non-causal edges excluded, deterministic sibling ordering, cycles terminate, unknown/leaf roots, governed_by spec resolution, step-list construction.
- `web/src/__tests__/ExplorerCanvasTrace.test.js` — 18 component tests: toolbar/right-click activation issues the exact causal query (focus/outgoing/calls+routes_to/`_trace`/dim 0.2); disabled without selection; both warning paths show the exact spec message and issue no query; "Clear trace" emits `null`; window Escape exits trace mode (and does not clear non-trace queries); step badges ①–⑤ drawn; unreachable node dims to exactly 0.2× its untraced opacity; trace-overlay edge labels drawn in trace red; `focusRequest` selects + zooms; NodeDetailPanel step list renders and clicks dispatch `onTraceStepNavigate`.
- Re-verified this round at branch head (after base merge, no code changes needed — product files content-identical to 09580636): focused probe `vitest run causal-trace.test.js ExplorerCanvasTrace.test.js ExplorerCanvas.test.js NodeDetailPanel.test.js` → **4 files, 249/249 passed, 0 failed** (log: `/tmp/stage/review-evidence/task-081-focused-vitest.log`, exit 0).

### Notes for review

- Dimming semantics: "dim to 20%" composes with the semantic-zoom LOD fade (unmatched opacity = LOD × 0.2; matched nodes are boosted to full opacity by the query composition). The test pins the 0.2 factor against a no-query baseline of the same fixture.
- The trace suite uses a queued-rAF mock (matching real `requestAnimationFrame` semantics) because a synchronous rAF mock inverts `animLoop`'s `animFrame = requestAnimationFrame(animLoop)` handshake, silently dropping later redraw requests.
- The `zoom: 'fit'` directive on the trace query is unchanged from the pre-existing context-menu behavior.
