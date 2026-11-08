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
commits: ["da74a27ff27141e98d6b79532ced8730ba494ba8", "659f64e8e32cc4317f9b3b78f39b58c3561d1d3e"]
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

**Status: implemented — all four §31 integration-test scenarios and all four §32 visual-test scenarios have real, passing tests on the current head, which carries the full 15-test mainline+task-080 inventory in `explorer_ws_integration.rs` (11 mainline tests + 4 task-080 tests).**

### Review repair (this round)

Independent review found that the earlier merge resolution of `explorer_ws_integration.rs` had silently dropped two mainline tests from the assigned base (task-034 lineage, commit eff64e31d): `explorer_ws_self_check_refinement_flow` and `explorer_ws_self_check_sends_best_version_after_cap` — leaving the best-version-at-refinement-cap behavior (the `best_view_query` tracker in both the SDK and native `LlmPort` self-check loops of `explorer_ws.rs`) with zero test coverage, while the previous Shipped notes incorrectly described the resolution as a superset. Both tests are now restored (commit da74a27ff), ported onto the test-local `ScriptedLlmFactory` infrastructure with the shared `run_message_turn`/`TurnTrace` helper; the earlier claim that the checkpoint's assertions "pass unchanged" against the merged `explorer_ws.rs` was true only for the 9 retained tests and concealed the loss of the other 2. The base's `.expect()` on `seed_builtin_policies` was also restored, removing an `unused_must_use` warning from the test binary.

### Merge reconciliation (earlier round, corrected)

The retained source checkpoint (explorer tests) was merged with the task-070 lineage (LLM explorer agent, best-version refinement tracking, call-graph context notes). The conflicted `explorer_ws_integration.rs` was resolved to the checkpoint's version — 13 tests at the time (9 of the 11 mainline tests retained + the 4 task-080 tests), an accidental net loss of 2 mainline tests now repaired above. The merged `explorer_ws.rs` keeps the candidate's protocol contract and carries the base-side enhancements (best-version-across-refinement-rounds tracking; call-graph completeness note in the agent system prompt), which the two restored tests now cover.

### §31 Integration Tests (crates/gyre-server/tests/explorer_ws_integration.rs)

- **WebSocket session lifecycle** — `explorer_ws_full_session_lifecycle`: connect, `list_views` protocol exchange, a full message turn (thinking status → streamed text done=true → view_query → ready), then a clean client Close frame with the server confirmed closing the socket (Close reply or stream end, never a 5s hang).
- **Agent generates valid ViewQuery from a question** — `explorer_ws_agent_generates_view_query_from_question`: seeds real graph nodes (Space, TaskPort, Workspace, Tenant, RepoStore) into the graph store, asks "What would break if I change Space?" over the no-LLM fallback path, and asserts the emitted query is a `focus` scope on node `Space` (extracted from the question and validated against actual graph nodes) that deserializes against the ViewQuery grammar.
- **Self-check loop catches and fixes overly broad groups** — `explorer_ws_self_check_loop_refines_broad_groups`: a `ScriptedLlmFactory` (test-local `LlmPort` impl with a shared call counter) wires `AppState.llm` so the native agent path runs deterministically. Seeded 25 `node_N` graph nodes make the scripted broad group (`nodes: ["node"]`, substring-matches 25 > 20 threshold) trigger the dry-run "too broad" warning. Asserts: `refining` status observed, exactly 2 LLM calls (one refinement round), and exactly one view_query reaching the client — the refined one. Mutation-checked: setting `MAX_REFINEMENT_TURNS = 0` fails the test, so the pass is attributable to the real refinement loop, not fixture drift.
- **Self-check refinement flow (restored mainline)** — `explorer_ws_self_check_refinement_flow`: a first scripted attempt referencing a nonexistent callout node triggers an actionable unresolved-callout warning → `refining` status → the second attempt (produced after receiving the dry-run feedback as a synthetic tool result) resolves cleanly; exactly the refined view_query reaches the frontend.
- **Best version sent after refinement cap (restored mainline)** — `explorer_ws_self_check_sends_best_version_after_cap`: four scripted attempts with monotonically worsening warning counts run exactly `MAX_REFINEMENT_TURNS` (3) refinement rounds; at the cap the frontend receives the best attempt (fewest actionable warnings — the first one), proving a later regressed refinement does not displace an earlier, better attempt via the `best_view_query` tracker.
- **Saved view round-trip** — WS save→list→load covered by `explorer_ws_save_and_load_view`; `explorer_rest_saved_view_round_trip` covers the REST half: `POST /api/v1/repos/:id/views` (201) → list contains it → `GET /views/:view_id` returns name/description and exactly-equal query JSON (the render round-trip input) → `DELETE` → 404 on re-load.

