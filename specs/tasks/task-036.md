---
title: "Explorer Chat Streaming & Status Indicators"
spec_ref: "explorer-implementation.md §Phase 4: Polish"
depends_on: []
progress: ready-for-review
coverage_sections: []
commits: ["d988af76230b318bf435a89bee9fdd9e6db04b2e"]
---

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

**Implemented.** All acceptance criteria met:
- `ExplorerChat.svelte` lines 317-344: `case 'text'` handler accumulates streaming chunks via `streamingText += chunk` (done:false)
- Line 910: `renderMarkdown(streamingText)` renders live during streaming
- Lines 638-639, 917-931: status indicators with animated spinner for "thinking" and "refining" states
- `explorer_ws.rs` lines 2827-2840: LLM response streamed in token chunks over WebSocket

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

- [x] Chat displays text progressively as `done: false` chunks arrive
- [x] "Thinking..." indicator appears when status is "thinking"
- [x] "Refining query..." indicator appears when status is "refining"
- [x] Indicators hide when status is "ready" or text starts arriving
- [x] Final message is correct after all chunks are received
- [x] Component test verifies streaming and status behavior

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `web/src/lib/ExplorerChat.svelte` for the current message handling
3. Read `crates/gyre-server/src/explorer_ws.rs` for the server-side message sending
4. Check `scripts/explorer-agent.mjs` (if it exists) for Claude Agent SDK streaming callbacks
5. Focus on the frontend streaming — the backend may already send partial messages
6. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- `d988af762` — feat(task-036): component tests for chat streaming & status indicators

Earlier behavior shipped on the explorer-canvas branch prior to task
decomposition; this assignment added the missing verification layer.

## Shipped

### Behavior (pre-existing on this branch, verified this assignment)

- **Frontend streaming** (`web/src/lib/ExplorerChat.svelte`): the
  `case 'text'` handler accumulates `done:false` chunks into a Svelte 5
  `$state` buffer (`streamingText`), renders it live via
  `renderMarkdown(streamingText)` in a `.chat-message.streaming` bubble,
  and finalizes the full concatenated text as an assistant message on
  `done:true`. A 65s no-chunk timeout and disconnect handlers finalize
  orphaned streams; a `thinking`/`refining` status arriving mid-stream
  finalizes the partial text as a visually distinct `(thinking)` message
  instead of dropping it.
- **Status indicators** (`ExplorerChat.svelte`): `.thinking-indicator`
  (role=status, animated dots) renders for `thinking`/`refining` while no
  streaming text is on screen, labeled "Thinking..." / "Refining
  query..."; it hides on `ready` or once text chunks start arriving (the
  streaming bubble replaces it).
- **Backend token streaming** (`crates/gyre-server/src/explorer_ws.rs`):
  LLM responses are forwarded in ~60-char clause-boundary chunks as
  `{"type":"text","done":false}` frames with an 8ms inter-chunk delay,
  closed by a `done:true` frame.

### What this assignment added

The prior "component test" coverage in `ExplorerChat.test.js` replicated
the handler logic inside the test file (mirrored logic — it stayed green
if the component itself broke), so acceptance criterion 6 was not
actually satisfied. Added
`web/src/__tests__/ExplorerChat-streaming.test.js`: mounts the **real**
`ExplorerChat` component with a controllable `MockWebSocket`, completes
the ticket handshake, and pushes the spec's server→client frames through
the component's real `onmessage` path, asserting on rendered DOM:

1. Progressive rendering across `done:false` chunks + finalization on
   `done:true` (including a trailing chunk delivered with the done frame)
2. "Thinking..." → "Refining query..." indicator transitions, hidden on
   `ready`
3. Indicator hidden while streaming text is displayed
4. Mid-stream `refining` finalizes partial text as a `(thinking)` message
5. User round trip: typed message sent as `{"type":"message","text":...}`
   over the socket and rendered as a user bubble

### Test evidence

- `npx vitest run src/__tests__/ExplorerChat-streaming.test.js` — 6/6
  passed
- `npx vitest run src/__tests__/ExplorerChat.test.js` — 108/108 passed
  (no regression)
- Mutation checks (component restored after each): replacing
  `streamingText += chunk` with `streamingText = chunk` fails 2 tests;
  disabling the indicator render condition fails 4 tests — the suite
  kills real defects in the specced behavior
