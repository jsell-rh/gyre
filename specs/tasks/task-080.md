---
title: "Explorer Integration & Visual Tests"
spec_ref: "explorer-implementation.md §31–32"
depends_on:
  - task-069
  - task-070
  - task-065
progress: ready-for-review
coverage_sections:
  - "explorer-implementation.md §31 Integration Tests"
  - "explorer-implementation.md §32 Visual Tests"
commits: ["2f14f11f85cfffacad1670a262d84280225d38a1", "d4c1bd0ddc855dbf40b76e4ec138fc0ac3c10a11", "da74a27ff27141e98d6b79532ced8730ba494ba8", "659f64e8e32cc4317f9b3b78f39b58c3561d1d3e"]
---

## Spec Excerpt

**Integration Tests:**
- WebSocket explorer session lifecycle (connect, send message, receive response, disconnect)
- Agent generates valid view query from question
- Self-check loop catches and fixes overly broad groups
- Saved view round-trip (save → load → render)

**Visual Tests:**
- Semantic zoom at different zoom levels
- View query rendering (groups, callouts, narrative markers)
- Filter presets show correct subsets
- Blast radius interactive mode

## Implementation Plan

1. **WebSocket session lifecycle test:**
   - Connect to `WS /api/v1/repos/:repo_id/explorer` with a valid token
   - Send a `message` type with text and canvas_state
   - Receive `status` messages (`thinking`, `refining`, `ready`)
   - Receive `text` messages (streamed)
   - Receive `view_query` message with valid ViewQuery JSON
   - Disconnect cleanly
   - Use the existing test infrastructure in `crates/gyre-server/tests/`

2. **Agent view query generation test:**
   - Seed a test repo with known graph data (nodes + edges)
   - Send a question like "What would break if I change Space?"
   - Verify the response includes a view_query with scope referencing "Space"
   - Note: This requires an LLM — use a mock or set `GYRE_EXPLORER_SDK=0` for the fallback path

3. **Self-check loop test:**
   - Create a graph fixture where a broad query ("show everything in domain") would trigger the "too broad" warning
   - Send a question that would produce such a query
   - Verify the agent refines (multiple view_query messages before final)
   - This may need the mock LLM path for deterministic testing

4. **Saved view round-trip test:**
   - Connect via WebSocket
   - Send `save_view` message with a name, description, and query
   - Send `list_views` message → verify the saved view appears
   - Send `load_view` message → verify the view query is returned
   - Also test via REST API: `POST /api/v1/repos/:id/views` → `GET /api/v1/repos/:id/views/:view_id`

5. **Visual tests (frontend):**
   - Use vitest + browser mode or Playwright for screenshot tests
   - Test canvas rendering at zoom levels 0.5x, 1.0x, 2.0x
   - Test view query rendering with groups (colored regions), callouts (labels), narrative (numbered markers)
   - Test filter presets: click "Endpoints" → only endpoint nodes visible
   - Test blast radius: click a node → verify connected nodes highlighted, others dimmed
   - Compare against baseline screenshots (store in `web/test-results/`)

## Acceptance Criteria

- [ ] WebSocket session lifecycle test: connect, message, response, disconnect
- [ ] Agent generates valid ViewQuery from a natural language question
- [ ] Self-check loop test: agent refines overly broad queries
- [ ] Saved view round-trip: save → list → load works via WebSocket and REST
- [ ] Visual test for semantic zoom at multiple levels
- [ ] Visual test for view query rendering (groups, callouts, narrative)
- [ ] Visual test for filter presets
- [ ] All tests pass: `cargo test --all` and `cd web && npm test`

## Agent Instructions

Read `crates/gyre-server/src/explorer_ws.rs` for the WebSocket protocol implementation. Check `crates/gyre-server/tests/` for existing integration test patterns. For visual tests, check if Playwright or a similar tool is already configured in `web/` (look at `web/package.json` and `web/playwright.config.*`). The `web/test-results/` directory already exists (from git status), suggesting visual testing may be partially set up. For the LLM-dependent tests (agent query generation, self-check loop), use the `GYRE_EXPLORER_SDK=0` fallback path or implement a mock LLM response for deterministic testing.

## Shipped

**Status: implemented — all four §31 integration-test scenarios and all four §32 visual-test scenarios have real, passing tests on the current head: the 15-test mainline+task-080 inventory in `explorer_ws_integration.rs` (11 mainline + 4 task-080) and the 16-test Playwright suite in `explorer-visual.spec.js`, whose preset tests fail when the specced behavior breaks. Deterministic gates (rustfmt, clippy) are clean on the changed lines.**

### Verification repair (this round): rustfmt + clippy failures in the WS integration tests

The verification gate (`bash /tmp/stage/checks.sh`) failed with exit 1 on two findings in `crates/gyre-server/tests/explorer_ws_integration.rs`, both reproduced on the head before the repair: changed-line rustfmt (11 lines needing formatting) and four `clippy::useless_conversion` lints — `.to_string().into()` where `Message::Text` (tungstenite 0.24) already takes `String`, a useless String→String conversion.

