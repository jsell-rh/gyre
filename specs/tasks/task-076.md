---
title: "LSP Call Graph — Edge Types, LLM Integration & Performance"
spec_ref: "lsp-call-graph.md §7–9, §12"
depends_on:
  - task-072
progress: not-started
coverage_sections:
  - "lsp-call-graph.md §7 Edge Types Produced"
  - "lsp-call-graph.md §8 LLM Integration"
  - "lsp-call-graph.md §9 Performance Considerations"
  - "lsp-call-graph.md §12 Relationship to Other Specs"
commits: []
---

## Spec Excerpt

**Edge Types Produced (Pass 2):**
| Edge | Source | Coverage |
|---|---|---|
| `Calls` | Type checker (find-references) | 100% |

Pass 1 edges (`Contains`, `Implements`, `DependsOn`, `FieldOf`, `Returns`, `RoutesTo`, `GovernedBy`) remain unchanged.

**LLM Integration:** With complete call edges, the LLM can answer:
- **Blast radius:** "If I change `TaskPort::find_by_id`, what breaks?" → Follow `Implements` to implementors → Follow `Calls` to all callers (23 call sites) → Follow `GovernedBy` to affected specs.
- **Coupling analysis:** Count incoming/outgoing `Calls` edges per module → "Low coupling — safe to extract."
- **Dead code detection:** Functions with zero incoming `Calls` edges (excluding entry points with `RoutesTo`).
- **Spec change impact:** Follow `GovernedBy` → `Calls` outward → identify affected types and functions.

**Performance targets:**
- Rust workspace (~1800 nodes): ~20 seconds total
- Python project (~5300 nodes): ~30 seconds total
- Go project (~2000 nodes): ~2-5 seconds total
- TypeScript project (~3000 nodes): ~30 seconds total

All acceptable for background jobs on push/sync.

## Implementation Plan

1. **Verify edge type consistency:** Ensure Pass 2 `Calls` edges use the same `EdgeType::Calls` variant as Pass 1. The `EdgeType` enum in `gyre-common` should already have this variant — verify and add if missing.

2. **Update the `graph_summary` MCP tool** to include call graph completeness metrics:
   - `call_edges_pass1`: count of `Calls` edges from syntax analysis
   - `call_edges_pass2`: count of `Calls` edges from LSP analysis
   - `call_graph_complete`: boolean (Pass 2 has run)

3. **Update the `graph_query_dryrun` MCP tool** to leverage complete `Calls` edges for blast-radius queries:
   - The `scope.type: "focus"` with `edges: ["calls"]` should now return complete results
   - Add a warning if Pass 2 hasn't run yet: "Call graph incomplete — blast radius may be underreported"

4. **Add LLM prompt context** about call graph completeness to the explorer agent system prompt (in `explorer_ws.rs`): when the graph has complete call data, the agent can confidently answer blast-radius and coupling questions.

5. **Performance benchmarking:** Add a timing metric to the extraction pipeline that logs Pass 2 duration. Compare against the spec targets. If significantly slower, investigate (e.g., batch LSP requests, limit concurrent file opens).

6. **Update relationship documentation:** Verify cross-spec references are accurate in the code (comments linking to `system-explorer.md`, `realized-model.md`, etc.).

## Acceptance Criteria

- [ ] `EdgeType::Calls` is consistent between Pass 1 and Pass 2
- [ ] `graph_summary` MCP tool reports call graph completeness
- [ ] `graph_query_dryrun` warns when call graph is incomplete
- [ ] Explorer agent system prompt references call graph availability
- [ ] Pass 2 duration is logged as a metric
- [ ] Unit tests for updated MCP tool responses
- [ ] `cargo test --all` passes, `cargo fmt --all` clean

## Agent Instructions

Read `specs/system/lsp-call-graph.md` §7–9 and §12. This task depends on task-072 for the core pipeline. Focus on the integration points: MCP tools (`graph_summary`, `graph_query_dryrun` in `crates/gyre-server/src/mcp.rs` and `crates/gyre-domain/src/view_query_resolver.rs`), the explorer agent system prompt (`crates/gyre-server/src/explorer_ws.rs`), and graph edge types (`crates/gyre-common/`). The performance work is benchmarking/logging, not optimization — measure first, optimize only if targets are missed.
