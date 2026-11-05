# Review — task-075 (LSP Call Graph — TypeScript tsserver Integration)

Spec: `specs/system/lsp-call-graph.md` §5 TypeScript ("Recommendation: LSP via `typescript-language-server`"), §10 Phase 4. Task contract: `specs/tasks/task-075.md`.

Commits under review: `ecf3ea5b` (implementation), `43837ee9` (lint fix), on candidate `bf1f9567` (base `5df14f34`). Cumulative diff touches `scripts/ts-callgraph.mjs` (rewrite: tsc-compiler-API walker → LSP subprocess), `crates/gyre-adapters/src/call_graph.rs` (TS driver tests; dispatch itself is pre-existing task-072 code), `crates/gyre-adapters/tests/fixtures/ts-lsp-stub.mjs` (recorded-protocol stub), `scripts/test/ts-callgraph.test.mjs` (node:test unit tests), `gyre-domain`/`gyre-server` test additions, CI wiring, `docs/server-config.md`.

Verdict: **needs revision** (one code finding; see `/tmp/stage/verdict.json`).

This round had something the three earlier rounds did not: a real `typescript-language-server` install (`/tmp/ts-ls-install`, server 0.x + tsserver 5.9.3, installed via npm during review), so the real-toolchain path was exercised, not just the degradation branch.

## Independently verified working

- **Real server, task fixture**: manual driver run on the `write_ts_fixture` layout produced exactly the 5-edge set the hermetic stub test asserts (`src/app.compute→src/math.add`, `src/app.greetTwice→src/app.makeGreeting`, `src/consumer.useViaBarrel→src/math.add` barrel, `src/handler.handler→src/handler.serve` non-exported, `src/widgets.renderBadge→src/widgets.Badge` JSX); exit 0 (`review-evidence/real-toolchain-test.log`, `web-run-*` for the below).
- **Real server, cargo test**: `TS_CALLGRAPH_SERVER`+`TS_CALLGRAPH_TSSERVER` set → `ts_extractor_resolves_edges_with_real_toolchain_when_configured` passes; full `cargo test -p gyre-adapters call_graph` 6/6.
- **Gyre web UI** (`web/`): 53 edges, first one exactly the shipped example (`src/lib/types/view-spec.validateViewSpec -> src/lib/types/view-spec.isObject`); 67 `.svelte` skipped with warning, svelte-importers skipped, `src/__tests__`/`src/lib` wedged-dir skips logged; exit 0. Raw-chunk instrumentation of a driver copy confirms those web-run wedges involve **ASCII-only** frames — genuine tsserver stalls, not the finding below.
- **Hermetic suite**: node unit tests 15/15; `gyre-domain` call_graph 17/17 (TS marker selection incl. frontend-subdir `package.json`); `gyre-server` `sync_ts_repo_persists_calls_edges_in_graph_store` 1/1.
- **Degradation contracts**: missing server (env override) → stderr warning + `[]` + exit 0; npm-latest TypeScript 7.0.2 (no `tsserver.js`) → rejected, initialize fails, clean `[]` + exit 0 — both as documented in `docs/server-config.md`.
- **Fmt**: `cargo fmt --check` drift is confined to files brought by the base merge (`git2_ops.rs`, `sqlite/audit.rs`, `merge_processor.rs`, …) — no commit in `5df14f34..HEAD` touches them; none of the task's three Rust files appear in the drift. Owned by verification, matching the round-2 ruling.
- **Contract text**: assignment sections byte-identical (checkboxes still `[ ]`, notes only under `## Shipped`); both task commits present in frontmatter `commits:`.

## Finding (blocking) — receive framing mixes bytes with UTF-16 code units

`scripts/ts-callgraph.mjs`:

- `LspClient`'s stdout handler (line 443) accumulates `this.buffer += chunk.toString()` — a JS string measured in UTF-16 code units.
- `extractMessages` (line 118) parses the wire `Content-Length` (bytes) and gates on `rest.length < bodyStart + contentLen` (line 131), slicing `rest.slice(bodyStart, bodyStart + contentLen)`.

