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
commits: ["2f14f11f85cfffacad1670a262d84280225d38a1", "d4c1bd0ddc855dbf40b76e4ec138fc0ac3c10a11", "da74a27ff27141e98d6b79532ced8730ba494ba8", "659f64e8e32cc4317f9b3b78f39b58c3561d1d3e", "ce4f2044ec7273c5ed57fb198f54b92fdb3aff06"]
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

**Status: implemented — all four §31 integration-test scenarios and all four §32 visual-test scenarios have real, passing, mutation-verified tests on the current head: the 15-test mainline+task-080 inventory in `explorer_ws_integration.rs` and the 16-test Playwright suite in `explorer-visual.spec.js`. Deterministic gates (rustfmt, clippy) are clean on the changed lines.**

### Review repair (this round, commit ce4f2044e): clean-disconnect check passed on a hang

Independent review proved the clean-disconnect check at the end of `explorer_ws_full_session_lifecycle` treated the tokio timeout as success: the bare `Err(_)` (Elapsed) arm set `closed = true`, so a server holding the socket for the full 5s deadline (no Close reply, no stream end) passed the disconnect assertion vacuously — reproduced by parking the server's idle-loop Close/None arm on `std::future::pending`, which left the test green. The fix (test-only) splits the terminal arm: a stream error (`Ok(Some(Err(_)))`) still counts as closed, but `Err(_)` (Elapsed) now breaks without setting `closed`, so the assertion "server must close the explorer session after the client sends Close" fails on a hang. Only a Close reply, stream end, or stream error count as closed. Re-verified by the same mutation: the parked-server mutation now fails the test at the final assertion; the unmutated server still passes; the full suite is 15/15.

### §31 Integration Tests (crates/gyre-server/tests/explorer_ws_integration.rs)

