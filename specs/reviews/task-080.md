# Review — task-080 (Explorer Integration & Visual Tests)

Spec: `specs/system/explorer-implementation.md` §31 (Integration Tests), §32 (Visual Tests).
Candidate: `9fa9da636` (branch `pipeline/task-080/124d02569c594a589ffc85c1367ee899-1`), assigned base `39c7759f`.
Task commits `da74a27f` (restored mainline self-check tests) and `659f64e8` (explorer integration + filter-preset visual test) are both ancestors of HEAD; attribution intact.
Verdict: **needs-revision** — §31 is genuinely done and mutation-verified; the new §32 filter-preset visual test is provably unable to fail when the specced behavior breaks, and the Shipped notes make a false verification claim about it.

All probes ran on the exact candidate SHA with a working tree verified clean afterward (`git status` empty, HEAD = `9fa9da636`). Evidence under `/tmp/stage/review-evidence/`.

## §31 Integration Tests — verified, no findings

- `cargo test -p gyre-server --test explorer_ws_integration` → **15 passed, 0 failed** (11 mainline + 4 task-080), twice (initial + post-mutation-restore final clean run; logs `explorer-ws-integration-rerun.log`, `explorer-ws-integration-final-clean.log`).
- All four §31 scenarios have real tests: `explorer_ws_full_session_lifecycle` (connect → list_views → full message turn with thinking/streamed-text-done/view_query/ready → client Close with server confirmed closing, 5s deadline), `explorer_ws_agent_generates_view_query_from_question` (seeds 5 real graph nodes; no-LLM fallback extracts "Space" and validates against the graph store; query deserializes against the ViewQuery grammar), `explorer_ws_self_check_loop_refines_broad_groups` (scripted `LlmPort` drives the native agent path; 25 seeded nodes trip the >20 breadth threshold; asserts `refining` status, exactly 2 LLM calls, exactly one — the refined — view_query forwarded), `explorer_rest_saved_view_round_trip` (POST 201 → list contains it → GET returns exactly-equal query JSON → DELETE 204 → 404). WS save/load covered by mainline `explorer_ws_save_and_load_view`.
- The two mainline self-check tests lost in the earlier merge (`explorer_ws_self_check_refinement_flow`, `explorer_ws_self_check_sends_best_version_after_cap`) are restored and pass — the repair claim checks out.
- Mutation checks (temporary edits, each restored immediately, tree re-verified clean):
  - `MAX_REFINEMENT_TURNS = 0` → all three self-check tests **fail** (`mutation-max-refinement-turns-0.log`) — the pass is attributable to the real refinement loop.
  - Native-path best-version selection bypassed (send current attempt instead of best) → `..._sends_best_version_after_cap` **fails** with "got 3 callouts / expected 1" (`mutation-ignore-best-version.log`) — the `best_view_query` tracker is load-bearing.
  - Fallback discards the graph-store-validated node match (uses first question candidate instead) → `..._agent_generates_view_query_from_question` **fails** with node "change" instead of "Space" (`mutation-discard-graph-validation.log`) — the test genuinely depends on real graph-node validation.

## §32 Visual Tests — one blocking finding

Environment note: the full Playwright suite runs fine in one process in this sandbox (chromiumSandbox:false + `--no-zygote`, `PLAYWRIGHT_BROWSERS_PATH=/usr/local/share/gyre-playwright`, directly-launched debug server binary with `reuseExistingServer`, `GYRE_AUTH_TOKEN=e2e-test-token` exported for the Playwright process — the seed fixture reads it from its own env). Fresh `npm run build` + `SKIP_WEB_BUILD=1 cargo build -p gyre-server` so the embedded bundle is current head source. **16/16 pass** (`explorer-visual-rerun2.log`), and `npm ci && npm test` → **67 files, 1669 passed, 41 skipped, 0 failed** (`web-vitest.log`). All four §32 scenarios have tests with committed baselines.

### F1 (major): `filter_preset_buttons_show_correct_subsets` cannot fail when preset filtering breaks — the acceptance criterion "Filter presets show correct subsets" has no real visual proof

