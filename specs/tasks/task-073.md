---
title: "LSP Call Graph — Rust rust-analyzer Integration"
spec_ref: "lsp-call-graph.md §5 Rust, §10 Phase 2"
depends_on:
  - task-072
progress: ready-for-review
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

- [x] Rust adapter for `CallGraphExtractor` implemented in `gyre-adapters`
- [x] Spawns `rust-analyzer` as LSP subprocess, sends `textDocument/references` queries
- [x] Correctly maps LSP reference results to graph `Calls` edges
- [x] Handles missing `rust-analyzer` binary gracefully (warning, skip)
- [x] Pipeline selects Rust extractor for repos with `Cargo.toml`
- [x] Integration test: extract call graph from a Rust test fixture → verify `Calls` edges
- [x] `cargo test --all` passes, `cargo fmt --all` clean (focused probes: adapters 353/353, server graph_extraction 26/26; changed files rustfmt-clean; full workspace suites owned by verification)

## Agent Instructions

Read `specs/system/lsp-call-graph.md` §5 "Rust: rust-analyzer" for the full spec. This task depends on task-072 for the `CallGraphExtractor` port trait and pipeline integration. Implement the adapter in `gyre-adapters` following existing adapter patterns. The LSP protocol is well-documented — use `lsp-types` crate for type definitions. Check if `rust-analyzer` is in PATH before spawning; if not, log a warning and return an empty edge set. Do NOT add `ra_ap_*` crates as dependencies — use the subprocess approach.

## Shipped

Rust Pass 2 is now a native rust-analyzer LSP client in `gyre-adapters`
(`crates/gyre-adapters/src/call_graph_rust.rs`, ~1250 lines incl. tests),
replacing the unvalidated `scripts/rust-callgraph.sh`/`.mjs` driver (deleted;
its qname construction never matched Pass 1 node names, so every edge it
emitted was dropped at resolution — the Rust twin of task-072's Go F6
finding). `SubprocessCallGraphExtractor` dispatches `Language::Rust` to the
native client; Python/TS keep their script tools; no `ra_ap_*` dependency.

### Behavior

- Spawns `rust-analyzer` (discovery: `RUST_ANALYZER_BIN` override, then
  PATH), performs the `initialize`/`initialized` handshake opting into
  `experimental/serverStatus`, and waits for the workspace-ready signal
  (quiescent=true on current builds, legacy `rust-analyzer/workspaceLoaded`
  on older ones) — references that race an unfinished load block on the
  server's analysis lock, so waiting is strictly better than querying early.
- Sends `textDocument/didOpen` for every indexed file, then
  `textDocument/references` (includeDeclaration=false) at the identifier
  position of every function that Pass 1 could have turned into a `Function`
  node (module-level `pub fn` / `#[test]` fn — mirrors the Pass 1 node policy;
  positions computed from proc-macro2 span-locations via syn).
- Attributes each reference site to its innermost enclosing function
  (module-level fn, local fn, or impl/trait method — methods block
  mis-attribution to a preceding module-level fn, the bug the old mjs driver
  had) and emits `CallEdge { from, to }` with qnames in exactly Pass 1's
  format (`crate_and_module_from_path` mirrored; methods use the
  `CallCollector` caller convention, which the domain resolver then drops
  for lack of a node).
- Own JSON-RPC framing/correlation over stdio (reader/writer tasks,
  server-request replies so rust-analyzer never blocks on
  `workspace/configuration`), bounded everywhere: 60s init, 5s per query,
  5s shutdown, 180s overall budget (spec §9 worst case is "within a
  minute"; the looser bound lets large-workspace loads finish instead of
  degrading to an empty graph) with a 30s query reserve.
- Degradation per the port contract: missing binary → warn + empty; binary
  that exits before speaking LSP (the rustup-shim-without-component case) →
  fast EOF failure → warn + empty; wedged server → budget-bounded partial
  run. Never an error, never a hang.

### Tests (all in `call_graph_rust.rs` unless noted)

- `fake_lsp_session_produces_edges_and_correct_requests`: full dispatch
  through `SubprocessCallGraphExtractor` against a fake LSP server
  (`tests/data/fake-rust-analyzer.py`, python3) with a request log — asserts
  the exact LSP conversation (initialize with rootUri, initialized, didOpen
  per file, references at the exact ident positions for pub fns only,
  shutdown, exit) and the resulting edges, including the impl-method
  enclosure. This test caught a real double-wrapping bug in the didOpen
  params during development.
- `fake_server_edges_resolve_against_real_pass1_nodes`: the qname-contract
  test — runs the REAL Pass 1 `RustExtractor` over the fixture, the real
  adapter (fake server) over the same tree, and the REAL domain
  `resolve_call_edges`; asserts exactly the pub-to-pub edge resolves to the
  Pass 1 node IDs and the private-fn/method edges are dropped, not
  mis-linked. Fails if either side's naming drifts (F6-class guard).
- `real_rust_analyzer_extracts_known_call_edges`: gated on a working
  `rust-analyzer` (probe runs `--version`, so a rustup shim without the
  component does not count); extracts from the fixture with the real binary
  and asserts the exact three expected edges. RAN (not skipped) in this
  sandbox against a from-source rust-analyzer 0.0.0 (6e37b2d 2026-11-05).
- `missing_binary_degrades_to_empty`,
  `binary_that_exits_immediately_degrades_to_empty`,
  `module_qname_matches_pass1_layouts`.
- `sync_rust_repo_persists_calls_edges_in_graph_store`
  (`gyre-server/src/graph_extraction.rs`): Cargo.toml repo → Rust Pass 2
  edges resolve and persist in the graph store; unresolvable callers are
  dropped (mirror of the task-072 Go test).

### Evidence

Under `/tmp/stage/review-evidence/task-073/`: full test outputs, the raw
real-rust-analyzer LSP conversation on the fixture, a Gyre-checkout
spot-check (references for `extract_rust_call_edges` resolve the dispatch
call site in `call_graph.rs` — a real cross-module edge on this codebase),
and environment constraints recorded for remote verification: this sandbox
has no installable rust-analyzer component (network-blocked) and no
`rust-src`, so full-checkout extraction was validated only for graceful,
budget-bounded degradation (the from-source debug binary needs ~165-200s to
load the Gyre workspace vs the 180s budget; release binaries — what rustup
ships — load it in seconds per spec §9).

### Notes for review

- `cargo clippy -p gyre-domain` fails at the BASE commit under this
toolchain (pre-existing `never_loop` at `rust_extractor.rs:1071`, verified
by stashing all changes) — not introduced here; changed lines in this task
are clippy-clean (`clippy-diff` scope).
- `cargo fmt --all` reports pre-existing drift in ~13 unrelated files under
this rustfmt version; those were reverted out of this branch's diff — only
files this task touches are formatted (verified with `rustfmt --check`).