### §32 Visual Tests (web/tests/e2e/explorer-visual.spec.js)

The Playwright suite (deterministic mock graph + forced-webfont environment, baselines committed in `-snapshots/`) covers all four spec scenarios: semantic zoom at three levels, view-query rendering (groups/callouts/narrative + annotation bar), filter subsets via the query editor, and blast-radius interactive mode (tiered coloring + node click). `filter_preset_buttons_show_correct_subsets` clicks the **real toolbar preset buttons** (Endpoints/Types) as the spec's implementation plan describes, with baselines `filter-preset-endpoints.png` / `filter-preset-types.png`; the All/Endpoints/Types states render pixel-distinct screenshots (not just below the 2% diff threshold), so a preset that stops applying its filter fails the test. The base-side `ExplorerCanvas.svelte` changes merged this round (evaluative-lens chip badges, frame-scheduling rework) do not alter any committed baseline — the chips require trace span stats the mock graph does not provide, and settled rendering is unchanged — confirmed by the full-suite pass on the merged head.

### Test evidence (current head, after review repair)

- `cargo test -p gyre-server --test explorer_ws_integration`: **15 passed, 0 failed** (11 mainline + 4 task-080) — evidence: `/tmp/stage/review-evidence/explorer-ws-integration-15tests.log`.
- `cd web && npm ci && npm test` (vitest): **67 files, 1668 passed, 41 skipped, 0 failed** (measured on the pre-repair merged head; this round's repair touched no frontend code).
- Playwright `explorer-visual` on the merged head: **all 16 tests pass in a single browser process (1.1m)** — evidence: `/tmp/stage/review-evidence/explorer-visual-postmerge.log`. This supersedes the earlier single-process/test-by-test restriction: with the committed `chromiumSandbox: false` + `--no-zygote` config and the preinstalled headless shell (`PLAYWRIGHT_BROWSERS_PATH=/usr/local/share/gyre-playwright`), the full suite runs in one process against a directly-launched server binary. Not re-run this round: the repair touched only the Rust test file and this task doc.
- Mutation checks retained from the checkpoint round: `MAX_REFINEMENT_TURNS = 0` fails the self-check test; breaking the `types` preset dimming is caught by `canvas-filters.test.js` (unit); pixel-distinct preset baselines catch a preset that applies no filter. The restored best-version test fails if the `best_view_query` tracker stops preferring the attempt with the fewest actionable warnings (e.g. sending the last attempt instead).

### Sandbox restrictions (for remote verification, not code defects)

- This sandbox's command harness blocks loopback HTTP from agent-issued probes (curl to `127.0.0.1`/`localhost` is SSRF-guarded), and `/tmp/stage/bin/{cargo,npx,npm}` serialize via a build lock that child processes inherit — a server started through the wrapped `cargo run` holds the lock and blocks subsequent wrapped commands. Visual-suite runs therefore used the prebuilt server binary launched directly (`GYRE_PORT=2222 GYRE_AUTH_TOKEN=e2e-test-token .gyre-target/debug/gyre-server`) with Playwright's `reuseExistingServer`. Exact-head GitHub checks with a normal browser and CI-spawned server remain mandatory.
- No LLM credentials in the sandbox: the agent-generation test uses the production no-LLM fallback path; the self-check test uses a scripted in-process `LlmPort` (no network, no mocks of the protocol under test — the WS protocol, dry-run, and refinement loop are the real production code paths).

Independent review, deterministic gates, and GitHub CI on the exact head remain required before merge.
