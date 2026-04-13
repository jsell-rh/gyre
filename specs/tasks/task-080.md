---
title: "Explorer Integration & Visual Tests"
spec_ref: "explorer-implementation.md §31–32"
depends_on:
  - task-069
  - task-070
  - task-065
progress: not-started
coverage_sections:
  - "explorer-implementation.md §31 Integration Tests"
  - "explorer-implementation.md §32 Visual Tests"
commits: []
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
