# Review: task-073 — LSP Call Graph — Rust rust-analyzer Integration

**Reviewer:** Review agent (independent round)
**Candidate:** 4b31bb6fbe8b9c4d8f0d7c190c853dc6e80a17cf
**Base:** 233664cf81cbb1189b2aa00f76e80c8445745955
**Task commits in scope:** 0ae61233 (implementation), 01617175e (E0369 test repair)
**Spec ref:** lsp-call-graph.md §5 Rust, §10 Phase 2
**Status:** approved
**Evidence:** /tmp/stage/review-evidence/task-073/ (probe logs, mutation probe, static verification notes)

---

## Summary

The candidate ships a native rust-analyzer LSP client in
`crates/gyre-adapters/src/call_graph_rust.rs` (~1270 lines incl. tests) implementing the
spec's Option B (LSP subprocess, no `ra_ap_*` dependency): spawn `rust-analyzer`,
initialize handshake with `experimental/serverStatus` opt-in, wait for the
workspace-ready signal (quiescent=true, legacy `rust-analyzer/workspaceLoaded`),
`textDocument/didOpen` per indexed file, `textDocument/references`
(includeDeclaration=false) at the identifier position of every function Pass 1 could
have turned into a `Function` node, innermost-enclosing-function attribution of each
reference site, `CallEdge` emission with qnames in exactly Pass 1's format, and a clean
shutdown/exit. All waits are bounded (60s init, 5s/request, 5s shutdown, 180s overall
with a 30s query reserve); every failure mode degrades to warn + empty/partial `Ok`
per the port contract. The Apr-2026 `scripts/rust-callgraph.sh`/`.mjs` driver (whose
qnames never matched Pass 1 node names, so all its edges were dropped at resolution)
is deleted. `SubprocessCallGraphExtractor` dispatches `Language::Rust` to the new
client; `detect_all_languages` selects Rust on `Cargo.toml`.

## What was verified independently

1. **Real-binary extraction.** rust-analyzer was built from source in the review
   sandbox (tag 2026-09-28, commit 03fcb7724) and
   `real_rust_analyzer_extracts_known_call_edges` run with `RUST_ANALYZER_BIN` set:
   passed in 0.96s with no skip, resolving the fixture's three call sites exactly
   (private-fn caller, pub-fn cross-module caller, impl-method caller).
2. **Non-vacuousness (mutation probe).** The same test with `RUST_ANALYZER_BIN`
   pointed at an LSP-speaking fake returning zero references FAILS with
   `left: []` vs the three expected edges — the test asserts real extraction output.
3. **Protocol conversation.** `fake_lsp_session_produces_edges_and_correct_requests`
   passed; the fake server test double was additionally driven directly over stdio
   (independent Python client) and behaves exactly as the tests assume (initialize
   echo, workspaceLoaded signal, refs keyed `uri|line|char`, shutdown/exit, request
   log).
4. **Qname contract (task-072's F6 failure class).** `module_qname_for_path` was
   compared line-by-line against the domain's `crate_and_module_from_path`, and the
   query-target policy against the ItemVisitor `Item::Fn` arm (pub/#[test] only);
   `fake_server_edges_resolve_against_real_pass1_nodes` runs the real Pass 1
   `RustExtractor` + real `resolve_call_edges` over the adapter's edges and passed.
5. **Graceful degradation.** `missing_binary_degrades_to_empty` and
   `binary_that_exits_immediately_degrades_to_empty` passed; the sandbox's own broken
   rustup shim (component not installed) is correctly rejected by the `--version`
   probe (real-binary test skips rather than extracting garbage). Error-path audit of
   `extract_with_binary` found no reachable `Err` return.
6. **Merge repair.** 01617175e (test-only, +13/-2) adapts the Rust pipeline test to
   the base's `Pass2Outcome` return and pins the §7 `PASS2_EDGE_METADATA` provenance
   tag like the Go/TS siblings; `sync_rust_repo_persists_calls_edges_in_graph_store`
   passes, and the full `graph_extraction` module (28 tests) shows no sibling
   regression. Production files are byte-identical from 0ae61233 through HEAD.
7. **Suites.** `cargo test -p gyre-adapters --lib` 359 passed / 0 failed / 12 ignored;
   `cargo test -p gyre-server --lib graph_extraction` 28/28;
   `cargo fmt --check -p gyre-adapters -p gyre-server` clean. Mechanical checks
   (byte-slice, relative-path, in-memory-state, unbounded-http, commit-attribution,
   fail-open-ref, lossy-secret) all pass.
8. **Coverage honesty.** Coverage rows §5/§10 remain `task-assigned` (Go F6/F7 and TS
   are other tasks' open scope); the Rust entries record the validation without
   closing shared rows — no fake completion.

## Notes (non-blocking)

- The Rust leg's 180s overall budget is looser than spec §9's "~20s estimate / within
  a minute" worst case; the deviation is documented in-code as a deliberate tradeoff
  (avoid empty-graph degradation on large workspaces) and §9 is task-076's coverage
  section. Performance instrumentation remains open there.
- Method-caller qnames (`module::method`) can collide with a same-named module-level
  fn node; this exactly mirrors Pass 1's own `CallCollector` convention (the contract
  this task was required to match) and is a pre-existing architectural limitation of
  the shared qname scheme, not something this task introduced.
- Workspace-wide `cargo test --all`, all-target Clippy, and CI are deferred to the
  verification stage per the review-stage scope rules.

## Verdict

Approved. Every acceptance criterion has independent evidence: the adapter exists and
dispatches, the LSP conversation is real and asserted, reference→`Calls` mapping
verified against real Pass 1 + real resolver and a real rust-analyzer binary, missing
binary degrades gracefully, Cargo.toml selects the Rust extractor, the fixture
integration tests pass, and fmt is clean on the touched crates.
