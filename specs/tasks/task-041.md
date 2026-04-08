# TASK-041: LSP Call Graph — TypeScript Extractor

**Spec reference:** `lsp-call-graph.md` §Phase 4: TypeScript  
**Depends on:** None (independent; follows the same pattern as Rust/Python extractors)  
**Progress:** `not-started`

## Spec Excerpt

From `lsp-call-graph.md` §Per-Language Implementation — TypeScript:

> Or via LSP (simpler):
> ```bash
> # Start tsserver, send textDocument/references requests
> ```
>
> **Recommendation:** LSP via `typescript-language-server`. Same pattern as Rust/Python.
>
> **Prerequisites:** `typescript` and `typescript-language-server` npm packages.

From §Performance Considerations:

> **TypeScript project** (~3000 nodes): tsc loads in ~3-8 seconds, queries ~10ms each. Total: ~30 seconds.

## Current State

- Rust and (after TASK-040) Python LSP extractors exist.
- TypeScript code extraction uses tree-sitter (Pass 1 only) — produces declarations but incomplete `Calls` edges.
- No TypeScript LSP integration for Pass 2.
- The Gyre frontend (`web/src/`) is itself a TypeScript/Svelte project — this extractor would enable self-analysis.

## Implementation Plan

1. In `gyre-domain`, add a TypeScript-specific Pass 2 extractor:
   - Check for `typescript-language-server` binary availability
   - Start as an LSP subprocess (`typescript-language-server --stdio`)
   - Send LSP `initialize` request with the repo root as `rootUri`
   - Ensure `tsconfig.json` is present (required for full type resolution)

2. For each TypeScript function/method node from Pass 1:
   - Send `textDocument/references` request
   - For each reference: resolve the enclosing function → emit `Calls` edge

3. Handle TypeScript-specific patterns:
   - Svelte files (`.svelte` with `<script lang="ts">`) — may need separate handling if tsserver doesn't support them natively
   - Re-exported types (barrel files like `index.ts`)
   - Interface/class method implementations

4. Match references to existing graph nodes by `file_path` + `line_start`.

5. Integrate into the extraction pipeline (same pattern as Go/Python):
   - Background Pass 2 after tree-sitter Pass 1
   - Merge `Calls` edges
   - Graceful degradation if typescript-language-server not available

6. Add tests with a small TypeScript project fixture.

## Acceptance Criteria

- [ ] TypeScript LSP subprocess starts and initializes
- [ ] `textDocument/references` resolves function/method call sites
- [ ] Resulting `Calls` edges are stored in the graph
- [ ] Cross-module imports are resolved
- [ ] Graceful degradation if typescript-language-server not available
- [ ] Tests for LSP response parsing

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `crates/gyre-domain/src/lsp_call_graph.rs` for the Rust LSP pattern
3. The TypeScript LSP is `typescript-language-server --stdio` (from npm)
4. Ensure `tsconfig.json` handling — the server needs a project root with a TS config
5. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

_(none yet)_
