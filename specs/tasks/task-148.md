---
title: "Implement gyre_analytics_decide MCP tool and Ralph Loop integration"
spec_ref: "analytics.md §MCP Tool: gyre_analytics_decide"
depends_on:
  - task-147
progress: not-started
coverage_sections:
  - "analytics.md §MCP Tool: gyre_analytics_decide"
  - "analytics.md §Integration with the Ralph Loop"
commits: []
---

## Spec Excerpt

From `analytics.md` §MCP Tool:

Agents can invoke the decision API via the MCP server at `/mcp`:

```json
{
  "tool": "gyre_analytics_decide",
  "arguments": {
    "decision": "feature.promote",
    "context": {
      "flag": "new-diff-viewer",
      "baseline_days": 7
    }
  }
}
```

This is the primary way agent code invokes analytics decisions without constructing raw HTTP calls. The MCP tool is included in the `tools/list` response.

From §Integration with the Ralph Loop:

Analytics close the feedback loop for autonomous development:
1. Workspace orchestrator creates delegation tasks
2. Implementation agent ships code, opens MR
3. MR merges → `mr.merged` event recorded
4. Analytics tracks adoption over N days
5. Repo orchestrator calls `GET /api/v1/analytics/decide?decision=feature.promote`
6. Agent either promotes the flag or rolls back

## Implementation Plan

1. **Add `gyre_analytics_decide` to MCP tools/list** in `crates/gyre-server/src/mcp.rs`:
   - Tool name: `gyre_analytics_decide`
   - Description: "Evaluate a named analytics decision (feature.promote, feature.rollback, gate.effectiveness, agent.performance, budget.headroom, merge_queue.health) or a custom rule DSL"
   - Input schema: `{ decision: string, context: object, custom_rules?: array }`
   - Add to the tools list alongside existing `gyre_analytics_query`

2. **Implement `handle_analytics_decide`** in `mcp.rs`:
   - Parse `decision` and `context` from tool arguments
   - If `custom_rules` present, call the custom decision evaluator (POST /decide/custom logic)
   - Otherwise, call the built-in decision evaluator (GET /decide logic)
   - Return the `DecisionResult` as the tool response

3. **Wire into MCP tool dispatch** — add `"gyre_analytics_decide"` to the tool dispatch match in the MCP handler.

4. **Document Ralph Loop integration pattern:**
   - Add an example to `docs/agent-protocol.md` showing how an orchestrator agent uses `gyre_analytics_decide` to make promote/rollback decisions
   - The integration is a pattern, not code — agents call the MCP tool in their decision-making loop

5. **Tests:**
   - MCP integration test: call `gyre_analytics_decide` via MCP protocol, verify decision result returned
   - MCP integration test: `tools/list` includes `gyre_analytics_decide`
   - MCP integration test: custom rules via MCP tool
   - Unit test: handle_analytics_decide delegates to correct evaluator based on arguments

## Acceptance Criteria

- [ ] `gyre_analytics_decide` appears in MCP `tools/list` response
- [ ] MCP tool delegates to the decision API (built-in and custom)
- [ ] Tool response matches the `DecisionResult` shape
- [ ] Agents can invoke decisions without raw HTTP calls
- [ ] Ralph Loop integration pattern documented
- [ ] Tests pass

## Agent Instructions

- Read `crates/gyre-server/src/mcp.rs` for the MCP tool implementation pattern
- Search for `gyre_analytics_query` in mcp.rs — follow the exact same pattern for `gyre_analytics_decide`
- Read task-147 output for the decision API implementation
- The MCP tool is a thin wrapper around the decision API — do NOT duplicate decision logic
- Read `docs/agent-protocol.md` for documentation conventions
