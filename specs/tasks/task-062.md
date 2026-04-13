---
title: "View Query Grammar — Core Type Definitions & Computed Reference Resolver"
spec_ref: "view-query-grammar.md §1–3"
depends_on: []
progress: not-started
coverage_sections:
  - "view-query-grammar.md §1 Problem"
  - "view-query-grammar.md §2 Primitives"
  - "view-query-grammar.md §3 1. Computed References"
commits: []
---

## Spec Excerpt

**§1 Problem:** The explorer canvas can render graphs, overlays, particles, diffs, and semantic zoom. But each visualization mode is hardcoded. There's no way for the LLM to say "show the user THIS subgraph, colored THIS way" and have the explorer render it. The gap: a declarative grammar that both the LLM and the renderer understand.

**§2 Primitives:** The view query grammar consists of seven primitives: Computed References, Scope, Emphasis, Edges, Zoom, Annotation, and Interactive Bindings.

**§3 Computed References:** Graph traversals resolved by the renderer, not by the LLM. The LLM references them; the renderer computes them from the knowledge graph. All computations are deterministic.

Required computed references:
- `$clicked`, `$selected` — interactive node references
- `$callers(node, depth?)`, `$callees(node, depth?)` — call graph traversal
- `$implementors(node)`, `$fields(node)` — type relationship traversal
- `$descendants(node)`, `$ancestors(node)` — containment hierarchy
- `$governed_by(spec_path)` — spec linkage
- `$reachable(node, edge_types[], dir, depth)` — general BFS
- `$where(property, operator, value)` — metric-based selection
- `$test_reachable`, `$test_unreachable` — test coverage analysis
- `$test_fragility(node)` — test path counting
- `$intersect(set_a, set_b)`, `$union(set_a, set_b)`, `$diff(set_a, set_b)` — set operations

## Implementation Plan

### Existing Code

- **Rust types**: `crates/gyre-common/src/view_query.rs` (~1288 lines) — already defines `ViewQuery`, `Scope`, `Emphasis`, `EdgeFilter`, `Zoom`, `Annotation`, `ExplorerClientMessage`, `ExplorerServerMessage`, `SavedViewSummary`.
- **TypeScript types**: `web/src/lib/types/view-query.ts` (168 lines) — mirrors the Rust types.
- **Validator**: `web/src/lib/view-query-validator.js` (355 lines) — validates view query JSON.

### Work Required

1. **Audit Rust types** against the spec's §2–3 definitions. Verify all computed reference strings are documented/handled in the resolver. Check that `ExplorerClientMessage` and `ExplorerServerMessage` cover all message types from the spec.

2. **Implement computed reference resolver** in Rust (likely in `gyre-common` or `gyre-domain`). This is the function that takes a computed reference string (e.g., `$callers("TaskPort", 3)`) and resolves it against the knowledge graph to produce a set of node IDs. The resolver must handle:
   - Parsing computed reference expressions from scope/emphasis/group fields
   - BFS traversal for `$callers`, `$callees`, `$reachable`, `$descendants`, `$ancestors`
   - Set operations: `$intersect`, `$union`, `$diff`
   - Metric filtering: `$where(property, operator, value)`
   - Test coverage: `$test_reachable`, `$test_unreachable`, `$test_fragility`

3. **Add unit tests** for the computed reference resolver covering each reference type.

4. **Verify TypeScript types** match the Rust types. Ensure `view-query.ts` includes all scope variants and emphasis fields from the spec.

## Acceptance Criteria

- [ ] All 16 computed reference functions from §3 are parseable and resolvable
- [ ] Rust `ViewQuery` type includes all seven primitive categories from §2
- [ ] TypeScript `ViewQuery` type mirrors the Rust type
- [ ] Unit tests cover each computed reference: `$callers`, `$callees`, `$implementors`, `$fields`, `$descendants`, `$ancestors`, `$governed_by`, `$reachable`, `$where`, `$test_reachable`, `$test_unreachable`, `$test_fragility`, `$intersect`, `$union`, `$diff`, `$clicked`, `$selected`
- [ ] `cargo test --all` passes
- [ ] `cd web && npm test` passes

## Agent Instructions

Read `specs/system/view-query-grammar.md` §1–3 in full. Then read the existing implementations:
- `crates/gyre-common/src/view_query.rs`
- `web/src/lib/types/view-query.ts`
- `web/src/lib/view-query-validator.js`

Audit the existing types against the spec. The types likely already exist — focus on gaps. The main new work is the **computed reference resolver**: a function that parses expression strings like `$callers("NodeName", 3)` and resolves them against a graph (nodes + edges) to produce a `HashSet<Id>`. Place this in `gyre-common` or `gyre-domain` depending on whether it needs port access (BFS over in-memory node/edge vecs → `gyre-common`; if it calls `GraphPort` → `gyre-domain`).

Write tests in the same crate's `#[cfg(test)]` module. Each computed reference needs at least one test with a small synthetic graph.
