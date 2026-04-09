# TASK-043: LSP Call Graph — Rust Extractor

**Spec reference:** `lsp-call-graph.md` §Phase 2: Rust

**Progress:** `complete`

**Commits:**
(implemented in feat/explorer-canvas branch — lsp_call_graph.rs fully built with rust-analyzer LSP subprocess, 22 unit tests, wired into graph_extraction.rs extraction pipeline)

---

## Context

The LSP-powered call graph spec defines a two-pass extraction pipeline. Pass 1 (tree-sitter/syn) extracts declarations and basic edges. Pass 2 delegates to language type checkers for complete `Calls` edge resolution. The spec defines 4 implementation phases:

- Phase 1: Go — **complete** (TASK-039)
- Phase 2: Rust — **this task**
- Phase 3: Python — **complete** (TASK-040)
- Phase 4: TypeScript — **complete** (TASK-041)

Phase 2 (Rust) is described as "most impactful for Gyre itself" since Gyre is a Rust codebase. Without it, the structural lens in the explorer canvas cannot show complete causal flow for the Gyre codebase, and LLM blast-radius queries are incomplete.

The existing `RustExtractor` (`crates/gyre-domain/src/rust_extractor.rs`) already does Pass 1 via `syn` and has a `resolve_calls()` function with import alias tracking and cross-crate resolution. However, this only resolves a fraction of actual calls because it cannot handle trait dispatch, generics, re-exports, or macro-generated calls — exactly the gaps that `rust-analyzer` solves.

## Spec Excerpt

> #### Rust: `rust-analyzer`
>
> Use `rust-analyzer` as a library (the `ra_ap_*` crates) or via LSP:
>
> **Option A: Library (preferred)**
> ```rust
> use ra_ap_ide::{Analysis, FileId, ReferenceSearchResult};
> use ra_ap_project_model::ProjectManifest;
> ```
>
> **Option B: LSP subprocess**
> ```bash
> # Start rust-analyzer in LSP mode, send textDocument/references requests
> ```
>
> **Recommendation:** Start with LSP subprocess (simpler integration, test the approach), migrate to library if performance matters.
>
> **Prerequisites:** `rust-analyzer` binary must be available.

## Requirements

1. **LSP subprocess approach**: Shell out to `rust-analyzer` in LSP mode and send `textDocument/references` requests for each function/method/trait-method node extracted by Pass 1.

2. **Call edge emission**: For each reference found, resolve the enclosing function at the reference site and emit a `Calls` edge from caller to callee. Deduplicate against edges already emitted by the syn-based Pass 1.

3. **Integration with extraction pipeline**: Wire the Rust LSP pass into `graph_extraction.rs` as a background step after the syn-based Pass 1 completes. Pass 2 runs asynchronously — the graph is usable after Pass 1, and becomes complete when Pass 2 finishes.

4. **Graceful degradation**: If `rust-analyzer` is not available (binary not found), log a warning and skip Pass 2. The graph remains usable with Pass 1 data only.

5. **Performance target**: Per the spec, a Rust workspace of ~1800 nodes should complete in ~20 seconds (rust-analyzer loads in ~3-5s, find-references queries ~10ms each).

6. **Tests**: Validate with the Gyre codebase itself. Verify that known cross-crate trait dispatch calls (e.g., `TaskPort::find_by_id` called via `dyn TaskPort` in gyre-domain) are correctly resolved as `Calls` edges.

## Existing Code References

- `crates/gyre-domain/src/rust_extractor.rs` — Pass 1 extractor with `resolve_calls()`
- `crates/gyre-server/src/graph_extraction.rs` — extraction pipeline entry point
- `crates/gyre-common/src/graph.rs` — `GraphNode`, `GraphEdge`, `EdgeType::Calls`
- `crates/gyre-ports/src/graph.rs` — `GraphPort` trait
- TASK-039, TASK-040, TASK-041 — reference implementations for Go, Python, TypeScript LSP extractors

## Agent Instructions

When working on this task:
1. Update this file's progress to `in-progress` when you start.
2. Add commit SHAs to the "Commits" section above as you go.
3. When complete, update progress to `ready-for-review`.