The repair (commit 2f14f11f, test-only, no production code touched):

1. **Removed all 13 useless `.to_string().into()` conversions** in the file — the four flagged changed lines plus the same pre-existing pattern on unchanged lines, which the rustfmt reflow would have pulled into the changed set anyway.
2. **Applied `cargo fmt`** — the file now passes `cargo fmt --check -p gyre-server` with zero diffs, and `cargo clippy -p gyre-server --test explorer_ws_integration` reports zero warnings for the file.
3. **Behavior unchanged and re-verified**: `cargo test -p gyre-server --test explorer_ws_integration` — **15 passed, 0 failed** (evidence: `/tmp/stage/review-evidence/explorer-ws-integration-postlint.log`; also `rustfmt-postfix.log`, `clippy-postfix.log`).

### Review repair (prior round): preset visual tests could not fail

Independent review proved `filter_preset_buttons_show_correct_subsets` passed with the specced behavior disabled: with `nodeFilterOpacity()` forced to `1.0` (dimming fully disabled, web rebuilt), the test still passed, and under Playwright's default pixelmatch threshold (0.2, perceptual YIQ) the Endpoints/Types baselines were interchangeable — this dark theme's dimming shifts (~0.1–0.15 YIQ) sit inside the discounted band. The repair (test-only, commit d4c1bd0dd):

1. **Tighter comparator** — all six preset-block tests compare at `threshold: 0.05`; at 0.05 the Endpoints/Types baselines differ by 126,101 px (35%) and Endpoints vs the unfiltered default view by 128,996 px — no longer interchangeable; a dimming-disabled render misses the baseline by ~83k px, far above the 2% allowance.
2. **Baseline-independent behavioral assertions** — the test counts live-canvas pixels that visibly change (>13/255 per RGBA channel) across each transition and asserts a 20,000-px minimum: 5-wheel zoom repaints 239,181 px, clicking Endpoints 99,606 px, Endpoints→Types 127,482 px of 363,924. With dimming disabled the same transitions repaint 19,225 / 38 px, so the test fails at the assertion before the baselines are consulted — re-recording baselines from a broken build cannot greenwash it. Counts are deterministic (forced webfonts, fixed viewport, reduced motion).
3. **Baselines unchanged** — they already record real dimming on this head; the full 16-test suite passes against them at the tighter threshold.

Mutation/swap evidence (logs in `/tmp/stage/review-evidence/`): dimming-disabled → test fails (19,225 < 20,000 behavioral); endpoints↔types baseline swap fails (126,282 px); endpoints↔default-view swap fails (128,143 px) — both swaps passed before the repair. The query-editor filter tests still pass under the toolbar-dimming mutation because they exercise the view-query `dim_unmatched` path, not the toolbar preset path.

### Earlier repair rounds (condensed)

- **Restored mainline tests** (commit da74a27ff): the merge resolution of `explorer_ws_integration.rs` had silently dropped `explorer_ws_self_check_refinement_flow` and `explorer_ws_self_check_sends_best_version_after_cap` from the assigned base (task-034 lineage); both were restored onto the `ScriptedLlmFactory` infrastructure, restoring coverage of the `best_view_query` tracker in both self-check loops of `explorer_ws.rs`.
- **Merge reconciliation**: the retained explorer-tests checkpoint was merged with the task-070 lineage (LLM explorer agent, best-version refinement tracking, call-graph context notes); `explorer_ws.rs` keeps the candidate's protocol contract and the base-side enhancements.

### §31 Integration Tests (crates/gyre-server/tests/explorer_ws_integration.rs)

