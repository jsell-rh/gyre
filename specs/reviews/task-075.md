# Review: task-075 — LSP Call Graph — TypeScript tsserver Integration

**Reviewer:** Independent review (pi runtime)
**Candidate:** `95b1cfdbee683e91c8b2cdc69f96997367e3b7fa` (base `5df14f34`)
**Production commits:** `ecf3ea5b`, `43837ee9`, `1fff426c`
**Spec ref:** lsp-call-graph.md §5 TypeScript, §10 Phase 4
**Status:** approved

---

## Scope

Diff `5df14f34..95b1cfdb` touches only task-075 files: `scripts/ts-callgraph.mjs`
(driver), `crates/gyre-adapters/src/call_graph.rs` (TS dispatch + tests),
`crates/gyre-adapters/tests/fixtures/ts-lsp-stub.mjs` (recorded-protocol stub),
`scripts/test/ts-callgraph.test.mjs` (node unit tests), test-only additions in
`gyre-domain/call_graph_resolve.rs` and `gyre-server/graph_extraction.rs`,
CI wiring for `scripts/test/*.test.mjs`, and `docs/server-config.md`
(`TS_CALLGRAPH_SERVER`/`TS_CALLGRAPH_TSSERVER`). The protected task contract in
`specs/tasks/task-075.md` is byte-identical to the assignment.

## Independent probes (evidence in /tmp/stage/review-evidence)

Toolchain installed for this review: `typescript-language-server` 4.3.4 +
`typescript` 5.9.3 (npm, `/tmp/tslsp-install`).

1. **Real-server run on an independent fixture** (`real-server-run.json`,
   `.stderr`; fixture `/tmp/tsreview-fixture`, deliberately different from the
   recorded one): `node scripts/ts-callgraph.mjs` with
   `TS_CALLGRAPH_SERVER`/`TS_CALLGRAPH_TSSERVER` → exit 0, exactly the correct
   3 edges:
   - `src/café.résumé -> src/math.add` — non-ASCII identifier, the durable
     finding `8de91bdf` UTF-8 framing regression: reassembles correctly now
   - `src/consumer.viaBarrel -> src/math.add` — barrel re-export via `index.ts`
   - `src/service.Calc.run -> src/math.add` — enclosing chain `Class.method`
   No import-site edges, no re-export-only edges. Selectivity holds.
2. **Gyre web UI validation** (`web-ui-real-server-run.json`, `.stderr`):
   exit 0, **685 edges** (e.g. `src/lib/types/view-spec.validateViewSpec ->
   src/lib/types/view-spec.isObject`), 67 `.svelte` files and 52
   svelte-importing files skipped with warnings — reproduces the Shipped claim
   exactly. Acceptance criterion "Test with the Gyre web UI" confirmed against
   the real toolchain on the current head.
3. **Node unit tests** (`node-unit-tests.log`): 18/18 pass, including the three
   byte-framing regression tests (non-ASCII body, chunk split inside a
   multi-byte sequence, frame after a non-ASCII body not swallowed).
4. **Rust adapter tests** (`adapters-call-graph-real-toolchain.log`,
   `real-toolchain-test-isolated.log`): `cargo test -p gyre-adapters --lib
   call_graph` 6/6 both hermetically and with the real toolchain configured.
   The real-toolchain test run in isolation takes 3.29s (real server spawn +
   initialize + queries) and takes the strong branch (no degradation note),
   asserting the full expected edge set including `src/café.résumé`.
5. **Pipeline selection** (`detect_all_languages_typescript_markers`) and
   end-to-end persistence (`sync_ts_repo_persists_calls_edges_in_graph_store`)
   pass; production TypeScript detection (`has_typescript_manifest` →
   `Language::TypeScript`) pre-exists in `gyre-domain` and is dispatched by the
   adapter.
6. **Format**: `cargo fmt --check` drift is confined to base-merge files
   (`git2_ops.rs`, `sqlite/audit.rs`, `recovery.rs`, …) with **zero overlap**
   with candidate-touched files (`candidate-files.txt`,
   `fmt-drift-files.txt`); the candidate's own files are fmt-clean.
   Workspace-wide `cargo test --all` / `fmt --all` belong to verification
   (the executor wrapper rejects workspace invocations — `fmt-all.log`).

## Findings from prior rounds

- Durable finding `8de91bdf` (UTF-8 framing): **fixed and verified**. The
  driver's `LspClient` accumulates stdout as a `Buffer` and `extractMessages`
  slices bodies by byte offset against the wire `Content-Length`, decoding
  each body only after extraction; the recorded-protocol stub's stdin parser
  was fixed the same way. Verified against the real server with a non-ASCII
  identifier (probe 1) plus three regression unit tests (probe 3).

## Notes (non-blocking)

- `ts_extractor_resolves_edges_with_real_toolchain_when_configured` reads
  `TS_CALLGRAPH_SERVER` without the `ENV_LOCK` used by the stub test, so in a
  full-suite run with the var configured externally it can observe the stub
  guard's set/drop of the same variable. All outcomes still pass (stub yields
  the full expected edge set; degradation branch asserts well-formedness), CI
  pins degradation by design, and the strong branch is genuine when the test
  runs configured (probe 4). Test-hygiene note only, no production impact.
- `specs/coverage/system/lsp-call-graph.md` rows 5/10 still describe task-075
  as not-started — the candidate correctly did not touch the coverage matrix
  (controller/PM scope); this record supplies the review evidence for the
  coverage update.

## Verdict

Approved. All acceptance criteria are backed by production code and
independent evidence: adapter dispatch in `gyre-adapters`, real
`typescript-language-server --stdio` subprocess with correct references →
`Calls` edge mapping (verified against the real server on an independent
fixture and on the Gyre web UI), graceful degradation on missing toolchain,
pipeline selection via TypeScript markers, hermetic recorded-protocol +
real-toolchain integration tests, and fmt-clean candidate files.