- **WebSocket session lifecycle** — `explorer_ws_full_session_lifecycle`: connect, `list_views` protocol exchange, a full message turn (thinking status → streamed text done=true → view_query → ready), then a clean client Close frame with the server confirmed closing the socket (Close reply or stream end — never a 5s hang, per this round's fix).
- **Agent generates valid ViewQuery from a question** — `explorer_ws_agent_generates_view_query_from_question`: seeds real graph nodes (Space, TaskPort, Workspace, Tenant, RepoStore) into the graph store, asks "What would break if I change Space?" over the no-LLM fallback path, and asserts the emitted query is a `focus` scope on node `Space` (extracted from the question and validated against actual graph nodes) that deserializes against the ViewQuery grammar.
- **Self-check loop catches and fixes overly broad groups** — `explorer_ws_self_check_loop_refines_broad_groups`: a `ScriptedLlmFactory` (test-local `LlmPort` impl with a shared call counter) wires `AppState.llm` so the native agent path runs deterministically. Seeded 25 `node_N` graph nodes make the scripted broad group (`nodes: ["node"]`, substring-matches 25 > 20 threshold) trigger the dry-run "too broad" warning. Asserts: `refining` status observed, exactly 2 LLM calls (one refinement round), and exactly one view_query reaching the client — the refined one. Mutation-checked: setting `MAX_REFINEMENT_TURNS = 0` fails the test.
- **Self-check refinement flow** — `explorer_ws_self_check_refinement_flow`: a first scripted attempt referencing a nonexistent callout node triggers an actionable unresolved-callout warning → `refining` status → the second attempt (produced after receiving the dry-run feedback as a synthetic tool result) resolves cleanly; exactly the refined view_query reaches the frontend.
- **Best version sent after refinement cap** — `explorer_ws_self_check_sends_best_version_after_cap`: four scripted attempts with monotonically worsening warning counts run exactly `MAX_REFINEMENT_TURNS` (3) refinement rounds; at the cap the frontend receives the best attempt (fewest actionable warnings — the first one), proving a later regressed refinement does not displace an earlier, better attempt via the `best_view_query` tracker.
- **Saved view round-trip** — WS save→list→load covered by `explorer_ws_save_and_load_view`; `explorer_rest_saved_view_round_trip` covers the REST half: `POST /api/v1/repos/:id/views` (201) → list contains it → `GET /views/:view_id` returns name/description and exactly-equal query JSON (the render round-trip input) → `DELETE` → 404 on re-load.

### §32 Visual Tests (web/tests/e2e/explorer-visual.spec.js)

The Playwright suite (deterministic mock graph + forced-webfont environment, baselines committed in `-snapshots/`) covers all four spec scenarios: semantic zoom at three levels, view-query rendering (groups/callouts/narrative + annotation bar), filter subsets via the query editor and via the real toolbar preset buttons, and blast-radius interactive mode (tiered coloring + node click). `filter_preset_buttons_show_correct_subsets` clicks the **real toolbar preset buttons** (Endpoints/Types) and verifies the subset two independent ways — live-canvas visible-change counts per preset transition (fails when dimming breaks, regardless of baselines) and screenshot comparison at threshold 0.05 (fails when the render deviates from the recorded dimmed subsets; the baselines are not interchangeable at that threshold).

### Repair history (condensed)

- **Clean-disconnect vacuous pass** (this round, ce4f2044e): timeout-as-success in the lifecycle test's disconnect loop fixed; mutation now fails the test — see above.
- **rustfmt + clippy** (2f14f11f): removed 13 useless `.to_string().into()` conversions and applied `cargo fmt`; gates clean on the file since.
- **Preset visual tests could not fail** (d4c1bd0dd): pixelmatch threshold tightened to 0.05 and baseline-independent behavioral assertions (≥20,000 visibly-changed pixels per preset transition) added, after review proved the tests passed with `nodeFilterOpacity()` dimming disabled and the baselines interchangeable at the default 0.2 threshold.
- **Restored mainline tests** (da74a27ff): merge resolution had silently dropped `explorer_ws_self_check_refinement_flow` and `explorer_ws_self_check_sends_best_version_after_cap`; both restored onto the `ScriptedLlmFactory` infrastructure.

### Test evidence (current head, after this round's disconnect fix)

- `cargo test -p gyre-server --test explorer_ws_integration`: **15 passed, 0 failed** — evidence: `/tmp/stage/review-evidence/explorer-ws-integration-disconnect-fix.log`.
- Mutation re-check (this round): parking the server's idle-loop Close/None arm on `std::future::pending` **fails** `explorer_ws_full_session_lifecycle` at the disconnect assertion (was green before the fix) — evidence: `/tmp/stage/review-evidence/disconnect-fix-notes.md`.
- `cargo fmt --check -p gyre-server`: **exit 0** (evidence: `rustfmt-disconnect-fix.log`); `cargo clippy -p gyre-server --test explorer_ws_integration`: **exit 0, zero warnings for the file** (evidence: `clippy-disconnect-fix-full.log`).
- Playwright `explorer-visual` full suite (prior round; no web files touched this round): **all 16 tests pass** — evidence: `explorer-visual-full-suite-postrepair.log`.
- `cd web && npm ci && npm test` (vitest, prior round): **1737 passed, 0 failed**.
- Retained mutation checks: `nodeFilterOpacity()` → `1.0` fails `filter_preset_buttons_show_correct_subsets` (behavioral assertion 19,225 < 20,000) and `canvas-filters.test.js` (unit); baseline swaps fail at threshold 0.05; `MAX_REFINEMENT_TURNS = 0` fails the self-check test; the best-version test fails if the `best_view_query` tracker stops preferring the attempt with the fewest actionable warnings.

### Sandbox restrictions (for remote verification, not code defects)

- This sandbox's command harness blocks loopback HTTP from agent-issued probes (curl to `127.0.0.1`/`localhost` is SSRF-guarded), and `/tmp/stage/bin/{cargo,npx,npm}` serialize via a build lock that child processes inherit — a server started through the wrapped `cargo run` holds the lock and blocks subsequent wrapped commands. Visual-suite runs therefore used the prebuilt server binary launched directly (`GYRE_PORT=2222 GYRE_AUTH_TOKEN=e2e-test-token` + `CARGO_TARGET_DIR` debug binary) with Playwright's `reuseExistingServer`, and the auth token exported for the Playwright process. Exact-head GitHub checks with a normal browser and CI-spawned server remain mandatory.
- No LLM credentials in the sandbox: the agent-generation test uses the production no-LLM fallback path; the self-check tests use a scripted in-process `LlmPort` (no network, no mocks of the protocol under test — the WS protocol, dry-run, and refinement loop are the real production code paths).

Independent review, deterministic gates, and GitHub CI on the exact head remain required before merge.
