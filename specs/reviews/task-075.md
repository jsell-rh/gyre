# Review: TASK-075 — LSP Call Graph — TypeScript tsserver Integration

**Reviewer:** independent review (re-review after interruption; all evidence re-established against the exact candidate)
**Candidate:** `c0c84374e41ede022c79d04634b423d301b2eb7a` (checkpoint merge of the task branch with assigned base `1ae18d1c42bf9c48ae1e3c10369dbc81920664c9`)
**Task commits:** `ecf3ea5b`, `43837ee9`, `1fff426c`, `fe02e97b` (all listed in task frontmatter; contract section byte-identical to the assignment)
**Spec ref:** lsp-call-graph.md §5 TypeScript, §10 Phase 4

## Evidence (all probes run at the exact candidate; logs under /tmp/stage/review-evidence/)

- `node --test scripts/test/ts-callgraph.test.mjs` — 18/18 pass, exit 0 (`node-unit-tests.log`). Includes the three UTF-8 byte-framing regression tests added with the round-4 fix (byte-length Content-Length, chunk split inside a multi-byte sequence, frame after a non-ASCII body not swallowed).
- `cargo test -p gyre-adapters --lib call_graph` — 7/7 pass, exit 0 (`adapters-call-graph-hermetic.log`), including `ts_driver_maps_references_to_edges_via_recorded_protocol` (full driver against the recorded-protocol stub, exact 6-edge set + selectivity assertions) and `ts_driver_warns_and_skips_when_server_missing` (missing-server warning, `[]`, exit 0).
- `cargo test -p gyre-domain --lib detect_all_languages_typescript_markers` — pass (`domain-ts-markers.log`): tsconfig.json, package.json with TS dep, and frontend-subdir package.json all select the TypeScript extractor.
- `cargo test -p gyre-server --lib sync_ts_repo_persists_calls_edges_in_graph_store` — pass (`server-ts-edges.log`; cold compile, needed SKIP_WEB_BUILD=1): Pass 2 TS qnames resolve to Pass 1 node IDs in the graph store.
- Direct driver run against the recorded stub with the test fixture reconstructed in /tmp (`driver-baseline-run.log`): exit 0, exactly the 6 expected edges including `src/café.résumé -> src/math.add` — matching `expected_ts_edges()` in the Rust test.
- **Mutation probes (copies in /tmp; repo untouched)** — prove the hermetic test pins real production behavior, not stub mirroring:
  - Mutation 1 reintroduced the round-4 UTF-16 code-unit framing bug into a copy of the driver: output degraded to 1 edge (café edge lost, wedged-dir cascade skipped pure-ASCII siblings), exit 0 (`mutation-1-utf16-framing.log`). The recorded-protocol test's exact 6-edge assertion fails on this — the regression coverage is real.
  - Mutation 2 removed the "top-level import references are not calls" exclusion: 12 edges including spurious module-attributed import edges (`src/app -> src/math.add`, `src/index -> src/math.mul`) (`mutation-2-import-edges.log`). Both the exact-set and the explicit selectivity assertions fail on this.
- Clippy (`cargo clippy -p gyre-adapters --lib --tests -W clippy::all`): zero findings in task-075 changed code; the single call_graph hit is pre-existing in `gyre-ports/src/call_graph.rs` (untouched by the candidate).
- `cargo fmt --check`: drift confined to files pre-existing at the assigned base (`git2_ops.rs`, `sqlite/audit.rs`, `merge_processor.rs`, … — verified by checking out base `1ae18d1c` and observing the identical drift); no candidate commit touches them.
- Toolchain note: this sandbox has no `typescript-language-server` and no `tsserver.js` anywhere, so `ts_extractor_resolves_edges_with_real_toolchain_when_configured` exercised its documented never-error degradation branch (same as the round-3 review environment). Not a code defect. Remote verification should re-run: `TS_CALLGRAPH_SERVER=<real bin> cargo test -p gyre-adapters --lib call_graph` and the web-UI extraction (implementer's round-4 logs claim 685 edges; not reproducible here).

## Findings

None blocking. Acceptance criteria verified: adapter dispatch (`call_graph.rs` TypeScript arm → node + ts-callgraph.mjs), real LSP subprocess spawn via the recorded protocol, reference→Calls-edge mapping with exact-set assertions, graceful missing-server degradation, pipeline marker selection, integration tests with a TS fixture, focused test/fmt/clippy gates green (workspace-wide `cargo test --all` deferred to verification per the executor wrapper; drift is pre-existing at base).

Non-blocking observations for the audit/merge stages:

1. **Coverage-row note text is stale (safe direction).** `specs/coverage/system/lsp-call-graph.md` rows 5/10 still read "TS — ts-callgraph.mjs exists (tsc compiler API per spec), task-075 not-started, unvalidated" — written by the 86521e2c audit, before this candidate replaced the tsc-compiler-API script with the typescript-language-server LSP driver. The rows remain `task-assigned` (no false completion), and task-073's parallel coverage edit to the same rows is unmerged, so updating the shared row here would only conflict; sibling tasks 073/074 updated their rows in their feat commits, so the TS row refresh should happen at the audit/merge stage. Not a task-contract violation.
2. **Query budget anchor.** `QUERY_BUDGET_MS` (50s) starts inside `extractEdges`, after the LSP initialize handshake (up to 30s timeout), so a pathologically slow initialize could push total runtime past the adapter's 60s kill — worst case degrades to `[]` (contract-compliant never-error) instead of the partial results the budget intends. Real servers initialize in seconds; robustness nuance only.

## Verdict

approved — the task contract is satisfied by production code with independent behavioral evidence; the hermetic test suite demonstrably fails when the driver's key behaviors (byte-exact LSP framing, import-site exclusion) are broken.
