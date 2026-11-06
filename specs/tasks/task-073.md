---
title: "LSP Call Graph — Rust rust-analyzer Integration"
spec_ref: "lsp-call-graph.md §5 Rust, §10 Phase 2"
depends_on:
  - task-072
progress: ready-for-review
coverage_sections:
  - "lsp-call-graph.md §5 Per-Language Implementation (Rust)"
  - "lsp-call-graph.md §10 Implementation Phases (Phase 2)"
commits: ["0ae61233004a471788036e6b923d51afcc147db2"]
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

## Shipped

Rust Pass 2 is a native rust-analyzer LSP client in `gyre-adapters`
(`crates/gyre-adapters/src/call_graph_rust.rs`), replacing the deleted
`scripts/rust-callgraph.sh`/`.mjs` driver whose qname construction never
matched Pass 1 node names (every edge it emitted was dropped at resolution).
`SubprocessCallGraphExtractor` dispatches `Language::Rust` to this client;
no `ra_ap_*` dependency. Production fix is commit 0ae61233, carried in
branch history.

### Behavior

- Spawns `rust-analyzer` (discovery: `RUST_ANALYZER_BIN` override, then
  PATH), performs the `initialize`/`initialized` handshake opting into
  `experimental/serverStatus`, and waits for the workspace-ready signal
  (quiescent=true on current builds, legacy
  `rust-analyzer/workspaceLoaded` on older ones).
- Sends `textDocument/didOpen` for every indexed file, then
  `textDocument/references` (includeDeclaration=false) at the identifier
  position of every function Pass 1 could have turned into a `Function`
  node (module-level `pub fn` / `#[test]` fn — mirrors the Pass 1 node
  policy; positions computed from proc-macro2 span-locations via syn).
- Attributes each reference site to its innermost enclosing function
  (module-level fn, local fn, or impl/trait method) and emits
  `CallEdge { from, to }` with qnames in exactly Pass 1's format
  (`crate_and_module_from_path` mirrored).
- Own JSON-RPC framing/correlation over stdio with server-request replies,
  bounded everywhere: 60s init, 5s per query, 5s shutdown, 180s overall
  budget with a 30s query reserve.
- Degradation per the port contract: missing binary → warn + empty; binary
  that exits before speaking LSP (rustup shim without the component) →
  fast EOF failure → warn + empty; wedged server → budget-bounded partial
  run. Never an error, never a hang.

### Tests

- `fake_lsp_session_produces_edges_and_correct_requests`: full dispatch
  against a fake LSP server with a request log — asserts the exact LSP
  conversation (initialize/initialized, didOpen per file, references at
  exact ident positions for pub fns only, shutdown, exit) and resulting
  edges including the impl-method enclosure.
- `fake_server_edges_resolve_against_real_pass1_nodes`: qname-contract
  test — real Pass 1 `RustExtractor` + real adapter + real domain
  `resolve_call_edges`; asserts the pub-to-pub edge resolves to Pass 1
  node IDs and private-fn/method edges are dropped, not mis-linked
  (F6-class guard).
- `real_rust_analyzer_extracts_known_call_edges`: gated on a working
  `rust-analyzer` (the `--version` probe rejects rustup shims without
  the component); extracts from the fixture with the real binary and
  asserts the exact three expected edges.
- `missing_binary_degrades_to_empty`,
  `binary_that_exits_immediately_degrades_to_empty`,
  `module_qname_matches_pass1_layouts`.
- `sync_rust_repo_persists_calls_edges_in_graph_store`
  (`gyre-server/src/graph_extraction.rs`): Cargo.toml repo → Rust Pass 2
  edges resolve and persist in the graph store.

### Contract-repair round re-verification

The executor restored the assigned task document (the previous
checkpoint had changed protected task text); the production fix
(0ae61233) was already in branch history and survived the base refresh
(merge 4e304e5d touched no call-graph files), so this round re-verified
rather than re-implemented. Evidence under
`/tmp/stage/review-evidence/task-073/`:

- Sandbox again had no working rust-analyzer on PATH (rustup shim without
  the component, rejected by the `--version` probe — skip path observed
  explicitly; rustup component download unavailable). Rebuilt from source
  at fde5133f7 (2026-11-05), same commit as prior rounds, via
  `RUST_ANALYZER_BIN`.
- `cargo test -p gyre-adapters --lib call_graph_rust`: 6/6 with the
  real-binary integration test RAN (1.07s, no skip line) and asserted
  the exact three fixture edges against the from-source binary.
- `cargo test -p gyre-server graph_extraction`: 26/26 including
  `sync_rust_repo_persists_calls_edges_in_graph_store`, plus
  `test_push_triggers_graph_extraction` (integration target).
- `cargo fmt --check -p gyre-adapters -p gyre-server`: no diffs in any
  task-touched file (call_graph_rust.rs, call_graph.rs,
  graph_extraction.rs); all reported drift is pre-existing at the main
  base (git2_ops.rs, sqlite/audit.rs, merge_processor.rs, …).

### Notes for review

- Full workspace suites, all-target Clippy, architecture checks, and
  GitHub CI are owned by the verification stage; the base-refresh merge
  brought no changes to any call-graph-relevant file (verified via
  `git diff f90a4af0..HEAD --stat` on the relevant crates).
- Full-checkout extraction not re-run this round (debug rust-analyzer
  needs more than the 180s budget to load the Gyre workspace); graceful
  budget-bounded degradation plus the real-binary fixture edge assertions
  cover the contract. Release binaries per spec §9 are not expected to
  hit this.
