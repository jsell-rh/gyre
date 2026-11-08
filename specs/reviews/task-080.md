# Review: TASK-080 — Explorer Integration & Visual Tests

**Reviewer:** Independent review (gyre review stage)
**Candidate:** `99f9ad989fbb3d26479311e79af7796b10040c4a`
**Base:** `78cd03102d1d2a0226bc6ad4a45d454cb8aac928`
**Spec ref:** explorer-implementation.md §31 (Integration Tests), §32 (Visual Tests)
**Evidence:** `/tmp/stage/review-evidence/` (all probes run fresh on the exact candidate head in this sandbox; no implementer evidence was reused — this sandbox starts with an empty evidence directory)

## Scope check

Diff from the assigned base is test/spec-only: `crates/gyre-server/tests/explorer_ws_integration.rs` (+503), `web/tests/e2e/explorer-visual.spec.js` (+181, repair), two new preset baselines, coverage matrix, task notes. No production code changed. The three attributed commits (`659f64e8e`, `da74a27ff`, `d4c1bd0dd`) are all test/spec-scoped.

## Prior durable findings — both resolved, independently re-verified

1. **Deleted mainline self-check tests (bbc6c272 round).** The candidate carries all 15 tests in `explorer_ws_integration.rs` (11 mainline incl. both restored self-check refinement tests + 4 task-080 tests). Fresh run: **15 passed, 0 failed** (`explorer-ws-integration-15tests-fresh.log`).
2. **Preset visual test could not fail (124d0256 round).** Repaired by commit `d4c1bd0dd` with two independent layers, both re-verified by fresh mutation/swap probes below.

## §31 Integration Tests — verified

- `explorer_ws_full_session_lifecycle`: connect → `list_views` exchange → full message turn (thinking status, streamed text done=true, view_query) → clean client Close with server-side close confirmed. Passes.
- `explorer_ws_agent_generates_view_query_from_question`: seeds 5 real graph nodes, asks "What would break if I change Space?" over the production no-LLM fallback, asserts a `focus` scope on `Space` that deserializes against the ViewQuery grammar. Passes.
- `explorer_ws_self_check_loop_refines_broad_groups`: 25 seeded `node_N` nodes make a substring-matched broad group exceed the 20-node breadth threshold; scripted in-process `LlmPort` drives the native agent path. Asserts `refining` status, exactly 2 LLM calls, exactly one (refined) view_query forwarded. Passes.
- Restored mainline `explorer_ws_self_check_refinement_flow` and `explorer_ws_self_check_sends_best_version_after_cap` cover the unresolved-callout refinement and the `best_view_query` tracker at the refinement cap. Passes.
- `explorer_rest_saved_view_round_trip`: POST 201 → list contains it → GET returns exactly-equal query JSON → DELETE 204 → re-load 404. Passes. WS save→list→load is covered by the mainline `explorer_ws_save_and_load_view`. Passes.
- **Mutation probe (fresh):** `MAX_REFINEMENT_TURNS = 0` in `explorer_ws.rs` → all three self-check tests FAIL (`mutation-max-refinement-turns-0.log`); production code restored, tree verified clean. The self-check tests have teeth.

## §32 Visual Tests — verified

