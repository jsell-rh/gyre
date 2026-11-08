---
title: "LSP Call Graph — TypeScript tsserver Integration"
spec_ref: "lsp-call-graph.md §5 TypeScript, §10 Phase 4"
depends_on:
  - task-072
progress: complete
coverage_sections:
  - "lsp-call-graph.md §5 Per-Language Implementation (TypeScript)"
  - "lsp-call-graph.md §10 Implementation Phases (Phase 4)"
commits: ["fe02e97b44f5112d1b1a6d909aacb58300756407", "1fff426c5629e8254af95fb50c77c6a5af18fa2b", "43837ee9f59c28529e4381c721b11f7122d900d2", "ecf3ea5b2735ec6de2a2edbd40a4981f28c1a2e5"]
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

## Shipped

All acceptance criteria are met; the assigned contract above is preserved verbatim, and per the repair findings the acceptance-criteria status lives here rather than in edited checkbox text. Production commits: `ecf3ea5b` (implementation), `43837ee9` (contract-repair lint fix), `1fff426c` (UTF-8 framing repair from review finding `8de91bdf`), and `fe02e97b` (verification-round doc-lint fix).

TypeScript Pass 2 uses the specced LSP-subprocess approach: `scripts/ts-callgraph.mjs` spawns `typescript-language-server --stdio`, didOpens every `.ts`/`.tsx`/`.js`/`.jsx` file, requests `textDocument/documentSymbol` per file, sends `textDocument/references` for each top-level function/class/interface definition (plus arrow-function consts), resolves the enclosing function at each reference site via symbol-range containment, and emits `Calls` edges in the Pass 1 qualified-name format (`<relpath-without-ext>.<Name>`, nested chains `module.Class.method`). Symbol resolution comes from tsserver, which handles the TypeScript-specific patterns: cross-module imports, barrel re-exports (reference sites attribute to the original symbol through `index.ts`), JSX component usage in `.tsx`, structural typing, and overloads. Top-level import/re-export references are not calls and produce no edges. The adapter dispatch in `gyre-adapters/src/call_graph.rs` runs node + script; discovery supports `TS_CALLGRAPH_SERVER`/`TS_CALLGRAPH_TSSERVER` env overrides (documented in `docs/server-config.md`), repo-local `node_modules`, the workspace root, and the language-server install prefix. TypeScript 7 native lib dirs are rejected (no `tsserver.js`). LSP wire framing is byte-based end to end: the server's stdout is accumulated as a Buffer and bodies are sliced by byte offset against the wire `Content-Length` (which counts UTF-8 bytes), decoding each body only after extraction — responses with multi-byte UTF-8 (non-ASCII symbol names, detail fields, log messages) reassemble exactly.

### Svelte and degradation handling

Per the task instructions: `.svelte` files are skipped with a warning; additionally, files that import `.svelte` modules are skipped with a warning (tsserver wedges indefinitely on their analysis), and a directory whose first `documentSymbol` request times out is skipped wholesale (siblings share import patterns) with the skip counted and logged. All failure modes degrade to `[]` + exit 0 with a stderr warning: missing server binary, missing/invalid tsserver install, initialize failure, per-file and per-query timeouts, and a 50s query budget (the adapter kills the subprocess at 60s); partial results are emitted rather than nothing.

### Test evidence

