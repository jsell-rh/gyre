---
title: "Implement gyre_analytics_decide MCP tool and Ralph Loop integration"
spec_ref: "analytics.md §MCP Tool: gyre_analytics_decide"
depends_on:
  - task-147
progress: ready-for-review
coverage_sections:
  - "analytics.md §MCP Tool: gyre_analytics_decide"
  - "analytics.md §Integration with the Ralph Loop"
commits: ["c103a38b4ffd830795540d8c6f11d3fd6d85e803"]
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

## Shipped

`gyre_analytics_decide` MCP tool (analytics.md §MCP Tool) plus the Ralph
Loop integration pattern documentation (§Integration with the Ralph
Loop), in commit c103a38b4ffd.

### Behavior

- **tools/list** exposes `gyre_analytics_decide` with the spec's input
  schema (`decision` required; `context` object; optional `custom_rules`
  array) and the spec's description naming all six built-in decisions.
- **tools/call dispatch** routes `gyre_analytics_decide` to
  `handle_analytics_decide`. It is a read-only tool (queries the
  analytics/budget/merge-queue stores; no repository writes), so it is
  correctly absent from the `needs_write` RBAC gate — verified by
  `scripts/check-mcp-write-tools.sh`.
- **Thin wrapper, no duplicated logic**: the HTTP `decide` /
  `decide_custom` handler bodies were extracted into shared core
  functions `api::analytics::evaluate_builtin_decision` and
  `api::analytics::evaluate_custom_decision`. The HTTP routes and the
  MCP handler call the same cores, so MCP calls get identical caller-
  scope validation (`validate_decision_scope` — cross-tenant context
  scopes rejected), event fetching, budget/merge-queue reads, and
  `DecisionResult` response shapes as the HTTP API.
- **Delegation contract**: without `custom_rules` the built-in evaluator
  runs (unknown names rejected with the built-in list); with
  `custom_rules` the tool assembles a `CustomDecisionSpec` (name from
  `decision`, `context`, `default` arg honored) and runs the custom rule
  DSL evaluator (first match wins, default otherwise). Invalid DSL is
  rejected naming the field.
- **Token scoping**: `validate_tool_scope` gained a `gyre_analytics_decide`
  arm — `context.workspace_id`/`context.repo_id` are scope-addressable
  args (the events a decision is computed over), checked against the
  caller's token tier exactly like `gyre_search`'s workspace arg.
- **Ralph Loop pattern** documented in `docs/agent-protocol.md` (§Analytics
  Decisions in the Ralph Loop): the six-step loop with built-in
  `feature.promote` and custom-rule MCP invocation examples; steps 5–6
  are agent decisions via the MCP tool, per the spec (pattern, not code).

### Test evidence

All tests are in `crates/gyre-server/src/mcp.rs` (tests module) and
`api/analytics.rs` (existing decide suites, unchanged semantics — the
HTTP handlers now delegate to the extracted cores):

- `mcp_tools_list_includes_analytics_decide` — tools/list entry + schema.
- `mcp_analytics_decide_builtin_via_protocol` — full MCP protocol call
  over real seeded adoption events; asserts the DecisionResult shape
  (decision/promote/confidence/metrics/reason/evaluated_at), mirroring
  the HTTP `decide_feature_promote_from_real_event_data`.
- `mcp_analytics_decide_custom_rules_via_protocol` — custom DSL via MCP:
  default outcome on no match, first-match wins, matched_rule index.
- `mcp_analytics_decide_unknown_decision_is_tool_error` /
  `mcp_analytics_decide_invalid_custom_rule_is_tool_error` — evaluator
  rejections surface as MCP tool errors naming the cause.
- `handle_analytics_decide_delegates_by_custom_rules_presence` — unit
  test pinning the delegation contract (same decision name: built-in
  path rejects it as unknown, custom path evaluates it).

`cargo check -p gyre-server --lib --tests` and `cargo clippy -p gyre-server
--lib --tests` pass with no warnings in the changed code; `cargo fmt`
clean; `scripts/check-arch.sh` and `check-mcp-write-tools.sh` pass. Full
gyre-server test compilation/run is deferred to the independent verifier
(sandbox memory budget) — execute:

    cargo test -p gyre-server --lib mcp::tests::mcp_tools_list_includes_analytics_decide
    cargo test -p gyre-server --lib mcp::tests::mcp_analytics_decide
    cargo test -p gyre-server --lib mcp::tests::handle_analytics_decide
    cargo test -p gyre-server --lib api::analytics::tests::decide
    cargo test -p gyre-server --lib mcp::tests::