- **Full suite on current source:** rebuilt `web/dist` from the candidate's `web/src` (rebuild is byte-identical to the committed bundle — the committed bundle proves current UI code), rebuilt server, ran the 16-test Playwright suite against a directly-launched debug server: **16 passed** (`explorer-visual-full-suite.log`, re-confirmed after all mutation restores in `explorer-visual-full-suite-final.log`).
- **Behavioral assertions are real:** a temporary probe spec (deleted afterward; repo tree clean) reproducing the test's transitions measured zoom=239,367 / Endpoints=100,785 / Endpoints→Types=127,483 changed pixels on the healthy build (`probe-states-run.log`, `analyze-probe-states.log`).
- **Mutation probe (fresh):** `nodeFilterOpacity()` forced to `1.0` in `web/src/lib/canvas-filters.js`, web+server rebuilt → `filter_preset_buttons_show_correct_subsets` **FAILS** on the real test file at the behavioral assertion (19,225 < 20,000; `mutation-dimming-disabled-visual.log`); the probe measured the Endpoints→Types transition at **38 px** under the same mutation (`mutation-probe-counts.log`). The vitest unit layer independently catches the same mutation with 5 failures (`mutation-dimming-disabled-unit.log`). Production code restored; tree verified clean.
- **Baseline-swap probes (fresh, on the healthy build):** swapping the Endpoints baseline for the Types baseline fails (126,282 px differ) and for the default-view baseline fails (129,054 px differ) — both far above the 7,296-px 2% allowance (`baseline-swap-endpoints-{types,defaultview}.log`). The prior interchangeability finding is repaired. Pixel-level analysis of the committed baselines with the suite's own bundled pixelmatch at threshold 0.05 (`baseline-pixel-comparison.log`, `baseline-matrix.log`) independently confirms the recorded Endpoints state is a real dimmed-subset render: it shares geometry with the 5-wheel-zoom state (279,907 identical px vs the zoom-level-1 baseline) plus ~61k dimming-band pixels, and differs from every other recorded state by ≥126k counted pixels.
- Semantic zoom (3 levels), view-query rendering (groups/callouts/narrative + annotation bar), blast radius (tiered coloring + fixed-node dimming) all covered and passing.

## Acceptance criteria

- WS lifecycle / agent ViewQuery / self-check refinement / saved-view round-trip (WS+REST): **verified** (fresh runs above).
- Visual tests for semantic zoom / view-query rendering / filter presets: **verified**.
- `cd web && npm test`: **68 files, 1688 passed, 41 skipped, 0 failed** on the candidate head (`vitest-full-suite.log`).
- `cargo test --all`: focused `cargo test -p gyre-server --test explorer_ws_integration` = 15/15 on the exact head; the diff touches no other crate (test-only), so full-workspace results are left to the verification stage per review-scope constraints.

## Non-blocking observations (recorded for future hardening; do not affect the verdict)

1. **The Types-preset screenshot layer is weak in isolation.** The committed `filter-preset-types` baseline is within the suite's own allowance of the unfiltered default view (3,591 px < 7,296 px at threshold 0.05) — swapping the Types baseline with the default-view baseline still passes the test (`baseline-swap-types-defaultview.log`). This is not a greenwash path: the dimming-dependent behavioral assertion fails at 38 px with dimming disabled, and a wrong-subset render at the zoomed state would miss the Types baseline by ~126k px. The two layers cover each other's gaps, so the combined test is sound; but a future contributor re-recording only the Types baseline from a subtly wrong build would not be caught by that layer alone.
2. **Thin margin on the Endpoints behavioral assertion under mutation.** With dimming disabled, the Endpoints-click transition measured 19,225 px in the real-test run (fails) but 20,634 px in the probe run (would pass) — the 20,000 threshold sits inside run-to-run variance for the mutated build. The test still fails under the mutation via the Endpoints→Types assertion (38 px) and the screenshot layer (~60k+ px), so the outcome is robust, but the Shipped phrase "transitions repaint ≤19k/38 px" is slightly optimistic for the Endpoints transition specifically.
3. **Clicking the Types toolbar button resets the camera to the overview** (the Endpoints→Types repaint is dominated by a dimming-coupled camera refit; with dimming disabled no refit occurs). This is pre-existing production behavior of the base (`web/src` is unchanged from the assigned base) and outside this candidate's test-only diff; the recorded baselines faithfully reflect actual behavior. Noted in case a later task wants preset clicks to preserve camera state.

## Sandbox restrictions (for remote verification, not code defects)

Loopback HTTP from agent-issued curl is SSRF-guarded; the visual suite ran against a directly-launched debug server (`GYRE_PORT=2222 GYRE_AUTH_TOKEN=e2e-test-token`) with Playwright's `reuseExistingServer`, node-based health checks, and `PLAYWRIGHT_BROWSERS_PATH=/usr/local/share/gyre-playwright`. Exact-head GitHub CI with a CI-spawned server remains mandatory.

## Verdict

**Approved.** All §31/§32 scenarios have real, passing tests on the exact candidate head; both prior review findings are genuinely repaired; the preset test now fails under a behavior-disabling production mutation and under baseline swaps, verified by fresh independent probes; the Shipped/coverage claims match measured reality (mutation counts reproduce within run-to-run variance). Full-workspace `cargo test --all` and exact-head CI belong to the verification stage.