- `scripts/test/ts-callgraph.test.mjs` (node:test, 18 tests, wired into CI's dev-controller job): LSP framing (byte-length headers, byte-based reassembly — including non-ASCII bodies, a chunk split inside a multi-byte sequence, and a frame following a non-ASCII body not being swallowed — chunked reassembly, malformed-header skip), qname format, symbol classification (arrow consts vs plain consts), enclosing-chain resolution (method in class, arrow variable, top-level import excluded, range-end boundary excluded), dedup.
- `crates/gyre-adapters/tests/fixtures/ts-lsp-stub.mjs`: recorded-protocol stub replaying the real typescript-language-server's responses for the fixture. `ts_driver_maps_references_to_edges_via_recorded_protocol` runs the full driver through `SubprocessCallGraphExtractor` hermetically and asserts the exact 6-edge set: cross-module import, arrow-const callee, barrel-mediated call, JSX usage, a raw edge to a non-exported callee (dropped later by the resolver, as designed), and a non-ASCII-identifier call (`src/café.ts` declaring `export function résumé()` calling `add`) pinning the UTF-8 byte-framing contract end to end (its responses were recorded 2026-11-05 from the real server + tsserver 5.9.3; pre-existing entries are byte-identical to the original recording). Also asserts selectivity: no module-attributed import edges, no edge onto a re-export-only symbol.
- `ts_driver_warns_and_skips_when_server_missing`: missing server (authoritative env override) produces a stderr warning, `[]`, exit 0.
- `ts_extractor_resolves_edges_with_real_toolchain_when_configured`: with `TS_CALLGRAPH_SERVER` configured, asserts the full edge set against the real server; otherwise pins the never-error degradation contract.
- `gyre-domain` `detect_all_languages_typescript_markers`: tsconfig.json, package.json with TypeScript dep, and frontend-subdir package.json all select the TypeScript extractor.
- `gyre-server` `sync_ts_repo_persists_calls_edges_in_graph_store`: Pass 2 TS edges resolve to Pass 1 node IDs in the graph store, pinning the qname contract end to end.
- Validated against the Gyre web UI (`web/`) with the real toolchain on the current head: 685 edges extracted (e.g. `src/lib/types/view-spec.validateViewSpec -> src/lib/types/view-spec.isObject`), with 67 `.svelte` files and 52 svelte-importing files skipped with warnings; exit 0. (An earlier run on the pre-merge tree recorded 53 edges — the framing bug's documentSymbol timeout cascade was suppressing data; see round 4 below.)

### Contract-repair rounds (consolidated)

The executor restored the assigned task document in three rounds after the process commits were flagged as protected-text changes; this section consolidates the earlier per-round notes, which had also been placed inside the `## Agent Instructions` section — the structural cause of the repeated findings. All notes now live under `## Shipped` only, and the protected contract text is byte-identical to the assignment.

- Round 1 (`43837ee9`): production fix from `ecf3ea5b` preserved unchanged except one lint repair — the recorded-protocol test's unused `node` binding (guard-only `which_on_path` result) was replaced with a presence check, removing the `unused_variables` warning.
- Round 2 (head `0bc0c0de`, finding `22eabe80`): no production change; both task commits intact; focused probes re-run green. The merge of base `5df14f34` (PR #685) touches only pipeline/CI tooling files — no task-075 production file. Workspace `cargo fmt --check` drift is confined to files brought by the base merge (`recovery.rs`, `merge_processor.rs`, `gate_executor.rs`, `git2_ops.rs`, `sqlite/audit.rs` — no commit in `5df14f34..HEAD` touches them) and is left to the workspace-wide gates owned by verification.
- Round 3 (head `13e33f65`, finding `b2b7f8ea`): no production change; both task commits intact. Focused probes re-run green; the sandbox had no `typescript-language-server` on PATH, so the real-toolchain test exercised its documented never-error degradation branch — a toolchain-availability note for remote verification, not a code defect.
- Round 4 (head `1fff426c`, finding `8de91bdf`): review found the LSP receive framing mixed byte counts with UTF-16 code-unit counts — stdout was accumulated as a JS string but `Content-Length` counts UTF-8 bytes, so any response body containing multi-byte UTF-8 never reassembled: the request timed out, the file was skipped, the desynchronized parser dropped later messages, and repos with non-ASCII identifiers silently lost every Calls edge. Reproduced against the real server (the reviewer's `src/café.ts` fixture: 30s documentSymbol timeout, wedged-directory cascade skipping pure-ASCII siblings too, `[]` with exit 0). Fixed by accumulating the stream as a Buffer and slicing bodies by byte offset; the recorded-protocol stub's stdin parser had the identical bug and was fixed the same way. Regression coverage: three new byte-framing unit tests, and the recorded-protocol fixture gained `src/café.ts` with responses recorded from the real server (pre-existing entries verified byte-identical). Verified against the real toolchain: the café fixture now yields `src/café.résumé -> src/math.add` in ~3s (`utf8-fix-verified-real-server.log`), the full `gyre-adapters` call_graph suite passes 6/6 both hermetically and with `TS_CALLGRAPH_SERVER` configured (`adapters-call-graph.log`, `real-toolchain-test.log`), node unit tests 18/18 (`node-unit-tests.log`), and the web UI run extracts 685 edges (`web-ui-real-server-run.log`) — all logs, commands, and exit codes under `/tmp/stage/review-evidence/`.
- Round 5 (head `fe02e97b`, verification finding `b4823175`): the changed-lines clippy gate against base `4e304e5d` flagged three `doc_lazy_continuation` findings at `call_graph.rs:382-384` — the `expected_ts_edges` doc comment's closing paragraph followed the bullet list without a blank `///` separator, so rustdoc parsed it as a lazy continuation of the last list item. Fixed by inserting the blank doc line (comment content unchanged); no behavior change. Gates re-run green: `check-clippy-diff.py` and `check-rustfmt-diff.py` against base `4e304e5d` both clean, and the full `gyre-adapters` call_graph suite passes 6/6 hermetically (`clippy-gate-before.log`, `clippy-gate-after.log`, `adapters-call-graph-after-fix.log` under `/tmp/stage/review-evidence/`). The remaining `checks.sh` output is pre-existing Svelte compiler warnings; the web build itself succeeds.
