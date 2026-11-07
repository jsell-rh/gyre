# Review: task-073 — LSP Call Graph — Rust rust-analyzer Integration

**Reviewer:** Independent review (pi runtime), rebase-resolution round
**Candidate:** 52ed495fb2e8aae101bebca8d777b55b41752c82 (base 7da7bbff; production commit 0ae61233)
**Spec ref:** lsp-call-graph.md §5 "Rust: rust-analyzer" (Option B), §10 Phase 2
**Status:** approved

---

## Scope reviewed

Diff 7da7bbff..52ed495fb (what this branch delivers on top of the assigned
base): new `crates/gyre-adapters/src/call_graph_rust.rs` (native
rust-analyzer LSP client), `Language::Rust` dispatch in
`crates/gyre-adapters/src/call_graph.rs`, new deps (lsp-types 0.97, syn,
proc-macro2 span-locations, walkdir, url), fake LSP server
(`tests/data/fake-rust-analyzer.py`), pipeline test
`sync_rust_repo_persists_calls_edges_in_graph_store`, deletion of
`scripts/rust-callgraph.sh`/`.mjs`, spec/task-doc updates. This round is a
base-refresh merge of main (carrying task-074's pyright driver and
task-075-adjacent TS work); the merge conflict resolution in `call_graph.rs`
and the coverage rows were the re-verification focus. A prior round approved
e29b94db; that approval did not carry forward — everything below was re-run
on the merged tree.

## Findings

None blocking.

- **Merge resolution correct.** `call_graph_rust.rs` is byte-identical to
  the previously approved candidate. The candidate-vs-base diff touches
  neither `call_graph_resolve.rs` nor `rust_extractor.rs` (the merge took
  main's versions wholesale; main's changes there are test-only additions
  and an unrelated DependsOn edge simplification). The merged `call_graph.rs`
  dispatches `Language::Rust` to the native client and keeps main's
  Python/TS arms; a mutation probe that disabled the Rust dispatch arm made
  `fake_lsp_session_produces_edges_and_correct_requests` fail on its edge
  assertion, proving the merged wiring is what the tests exercise (source
  restored bit-identical; worktree clean at review end).
- **Real LSP subprocess, real queries, re-verified with a real binary.** The
  sandbox PATH holds only a rustup shim without the component and both the
  component download and the release-asset CDN are blocked (environment
  restriction, not a code defect); rust-analyzer was rebuilt from source at
  55effabb via git transport. With `RUST_ANALYZER_BIN` pointing at it, the
  gated test `real_rust_analyzer_extracts_known_call_edges` RAN (no skip
  line) and passed, asserting the exact three fixture edges.
- **Qname contract in Gyre's real layout.** Independent probe (outside the
  repo): a `crates/<crate>/src` workspace-layout fixture — real rust-analyzer
  + real adapter + real Pass 1 `RustExtractor` + real domain
  `resolve_call_edges` — produced `mycrate::<module>::<fn>` qnames on both
  sides; exactly the pub-to-pub edge resolved to Pass 1 node IDs as `Calls`,
  the trait-impl method edge correctly emitted and dropped at resolution.
  The in-repo contract test pins only the flat `src/<mod>.rs` layout, so this
  probe covers Gyre's actual `crates/<crate>/src` layout.
- **Gyre codebase itself (plan item 5).** `crates/gyre-common` in a
  standalone workspace with the same layout: 7 edges in 34s, all genuine
  (`view_spec` test fns → `validate_view_spec`), all resolving against real
  Pass 1 nodes.
- **Graceful degradation** per the port contract: missing binary → warn +
  empty; binary that exits before speaking LSP → fast EOF + empty (both
  tested); the PATH rustup shim is rejected by the `--version` probe in the
  test gate (observed). Budget-bounded everywhere (60s init, 5s/query, 180s
  overall with 30s query reserve).
- **Tests:** adapters `call_graph_rust` 6/6 (real-binary test ran),
  `call_graph` 13/13; server `graph_extraction` 27/27 including
  `sync_rust_repo_persists_calls_edges_in_graph_store`. `cargo fmt --check`
  clean on both touched packages. Static checks pass: check-arch,
  byte-slice truncation, relative-path defaults, unbounded external HTTP,
  task-commit attribution. No dangling references to the deleted
  rust-callgraph scripts.

## Notes (non-blocking)

1. The shipped notes' rebase section says `call_graph_resolve.rs` and
   `rust_extractor.rs` are "byte-identical to the verified candidate
   e29b94db (git diff empty)" — imprecise: both files carry main's changes
   from the base refresh (test-only additions; an unrelated DependsOn
   simplification). They are identical to the BASE, and the branch delivers
   no changes to them, so the substantive claim (this branch's Rust Pass 2
   code unchanged by the rebase) holds; probes above confirm the contract
   on the merged tree regardless.
2. A Rust repo whose crates sit under `crates/<name>/Cargo.toml` without a
   root workspace manifest yields zero edges from real rust-analyzer (no
   workspace → detached-file analysis doesn't resolve `crate::` paths).
   Graceful degradation per the port contract, never an error; repos with a
   root manifest (like Gyre) work.
3. LSP `character` is a proc-macro2 byte column, documented as equal to the
   UTF-16 column because the identifier prefix is ASCII keywords; a
   non-ASCII same-line prefix would only lose that query's edges —
   degradation, never a wrong edge.
4. A null `references` result (no references) increments the
   `failed_queries` warning counter — cosmetic log noise only.

## Deferred to verification

Full workspace `cargo test --all`, all-target Clippy, GitHub CI, and one
full-checkout extraction with a release rust-analyzer (the from-source debug
binary loads the whole Gyre workspace slower than the 180s budget; spec §9
assumes release timings).

## Evidence

`/tmp/stage/review-evidence/task-073/` — README with environment notes,
from-source rust-analyzer build log, test transcripts (real-binary run
included), crates-layout and Gyre-code probes, mutation probe with restore
proof, static-check results.
