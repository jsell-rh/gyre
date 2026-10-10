---
title: "Graph Summary & Dry-Run MCP Tools"
spec_ref: "explorer-implementation.md §9, §22–23"
depends_on:
  - task-062
progress: ready-for-review
coverage_sections:
  - "explorer-implementation.md §9 MCP Tools Available to the Agent"
  - "explorer-implementation.md §22 Graph Summary MCP Tool"
  - "explorer-implementation.md §23 Dry-Run MCP Tool"
commits: ["8cdde883b3bee190feee0d7effaa1483bf769272", "fb9893c2ba72120b64bd2d6a58d734f43a38a71b", "a212bc9a74de37e5385ad44f1fc3f571166d7b90", "2b2d923cbe53c6238c5f1b795f3d19193352b341", "9255a5762e01a4d858352f4db0ceafd9e15feffc", "1607616acc94607f01d1a6a21892759b99c553f6", "e194f1c8271c11372a6b3c5651c80f9afe9fa4ce", "7e3a2b05145b6294f130d65819449af6a4bc4b28", "d55bb16ff9e1d9becb80fd1b1856d5c294713076"]
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

## Shipped

All five §9 explorer agent tools (`graph_summary`, `graph_query_dryrun`,
`graph_nodes`, `graph_edges`, `search`) are callable over the real MCP
protocol path (JSON-RPC `tools/call` → `build_router` →
`AuthenticatedAgent` extractor → dispatch in `crates/gyre-server/src/mcp.rs`),
with the explorer SDK script allowlisting the matching `mcp__gyre__*` names
and Bearer-token `mcpServers.gyre` config.

- `graph_summary` (`mcp.rs` `handle_graph_summary` →
  `gyre_domain::view_query_resolver::compute_graph_summary`) returns all six
  §22 spec fields: `node_counts`, `edge_counts`, `top_types_by_fields`,
  `top_functions_by_calls`, `modules`, `test_coverage`. `test_coverage` is a
  real multi-source BFS from `test_node == true` nodes over outgoing `Calls`
  edges (soft-deleted edges excluded via `build_adjacency`), proven against a
  real graph store (`mcp_graph_summary_tool_call` asserts
  test_functions=1, reachable=2, unreachable=1).
- `graph_query_dryrun` (`handle_graph_query_dryrun` →
  `view_query_resolver::dry_run`) deserializes a real `ViewQuery`, resolves
  scope, groups, callouts, and narrative, returns the spec's
  `{"query": …, "result": {DryRunResult}}` envelope, and generates all three
  warning classes with tested boundary semantics: empty scope ("Scope matched
  0 nodes"), >200-node cluttered scope (200 no-warn / 201 warns), >20-node
  too-broad groups, and unresolved callouts (exact→prefix→substring callout
  resolution with a precision warning on substring).
- `search` is a domain-level ranked full-text search (`search_graph_nodes`:
  name/qualified_name/file_path/spec_path/doc_comment, soft-deleted excluded,
  char-boundary-safe truncation) shared by both the MCP handler and the
  inline explorer fallback.
- Read-only tools are outside the `needs_write` RBAC gate (Agent role not
  required), matching their handler effects.

Test evidence (2026-10-10, HEAD `4b0baced` on base `c9b0a6f9`; includes the
`8cdde883` clippy/rustfmt repair that closed the failing check-clippy-diff gate
— source is otherwise byte-identical to the reviewed round-2 tree, which
carries the complete verdict in specs/reviews/task-068.md):

- `python3 scripts/check-clippy-diff.py c9b0a6f9` → exit 0 ("changed lines
  clean, 4 Rust files, 1141 existing warnings outside changes"); merge-state
  rerun at HEAD (base HEAD^1) also exit 0.
- `python3 scripts/check-rustfmt-diff.py c9b0a6f9` → exit 0.
- `cargo test -p gyre-domain view_query_resolver` → 124 passed, 0 failed.
- `cargo test -p gyre-server --lib mcp_graph` → 10 passed, 0 failed
  (in-process JSON-RPC through the router, including tools/list registration
  assertions for all five tools via `mcp_tools_list`).
- `cargo test -p gyre-server --lib mcp::` → 77 passed, 0 failed.
- Static gates at HEAD: arch, hierarchy, mcp-write-tools, commit-attribution,
  byte-slice-truncation, abac-route-registry, migration-versions,
  dead-message-kinds, relative-path-defaults, fail-open-ref-resolution,
  mem-port-contracts, fabricated-scope-defaults, lossy-secret-conversion,
  scope-literal-defaults, inert-enforcement, forged-scope-fields,
  forwarded-header-trust, in-memory-state-stores, unbounded-external-http,
  migration-sql-portability, abac-exempt-handlers — all pass; no new
  exemption entries.
- TCP twins (`tests/graph_integration.rs`: `test_mcp_graph_summary`,
  `test_mcp_graph_query_dryrun`, `test_mcp_graph_nodes`,
  `test_mcp_graph_edges`, `test_mcp_graph_search`) cannot run in this
  sandbox: loopback `accept()` is seccomp-blocked (errno 95, recorded in
  `/tmp/stage/review-evidence/task-068-attempt523ba30e-sandbox.txt`; the twin
  panics with `hyper IncompleteMessage` because `axum::serve` never accepts).
  Host verification / exact-head GitHub CI must run these five twins.

## Agent Instructions

Read `specs/system/explorer-implementation.md` §9, §22–23. Then audit:
- `crates/gyre-server/src/explorer_ws.rs` — look for `graph_summary` and `graph_query_dryrun` function implementations (these are inline tool handlers called during the agent loop)
- `crates/gyre-server/src/mcp.rs` — verify MCP tool registration for all 5 tools

The graph_summary and dry-run functions may already exist inline in explorer_ws.rs. The key question is whether they're ALSO registered as proper MCP tools in mcp.rs (so the Claude Agent SDK subprocess can call them over MCP HTTP). If they're only inline tool handlers, you need to expose them as MCP tools too.

For test_coverage computation in graph_summary: BFS from all nodes where `test_node == true` following outgoing `Calls` edges. Count reachable vs unreachable non-test nodes.
