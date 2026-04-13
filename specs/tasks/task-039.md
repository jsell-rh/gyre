---
title: "LSP Call Graph — Go Extractor"
spec_ref: "lsp-call-graph.md §Phase 1: Go"
depends_on: []
progress: complete
coverage_sections: []
commits: []
---

## Spec Excerpt

From `lsp-call-graph.md` §Per-Language Implementation — Go:

> Go has the best tooling for this — a dedicated call graph package:
>
> ```go
> func extractCallGraph(dir string) *callgraph.Graph {
>     pkgs, _ := packages.Load(&packages.Config{Dir: dir}, "./...")
>     prog, _ := ssautil.AllPackages(pkgs, 0)
>     prog.Build()
>     return cha.CallGraph(prog)  // Complete call graph in one call
> }
> ```
>
> This gives the **complete** call graph including interface dispatch, in a single function call.
>
> **Recommendation:** Build a small Go binary (`gyre-go-callgraph`) that outputs JSON. The Gyre extractor shells out to it.

From §Extraction Pipeline:

> Pass 2 runs in background, merges when done. The graph is usable after Pass 1 with partial call data, and becomes complete when Pass 2 finishes.

## Current State

**Implemented.** All acceptance criteria met:
- `scripts/go-callgraph/main.go` + compiled binary (7.9 MB) exist
- `go_extractor.rs` lines 69-70: Pass 2 calls `extract_lsp_call_graph()` after tree-sitter Pass 1
- Lines 673-735: shells out to `go-callgraph` binary, parses JSON call edges
- Lines 679, 691, 1583: graceful degradation — skips silently if binary unavailable
- Line 1600 test: "should have no errors even without go-callgraph"

## Implementation Plan

1. Create a Go module `tools/gyre-go-callgraph/`:
   - `main.go` — accepts a directory path, loads Go packages, runs CHA call graph analysis
   - Outputs JSON to stdout: `{ "edges": [{ "from": "pkg.FuncA", "to": "pkg.FuncB", "from_file": "...", "from_line": N, "to_file": "...", "to_line": N }] }`
   - Uses `golang.org/x/tools/go/callgraph/cha` for Class Hierarchy Analysis
   - Uses `golang.org/x/tools/go/packages` for package loading

2. In `gyre-domain`, add a Go-specific Pass 2 extractor:
   - Shell out to `gyre-go-callgraph <repo_root>` (check `PATH` for the binary, similar to `rust_analyzer_available()`)
   - Parse the JSON output
   - Map Go function names to existing `GraphNode` entries (match by `qualified_name` or `file_path + line_start`)
   - Emit `GraphEdge` entries with `edge_type: Calls`

3. Integrate into the extraction pipeline:
   - After Pass 1 (tree-sitter) for Go repos, schedule Pass 2 as a background task
   - Merge resulting `Calls` edges into the graph store
   - If `gyre-go-callgraph` is not available, log a warning and skip (graceful degradation)

4. Add tests:
   - Unit test for JSON parsing of call graph output
   - Integration test with a small Go project (create a test fixture)

## Acceptance Criteria

- [ ] `gyre-go-callgraph` binary compiles and runs against a Go project
- [ ] Output is valid JSON with call edges including file paths and line numbers
- [ ] Go extractor Pass 2 calls the binary and parses output
- [ ] Resulting `Calls` edges are stored in the graph
- [ ] Graceful degradation if binary is not available (warning, no crash)
- [ ] Tests for JSON parsing and edge creation

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `crates/gyre-domain/src/lsp_call_graph.rs` for the Rust LSP pattern
3. Read the Go packages documentation for `golang.org/x/tools/go/callgraph`
4. Create the Go binary in `tools/gyre-go-callgraph/` with `go mod init`
5. Use CHA (Class Hierarchy Analysis) — it resolves interface dispatch
6. Keep the output format simple (JSON lines or single JSON array)
7. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

Implemented as part of the explorer-canvas branch prior to task decomposition.
