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
commits: ["da74a27ff27141e98d6b79532ced8730ba494ba8", "659f64e8e32cc4317f9b3b78f39b58c3561d1d3e", "d4c1bd0ddc855dbf40b76e4ec138fc0ac3c10a11"]
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

**Status: implemented — all four §31 integration-test scenarios and all four §32 visual-test scenarios have real, passing tests on the current head: the 15-test mainline+task-080 inventory in `explorer_ws_integration.rs` (11 mainline + 4 task-080) and the 16-test Playwright suite in `explorer-visual.spec.js`, whose preset tests now fail when the specced behavior breaks.**

### Review repair (this round): preset visual tests could not fail

Independent review proved that `filter_preset_buttons_show_correct_subsets` passed with the specced behavior disabled, and that the previous Shipped claim "the All/Endpoints/Types states render pixel-distinct screenshots … so a preset that stops applying its filter fails the test" was false. Reproduced on this head before the repair: with `nodeFilterOpacity()` forced to `1.0` in `web/src/lib/canvas-filters.js` (preset dimming fully disabled, web rebuilt), the test still passed; under the suite's own comparator the Endpoints and Types baselines were interchangeable (1,373 counted pixels < the 7,296-pixel 2% allowance). Root cause: Playwright's default pixelmatch threshold (0.2, perceptual YIQ) discounts this dark theme's dimming shifts — dimming a leaf from full opacity to 0.1 over the dark background moves the affected pixels by only ~0.1–0.15 normalized YIQ, and 124,715 of the pixels differing between the Endpoints and Types baselines sit in exactly that band.

The repair (test-only; no production code changed):

1. **Tighter comparator for the preset tests** — all six tests in the "Filter presets visual regression" describe block now pass `threshold: 0.05` (shared `PRESET_SNAPSHOT_OPTIONS`). At 0.05 the Endpoints/Types baselines differ by 126,101 pixels (35%) and the Endpoints baseline differs from the default view by 128,996 pixels — no longer interchangeable, and a dimming-disabled render misses the baseline by ~83k pixels, far above the 2% allowance.
2. **Behavioral canvas assertions independent of the baselines** — `filter_preset_buttons_show_correct_subsets` now counts live-canvas pixels that visibly change (>13/255 per RGBA channel) across each transition and asserts a 20,000-pixel minimum: the 5-wheel zoom must repaint the canvas (239,181 px), clicking Endpoints must re-render it (99,606 px), and the Endpoints→Types subset change must be visible (127,482 px of 363,924). With preset dimming disabled the same transitions repaint 19,225 / 38 pixels, so the test fails at the assertion before the baselines are even consulted — re-recording baselines from a broken build cannot greenwash it. The counts are deterministic (identical across repeated runs; forced webfonts + fixed viewport + reduced motion).
3. **Baselines unchanged** — the committed preset baselines already record real dimming on this head (they were regenerated by the task-071 base merge); no re-recording was needed, and the full 16-test suite passes against them at the tighter threshold.

Mutation and swap evidence (all on the current head, logs in `/tmp/stage/review-evidence/`): `nodeFilterOpacity()` → `1.0` fails `filter_preset_buttons_show_correct_subsets` (behavioral assertion: 19,225 < 20,000; the screenshot comparison would also fail at ~83k pixels); swapping `filter-preset-endpoints.png` for the Types baseline fails (126,282 px differ) and for the unfiltered default-view baseline fails (128,143 px differ) — both swaps passed before the repair. The query-editor filter tests (`filter-endpoints/types/calls/dependencies`) still pass under the toolbar-dimming mutation because they exercise the view-query `dim_unmatched` path, not the toolbar preset path — their dimming is covered by the view-query engine and unit tests respectively.

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

### Test evidence (current head, after this round's repair)

- Playwright `explorer-visual` full suite after the repair: **all 16 tests pass in a single browser process (1.1m)** — evidence: `/tmp/stage/review-evidence/explorer-visual-full-suite-postrepair.log`.
- Mutation re-check after the repair: `nodeFilterOpacity()` → `1.0`, web rebuilt → `filter_preset_buttons_show_correct_subsets` **fails** (behavioral assertion 19,225 < 20,000) — evidence: `mutation-dimming-disabled-visual-postrepair.log`. Baseline-swap probes after the repair: endpoints↔types fails (126,282 px), endpoints↔default-view fails (128,143 px) — evidence: `baseline-swap-endpoints-{types,defaultview}-postrepair.log`. Pixel-level analysis of the baselines and live canvas dumps: `baseline-pixel-analysis.txt` (+ `compare-pngs.cjs` / `png-delta-hist.cjs` / `probe-*.png`).
- `cargo test -p gyre-server --test explorer_ws_integration`: **15 passed, 0 failed** (11 mainline + 4 task-080) — evidence from the previous round: `explorer-ws-integration-15tests.log`; this round's repair touched no Rust code.
- `cd web && npm ci && npm test` (vitest): **67 files, 1668 passed, 41 skipped, 0 failed** (measured on the pre-repair merged head; this round's repair touched only the e2e spec file, which vitest does not run).
- Retained mutation checks: `MAX_REFINEMENT_TURNS = 0` fails the self-check test; breaking preset dimming is caught by `canvas-filters.test.js` (unit, 5 failures) and now by the repaired visual test; the best-version test fails if the `best_view_query` tracker stops preferring the attempt with the fewest actionable warnings.

### Sandbox restrictions (for remote verification, not code defects)

- This sandbox's command harness blocks loopback HTTP from agent-issued probes (curl to `127.0.0.1`/`localhost` is SSRF-guarded), and `/tmp/stage/bin/{cargo,npx,npm}` serialize via a build lock that child processes inherit — a server started through the wrapped `cargo run` holds the lock and blocks subsequent wrapped commands. Visual-suite runs therefore used the prebuilt server binary launched directly (`GYRE_PORT=2222 GYRE_AUTH_TOKEN=e2e-test-token` + `CARGO_TARGET_DIR` debug binary) with Playwright's `reuseExistingServer`, and the auth token exported for the Playwright process. Exact-head GitHub checks with a normal browser and CI-spawned server remain mandatory.
- No LLM credentials in the sandbox: the agent-generation test uses the production no-LLM fallback path; the self-check test uses a scripted in-process `LlmPort` (no network, no mocks of the protocol under test — the WS protocol, dry-run, and refinement loop are the real production code paths).

Independent review, deterministic gates, and GitHub CI on the exact head remain required before merge.
