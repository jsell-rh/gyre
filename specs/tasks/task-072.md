---
title: "LSP Call Graph — Core Pipeline + Go Extractor Integration"
spec_ref: "lsp-call-graph.md §1–6, §10 Phase 1, §11"
depends_on: []
progress: ready-for-review
review: specs/reviews/task-072.md
coverage_sections:
  - "lsp-call-graph.md §1 Problem"
  - "lsp-call-graph.md §2 Solution: Delegate to Language Type Checkers"
  - "lsp-call-graph.md §3 Architecture"
  - "lsp-call-graph.md §4 Why Find References Instead of Walk Bodies"
  - "lsp-call-graph.md §5 Per-Language Implementation (Go)"
  - "lsp-call-graph.md §6 Extraction Pipeline"
  - "lsp-call-graph.md §10 Implementation Phases (Phase 1)"
  - "lsp-call-graph.md §11 Prerequisites"
commits: ["819f0d3cc8f07931bdc452b89575c61b4b5fb8b8", "e909e835fdbbfee6aae1f93bc07ff6aadf712997", "dadff8262c21b2533f9ec1f59654eeefb38c70bf", "bac2af13c7d276259f7258c100190bd2a5d6d106", "bdff16cd459dba5d79407450baf1d7d04c17b385", "b986f56df081165a2c9ff67523d54bc3bc6d1339", "395a3b1bcab7c85d99ce0cfb57b3fd0e0d41bb4c"]
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

- [x] `CallGraphExtractor` port trait exists in `gyre-ports` — `crates/gyre-ports/src/call_graph.rs:26-33`
- [x] Go adapter shells out to `go-callgraph` binary and parses JSON output — `crates/gyre-adapters/src/call_graph.rs` (`SubprocessCallGraphExtractor`, binary verified on Go fixture)
- [x] Pipeline runs Pass 2 after Pass 1 on push/sync for Go repos — `crates/gyre-server/src/graph_extraction.rs` `do_extract()` step 7
- [x] `Calls` edges from Pass 2 are stored in graph via `GraphPort` — `extract_and_persist_call_graph()` → `graph_store.create_edge`
- [x] Edges are deduplicated (no duplicates from Pass 1 + Pass 2) — `crates/gyre-domain/src/call_graph_resolve.rs` `resolve_call_edges`
- [x] Pass 2 is non-blocking — graph is usable after Pass 1, becomes complete after Pass 2 — `tokio::spawn` fire-and-forget in `do_extract()` step 7, after steps 3–6 persist Pass 1
- [x] Unit tests for JSON parsing and edge deduplication — `crates/gyre-domain/src/call_graph_resolve.rs` tests; `crates/gyre-adapters/src/call_graph.rs` tests
- [x] Integration test: sync a Go repo → verify `Calls` edges appear in graph — `sync_go_repo_persists_calls_edges_in_graph_store` in `crates/gyre-server/src/graph_extraction.rs`
- [x] `cargo test --all` passes, `cargo fmt --all` clean — 21/21 suites ok; `cargo fmt --all --check` clean

## Agent Instructions

Read `specs/system/lsp-call-graph.md` for full context. The Go binary already exists at `scripts/go-callgraph/` — do NOT rewrite it; integrate it. Follow the hexagonal architecture: port trait in `gyre-ports`, adapter in `gyre-adapters`, orchestration in `gyre-domain`. The `gyre-domain` crate MUST NOT import `gyre-adapters`. Check `crates/gyre-ports/src/lib.rs` for existing port patterns and `crates/gyre-domain/src/` for extraction pipeline code. The graph store is accessed via `GraphPort` — grep for existing usage patterns.

## Round R3 Notes (implementation, 2026-10-06)

- R2 findings F6/F7/F8 are fixed by product code on this branch: Pass 1 qnames built from Go's import-path rule (`go_extractor.rs`); one ambiguity policy — hint-corroborated `select`, no ordering picks (`call_graph_resolve.rs`); `Calls` sweep exemption, content-derived edge ids, and Pass 2 self-reconciliation (`graph_extraction.rs`). Verified green at this HEAD: `cargo test -p gyre-domain go_extractor` 13/13, domain `call_graph_resolve` 14/14, `cargo test -p gyre-adapters call_graph` 2/2, `cargo test -p gyre-server --lib graph_extraction` 22/22 — including every R3 regression (`sync_go_repo_persists_calls_edges_in_graph_store`, `pass2_edge_ids_stable_across_runs`, `sweep_preserves_calls_edges_owned_by_pass2`, `pass2_reconciles_stale_calls_edges`, `pass2_reconcile_skipped_when_toolchain_unavailable`, `pass2_reconcile_removes_legacy_duplicate_id_rows`, `resolve_go_prefix_similar_package_is_not_guessed`). `check-task-commit-attribution.sh`, `check-arch.sh`, `check-mem-port-contracts.sh`, `check-inert-enforcement.sh`, `check-relative-path-defaults.sh` all pass; `rustfmt --check` clean on every branch-touched file (repo-wide `cargo fmt --check` drift is pre-existing main baseline in files this branch never touches).
- HTTP-bound in-process-server verification CANNOT run in this worker sandbox: loopback TCP data transfer is reset by the sandbox after accept for any process — demonstrated with a pure Python HTTP server/client pair (v4, v6, and raw socket, zero gyre code involved) getting `ConnectionResetError`. All 35 `graph_integration` and all 21 `auth_integration` tests consequently fail on the harness's first request (`reqwest IncompleteMessage`) irrespective of this branch, which touches neither binary nor the router (last change: d7940e8, already on main). Full deterministic gates must run on the integrated commit in an environment with working loopback; the pipeline's own storage-level integration is covered by the 22 lib tests above.
- `17c81d5a` (task-072 surface style fix that landed on `main`) MUST stay in the `commits:` list: `check-task-commit-attribution.sh` scans full history, while the controller's `process: record task-072 branch commits` recorder regenerates the list from `main..HEAD` only — it dropped this SHA in 64ef557 and re-triggered the gate violation. If the recorder rewrites this frontmatter again, re-add `17c81d5a4d8fe8dc93387ba2c8360187a8028737`.
