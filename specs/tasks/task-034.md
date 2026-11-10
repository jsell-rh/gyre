---
title: "Explorer Agent Self-Check Dry-Run Loop"
spec_ref: "explorer-implementation.md §Self-Check Loop"
depends_on: []
progress: complete
coverage_sections: []
commits: ["eff64e31dd1d0b3fccb66d7ace299e7f32e42bbe"]
---

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

**Implemented.** All acceptance criteria met:
- `explorer_ws.rs` line 3566: `parse_view_query_from_text()` extracts `<view_query>...</view_query>` blocks
- Line 128-129: `MAX_REFINEMENT_TURNS = 3` caps the loop
- Lines 2843-2950: dry-run validation, actionable warning collection, re-prompting with tool result
- Line 2955, 3068: `send_status(sender, "refining")` during refinement turns
- Lines 3141-3173: final fallback with nudge if agent doesn't produce view_query

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

Implemented as part of the explorer-canvas branch prior to task decomposition.

## Shipped

The self-check loop (extraction, server-side dry-run, synthetic tool-result
feedback, 3-turn cap, "refining"/"ready" statuses) is real production code on
both agent paths in `crates/gyre-server/src/explorer_ws.rs` (native LlmPort
loop and Claude Agent SDK loop; predates task decomposition — task-070 owns
coverage section explorer-implementation.md §11). This assignment audited it
against the acceptance criteria and closed the two real gaps found; this
round is a contract repair that preserves that fix unchanged and restores
the assigned task document (progress notes live only here, under Shipped).

### Best-version tracking at the refinement cap (eff64e31d)

Spec: "If the agent hasn't resolved warnings by turn 3, send the best
version anyway." Both loops previously sent the *latest* attempt at the
cap — a later refinement that regressed (more actionable dry-run warnings)
displaced an earlier, better one. Both loops now track the attempt with the
fewest actionable warnings (`best_view_query: Option<(count, query, text)>`)
and send that version when the cap is reached; ties keep the earlier
attempt. On the clean path (zero warnings) the current query is the best,
so behavior there is unchanged.

### Integration tests for the refinement flow (eff64e31d)

Added a scripted mode to `MockLlmPortFactory` (`scripted_tool_responses`:
successive `complete_with_tools` calls return successive responses, last
repeats) and two WebSocket integration tests in
`crates/gyre-server/tests/explorer_ws_integration.rs` driving the loop
end-to-end against a seeded graph store:

- `explorer_ws_self_check_refinement_flow` — a first attempt with an
  actionable warning (unresolved callout) is NOT forwarded; status
  "refining" is emitted; the dry-run feedback triggers a second LLM call;
  only the refined, warning-free query reaches the frontend; the turn ends
  with terminal "ready".
- `explorer_ws_self_check_sends_best_version_after_cap` — four attempts
  with worsening warning counts run exactly 3 refinement turns; after the
  cap the fewest-warnings (first) attempt is the one forwarded.

### Test evidence (this round, post base-merge)

- `cargo test -p gyre-server --test explorer_ws_integration` — 11 passed,
  0 failed (incl. both tests above); `--lib explorer_ws` — 43 passed;
  `cargo test -p gyre-adapters --lib llm::mock` — 9 passed (incl. 3
  scripted-mode tests). Evidence retained in
  /tmp/stage/review-evidence/task-034-focused-tests.txt.
- The base merge (233664cf8) did not touch the fix: diff over the three
  changed files between the previous checkpoint head and this HEAD is empty.
- Earlier round mutation checks: reverting best-version tracking made the
cap test fail; disabling the refinement trigger made both tests fail.
