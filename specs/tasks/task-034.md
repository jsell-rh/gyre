---
title: "Explorer Agent Self-Check Dry-Run Loop"
spec_ref: "explorer-implementation.md §Self-Check Loop"
depends_on: []
progress: ready-for-review
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

Audited against the acceptance criteria on this assignment. The loop itself
(extraction, dry-run, feedback, cap, statuses) was already real on both agent
paths; two criteria were nonetheless open and are now closed (see Shipped):

- "best query sent after 3 turns" — both loops sent the *latest* attempt at
  the cap, not the fewest-warnings one → fixed (best-version tracking).
- "integration test verifies the refinement flow" — no test exercised the
  loop → added two WebSocket integration tests plus a scripted mock mode.

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

- [x] Server extracts `<view_query>` blocks from agent output
- [x] Extracted query is validated via dry-run
- [x] If warnings exist, agent receives feedback and can refine (up to 3 turns)
- [x] After 3 turns or no warnings, the best query is sent to the frontend
- [x] Status messages ("refining", "ready") are sent during the loop
- [x] Integration test verifies the refinement flow

## Shipped

The self-check loop itself (extraction, server-side dry-run, synthetic
tool-result feedback, 3-turn cap, "refining"/"ready" statuses) was already
implemented on both agent paths — native LlmPort loop and Claude Agent SDK
loop — in `crates/gyre-server/src/explorer_ws.rs` (via task-070's branch).
This assignment audited it against the acceptance criteria and closed the
two real gaps found:

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

No test previously exercised the refinement loop. Added a scripted mode to
`MockLlmPortFactory` (`scripted_tool_responses`: successive
`complete_with_tools` calls return successive responses, last repeats) and
two WebSocket integration tests in
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

### Test evidence

- `cargo test -p gyre-server --test explorer_ws_integration` — 11 passed
  (incl. the 2 new tests); `--lib explorer_ws` — 43 passed;
  `cargo test -p gyre-adapters --lib llm::mock` — 9 passed (incl. 3 new
  scripted-mode tests); `cargo test -p gyre-server --test graph_integration`
  (mock-LLM consumer regression) — 37 passed.
- Mutation-tested: reverting the best-version tracking makes the cap test
  fail ("got 3 callouts" instead of the best 1); disabling the refinement
  trigger (`actionable_dry_run_warnings` → empty) makes both tests fail.
  Evidence retained in the assignment's review-evidence artifacts.

## Git Commits

- eff64e31d — feat(explorer): task-034 send best view query at refinement cap + integration tests

(The original loop implementation predates task decomposition — see
task-070, which owns coverage section explorer-implementation.md §11.)
