---
title: "Explorer WebSocket Protocol & Server Handler"
spec_ref: "explorer-implementation.md §3–6, §20–21"
depends_on:
  - task-068
progress: not-started
coverage_sections:
  - "explorer-implementation.md §3 WebSocket Protocol"
  - "explorer-implementation.md §4 Endpoint"
  - "explorer-implementation.md §5 Messages: Client → Server"
  - "explorer-implementation.md §6 Messages: Server → Client"
  - "explorer-implementation.md §20 Server Implementation"
  - "explorer-implementation.md §21 Explorer WebSocket Handler"
commits: []
---

## Spec Excerpt

**§4 Endpoint:** `WS /api/v1/repos/:repo_id/explorer` with `Authorization: Bearer <token>`.

**§5 Messages: Client → Server:**
- `message` — user text + canvas_state (selected_node, zoom_level, visible_tree_groups, active_filter, active_query)
- `save_view` — name, description, query JSON
- `load_view` — view_id
- `list_views`

**§6 Messages: Server → Client:**
- `text` — LLM text response (streamed, `done: false` until complete)
- `view_query` — final view query JSON after dry-run satisfaction
- `views` — list of saved views
- `status` — `"thinking"` | `"refining"` | `"ready"`

**§21 Explorer WebSocket Handler:**
```rust
pub async fn explorer_ws(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    auth: AuthenticatedAgent,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_explorer_session(socket, state, repo_id, auth))
}

async fn handle_explorer_session(...) {
    // Message dispatch: UserMessage, SaveView, LoadView, ListViews
}
```

## Implementation Plan

### Existing Code

- `crates/gyre-server/src/explorer_ws.rs` (4271 lines) — already implements the WebSocket handler with Claude Agent SDK integration and LLM port fallback.
- Route registration: Check `api/mod.rs` for the WebSocket route.

### Work Required

1. **Verify route registration**: Grep `api/mod.rs` for `/repos/:repo_id/explorer` or similar WebSocket route. If not registered, add it.

2. **Audit message types**: Verify `ExplorerClientMessage` enum in `gyre-common/src/view_query.rs` handles all 4 client message types: `message`, `save_view`, `load_view`, `list_views`.

3. **Audit server responses**: Verify `ExplorerServerMessage` enum handles all 4 server message types: `text` (with `done` flag), `view_query`, `views`, `status`.

4. **Streaming text**: Verify the handler streams `text` messages with `done: false` as the LLM generates output, then sends a final `text` with `done: true`.

5. **Status messages**: Verify the handler sends `status: "thinking"` when the agent starts, `status: "refining"` during self-check loop iterations, and `status: "ready"` when the final view query is sent.

6. **View CRUD over WebSocket**: Verify `save_view`, `load_view`, and `list_views` messages are handled by the WebSocket handler (delegating to the saved_views port/repository).

7. **Auth**: Verify the WebSocket upgrade requires a valid Bearer token (via `AuthenticatedAgent` extractor or `?token=` query parameter).

8. **Integration test**: Write a test that connects to the WebSocket endpoint, sends a `list_views` message, and verifies a `views` response.

## Acceptance Criteria

- [ ] WebSocket route registered at `WS /api/v1/repos/:repo_id/explorer`
- [ ] Client messages: `message`, `save_view`, `load_view`, `list_views` all parsed and dispatched
- [ ] Server messages: `text` (streamed with `done` flag), `view_query`, `views`, `status` all sent correctly
- [ ] Status progression: `thinking` → `refining` (0–3 times) → `ready`
- [ ] Text streaming sends incremental `text` messages with `done: false`, final with `done: true`
- [ ] View CRUD: save_view stores to DB, load_view retrieves and sends view_query, list_views returns all views
- [ ] Auth required on WebSocket upgrade
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/explorer-implementation.md` §3–6, §20–21. Then audit:
- `crates/gyre-server/src/explorer_ws.rs` — the main handler (4271 lines). Read thoroughly.
- `crates/gyre-common/src/view_query.rs` — `ExplorerClientMessage` and `ExplorerServerMessage` enums
- `crates/gyre-server/src/api/mod.rs` — route registration (search for "explorer")

The handler already exists and is substantial. This task is an audit + gap-fill. Check each message type against the spec. The most likely gaps are:
1. Missing WebSocket route registration in `api/mod.rs` (the handler exists but may not be wired up)
2. Missing or incomplete `status` message flow (thinking/refining/ready)
3. Missing `save_view`/`load_view`/`list_views` handling in the WebSocket handler (these might only be REST endpoints currently)

Verify by grepping `mod.rs` for the route before writing code.
