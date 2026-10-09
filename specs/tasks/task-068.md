---
title: "Graph Summary & Dry-Run MCP Tools"
spec_ref: "explorer-implementation.md §9, §22–23"
depends_on:
  - task-062
progress: not-started
coverage_sections:
  - "explorer-implementation.md §9 MCP Tools Available to the Agent"
  - "explorer-implementation.md §22 Graph Summary MCP Tool"
  - "explorer-implementation.md §23 Dry-Run MCP Tool"
commits: []
---

## Spec Excerpt

**§9 MCP Tools Available to the Agent:**
- `graph_summary` — condensed overview of the repo's knowledge graph (node/edge counts by type, top types by fields, top functions by calls, modules list, test coverage stats)
- `graph_query_dryrun` — takes a view query JSON, resolves it, returns preview (matched_nodes count, matched_node_names, groups_resolved, callouts_resolved/unresolved, narrative_resolved, warnings)
- `graph_nodes` — query specific nodes by ID, name, type, or qualified_name pattern
- `graph_edges` — query edges by source/target node or edge type
- `search` — full-text search across the graph

**§22 Graph Summary MCP Tool:**
```rust
pub async fn graph_summary(state: &AppState, repo_id: &str) -> GraphSummary {
    // Count by type, top types by field count, top functions by incoming calls,
    // test coverage (test_functions count, reachable_from_tests, unreachable)
}
```

Response shape:
```json
{
  "node_counts": { "type": 55, "function": 265, "endpoint": 73, "module": 61, "interface": 7 },
  "edge_counts": { "calls": 129, "contains": 684, "field_of": 216, "depends_on": 5 },
  "top_types_by_fields": ["Space (8 fields)", "AgentRecord (12 fields)"],
  "top_functions_by_calls": ["lifecycleErr.Error (28)", "NewKnowledgeSpace (8)"],
  "modules": ["domain", "coordinator", "db.sqlite", "ports"],
  "test_coverage": { "test_functions": 45, "reachable_from_tests": 180, "unreachable": 85 }
}
```

**§23 Dry-Run MCP Tool:**
```rust
pub async fn graph_query_dryrun(state: &AppState, repo_id: &str, query: ViewQuery) -> DryRunResult {
    // Resolve scope, groups, callouts, narrative. Generate warnings for
    // empty results, too-broad groups (>20 nodes), unresolved callouts.
}
```

Response shape:
```json
{
  "matched_nodes": 14,
  "matched_node_names": ["KnowledgeSpace", "NewKnowledgeSpace", "Server"],
  "groups_resolved": [{ "name": "Tenant Boundary", "matched": 3, "nodes": ["..."] }],
  "callouts_resolved": 2,
  "callouts_unresolved": [],
  "narrative_resolved": 3,
  "warnings": ["Group 'Persistence' matched 47 nodes - too broad"]
}
```

## Implementation Plan

### Existing Code

- `crates/gyre-server/src/mcp.rs` — existing MCP tool infrastructure. Grep for `graph_summary` and `graph_query_dryrun` registrations.
- `crates/gyre-server/src/explorer_ws.rs` — contains `graph_summary` and `graph_query_dryrun` implementations (used by the explorer WebSocket handler directly as tool call handlers for the LLM agent loop).

### Work Required

1. **Audit `graph_summary`**: Verify it returns all fields from the spec — `node_counts`, `edge_counts`, `top_types_by_fields`, `top_functions_by_calls`, `modules`, `test_coverage`. The test coverage computation requires BFS from test nodes via `Calls` edges.

2. **Audit `graph_query_dryrun`**: Verify it:
   - Resolves the scope to matched nodes
   - Resolves groups and reports per-group match counts
   - Resolves callouts and identifies unresolved ones
   - Resolves narrative steps
   - Generates warnings: empty scope → "Scope matched 0 nodes", scope >200 nodes → "may be cluttered", group >20 nodes → "too broad"

3. **Register as MCP tools**: Verify `graph_summary`, `graph_query_dryrun`, `graph_nodes`, `graph_edges`, and `search` are registered in the MCP tool registry (`mcp.rs`) so the Claude Agent SDK can call them via MCP protocol, not just the inline tool-call handler.

4. **Unit tests**: Test graph_summary with a synthetic graph. Test dry-run with various queries and verify warning generation.

## Acceptance Criteria

- [ ] `graph_summary` returns all spec fields: node_counts, edge_counts, top_types_by_fields, top_functions_by_calls, modules, test_coverage
- [ ] `graph_summary` test_coverage correctly counts test functions, reachable (BFS from tests via Calls), unreachable
- [ ] `graph_query_dryrun` resolves scope, groups, callouts, narrative
- [ ] `graph_query_dryrun` generates warning for empty scope (0 nodes)
- [ ] `graph_query_dryrun` generates warning for broad scope (>200 nodes)
- [ ] `graph_query_dryrun` generates warning for broad groups (>20 nodes per group)
- [ ] `graph_query_dryrun` reports unresolved callouts
- [ ] All 5 MCP tools (`graph_summary`, `graph_query_dryrun`, `graph_nodes`, `graph_edges`, `search`) are callable via MCP protocol
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/explorer-implementation.md` §9, §22–23. Then audit:
- `crates/gyre-server/src/explorer_ws.rs` — look for `graph_summary` and `graph_query_dryrun` function implementations (these are inline tool handlers called during the agent loop)
- `crates/gyre-server/src/mcp.rs` — verify MCP tool registration for all 5 tools

The graph_summary and dry-run functions may already exist inline in explorer_ws.rs. The key question is whether they're ALSO registered as proper MCP tools in mcp.rs (so the Claude Agent SDK subprocess can call them over MCP HTTP). If they're only inline tool handlers, you need to expose them as MCP tools too.

For test_coverage computation in graph_summary: BFS from all nodes where `test_node == true` following outgoing `Calls` edges. Count reachable vs unreachable non-test nodes.
