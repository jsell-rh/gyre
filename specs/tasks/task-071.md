---
title: "ExplorerChat Component & ExplorerView Container"
spec_ref: "explorer-implementation.md §18–19, §26"
depends_on:
  - task-069
  - task-070
progress: not-started
coverage_sections:
  - "explorer-implementation.md §18 ExplorerChat (Svelte)"
  - "explorer-implementation.md §19 ExplorerView (Svelte)"
  - "explorer-implementation.md §26 Phase 2: Chat + Agent"
commits: []
---

## Spec Excerpt

**§18 ExplorerChat (Svelte):**
Props:
```typescript
{
  repoId: string;
  canvasState: CanvasState;
  onViewQuery: (query: ViewQuery) => void;
  savedViews: SavedView[];
}
```

Responsibilities:
- WebSocket connection to `/api/v1/repos/:repo_id/explorer`
- Send user messages with canvas state
- Receive and display LLM text responses (streamed)
- Receive and forward view queries to ExplorerCanvas
- Saved views dropdown + "Save this view" button
- Show refinement status ("thinking...", "refining query...", "ready")

**§19 ExplorerView (Svelte):**
```svelte
<div class="explorer-view">
  <ExplorerCanvas {repoId} {nodes} {edges} {activeQuery} {filter} {lens} bind:canvasState />
  <ExplorerChat {repoId} {canvasState} onViewQuery={q => activeQuery = q} {savedViews} />
</div>
```

**§26 Phase 2: Chat + Agent:**
- Build ExplorerChat with WebSocket connection
- Implement explorer WebSocket handler in server
- Implement graph_summary and graph_query_dryrun MCP tools
- Wire up Claude Agent SDK for the explorer agent
- Self-check loop

## Implementation Plan

### Existing Code

- `web/src/lib/ExplorerChat.svelte` (1848 lines) — already implements chat panel with WebSocket connection, message rendering, saved views.
- `web/src/components/ExplorerView.svelte` (3729 lines) — container component wiring canvas + chat.

### Work Required

1. **Audit ExplorerChat props**: Verify it accepts `repoId`, `canvasState`, `onViewQuery`, `savedViews` as props. Check that `canvasState` is passed with each user message over WebSocket.

2. **WebSocket connection**: Verify ExplorerChat connects to `WS /api/v1/repos/:repo_id/explorer` with Bearer token auth. Verify reconnection logic on disconnect.

3. **Message display**: Verify streamed `text` messages are rendered incrementally (not waiting for `done: true`). Check markdown rendering support.

4. **View query forwarding**: When the server sends a `view_query` message, verify ExplorerChat calls `onViewQuery(query)` to apply it to the canvas.

5. **Saved views**: Verify the saved views dropdown lists available views. "Save this view" button sends a `save_view` WebSocket message. Loading a view sends `load_view`.

6. **Status indicators**: Verify "thinking...", "refining query...", "ready" states are displayed based on `status` messages from the server.

7. **ExplorerView wiring**: Verify ExplorerView correctly wires `bind:canvasState` from ExplorerCanvas to ExplorerChat, and `onViewQuery` from chat updates `activeQuery` on the canvas.

8. **Component tests**: Verify existing tests in `web/src/__tests__/ExplorerChat.test.js` cover WebSocket message handling, view query forwarding, and status display.

## Acceptance Criteria

- [ ] ExplorerChat connects to `WS /api/v1/repos/:repo_id/explorer` with Bearer auth
- [ ] User messages include `canvas_state` with selected_node, zoom_level, visible_tree_groups, active_filter, active_query
- [ ] Streamed text responses render incrementally with markdown support
- [ ] View queries from server are forwarded to canvas via `onViewQuery` callback
- [ ] Saved views dropdown lists views, "Save this view" button works
- [ ] Status indicators show "thinking...", "refining query...", "ready"
- [ ] ExplorerView wires canvas state to chat and view queries to canvas
- [ ] Reconnection logic handles WebSocket disconnects
- [ ] `cd web && npm test` passes

## Agent Instructions

Read `specs/system/explorer-implementation.md` §18–19, §26. Then audit:
- `web/src/lib/ExplorerChat.svelte` — chat panel implementation (1848 lines)
- `web/src/components/ExplorerView.svelte` — container component (3729 lines)
- `web/src/__tests__/ExplorerChat.test.js` — existing tests

The components already exist and are substantial. This is an audit + gap-fill task. Walk through each responsibility listed in the spec and verify the code implements it. Key areas to check:
1. Does `canvasState` flow bidirectionally between canvas and chat?
2. Does the chat handle all 4 server message types (text, view_query, views, status)?
3. Are saved views managed via WebSocket messages or REST calls? The spec says WebSocket.
4. Is there reconnection logic for WebSocket drops?
