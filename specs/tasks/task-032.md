# TASK-032: Causal Flow Trace Mode

**Spec reference:** `explorer-canvas.md` §Causal Flow Trace, `system-explorer.md` §5  
**Depends on:** TASK-029 (lens toggle — traces use structural lens)  
**Progress:** `not-started`

## Spec Excerpt

From `explorer-canvas.md` §Causal Flow Trace:

> When the human selects "Trace from here" on any node (right-click or toolbar button), the canvas enters **trace mode**:
>
> 1. The selected node highlights as the starting point
> 2. All nodes reachable via outgoing `Calls`/`RoutesTo` edges are highlighted in sequence
> 3. Unreachable nodes dim to 20% opacity
> 4. The trace path is numbered: ① → ② → ③ showing execution order
> 5. Each step is annotated with its governing spec (if any)
> 6. The detail panel shows the full trace as a list, each step clickable

From `system-explorer.md` §5:

> Behavioral views that show how data flows through the system for a specific operation.
> Each step is clickable → navigates to the implementing code, the governing spec, or the agent that last modified it.

**Data requirement**: The knowledge graph must contain `Calls` edges. The Rust LSP extractor (`lsp_call_graph.rs`) already produces these for Rust codebases.

## Current State

- No right-click context menu exists on canvas nodes.
- No "Trace from here" action.
- No path numbering or trace visualization.
- `Calls` edges exist in the graph (from Rust LSP extraction) but are rendered identically to all other edges.
- No BFS traversal from a starting node along specific edge types in the frontend.

## Implementation Plan

1. Add right-click context menu to nodes in `ExplorerCanvas.svelte`:
   - Menu items: "Trace from here", "View spec", "View provenance"
   - Use a simple Svelte popover positioned at click coordinates

2. Implement trace mode:
   - On "Trace from here": run BFS from the selected node along outgoing `Calls` and `RoutesTo` edges
   - The BFS runs client-side using the graph edges already loaded in the canvas
   - Track BFS depth for each reachable node (for numbering)

3. Render trace visualization:
   - Starting node: highlight with a distinct start marker (bold border, start icon)
   - Reachable nodes: numbered labels (①, ②, ③...) based on BFS order
   - Reachable edges: highlighted with directional arrows (thicker, colored)
   - Unreachable nodes: dim to 20% opacity
   - Unreachable edges: dim to 10% opacity

4. Annotate steps with governing spec:
   - For each reachable node, check `spec_path` field
   - Show spec name next to the step number (e.g., "② GreetingService — platform-model.md")

5. Add trace panel to `NodeDetailPanel.svelte`:
   - When trace mode is active, show a "Trace" tab in the detail panel
   - List all steps in BFS order, each clickable to select that node on canvas
   - Show spec linkage, node type, and qualified name per step

6. Add "Exit trace" button/Escape key to leave trace mode and restore normal rendering.

7. Add test for BFS traversal logic and trace state management.

## Acceptance Criteria

- [ ] Right-click on a node shows a context menu with "Trace from here"
- [ ] Clicking "Trace from here" enters trace mode
- [ ] BFS traversal follows outgoing Calls/RoutesTo edges from the selected node
- [ ] Reachable nodes are numbered (①, ②, ③...) in BFS order
- [ ] Unreachable nodes dim to 20% opacity
- [ ] Each step shows its governing spec (if any)
- [ ] Detail panel shows the full trace as a clickable list
- [ ] Clicking a step in the trace list selects that node on canvas
- [ ] Escape or "Exit trace" button returns to normal rendering
- [ ] Test for BFS traversal logic

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `web/src/lib/ExplorerCanvas.svelte` for the current node rendering and click handling
3. Read `web/src/lib/NodeDetailPanel.svelte` for the detail panel tabs
4. Read `crates/gyre-common/src/graph.rs` for `EdgeType::Calls` and `EdgeType::RoutesTo`
5. Implement BFS in a separate utility function (e.g., `web/src/lib/graph-traversal.js`)
6. Use the existing node/edge data already loaded in the canvas (no new API calls needed)
7. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

_(none yet)_
