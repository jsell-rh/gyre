# Review: task-075 — LSP Call Graph — TypeScript tsserver Integration

**Reviewer:** Review (independent)
**Candidate:** `09218a0f975960770a6a813b5c57fbe95272af34`
**Base:** `4e304e5d8a9afaa9a1ffc52f1201c430f9808b4a`
**Spec ref:** lsp-call-graph.md §5 Per-Language Implementation (TypeScript), §10 Phase 4
**Status:** approved

---

## Scope

Diff base→candidate touches 9 files: `scripts/ts-callgraph.mjs` (the LSP driver,
rewritten from the old tsc-compiler-API script to the specced
`typescript-language-server --stdio` subprocess approach),
`crates/gyre-adapters/src/call_graph.rs` (doc-table + task-075 driver tests),
`crates/gyre-adapters/tests/fixtures/ts-lsp-stub.mjs` (recorded-protocol stub),
`crates/gyre-domain/src/call_graph_resolve.rs` (TS-marker detection tests),
`crates/gyre-server/src/graph_extraction.rs` (TS persistence test),
`scripts/test/ts-callgraph.test.mjs` (18 node:test unit tests),
`.github/workflows/ci.yml` (wires `node --test scripts/test/*.test.mjs` into
the dev-controller job), `docs/server-config.md`
(`TS_CALLGRAPH_SERVER`/`TS_CALLGRAPH_TSSERVER`), `specs/tasks/task-075.md`.

## Acceptance criteria — independent verification

All probes below were run in this review sandbox against the exact candidate;
commands, exit codes, and output are under `/tmp/stage/review-evidence/`.

1. **TypeScript adapter in `gyre-adapters`** — dispatch arm
   (`Language::TypeScript => find_script("ts-callgraph.mjs")` + node) verified
   in source; the driver it invokes is the real implementation (criterion 2–3
   evidence below), not a stand-in.
2. **Spawns `typescript-language-server --stdio`** — verified against the REAL
   toolchain: installed `typescript-language-server` + `typescript` 5.x into
   `/tmp/ts-toolchain`, ran
   `ts_extractor_resolves_edges_with_real_toolchain_when_configured` in
   isolation with `TS_CALLGRAPH_SERVER` set (`--nocapture`): the real-server
   branch was taken (no degradation note; 3.6s wall) and the full 6-edge set
   was asserted against the real server (`adapters-callgraph-real-server.log`).
3. **Correctly maps references → `Calls` edges** — the hermetic
   recorded-protocol test (`ts_driver_maps_references_to_edges_via_recorded_protocol`)
   asserts the exact 6-edge set (cross-module import, arrow-const callee,
   non-exported callee, barrel-mediated call, JSX usage, non-ASCII identifier)
   plus selectivity (no module-attributed import edges, no edge onto a
   re-export-only symbol). The stub replays responses recorded from the real
   server (realistic positions/details, URI rewriting — not a mirror of driver
   output). Mutation probe (below) proves the test pins real behavior.
4. **Missing server handled gracefully** —
   `ts_driver_warns_and_skips_when_server_missing` passes: stderr warning,
   `[]`, exit 0.
5. **Pipeline selects TS extractor for TS markers** —
   `detect_all_languages_typescript_markers` (tsconfig.json / package.json
   with TS dep / frontend-subdir package.json) passes; production detection
   logic pre-exists from task-072 (base), pinned by new tests.
6. **Integration test with a TS fixture** — recorded-protocol test (hermetic,
   full `SubprocessCallGraphExtractor` path) + real-toolchain test + the
   server-side `sync_ts_repo_persists_calls_edges_in_graph_store` (Pass 2
   qnames resolve to Pass 1 node IDs in the graph store) all pass.
7. **`cargo test --all` / fmt** — focused probes: adapters call_graph suite
   6/6 (hermetic AND with real toolchain configured); gyre-domain
   call_graph_resolve 17/17; gyre-server graph_extraction 26/26;
   `check-rustfmt-diff.py 4e304e5d` clean (3 Rust files); clippy: zero
   warnings on changed lines in all three changed Rust files. Full
   workspace-wide `cargo test --all`/clippy belong to verification (executor
   wrapper rejects workspace invocations here).

