# Review: task-072 — LSP Call Graph — Core Pipeline + Go Extractor Integration

**Reviewer:** Verifier
**Round:** R1
**Spec ref:** lsp-call-graph.md §1–6, §10 Phase 1, §11
**Status:** needs-revision

---

## Findings

- [-] [process-revision-complete] **F1: Missing `CallGraphExtractor` port trait in `gyre-ports`.** Acceptance criterion #1 says "`CallGraphExtractor` port trait exists in `gyre-ports`." The task implementation plan step 2 defines the trait signature: `pub trait CallGraphExtractor: Send + Sync { async fn extract_call_edges(&self, repo_path: &Path, language: Language) -> Result<Vec<CallEdge>>; }`. No such trait exists in `gyre-ports/src/`. The subprocess I/O (shelling out to `go-callgraph`) is placed directly in `gyre-domain/src/go_extractor.rs:extract_lsp_call_graph()` (line 673) and `gyre-domain/src/lsp_call_graph.rs:try_go_callgraph_binary()` (line 1513), bypassing the hexagonal architecture boundary. The task's agent instructions say: "Follow the hexagonal architecture: port trait in `gyre-ports`, adapter in `gyre-adapters`, orchestration in `gyre-domain`."

- [-] [process-revision-complete] **F2: Go adapter subprocess I/O in `gyre-domain` instead of `gyre-adapters`.** Task implementation plan step 3 says "Implement Go adapter in `gyre-adapters`: shell out to `scripts/go-callgraph/go-callgraph <repo-path>`, parse the JSON output." The implementation places this subprocess I/O directly in `gyre-domain/src/go_extractor.rs:685` (`Command::new(&binary).arg(...)`) and `gyre-domain/src/lsp_call_graph.rs:1575` (`Command::new(&binary_path).arg(repo_root)...spawn()`). The `LanguageExtractor` trait doc comment (`extractor.rs:77-78`) explicitly states: "Extractors MUST NOT perform I/O beyond reading the repository root subtree." Shelling out to an external Go binary is I/O beyond reading the subtree, violating the trait's documented contract.

- [-] [process-revision-complete] **F3: Double execution of `go-callgraph` binary on every push for Go repos.** Two independent code paths both shell out to `go-callgraph`:
  1. **Inline (blocking):** `GoExtractor.extract()` (`go_extractor.rs:69-70`) calls `ctx.extract_lsp_call_graph()` which runs `go-callgraph` synchronously during `run_all_extractors()` (step 2 of `do_extract()`).
  2. **Background:** `extract_lsp_edges()` (`graph_extraction.rs:794`) → `extract_call_graph_auto()` → `extract_call_graph_go()` → `try_go_callgraph_binary()` runs `go-callgraph` again in the background task (step 7 of `do_extract()`).
  The second run produces zero new edges (the dedup set at `lsp_call_graph.rs:1715-1719` catches them), but the binary still executes — wasting up to 60 seconds of subprocess time per push. The spec's two-pass architecture intends Pass 2 to run only once, in the background.

- [-] [process-revision-complete] **F4: Non-blocking requirement violated — GoExtractor runs Pass 2 synchronously.** Acceptance criterion: "Pass 2 is non-blocking — graph is usable after Pass 1, becomes complete after Pass 2." Spec §6: "Pass 2 is non-blocking — the graph is usable after Pass 1 with partial call data, and becomes complete when Pass 2 finishes." `GoExtractor.extract()` (`go_extractor.rs:66-71`) runs both Pass 1 (tree-sitter) and Pass 2 (go-callgraph binary) synchronously. This method is called from `run_all_extractors()` (`graph_extraction.rs:763`), which runs inside `tokio::task::spawn_blocking` in step 2 of `do_extract()` (line 149). Graph persistence doesn't start until step 3 (line 163). The graph is not usable after Pass 1 — it's held in memory until Pass 2 completes (up to 60 seconds per go-callgraph timeout). The background Pass 2 in step 7 (line 391) correctly implements non-blocking semantics, but the inline Pass 2 in the GoExtractor defeats the purpose.

- [-] [process-revision-complete] **F5: Integration test tests extractor in isolation, not the sync pipeline.** Acceptance criterion: "Integration test: sync a Go repo → verify `Calls` edges appear in graph." The test `lsp_call_graph_produces_cross_package_calls` (`go_extractor.rs:1439`) calls `GoExtractor.extract(dir.path(), "abc123")` directly and asserts on the returned `ExtractionResult`. It does not invoke the sync pipeline (`extract_and_store_graph`), does not persist edges via `GraphPort`, and does not verify edges appear in the graph store. This is a unit test of the extractor, not an integration test of the sync → graph storage pipeline. The acceptance criterion specifically says "sync a Go repo → verify `Calls` edges appear in graph" — requiring persistence verification.
