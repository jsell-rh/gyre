# TASK-034: Explorer Agent Self-Check Dry-Run Loop

**Spec reference:** `explorer-implementation.md` §Self-Check Loop  
**Depends on:** None (MCP tools `graph_summary` and `graph_query_dryrun` already exist)  
**Progress:** `not-started`

## Spec Excerpt

From `explorer-implementation.md` §Self-Check Loop:

> The server extracts `<view_query>` blocks from the agent's output, runs the dry-run, and feeds the result back as a tool response. The agent can then refine.
>
> ```
> Agent output: "Let me check this query..."
>   → <view_query>{ ... }</view_query>
>   → Server extracts, runs dry-run
>   → Server injects tool result: { warnings: ["Group matched 47 nodes"] }
>   → Agent: "Too broad. Let me use more specific names..."
>   → <view_query>{ ... refined ... }</view_query>
>   → Server dry-runs again
>   → No warnings
>   → Agent: "Here's what I found: ..."
>   → <view_query>{ ... final ... }</view_query>
>   → Server sends to frontend
> ```
>
> The server caps this at 3 refinement turns. If the agent hasn't resolved warnings by turn 3, send the best version anyway.

From §Agent System Prompt:

> 4. Call graph_query_dryrun to check the query
> 5. If there are warnings (too many matches, unresolved nodes, etc.),
>    refine the query and dry-run again (max 3 refinements)
> 6. When satisfied, output the view query in a `<view_query>` block

## Current State

- `explorer_ws.rs` handles WebSocket messages and spawns the Claude Agent SDK.
- The agent has access to `graph_query_dryrun` MCP tool.
- No `<view_query>` block extraction logic exists in the WebSocket handler.
- No refinement loop — the agent's first output is sent directly to the frontend.
- No cap on refinement turns.

## Implementation Plan

1. In `explorer_ws.rs`, after receiving agent output:
   - Scan the agent's text output for `<view_query>{ ... }</view_query>` blocks (regex or simple string search)
   - If found, parse the JSON inside the block

2. Run dry-run validation:
   - Call the same `resolve_view_query_dryrun` function used by the `graph_query_dryrun` MCP tool
   - Check `DryRunResult.warnings` for issues

3. Implement refinement loop:
   - If warnings exist and refinement count < 3:
     - Increment refinement counter
     - Send `{ "type": "status", "status": "refining" }` to the frontend
     - Feed the dry-run result back to the agent as context for the next turn
     - Wait for the agent's refined output
   - If no warnings or refinement count >= 3:
     - Extract the final view query
     - Send `{ "type": "view_query", "query": {...} }` to the frontend
     - Send `{ "type": "status", "status": "ready" }` to the frontend

4. Track the "best" view query across refinements:
   - If a later refinement has more warnings than an earlier one, keep the earlier version
   - On turn 3, send the version with the fewest warnings

5. Add integration test:
   - Mock agent output with a `<view_query>` block that has warnings
   - Verify refinement loop runs and sends the refined query

## Acceptance Criteria

- [ ] Server extracts `<view_query>` blocks from agent output
- [ ] Extracted query is validated via dry-run
- [ ] If warnings exist, agent receives feedback and can refine (up to 3 turns)
- [ ] After 3 turns or no warnings, the best query is sent to the frontend
- [ ] Status messages ("refining", "ready") are sent during the loop
- [ ] Integration test verifies the refinement flow

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `crates/gyre-server/src/explorer_ws.rs` for the current WebSocket handler
3. Read `crates/gyre-domain/src/view_query_resolver.rs` for `resolve_view_query_dryrun` and `DryRunResult`
4. Read `crates/gyre-server/src/mcp.rs` for how the `graph_query_dryrun` tool works
5. The `<view_query>` extraction should be a simple regex: `<view_query>(.*?)</view_query>` (dotall mode)
6. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

_(none yet)_