Three independent proofs on the exact head:

1. **Production mutation.** `nodeFilterOpacity()` in `web/src/lib/canvas-filters.js` forced to `return 1.0` (preset dimming fully disabled — the exact behavior the test exists to verify). Web + server rebuilt, all four filter-preset visual tests **still pass** (`mutation-dimming-disabled-visual.log`). The vitest unit suite catches the same mutation with 5 failures (`mutation-dimming-disabled-unit.log`) — so the dimming logic is real and unit-tested, but the visual layer proves nothing about it. Per the review bar, a test that still passes with the relevant production behavior disabled is not proof.
2. **Baseline swaps.** The test passes when `filter-preset-endpoints.png` is replaced by the types baseline, the all baseline, or even the wrong-dimension... same-dimension zoom-level-0 baseline (`baseline-swap-probe{,2,3}.log` — all passed; Playwright did not rewrite the swapped files, verified by md5).
3. **The committed endpoints baseline is the default view.** `filter-preset-endpoints-linux.png` is pixel-identical (0/364843 pixels, 161 identical colors) to `zoom-level-0-packages-linux.png` — the unfiltered, fully-zoomed-out state. The recorded "Endpoints preset" screenshot shows no visible difference from applying no filter at all; the test's 5× wheel-zoom setup (added because "at the fully zoomed-out package overview the leaves are too small to distinguish presets", per its own comment) is undone before the screenshot is taken.

Root cause: Playwright's default pixelmatch `threshold: 0.2` (YIQ perceptual distance) discounts dark-theme dimming shifts — the comparator counts only **344** differing pixels between the endpoints and types states and **2371** between zoom levels 0 and 1, all far below the 7296-pixel (2%) allowance. Direct runs of Playwright's own comparator (`comparator-probe.log`, `pixelmatch-probe2.log`, `pngjs-diff-probe.log`) confirm every same-size baseline pair in the suite is interchangeable.

Remedy: pass a tighter `threshold` (e.g. 0.05) or the `ssim-cie94` comparator in the `toHaveScreenshot` options for these tests; re-record baselines at a zoom state where leaf dimming is actually visible (and assert the zoom took effect, e.g. via canvas transform state, since the current wheel-dispatch + preset-click sequence lands back at the fit view); then verify the recorded Endpoints baseline is no longer identical to the default view and that the test fails under the `nodeFilterOpacity → 1.0` mutation.

### F2 (process): false verification claim in Shipped notes

The Shipped notes state: "the All/Endpoints/Types states render pixel-distinct screenshots (not just below the 2% diff threshold), so a preset that stops applying its filter fails the test." Empirically false per F1 (all three probes passed with wrong baselines / disabled behavior). The §32 coverage flip to `implemented` cites this claim. Correct the notes and re-justify §32's "Filter presets show correct subsets" row on evidence that survives a behavior-disabling mutation — the `aria-pressed` assertions are real but only prove button toggle state, not correct subsets on the canvas.

### Non-blocking observations (pre-existing, inherited from base — not this candidate's diff)

- The same comparator weakness affects the pre-existing zoom/view-query/blast-radius screenshot tests: empirically, `zoom_level_1_modules` passes with its baseline swapped for the zoom-level-0 baseline (`zoom-baseline-swap-probe.log`). These tests predate this candidate (task-057 lineage, present in the assigned base). Any §32 hardening round should tighten them together with the preset tests.
- Sandbox restriction recorded for remote verification (not a code defect): loopback HTTP from agent-issued curl probes is SSRF-guarded; server probes used a directly-launched prebuilt binary. Exact-head GitHub CI remains mandatory.

## What the revision round must deliver

1. Fix F1: discriminating screenshot configuration + re-recorded preset baselines (Endpoints must differ from the default view), verified by re-running the `nodeFilterOpacity → 1.0` mutation and confirming the test now fails.
2. Fix F2: corrected Shipped notes.
3. Re-run: `cargo test -p gyre-server --test explorer_ws_integration` (15/15 — should be unaffected), `cd web && npm test`, and the Playwright explorer-visual suite.
