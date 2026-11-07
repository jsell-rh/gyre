# Review: task-073 — LSP Call Graph — Rust rust-analyzer Integration

**Reviewer:** Independent review (pi runtime)
**Candidate:** e29b94dba74dcbc3ef6420d13780c532609cb3ce (base 4e304e5d; production commit 0ae61233)
**Spec ref:** lsp-call-graph.md §5 "Rust: rust-analyzer" (Option B), §10 Phase 2
**Status:** approved

---

## Scope reviewed

Diff 4e304e5d..e29b94db: new `crates/gyre-adapters/src/call_graph_rust.rs` (native
rust-analyzer LSP client, 1270 lines), `Language::Rust` dispatch in
`crates/gyre-adapters/src/call_graph.rs`, new deps (lsp-types 0.97, syn,
proc-macro2 span-locations, walkdir, url), fake LSP server
(`tests/data/fake-rust-analyzer.py`), pipeline test
`sync_rust_repo_persists_calls_edges_in_graph_store`
(`gyre-server/src/graph_extraction.rs`), deletion of `scripts/rust-callgraph.sh`/`.mjs`,
spec/task-doc updates. Only 0ae61233 touches product code; the remaining commits are
spec-doc-only (attribution check passes).

## Findings

None blocking. The implementation is real and the contract holds under independent
probing:

- **Real LSP subprocess, real queries.** The adapter spawns `rust-analyzer`, performs
  the initialize/initialized handshake (opting into `experimental/serverStatus`, plus
  the legacy `rust-analyzer/workspaceLoaded` signal), didOpens every indexed file,
  sends `textDocument/references` (includeDeclaration=false) at the identifier
  positions of every function Pass 1 could turn into a node (module-level `pub fn` /
  `#[test]` fn — verified to mirror `rust_extractor.rs`'s node policy, including the
  exact-`pub` visibility check and `has_test_attr`), attributes each reference site to
  its innermost enclosing function (module fn, local fn, or impl/trait method), and
  shuts the server down (shutdown → exit → bounded kill). Verified end-to-end against
  a from-source rust-analyzer build (55effab): the gated integration test
  `real_rust_analyzer_extracts_known_call_edges` RAN (no skip) and asserted the exact
  three fixture edges.
- **Qname contract (task-072 F6 failure class).** `module_qname_for_path` mirrors
  `crate_and_module_from_path` + `extract_rs_file` exactly (flat module naming,
  lib/main stem dropout, `crates/<crate>/src` branch, stem fallback) — verified by
  code comparison and end-to-end. The in-repo contract test pins the fallback
  `src/<mod>.rs` layout; this review additionally pinned the `crates/<crate>/src`
  layout (Gyre's real layout) end-to-end with real Pass 1 + real adapter + real
  `resolve_call_edges`: 3 raw edges with correct `mycrate::<module>::<fn>` names,
  exactly 1 resolved to Pass 1 node IDs, method/private edges dropped not mis-linked.
- **Graceful degradation** on every path per the port contract: missing binary →
  warn + empty (tested); binary that exits before speaking LSP → fast EOF + empty
  (tested); rustup shim without the component → rejected by the `--version` probe in
  the test gate (observed: PATH shim exits 1, skip path taken); malformed/unloadable
  workspace → Ok(0 edges) (observed in probing); wedged/slow server → 180s overall
  budget with a 30s query reserve (observed: full-checkout run returned after 185s,
  Ok, no hang).
- **Test strength.** Mutation probe (`ident_character + 1`) made both fake-server
  protocol tests fail on the exact-edge and exact-position assertions while the
  real-binary test alone would have stayed green (rust-analyzer tolerates in-ident
  positions) — the request-log assertions are what guard the LSP position mapping,
  and they work. Source restored and verified bit-identical; review guard clean.
- **Gyre codebase itself** (plan item 5): extraction over `crates/gyre-common` with
  the real binary produced 7 edges, all verified genuine against source
  (`view_spec::<test fn>` → `view_spec::validate_view_spec`); both endpoint classes
  are Pass 1 node candidates.
- **Static checks:** fmt clean on all task-touched files (remaining drift is
  pre-existing at base in untouched files); check-arch.sh OK; byte-slice,
  relative-path, commit-attribution, and unbounded-HTTP checks OK; no `ra_ap_*`
  dependencies; deleted driver scripts have no live references.

## Notes (non-blocking)

1. LSP `character` is computed as a proc-macro2 byte column, documented as equal to
   the UTF-16 column because the prefix before a fn identifier is ASCII keywords.
   A non-ASCII same-line prefix (e.g. a unicode block comment before `pub fn` on the
   same line) would offset the query position and lose that query's edges —
   degradation only, never a wrong edge. Acceptable given rarity; worth hardening if
   ever observed.
2. Full-workspace extraction on Gyre itself needs a release rust-analyzer to fit the
   180s budget (spec §9 assumes release timings, ~20s for a Gyre-size workspace).
   The sandbox could only build a debug binary (1-job builds), under which the run
   is budget-bounded to zero edges — graceful, documented in the task notes. The
   verifier should run one full-checkout extraction with a release rust-analyzer.
3. File indexing (std::fs + syn parse) runs inline on the async runtime rather than
   `spawn_blocking`; it blocks one worker briefly during a background task. No
   correctness impact; cosmetic inconsistency with Pass 1's `spawn_blocking`.
4. `which_on_path` checks `is_file()` without the executable bit; a non-executable
   match fails at spawn and degrades gracefully. Harmless.

## Evidence

`/tmp/stage/review-evidence/task-073/` — README with environment notes, test
transcripts (adapter 6/6 with the real binary, server 26/26 + integration), mutation
probe, crates-layout contract probe, Gyre-code spot check, static-check results.
