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
commits: ["659f64e8e32cc4317f9b3b78f39b58c3561d1d3e"]
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

**Status: implemented — all four §31 integration-test scenarios and all four §32 visual-test scenarios have real, passing tests.**

### §31 Integration Tests (crates/gyre-server/tests/explorer_ws_integration.rs)

- **WebSocket session lifecycle** — `explorer_ws_full_session_lifecycle`: connect, `list_views` protocol exchange, a full message turn (thinking status → streamed text done=true → view_query → ready), then a clean client Close frame with the server confirmed closing the socket (Close reply or stream end, never a 5s hang).
- **Agent generates valid ViewQuery from a question** — `explorer_ws_agent_generates_view_query_from_question`: seeds real graph nodes (Space, TaskPort, Workspace, Tenant, RepoStore) into the graph store, asks "What would break if I change Space?" over the no-LLM fallback path, and asserts the emitted query is a `focus` scope on node `Space` (extracted from the question and validated against actual graph nodes) that deserializes against the ViewQuery grammar.
- **Self-check loop catches and fixes overly broad groups** — `explorer_ws_self_check_loop_refines_broad_groups`: a `ScriptedLlmFactory` (test-local `LlmPort` impl with a shared call counter) wires `AppState.llm` so the native agent path runs deterministically. Seeded 25 `node_N` graph nodes make the scripted broad group (`nodes: ["node"]`, substring-matches 25 > 20 threshold) trigger the dry-run "too broad" warning. Asserts: `refining` status observed, exactly 2 LLM calls (one refinement round), and exactly one view_query reaching the client — the refined one. Mutation-checked: setting `MAX_REFINEMENT_TURNS = 0` fails the test, so the pass is attributable to the real refinement loop, not fixture drift.
- **Saved view round-trip** — WS save→list→load was already covered (`explorer_ws_save_and_load_view`); added `explorer_rest_saved_view_round_trip` for the REST half: `POST /api/v1/repos/:id/views` (201) → list contains it → `GET /views/:view_id` returns name/description and exactly-equal query JSON (the render round-trip input) → `DELETE` → 404 on re-load.

### §32 Visual Tests (web/tests/e2e/explorer-visual.spec.js)

The Playwright suite (deterministic mock graph + forced-webfont environment, baselines committed in `-snapshots/`) already covered all four spec scenarios from task-065: semantic zoom at three levels, view-query rendering (groups/callouts/narrative + annotation bar), filter subsets via the query editor, and blast-radius interactive mode (tiered coloring + node click). Added `filter_preset_buttons_show_correct_subsets`, which clicks the **real toolbar preset buttons** (Endpoints/Types) as the spec's implementation plan describes, with new baselines `filter-preset-endpoints.png` / `filter-preset-types.png`. Verified the All/Endpoints/Types states render pixel-distinct screenshots (not just below the 2% diff threshold), so a preset that stops applying its filter fails the test.

### Test evidence

- `cargo test -p gyre-server --test explorer_ws_integration`: **13 passed, 0 failed** (9 pre-existing + 4 new); evidence: `/tmp/stage/review-evidence/explorer-ws-integration-final.log`.
- `cd web && npm test` (vitest): **64 files, 1626 passed, 41 skipped, 0 failed**.
- Playwright `explorer-visual`: all 16 tests pass; evidence: `/tmp/stage/review-evidence/preset-retest4.log` (new preset test) and the suite logs. Mutation checks: breaking the `types` preset dimming is caught by `canvas-filters.test.js` (unit), and the pixel-distinct baselines catch a preset that applies no filter.

### Sandbox restrictions (for remote verification, not code defects)

- This sandbox blocks chromium's multi-process sandbox and `/dev/shm`; the visual suite ran locally only with temporary launch args (`--no-sandbox --disable-gpu --single-process --disable-dev-shm-usage`, config reverted before commit). Under `--single-process`, contexts accumulate and the browser crashes after ~8 contexts, so the full 16-test suite was verified test-by-test (each passes individually) rather than in one process. Exact-head GitHub checks with a normal browser remain mandatory.
- No LLM credentials in the sandbox: the agent-generation test uses the production no-LLM fallback path; the self-check test uses a scripted in-process `LlmPort` (no network, no mocks of the protocol under test — the WS protocol, dry-run, and refinement loop are the real production code paths).

Independent review, deterministic gates, and GitHub CI on the exact head remain required before merge.

