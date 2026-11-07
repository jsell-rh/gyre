# Review — task-071 (ExplorerChat Component & ExplorerView Container)

Spec: `explorer-implementation.md` §18–19, §26 (frontend half; server handler, MCP tools, SDK wiring, and self-check loop shipped under task-068/069/070).
Comparison base: `11b7460496b7b0916d00476d7e9b4d9851153f68` → candidate `b00a8731c5335df9fa1a5bd90ff76bbd6b6335d9`.
Attributed commit: `318e8120f7720b70d7ca49e5c16e7eb1b33dad5d` (carried onto the assigned branch by merge `fdadc0874`); remaining range commits are `process:` attribution/notes only — attribution is complete.

## Scope of the delta

Audit + gap-fill task: the components pre-exist; the product delta is a 9-line reconnect fix in `web/src/lib/ExplorerChat.svelte` plus a new 423-line mounted-component test file `web/src/__tests__/ExplorerChat-component.test.js`. Every §18/§19 responsibility was re-audited against the merged code at the candidate, not just the delta.

## Verified working (production code at candidate)

- **§18 connection**: `connect()` opens `WS /api/v1/repos/:repo_id/explorer`, exchanging the Bearer token for a single-use ticket via `POST /api/v1/ws-ticket` (browsers cannot set Authorization headers on WS upgrades; ticket flow shipped and reviewed under task-069). No-token case shows a login prompt bubble and does not connect.
- **§18 canvas state on user messages**: `sendMessage()` sends `{type:'message', text, canvas_state: toWireCanvasState(canvasState)}` — §10 snake_case wire shape (selected_node, zoom_level, visible_tree_groups, active_filter, active_lens, active_query, recent_interactions); internal camelCase fields do not leak.
- **§18 streaming**: `text` chunks with `done:false` accumulate into a live streaming bubble rendered through `renderMarkdown`; `done:true` finalizes; 65s no-chunk timeout and disconnect both finalize orphaned streams.
- **§18 view queries**: `view_query` messages are validated (`validateViewQuery`; invalid → error bubble, NOT applied), then forwarded via `onViewQuery(query)`; prior queries stay re-appliable via per-message "Apply view" buttons.
- **§18 saved views over WebSocket** (not REST): `list_views` on open, `views` → `onSavedViewsUpdate`, dropdown with per-view `load_view`, "Save this view" dialog sending `save_view` with the last conversation query, delete for non-system views.
- **§18 status**: header status dot + label for Connecting/Thinking.../Refining query.../Ready/Error/Disconnected, plus thinking-indicator with Cancel.
- **§18 reconnection**: exponential backoff (1s base, MAX_RECONNECTS=5 cap), manual reconnect; terminal errors (session limit, session replaced) deliberately suppress auto-reconnect to avoid eviction loops. **The candidate's fix is real and correct**: a socket dropping inside the `CONNECT_DEBOUNCE_MS` window previously left auto-reconnect dead (backoff timer fired → `connect()` returned early on the debounce → nothing rescheduled). The fix re-arms the timer until the window lapses; it converges (the reschedule path never refreshes `lastConnectAttempt`, so the elapsed gap strictly grows; `reconnectTimer` is nulled before the callback so `scheduleReconnect()`'s early-return guard cannot block it).
- **§19 wiring**: ExplorerView uses `bind:canvasState={explorerCanvasState}` on ExplorerCanvas feeding ExplorerChat's `canvasState` prop; `onViewQuery` sets `activeViewQuery`, rendered into the canvas as `activeQuery`; an `$effect` mirrors `activeViewQuery` into `canvasState.active_query` so the LLM sees the applied view.
- **§26**: server-side halves present in-tree at the candidate (`crates/gyre-server/src/explorer_ws.rs`, `graph_summary`/`graph_query_dryrun` in mcp.rs + view_query_resolver.rs) — shipped under tasks 068/069/070 (complete, independently reviewed); spot-checked only.

## Test quality and probes (this sandbox, npm ci first)

- `npx vitest run ExplorerChat-component ExplorerChat` → **3 files, 135 tests passed** (19 mounted-component + 110 protocol + related).
- **Mutation probe (the load-bearing check):** temporarily reverting the 9-line fix (debounce-reschedule removed) makes `auto-reconnects after a disconnect that lands inside the connect-debounce window` fail exactly as the bug predicts — `sockets.length` stays 1 after the debounce window lapses (`expect(sockets.length).toBe(2)` fails, observed `- 2 + 1`). The new test genuinely kills the shipped bug; it is not self-confirming. File restored byte-identical afterwards (sha256 of working file == sha256 of the `b00a8731c` git blob; `git status` clean).
- The component tests render the real component with mocked transport only (WebSocket class + ticket fetch) and assert DOM output, sent WS frames, and callbacks — not mirrored component logic. The wire-format test asserts the exact §10 canvas_state object including absence of camelCase leakage; the view-query tests cover both forwarding and rejection of invalid queries; the reconnect tests cover the fast-drop case and the MAX_RECONNECTS cap with fake timers.
- Full AC run: `cd web && npm test` → **68 files, 1687 passed, 41 skipped** — green, matching the shipped claim.
- Evidence under `/tmp/stage/review-evidence/` (focused run, mutation probe output, full-suite tail).

## Non-findings (checked, no gap)

- Coverage matrix honestly shows §18/§19/§26 as `task-assigned` → task-071 — no premature `implemented` marking.
- `connect()` debounce (2000ms) vs first backoff delay (1000ms) interaction is exactly the bug class the fix addresses; no remaining path silently drops a scheduled reconnect.
- Process/notes commits in the range carry no product-surface changes.
- a11y compiler warning (`a11y_no_noninteractive_element_interactions`, ExplorerChat.svelte:837) is a pre-existing warning, not a failure, outside this task's scope.

## Verdict

**approved** — no findings. Every §18/§19 acceptance criterion is implemented in real production code at the candidate, the one shipped defect (swallowed reconnect) is fixed correctly with a mutation-verified test, and `cd web && npm test` passes.
