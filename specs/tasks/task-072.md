---
title: "LSP Call Graph — Core Pipeline + Go Extractor Integration"
spec_ref: "lsp-call-graph.md §1–6, §10 Phase 1, §11"
depends_on: []
progress: not-started
coverage_sections:
  - "lsp-call-graph.md §1 Problem"
  - "lsp-call-graph.md §2 Solution: Delegate to Language Type Checkers"
  - "lsp-call-graph.md §3 Architecture"
  - "lsp-call-graph.md §4 Why Find References Instead of Walk Bodies"
  - "lsp-call-graph.md §5 Per-Language Implementation (Go)"
  - "lsp-call-graph.md §6 Extraction Pipeline"
  - "lsp-call-graph.md §10 Implementation Phases (Phase 1)"
  - "lsp-call-graph.md §11 Prerequisites"
commits: []
---

## Spec Excerpt

The current extractors emit `Contains`, `Implements`, and basic `Calls` edges via syntax analysis (tree-sitter/syn). Only ~3% of actual calls resolve because cross-module calls, trait dispatch, generics, re-exports, and dynamic dispatch all require type information.

**Solution:** Delegate to language type checkers via a two-pass pipeline:
- **Pass 1 (existing):** tree-sitter/syn extracts declarations (nodes) — types, functions, interfaces, endpoints, modules + `Contains`, `Implements` edges. Fast (milliseconds).
- **Pass 2 (new):** LSP/type-checker extracts complete `Calls` edges. For each function/method node, ask "find all references to this definition", resolve enclosing function at each reference site → emit `Calls` edge. Slower (seconds) but complete.

**Go implementation:** A Go binary `gyre-go-callgraph` already exists at `scripts/go-callgraph/`. It uses `golang.org/x/tools/go/callgraph/cha` (Class Hierarchy Analysis) to compute the complete call graph in one function call and outputs JSON: `[{"from": "pkg.FuncName", "to": "pkg.OtherFunc"}, ...]`.

**Prerequisites:** Language toolchains must be available on the extraction host. For Go: Go toolchain.

## Implementation Plan

1. **Verify existing Go binary** (`scripts/go-callgraph/main.go`): ensure it compiles, runs on a sample Go project, and produces valid JSON output matching the spec format.

2. **Define the `CallGraphExtractor` port** in `gyre-ports`:
   ```rust
   pub trait CallGraphExtractor: Send + Sync {
       async fn extract_call_edges(&self, repo_path: &Path, language: Language) -> Result<Vec<CallEdge>>;
   }
   ```

3. **Implement Go adapter** in `gyre-adapters`: shell out to `scripts/go-callgraph/go-callgraph <repo-path>`, parse the JSON output, map qualified names to existing graph node IDs.

4. **Integrate into extraction pipeline** in `gyre-domain`: after Pass 1 (syntax extraction), schedule Pass 2 (semantic extraction) as a background task. Pass 2 calls the `CallGraphExtractor` port, then merges the resulting `Calls` edges into the graph store via `GraphPort`, deduplicating against Pass 1 edges.

5. **Register the pipeline** in the server's sync/push handler so Pass 2 runs automatically after every graph sync.

6. **Test with the `scripts/go-callgraph/` binary** on a real Go project (e.g., the `e2e-repo` if it contains Go, or a test fixture).

## Acceptance Criteria

- [ ] `CallGraphExtractor` port trait exists in `gyre-ports`
- [ ] Go adapter shells out to `go-callgraph` binary and parses JSON output
- [ ] Pipeline runs Pass 2 after Pass 1 on push/sync for Go repos
- [ ] `Calls` edges from Pass 2 are stored in graph via `GraphPort`
- [ ] Edges are deduplicated (no duplicates from Pass 1 + Pass 2)
- [ ] Pass 2 is non-blocking — graph is usable after Pass 1, becomes complete after Pass 2
- [ ] Unit tests for JSON parsing and edge deduplication
- [ ] Integration test: sync a Go repo → verify `Calls` edges appear in graph
- [ ] `cargo test --all` passes, `cargo fmt --all` clean

## Agent Instructions

Read `specs/system/lsp-call-graph.md` for full context. The Go binary already exists at `scripts/go-callgraph/` — do NOT rewrite it; integrate it. Follow the hexagonal architecture: port trait in `gyre-ports`, adapter in `gyre-adapters`, orchestration in `gyre-domain`. The `gyre-domain` crate MUST NOT import `gyre-adapters`. Check `crates/gyre-ports/src/lib.rs` for existing port patterns and `crates/gyre-domain/src/` for extraction pipeline code. The graph store is accessed via `GraphPort` — grep for existing usage patterns.
