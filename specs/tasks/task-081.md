---
title: "Explorer Canvas — Causal Flow Trace (Structural Lens Feature)"
spec_ref: "explorer-canvas.md §6"
depends_on:
  - task-072
  - task-065
progress: not-started
coverage_sections:
  - "explorer-canvas.md §6 Causal Flow Trace (Structural Lens Feature)"
commits: []
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
