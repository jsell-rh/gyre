---
title: "Explorer Phase 4 Polish — Streaming, Keyboard Shortcuts & Performance"
spec_ref: "explorer-implementation.md §28"
depends_on:
  - task-071
progress: not-started
coverage_sections:
  - "explorer-implementation.md §28 Phase 4: Polish"
commits: []
---

## Spec Excerpt

Phase 4 polish items from `explorer-implementation.md`:

- **Streaming text responses in chat** — LLM text should stream token-by-token to the chat panel, not wait for complete response
- **"Thinking..." / "Refining..." status indicators** — show agent status during self-check loop
- **Keyboard shortcuts:**
  - `Escape` — clear current view query, return to default view
  - `/` — focus the chat input
- **Mobile/responsive layout** — chat panel collapses to bottom sheet on narrow screens
- **Performance optimization for large graphs (>10k nodes)** — virtualized rendering, throttled updates
- **Visual regression tests** — screenshot-based tests for key canvas states

## Implementation Plan

1. **Audit existing streaming implementation** in `explorer_ws.rs`:
   - The WebSocket handler already streams `ExplorerServerMessage::Text` messages with `done: false/true`
   - Verify that `ExplorerChat.svelte` renders text incrementally as messages arrive
   - If text accumulates but only renders on `done: true`, fix the chat component to append on each message

2. **Audit status indicators:**
   - `ExplorerServerMessage::Status` messages (`thinking`, `refining`, `ready`) are already defined
   - Verify `ExplorerChat.svelte` displays a status indicator when status is `thinking` or `refining`
   - If missing, add a status line below the chat input: "Thinking...", "Refining query (attempt 2/3)..."

3. **Add keyboard shortcuts** to `ExplorerView.svelte`:
   - `Escape` → set `activeQuery = null` (clear current view query)
   - `/` → focus the chat input element (suppress when already in a text input)
   - Use the existing keyboard handling pattern from `App.svelte` (check for `e.target` being an input/textarea before handling)

4. **Responsive layout:**
   - In `ExplorerView.svelte`, use a CSS media query (`max-width: 768px`) to stack canvas and chat vertically
   - Chat panel becomes a collapsible bottom sheet on mobile (expandable via drag handle or button)
   - Canvas takes full width on mobile when chat is collapsed

5. **Performance for large graphs:**
   - In `ExplorerCanvas.svelte`, implement viewport culling: only render nodes/edges within the visible viewport
   - Throttle canvas redraws during pan/zoom (requestAnimationFrame)
   - For graphs >10k nodes, use WebGL rendering or canvas 2D with node batching instead of individual SVG elements
   - Add a performance warning banner when graph exceeds 10k nodes

6. **Wire up any missing integrations** between the three components (ExplorerCanvas, ExplorerChat, ExplorerView).

## Acceptance Criteria

- [ ] LLM text streams incrementally in the chat panel (not batch on completion)
- [ ] Status indicators show "Thinking..." and "Refining..." during agent processing
- [ ] `Escape` clears active view query
- [ ] `/` focuses chat input (suppressed in text inputs)
- [ ] Responsive layout: chat collapses on mobile (< 768px)
- [ ] Large graph (>10k nodes) renders without frame drops (>30fps pan/zoom)
- [ ] Frontend tests for keyboard shortcuts
- [ ] `npm test` passes in `web/`
- [ ] `cargo test --all` passes, `cargo fmt --all` clean

## Agent Instructions

This is primarily a frontend task. Start by reading the existing components: `web/src/lib/ExplorerCanvas.svelte`, `web/src/lib/ExplorerChat.svelte`, `web/src/components/ExplorerView.svelte`. Also read `crates/gyre-server/src/explorer_ws.rs` to understand the server-side message types. Many of these polish items may already be partially implemented — audit before adding. The spec says "Phase 4" which assumes Phases 1-3 are done. Check that the components render correctly with live data before polishing.
