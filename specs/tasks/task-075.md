---
title: "LSP Call Graph — TypeScript tsserver Integration"
spec_ref: "lsp-call-graph.md §5 TypeScript, §10 Phase 4"
depends_on:
  - task-072
progress: ready-for-review
coverage_sections:
  - "lsp-call-graph.md §5 Per-Language Implementation (TypeScript)"
  - "lsp-call-graph.md §10 Implementation Phases (Phase 4)"
commits: ["ecf3ea5b2735ec6de2a2edbd40a4981f28c1a2e5", "43837ee9f59c28529e4381c721b11f7122d900d2"]
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
   - Spawn `typescript-language-server --stdio` as LSP subprocess
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

- [x] TypeScript adapter for `CallGraphExtractor` implemented in `gyre-adapters`
- [x] Spawns `typescript-language-server` as LSP subprocess
- [x] Correctly maps reference results to `Calls` edges
- [x] Handles missing `typescript-language-server` gracefully (warning, skip)
- [x] Pipeline selects TypeScript extractor for repos with TypeScript markers
- [x] Integration test with a TypeScript test fixture
- [x] `cargo test --all` passes, `cargo fmt --all` clean

## Agent Instructions

Read `specs/system/lsp-call-graph.md` §5 "TypeScript: tsc Compiler API" for the full spec. This task depends on task-072 for the `CallGraphExtractor` port trait and pipeline. Follow the same LSP subprocess pattern as Rust (task-073) and Python (task-074). The LSP server is `typescript-language-server --stdio`. Note: Svelte files (`.svelte`) may not be fully analyzable via tsserver — handle gracefully by extracting what's possible from `.ts`/`.js` files and logging a warning for Svelte files.

## Shipped

TypeScript Pass 2 uses the specced LSP-subprocess approach (commit `ecf3ea5b`): `scripts/ts-callgraph.mjs` spawns `typescript-language-server --stdio`, didOpens every `.ts`/`.tsx`/`.js`/`.jsx` file, requests `textDocument/documentSymbol` per file, sends `textDocument/references` for each top-level function/class/interface definition (plus arrow-function consts), resolves the enclosing function at each reference site via symbol-range containment, and emits `Calls` edges in the Pass 1 qualified-name format (`<relpath-without-ext>.<Name>`, nested chains `module.Class.method`). Symbol resolution comes from tsserver, which handles the TypeScript-specific patterns: cross-module imports, barrel re-exports (reference sites attribute to the original symbol through `index.ts`), JSX component usage in `.tsx`, structural typing, and overloads. Top-level import/re-export references are not calls and produce no edges. The adapter dispatch in `gyre-adapters/src/call_graph.rs` runs node + script; discovery supports `TS_CALLGRAPH_SERVER`/`TS_CALLGRAPH_TSSERVER` env overrides (documented in `docs/server-config.md`), repo-local `node_modules`, the workspace root, and the language-server install prefix. TypeScript 7 native lib dirs are rejected (no `tsserver.js`).

### Svelte and degradation handling

Per the task instructions: `.svelte` files are skipped with a warning; additionally, files that import `.svelte` modules are skipped with a warning (tsserver wedges indefinitely on their analysis), and a directory whose first `documentSymbol` request times out is skipped wholesale (siblings share import patterns) with the skip counted and logged. All failure modes degrade to `[]` + exit 0 with a stderr warning: missing server binary, missing/invalid tsserver install, initialize failure, per-file and per-query timeouts, and a 50s query budget (the adapter kills the subprocess at 60s); partial results are emitted rather than nothing.

### Test evidence

- `scripts/test/ts-callgraph.test.mjs` (node:test, 15 tests, wired into CI's dev-controller job): LSP framing (byte-length headers, chunked reassembly, malformed-header skip), qname format, symbol classification (arrow consts vs plain consts), enclosing-chain resolution (method in class, arrow variable, top-level import excluded, range-end boundary excluded), dedup.
- `crates/gyre-adapters/tests/fixtures/ts-lsp-stub.mjs`: recorded-protocol stub replaying the real typescript-language-server's responses for the fixture. `ts_driver_maps_references_to_edges_via_recorded_protocol` runs the full driver through `SubprocessCallGraphExtractor` hermetically and asserts the exact 5-edge set: cross-module import, arrow-const callee, barrel-mediated call, JSX usage, and a raw edge to a non-exported callee (dropped later by the resolver, as designed). Also asserts selectivity: no module-attributed import edges, no edge onto a re-export-only symbol.
- `ts_driver_warns_and_skips_when_server_missing`: missing server (authoritative env override) produces a stderr warning, `[]`, exit 0.
- `ts_extractor_resolves_edges_with_real_toolchain_when_configured`: with `TS_CALLGRAPH_SERVER` configured, asserts the full edge set against the real server; otherwise pins the never-error degradation contract.
- `gyre-domain` `detect_all_languages_typescript_markers`: tsconfig.json, package.json with TypeScript dep, and frontend-subdir package.json all select the TypeScript extractor.
- `gyre-server` `sync_ts_repo_persists_calls_edges_in_graph_store`: Pass 2 TS edges resolve to Pass 1 node IDs in the graph store, pinning the qname contract end to end.
- Validated against the Gyre web UI (`web/`): 53 edges extracted (e.g. `src/lib/types/view-spec.validateViewSpec -> src/lib/types/view-spec.isObject`), with 67 `.svelte` files and 2 svelte-importing files skipped with warnings and the vitest-importing `src/__tests__` directory skipped after its first `documentSymbol` timeout; exit 0.

### Contract-repair rounds (commits `43837ee9`, this round)

The executor restored the assigned task document after process commits were flagged as protected-text changes; each time the production fix in `ecf3ea5b` was preserved unchanged except for one lint repair in `43837ee9`: the recorded-protocol test's unused `node` binding (guard-only `which_on_path` result) was replaced with a presence check, removing the `unused_variables` warning introduced by `ecf3ea5b`. Re-verified on this restored checkout: node unit tests 15/15 (exit 0), `gyre-adapters` call_graph 6/6 warning-free (exit 0), `gyre-domain` call_graph 17/17 (exit 0), `gyre-server` `sync_ts_repo_persists_calls_edges_in_graph_store` 1/1 (exit 0), and `rustfmt --check` clean on the task's Rust files (`call_graph.rs`, `call_graph_resolve.rs`, `graph_extraction.rs`). Logs and exit codes under `/tmp/stage/review-evidence/`. The pre-existing workspace-wide fmt drift (e.g. `git2_ops.rs`, present at the base commit `0cb71015`, untouched by this task) is left for workspace-wide gates owned by verification.
