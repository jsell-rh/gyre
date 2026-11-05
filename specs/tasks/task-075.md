---
title: "LSP Call Graph — TypeScript tsserver Integration"
spec_ref: "lsp-call-graph.md §5 TypeScript, §10 Phase 4"
depends_on:
  - task-072
progress: not-started
coverage_sections:
  - "lsp-call-graph.md §5 Per-Language Implementation (TypeScript)"
  - "lsp-call-graph.md §10 Implementation Phases (Phase 4)"
commits: []
---

## Spec Excerpt

Use `typescript-language-server` as an LSP subprocess to extract complete `Calls` edges for TypeScript projects. TypeScript's structural typing means the type checker is essential for resolving interface implementations and function overloads.

**Approach (LSP subprocess, per spec recommendation):**
- Start `typescript-language-server --stdio` as LSP server
- Send `textDocument/references` requests for each function/class/interface definition
- For each reference site, resolve the enclosing function
- Emit `Calls` edges
- Shut down the server

**Prerequisites:** `node`, `typescript`, `typescript-language-server` npm packages.

## Implementation Plan

1. **Implement TypeScript adapter** in `gyre-adapters` for the `CallGraphExtractor` port:
   - Spawn `typescript-language-server --stdio` as an LSP subprocess
   - Initialize LSP connection
   - For each function/class/method node from Pass 1:
     - Send `textDocument/references` request
     - Collect reference locations
     - Resolve enclosing function for each reference
     - Emit `CallEdge`
   - Shutdown

2. **Handle TypeScript-specific patterns:**
   - Interface implementations (structural typing — references may not have explicit `implements`)
   - Generic type parameters and type narrowing
   - JSX component usage (component references in JSX)
   - Re-exports from barrel files (`index.ts`)
   - Svelte files — may need separate handling or skipping (Svelte compiler is not TypeScript)

3. **Register the TypeScript extractor** in the extraction pipeline, selected based on presence of `tsconfig.json` or `package.json` with TypeScript dependency.

4. **Test with the Gyre web UI** (`web/` directory — a Svelte + TypeScript project).

## Acceptance Criteria

- [ ] TypeScript adapter for `CallGraphExtractor` implemented in `gyre-adapters`
- [ ] Spawns `typescript-language-server` as LSP subprocess
- [ ] Correctly maps reference results to `Calls` edges
- [ ] Handles missing `typescript-language-server` gracefully (warning, skip)
- [ ] Pipeline selects TypeScript extractor for repos with TypeScript markers
- [ ] Integration test with a TypeScript test fixture
- [ ] `cargo test --all` passes, `cargo fmt --all` clean

## Agent Instructions

Read `specs/system/lsp-call-graph.md` §5 "TypeScript: tsc Compiler API" for the full spec. This task depends on task-072 for the `CallGraphExtractor` port trait and pipeline. Follow the same LSP subprocess pattern as Rust (task-073) and Python (task-074). The LSP server is `typescript-language-server --stdio`. Note: Svelte files (`.svelte`) may not be fully analyzable via tsserver — handle gracefully by extracting what's possible from `.ts`/`.js` files and logging a warning for Svelte files.
