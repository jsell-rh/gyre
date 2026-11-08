# Review — task-080 (Explorer Integration & Visual Tests, §31–32)

Candidate: `b1f059a4d1a6a5a536c6bda40ea5710f6cb69d84` (base `637be7187617fcd00ccc75c70a6838bae9ad94eb`).
Verdict: **approved**.

## Scope

Diff base→candidate is test-and-specs only: `crates/gyre-server/tests/explorer_ws_integration.rs`
(+4 tests: full session lifecycle, agent view-query generation, self-check broad-group
refinement, REST saved-view round-trip; LLM scripting rewritten onto a local
`ScriptedLlmFactory` with a shared cross-port call counter), `web/tests/e2e/explorer-visual.spec.js`
(+`filter_preset_buttons_show_correct_subsets` with two independent verification layers;
pixelmatch threshold tightened 0.2→0.05 on all preset tests), two new baselines
(`filter-preset-{endpoints,types}-linux.png`, 919×363 — the CI-repair commit `9f7cc5250`),
coverage §31/§32 → implemented, task shipped notes. No production code changed — correct for a
test-delivery task. Counts verified: 15 Rust tests, 16 Playwright tests.

## Independent probes (evidence in /tmp/stage/review-evidence/)

- `cargo test -p gyre-server --test explorer_ws_integration` (cold, single-job via
  /tmp/stage/bin/cargo): **15 passed, 0 failed** (`rust-explorer-ws-integration.log`) — including
  all four new §31 tests, on the exact candidate.
- Playwright `explorer-visual` full suite against the candidate's debug server + committed
  `web/dist`: **16/16 passed** (`explorer-visual-full-suite.log`), including the previously
  CI-failing `filter_preset_buttons_show_correct_subsets`. A fresh `npm run build` of the
  candidate's web source reproduced `web/dist` byte-identically (git-clean after build), so the
  committed bundle proves the head's UI code. Local chromium is revision 1208, the same revision
  CI uses.
- **Baseline-swap mutation** (endpoints↔types PNGs swapped): test **fails** — 72,435 px differ
  (ratio 0.22 vs 0.02 allowed at threshold 0.05). The regenerated baselines are not
  interchangeable (`baseline-swap-mutation.log`); restored afterwards, test passes again.
- **Production-dimming mutation** (`nodeFilterOpacity()` forced to 1.0 in
  `web/src/lib/canvas-filters.js`, dist rebuilt): test **fails on the baseline-independent
  behavioral assertion** — Endpoints preset transition repaints 19,069 px < 20,000 threshold
  (`dimming-disabled-mutation.log`). Source and dist restored (git-clean), test passes again.
- `npx vitest run src/__tests__/canvas-filters.test.js`: 18/18 passed. The full vitest suite is
  unaffected by this diff (vitest includes only `src/**/*.test.js`; no `web/src` file changed
  base→candidate) — full `cargo test --all` / full vitest belong to the verification stage.
- `scripts/check-task-commit-attribution.sh`: OK (all six task-labeled product commits recorded
  in frontmatter).

## Code-level verification (no findings)

- The broad-group self-check test drives real production behavior: `view_query_resolver.rs`
  emits "Group 'X' matched N nodes - too broad" for >20 substring/exact matches;
  `is_actionable_warning` classifies it actionable; the native agent loop (explorer_ws.rs
  ~3130–3300) really calls `LlmPort::complete_with_tools`, dry-runs server-side against the
  seeded graph store, sends `refining`, and forwards exactly the refined query. Assertions
  (refining observed, exactly 2 LLM calls, exactly 1 view_query) map to real loop mechanics.
- The agent-generates-view-query test exercises the real no-LLM fallback: candidate names are
  extracted from the question and validated against actual graph nodes (exact then substring)
  before emitting a focus scope; the result is also deserialized against the ViewQuery grammar.
- The lifecycle test's disconnect loop fails on timeout (closed stays false → assert fails);
  Close reply, stream end, or stream error all count as closed. Not vacuous.
- The REST round-trip test hits real POST/GET/DELETE `/api/v1/repos/:id/views` with exact
  query-JSON equality and 404-after-delete.
- The CI failure this round repaired (run 42466473984 at `4ea64f4de`: expected 919×397 vs
  received 919×363) was a pure stale-baseline size mismatch; the regenerated baselines are
  919×363, consistent with every other canvas-area baseline on the post-task-083 UI, and pass
  locally in the CI chromium revision.

## Notes for verification

GitHub CI on the exact head `b1f059a4d` remains to confirm (the prior run failed only on the
now-regenerated baselines; local rendering uses the same chromium revision and passes 16/16).
Workspace-wide `cargo test --all` and the full vitest suite are deferred to the verification
stage per the review wrapper's focused-probe rule.