Any response body containing multi-byte UTF-8 can never satisfy the byte-count check with code-unit arithmetic. `typescript-language-server` emits raw UTF-8 (JSON.stringify does not escape non-ASCII), so a `documentSymbol` result for a file with a non-ASCII symbol name stalls forever: request times out (30s first / 10s later), the file is skipped, and the wedged-directory heuristic then skips the file's pure-ASCII siblings too.

Reproduction (real server + tsserver 5.9.3; evidence `raw-chunk-log.log`, `utf8-framing-probe.log`):

- Fixture `/tmp/tsfix-utf8`: `src/café.ts` with `export function résumé()` calling `add` from `src/math.ts`.
- Raw frame captured: `Content-Length: 250` (bytes) with body `"…{\"name\":\"résumé\",…}"` = 248 code units — the server **answers correctly**; the client's length check can never pass.
- Driver output: `documentSymbol failed for …/café.ts: LSP request timed out …; skipping the rest of …/src`, then `skipped 1 further file(s)…`, `[]`, exit 0, ~32s elapsed. Correct output: the `src/café.résumé → src/math.add` edge (both names are valid Pass 1 qnames — Pass 1's tree-sitter walker handles non-ASCII identifiers fine; only Pass 2 loses them).

Exposure is not limited to non-ASCII identifiers: non-ASCII in `documentSymbol` `detail` fields (e.g. string-literal types like `"↑" | "↓"`), and non-ASCII paths or text in server-initiated `window/logMessage` notifications (which precede responses in the stream) trigger the same stall. Once later data arrives behind a stalled message, the over-long code-unit slice swallows the next header and the parser drops subsequent messages (desync). Net effect: silent, warned-only loss of `Calls` edges — the task's "Correctly maps reference results to Calls edges" criterion and the spec's Pass 2 completeness promise — for affected repos.

Fix sketch: accumulate the stream as a `Buffer`, find the header/body boundaries by byte offset, `slice` the body as bytes, and decode each body to a string only after extraction (the send path already counts bytes correctly; the receive path must match).

## Minor (non-blocking, recorded for completeness)

- `ts_extractor_resolves_edges_with_real_toolchain_when_configured` reads `TS_CALLGRAPH_SERVER` without holding `ENV_LOCK` while the recorded-protocol test mutates it via `EnvVarGuard`; both interleavings pass (stub returns the same fixture edges; unset takes the degradation branch), so the race is benign today.
- The hermetic stub shares the driver's `extractMessages` framing bug class by construction (it also string-accumulates), which is why CI never caught the finding; the real-toolchain test is the only guard that could have, and it requires `TS_CALLGRAPH_SERVER` to be configured in the environment that runs it.
- `npm install typescript` now resolves to 7.x (no `tsserver.js`); the driver's documented rejection is correct behavior, but CI/verification environments wanting the real-server path must pin a TS 5.x line (as this review did with `typescript@5.9`).

## Probe inventory (all logs under /tmp/stage/review-evidence/)

`node-unit-tests.log` (15/15, exit 0), `adapters-call-graph.log` (6/6, exit 0), `domain-call-graph.log` (17/17, exit 0), `server-ts-integration.log` (1/1), `real-toolchain-test.log` (real server, 6/6 incl. `ts_extractor_*`), `web-run-edges.json`/`web-run-warnings.log` (53 edges, exit 0), `raw-chunk-log.log` (instrumented driver copy — server answers café.ts, client stalls), `utf8-framing-probe.log` (synthetic minimal repro), `raw-lsp-probe.log` (superseded first attempt). Production tree untouched: working tree clean at candidate `bf1f9567` after restoring `web/dist` artifacts rebuilt by `cargo test -p gyre-server`'s build.rs.