- **WebSocket session lifecycle** — `explorer_ws_full_session_lifecycle`: connect, `list_views` protocol exchange, a full message turn (thinking status → streamed text done=true → view_query → ready), then a clean client Close frame with the server confirmed closing the socket (Close reply or stream end, never a 5s hang).
- **Agent generates valid ViewQuery from a question** — `explorer_ws_agent_generates_view_query_from_question`: seeds real graph nodes (Space, TaskPort, Workspace, Tenant, RepoStore) into the graph store, asks "What would break if I change Space?" over the no-LLM fallback path, and asserts the emitted query is a `focus` scope on node `Space` (extracted from the question and validated against actual graph nodes) that deserializes against the ViewQuery grammar.
- **Self-check loop catches and fixes overly broad groups** — `explorer_ws_self_check_loop_refines_broad_groups`: a `ScriptedLlmFactory` (test-local `LlmPort` impl with a shared call counter) wires `AppState.llm` so the native agent path runs deterministically. Seeded 25 `node_N` graph nodes make the scripted broad group (`nodes: ["node"]`, substring-matches 25 > 20 threshold) trigger the dry-run "too broad" warning. Asserts: `refining` status observed, exactly 2 LLM calls (one refinement round), and exactly one view_query reaching the client — the refined one. Mutation-checked: setting `MAX_REFINEMENT_TURNS = 0` fails the test.
- **Self-check refinement flow (restored mainline)** — `explorer_ws_self_check_refinement_flow`: a first scripted attempt referencing a nonexistent callout node triggers an actionable unresolved-callout warning → `refining` status → the second attempt (produced after receiving the dry-run feedback as a synthetic tool result) resolves cleanly; exactly the refined view_query reaches the frontend.
- **Best version sent after refinement cap (restored mainline)** — `explorer_ws_self_check_sends_best_version_after_cap`: four scripted attempts with monotonically worsening warning counts run exactly `MAX_REFINEMENT_TURNS` (3) refinement rounds; at the cap the frontend receives the best attempt (fewest actionable warnings — the first one), proving a later regressed refinement does not displace an earlier, better attempt via the `best_view_query` tracker.
- **Saved view round-trip** — WS save→list→load covered by `explorer_ws_save_and_load_view`; `explorer_rest_saved_view_round_trip` covers the REST half: `POST /api/v1/repos/:id/views` (201) → list contains it → `GET /views/:view_id` returns name/description and exactly-equal query JSON (the render round-trip input) → `DELETE` → 404 on re-load.

### §32 Visual Tests (web/tests/e2e/explorer-visual.spec.js)

The Playwright suite (deterministic mock graph + forced-webfont environment, baselines committed in `-snapshots/`) covers all four spec scenarios: semantic zoom at three levels, view-query rendering (groups/callouts/narrative + annotation bar), filter subsets via the query editor and via the real toolbar preset buttons, and blast-radius interactive mode (tiered coloring + node click). `filter_preset_buttons_show_correct_subsets` clicks the **real toolbar preset buttons** (Endpoints/Types) as the spec's implementation plan describes and verifies the subset two independent ways — live-canvas visible-change counts per preset transition (fails when dimming breaks, regardless of baselines) and screenshot comparison at threshold 0.05 (fails when the render deviates from the recorded dimmed subsets, and the baselines are no longer interchangeable at that threshold). Mutation-verified both ways; see the review-repair section above.

### Test evidence (current head, after this round's lint repair)

- `cargo fmt --check -p gyre-server`: **clean** (evidence: `/tmp/stage/review-evidence/rustfmt-postfix.log`); `cargo clippy -p gyre-server --test explorer_ws_integration`: **zero warnings for the file** (evidence: `clippy-postfix.log`).
- `cargo test -p gyre-server --test explorer_ws_integration`: **15 passed, 0 failed** (11 mainline + 4 task-080) — evidence: `/tmp/stage/review-evidence/explorer-ws-integration-postlint.log` (this round, after the lint repair; the diff is mechanical — no test logic changed).
- Playwright `explorer-visual` full suite (prior round, unchanged this round — no web files touched): **all 16 tests pass in a single browser process (1.1m)** — evidence: `explorer-visual-full-suite-postrepair.log`.
- Mutation re-checks (prior round): `nodeFilterOpacity()` → `1.0` fails `filter_preset_buttons_show_correct_subsets` (behavioral assertion 19,225 < 20,000) — evidence: `mutation-dimming-disabled-visual-postrepair.log`; baseline swaps fail (`baseline-swap-endpoints-{types,defaultview}-postrepair.log`; pixel analysis: `baseline-pixel-analysis.txt`).
- `cd web && npm ci && npm test` (vitest, prior round): **67 files, 1668 passed, 41 skipped, 0 failed**.
- Retained mutation checks: `MAX_REFINEMENT_TURNS = 0` fails the self-check test; breaking preset dimming is caught by `canvas-filters.test.js` (unit, 5 failures) and by the repaired visual test; the best-version test fails if the `best_view_query` tracker stops preferring the attempt with the fewest actionable warnings.

### Sandbox restrictions (for remote verification, not code defects)

- This sandbox's command harness blocks loopback HTTP from agent-issued probes (curl to `127.0.0.1`/`localhost` is SSRF-guarded), and `/tmp/stage/bin/{cargo,npx,npm}` serialize via a build lock that child processes inherit — a server started through the wrapped `cargo run` holds the lock and blocks subsequent wrapped commands. Visual-suite runs therefore used the prebuilt server binary launched directly (`GYRE_PORT=2222 GYRE_AUTH_TOKEN=e2e-test-token` + `CARGO_TARGET_DIR` debug binary) with Playwright's `reuseExistingServer`, and the auth token exported for the Playwright process. Exact-head GitHub checks with a normal browser and CI-spawned server remain mandatory.
- No LLM credentials in the sandbox: the agent-generation test uses the production no-LLM fallback path; the self-check test uses a scripted in-process `LlmPort` (no network, no mocks of the protocol under test — the WS protocol, dry-run, and refinement loop are the real production code paths).

Independent review, deterministic gates, and GitHub CI on the exact head remain required before merge.
