---
title: "LSP Call Graph — Rust rust-analyzer Integration"
spec_ref: "lsp-call-graph.md §5 Rust, §10 Phase 2"
depends_on:
  - task-072
progress: not-started
coverage_sections:
  - "lsp-call-graph.md §5 Per-Language Implementation (Rust)"
  - "lsp-call-graph.md §10 Implementation Phases (Phase 2)"
commits: []
---

## Spec Excerpt

Use `rust-analyzer` as an LSP subprocess to extract complete `Calls` edges for Rust projects. For each function/method node in the graph (from Pass 1), send `textDocument/references` requests to rust-analyzer. For each reference, resolve the enclosing function → emit a `Calls` edge.

**Approach (LSP subprocess, per spec recommendation):**
- Start `rust-analyzer` in LSP mode
- Send `textDocument/references` requests for each function definition
- For each reference site, resolve the enclosing function
- Emit `Calls` edges
- Shut down the server

**Tradeoffs:** LSP subprocess is slower (IPC per request) but has no dependency on `ra_ap_*` crates (~50 crates). Spec recommends starting with LSP subprocess, migrating to library if performance matters.

**Prerequisites:** `rust-analyzer` binary must be available.

This catches: trait method calls through dynamic dispatch, generic instantiations, re-exported symbols, closure captures, macro-generated calls.

## Implementation Plan

1. **Implement Rust adapter** in `gyre-adapters` for the `CallGraphExtractor` port (defined in task-072):
   - Spawn `rust-analyzer` as an LSP subprocess
   - Initialize the LSP connection (initialize request, initialized notification)
   - Open workspace root via `textDocument/didOpen` for relevant files
   - For each function/method node from Pass 1 graph:
     - Send `textDocument/references` request with the node's file path + position
     - Collect reference locations
     - For each reference, determine the enclosing function (via `textDocument/documentSymbol` or offset-based lookup)
     - Emit `CallEdge { from: enclosing_function_id, to: target_function_id }`
   - Shutdown the LSP server

2. **Map LSP positions to graph nodes:** Use file path + line/column from Pass 1 nodes to construct LSP positions. Use qualified names from reference results to map back to graph node IDs.

3. **Handle edge cases:**
   - Trait impl method references (may return references in multiple impl blocks)
   - Macro-generated code (rust-analyzer may report synthetic positions)
   - Re-exports (references may resolve through `pub use` chains)
   - Missing `rust-analyzer` binary → graceful fallback (log warning, skip Pass 2)

4. **Register the Rust extractor** in the extraction pipeline (alongside Go from task-072) so it's selected based on detected project language (presence of `Cargo.toml`).

5. **Test with the Gyre codebase itself** — this is a Rust project, so we can validate call edges against known call relationships.

## Acceptance Criteria

- [ ] Rust adapter for `CallGraphExtractor` implemented in `gyre-adapters`
- [ ] Spawns `rust-analyzer` as LSP subprocess, sends `textDocument/references` queries
- [ ] Correctly maps LSP reference results to graph `Calls` edges
- [ ] Handles missing `rust-analyzer` binary gracefully (warning, skip)
- [ ] Pipeline selects Rust extractor for repos with `Cargo.toml`
- [ ] Integration test: extract call graph from a Rust test fixture → verify `Calls` edges
- [ ] `cargo test --all` passes, `cargo fmt --all` clean

## Agent Instructions

Read `specs/system/lsp-call-graph.md` §5 "Rust: rust-analyzer" for the full spec. This task depends on task-072 for the `CallGraphExtractor` port trait and pipeline integration. Implement the adapter in `gyre-adapters` following existing adapter patterns. The LSP protocol is well-documented — use `lsp-types` crate for type definitions. Check if `rust-analyzer` is in PATH before spawning; if not, log a warning and return an empty edge set. Do NOT add `ra_ap_*` crates as dependencies — use the subprocess approach.