## Mutation probe (test strength)

Temporarily reintroduced the round-4 framing bug class into
`scripts/ts-callgraph.mjs` `extractMessages` (decode-then-slice by code units
instead of byte-based slicing), then restored (working tree verified clean):

- `node --test scripts/test/ts-callgraph.test.mjs`: **3 failures** (the three
  byte-framing regression tests) — tests kill the bug.
- Hermetic Rust suite: `ts_driver_maps_references_to_edges_via_recorded_protocol`
  **FAILS** — 5 of 6 edges lost (`left:` shows only `greetTwice →
  makeGreeting`), exactly the original bug's silent-data-loss symptom.

The recorded-protocol test is not self-confirming: it fails when the
production behavior is broken.

## Real-toolchain end-to-end (beyond CI's reach)

- Driver run against the Gyre `web/` UI with the real server: **706 edges,
  exit 0, 0 malformed**, ~8s; 67 `.svelte` files skipped with the specced
  warning, 52 svelte-importing files skipped with a warning
  (`web-ui-run.json`, `web-ui-run.stderr`). Edge content is semantically
  sensible (e.g. `view-spec.validateViewSpec → view-spec.isObject`).
- The real-toolchain adapter test asserts the same 6-edge fixture set against
  the real `typescript-language-server` + tsserver 5.9.x.

## Spec-conformance notes

- The spec (§5 TypeScript) recommends "LSP via `typescript-language-server`,
  same pattern as Rust/Python" — implemented exactly; the previous
  tsc-compiler-API script is replaced by the LSP subprocess driver.
- Svelte handling matches the task instruction: `.svelte` files skipped with
  a warning; additionally files importing `.svelte` modules and
  wedged-directory siblings are skipped with counted, logged warnings —
  documented degradation, never silent, always `[]` + exit 0.
- qname format matches Pass 1 (`typescript_extractor.rs
  module_qname_from_path`), pinned end-to-end by the server persistence test.
- All failure modes (missing server, invalid tsserver, initialize failure,
  per-file/per-query timeouts, 50s budget vs the adapter's 60s kill) degrade
  to `[]` + exit 0 — Pass 2 never fails a push, per the port contract.

## Non-blocking observations (recorded for follow-up, not defects against this task's contract)

1. **`EnvVarGuard` removes rather than restores `TS_CALLGRAPH_SERVER`**
   (`crates/gyre-adapters/src/call_graph.rs`). Under `--test-threads=1` with
   the env var externally set, the recorded-protocol test's guard clobbers the
   ambient value, so `ts_extractor_resolves_edges_with_real_toolchain_when_configured`
   silently falls to its degradation branch (verified: the note prints and the
   suite finishes in 0.6s instead of ~3.3s). Under default parallel execution
   the real branch is taken (verified: 3.34s, real-server assertions ran).
   Impact is limited to assertion strength of this supplementary test in a
   non-default configuration; the load-bearing hermetic test is unaffected,
   and the real-server behavior was independently verified in isolation
   during this review. A save/restore guard would fix it.
2. Pre-existing, untouched by this candidate: `cargo clippy -p gyre-domain
   --lib` errors on `rust_extractor.rs:1071` (`never_loop`, file unchanged
   since d7940e85, before base) under this sandbox's clippy 0.1.99 —
   toolchain drift owned by workspace-wide verification, not a task-075
   change. The changed-lines clippy gate itself is clean on all three changed
   Rust files (verified with `--allow clippy::never_loop` to get past the
   pre-existing error).
3. Pre-existing Pass 1 nit noticed while checking the qname contract:
   `typescript_extractor.rs:module_qname_from_path` uses `rfind('.')` on the
   whole relative path, so a file under a dotted directory (e.g.
   `src/my.dir/f.ts`) would get a wrong Pass 1 module name. The task-075
   driver uses `path.extname` (correct). Out of task-075 scope (Pass 1 file,
   unchanged); noting for a future Pass 1 task.

## Verdict

**Approved.** Every acceptance criterion is backed by real, independently
reproduced evidence (real toolchain, real web UI, mutation-killed tests); the
implementation is the specced LSP-subprocess approach with genuine graceful
degradation, and no fake/hollow implementation was found.
