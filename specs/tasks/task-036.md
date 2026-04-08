# TASK-036: Explorer Chat Streaming & Status Indicators

**Spec reference:** `explorer-implementation.md` §Phase 4: Polish  
**Depends on:** None (WebSocket handler already sends status messages)  
**Progress:** `not-started`

## Spec Excerpt

From `explorer-implementation.md` §WebSocket Protocol (Server → Client messages):

> **LLM text response (streamed):**
> ```json
> { "type": "text", "content": "Space is the core domain entity...", "done": false }
> ```
>
> **Status:**
> ```json
> { "type": "status", "status": "thinking" | "refining" | "ready" }
> ```

From §Phase 4: Polish:

> - Streaming text responses in chat
> - "Thinking..." / "Refining..." status indicators

## Current State

- `explorer_ws.rs` sends `{ "type": "text", "content": "...", "done": true }` messages, but the full response is sent in one shot (not streamed token-by-token).
- `explorer_ws.rs` sends `{ "type": "status", "status": "..." }` messages.
- `ExplorerChat.svelte` receives messages but renders them only after `done: true` — no progressive rendering of partial text.
- No "Thinking..." or "Refining..." indicator is shown in the chat UI.

## Implementation Plan

1. **Frontend streaming** — In `ExplorerChat.svelte`:
   - On receiving `{ "type": "text", "content": "...", "done": false }`, append `content` to a running buffer for the current message
   - Render the buffer progressively (update the displayed message on each chunk)
   - On `{ "type": "text", "done": true }`, finalize the message
   - Use Svelte 5 `$state` for the streaming buffer

2. **Status indicators** — In `ExplorerChat.svelte`:
   - On receiving `{ "type": "status", "status": "thinking" }`, show a "Thinking..." indicator (animated dots or spinner) below the last message
   - On `"refining"`, show "Refining query..." indicator
   - On `"ready"`, hide the indicator
   - Use a subtle animation (CSS pulse or opacity cycle) to convey activity

3. **Backend token streaming** (if not already streaming):
   - In `explorer_ws.rs`, check whether the Claude Agent SDK callback sends partial text
   - If the SDK provides streaming callbacks, forward each chunk as a `{ "type": "text", "done": false }` message
   - Send `{ "type": "text", "done": true }` on completion
   - If the SDK does not stream, this is a no-op (the frontend already handles single-shot text)

4. Add component test for `ExplorerChat`:
   - Simulate a sequence of partial text messages and verify progressive rendering
   - Simulate status messages and verify indicator visibility

## Acceptance Criteria

- [ ] Chat displays text progressively as `done: false` chunks arrive
- [ ] "Thinking..." indicator appears when status is "thinking"
- [ ] "Refining query..." indicator appears when status is "refining"
- [ ] Indicators hide when status is "ready" or text starts arriving
- [ ] Final message is correct after all chunks are received
- [ ] Component test verifies streaming and status behavior

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `web/src/lib/ExplorerChat.svelte` for the current message handling
3. Read `crates/gyre-server/src/explorer_ws.rs` for the server-side message sending
4. Check `scripts/explorer-agent.mjs` (if it exists) for Claude Agent SDK streaming callbacks
5. Focus on the frontend streaming — the backend may already send partial messages
6. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

_(none yet)_
