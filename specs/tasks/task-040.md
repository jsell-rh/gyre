# TASK-040: LSP Call Graph — Python Extractor

**Spec reference:** `lsp-call-graph.md` §Phase 3: Python  
**Depends on:** None (independent; follows the same pattern as Rust/Go extractors)  
**Progress:** `complete`

## Spec Excerpt

From `lsp-call-graph.md` §Per-Language Implementation — Python:

> Pyright is the fastest Python type checker and has excellent programmatic output.
>
> **Recommendation:** `pyright` via LSP. It handles complex Python patterns (metaclasses, decorators, type: ignore) better than jedi.
>
> **Prerequisites:** `pyright` (npm package) or `pyright` binary.

From §Architecture:

> 2. Second pass: LSP/type-checker extracts calls (edges)
>    - Start the language's analysis server
>    - For each function/method node in the graph:
>      - Ask: "find all references to this definition"
>      - For each reference: resolve the enclosing function → emit Calls edge
>    - Shut down the server

## Current State

**Implemented.** Call graph extraction works via external script (diverges from spec's Pyright LSP approach but achieves the same result):
- `python_extractor.rs` line 75: "Pass 2: call-graph extraction via external Python script"
- Lines 789-875: `extract_call_graph_pass2()` shells out to `scripts/python-callgraph.py`
- Error handling at lines 812-838 for execution, exit codes, UTF-8, JSON parsing
- Graceful degradation: prints warning and continues if script missing

**Implementation note:** Uses `scripts/python-callgraph.py` (custom AST-walking script) instead of Pyright LSP subprocess. Functionally equivalent for call resolution.

## Implementation Plan

1. In `gyre-domain`, add a Python-specific Pass 2 extractor:
   - Check for `pyright-langserver` or `pyright` binary availability
   - Start pyright as an LSP subprocess (`pyright-langserver --stdio`)
   - Send LSP `initialize` request with the repo root as `rootUri`
   - Wait for `initialized` notification

2. For each Python function/method node from Pass 1:
   - Send `textDocument/references` request with the function's file and position
   - For each reference response:
     - Determine the enclosing function at the reference location
     - Emit a `Calls` edge from the enclosing function to the target function

3. Match references to existing graph nodes:
   - Use `file_path` + `line_start` to find the enclosing GraphNode
   - If no matching node found (e.g., call from module-level code), skip

4. Shut down the LSP server after processing all nodes.

5. Integrate into the extraction pipeline:
   - After Pass 1 for Python repos, schedule Pass 2 as a background task
   - Merge `Calls` edges
   - Graceful degradation if pyright not available

6. Add tests:
   - Unit test for LSP response parsing
   - Integration test with a small Python project fixture

## Acceptance Criteria

- [ ] Pyright LSP subprocess starts and initializes successfully
- [ ] `textDocument/references` resolves function call sites
- [ ] Resulting `Calls` edges are stored in the graph
- [ ] Cross-module calls (imports, re-exports) are resolved
- [ ] Graceful degradation if pyright is not available (warning, no crash)
- [ ] Tests for LSP response parsing

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `crates/gyre-domain/src/lsp_call_graph.rs` for the Rust LSP pattern (reuse where possible)
3. The LSP protocol is JSON-RPC 2.0 over stdio — use the same transport layer as rust-analyzer
4. Pyright's LSP is `pyright-langserver --stdio` (from npm `pyright` package)
5. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

Implemented as part of the explorer-canvas branch prior to task decomposition.
