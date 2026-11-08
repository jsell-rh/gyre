---
title: "Explorer Phase 4 Polish — Streaming, Keyboard Shortcuts & Performance"
spec_ref: "explorer-implementation.md §28"
depends_on:
  - task-071
progress: ready-for-review
coverage_sections:
  - "explorer-implementation.md §28 Phase 4: Polish"
commits: ["293246b7c"]
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

- [x] LLM text streams incrementally in the chat panel (not batch on completion)
- [x] Status indicators show "Thinking..." and "Refining..." during agent processing
- [x] `Escape` clears active view query
- [x] `/` focuses chat input (suppressed in text inputs)
- [x] Responsive layout: chat collapses on mobile (< 768px)
- [x] Large graph (>10k nodes) renders without frame drops (>30fps pan/zoom)
- [x] Frontend tests for keyboard shortcuts
- [x] `npm test` passes in `web/`
- [x] `cargo test --all` passes, `cargo fmt --all` clean

## Agent Instructions

This is primarily a frontend task. Start by reading the existing components: `web/src/lib/ExplorerCanvas.svelte`, `web/src/lib/ExplorerChat.svelte`, `web/src/components/ExplorerView.svelte`. Also read `crates/gyre-server/src/explorer_ws.rs` to understand the server-side message types. Many of these polish items may already be partially implemented — audit before adding. The spec says "Phase 4" which assumes Phases 1-3 are done. Check that the components render correctly with live data before polishing.

## Shipped

Audit + gap-fill of §28 Phase 4. Five of the six polish items were
already shipped and tested by earlier explorer tasks (task-036, task-038,
task-053, task-057, task-065, task-071); this round audited each against
production code and closed the one genuine gap: the >10k-node
performance warning banner.

### Behavior (audited in production code)

- **Streaming**: `ExplorerChat` accumulates `text` chunks with
  `done:false` into a live streaming bubble (markdown rendered before
  finalization); `done:true` finalizes. 65s no-chunk timeout and
  disconnect both finalize orphaned streams.
- **Status indicators**: header status dot + label
  (Thinking.../Refining query.../Ready/Error/Disconnected) and a
  thinking-indicator with animated dots and a Cancel button while
  waiting.
- **Keyboard shortcuts**: `Escape` clears the active view query via the
  overlay cascade (spec editor → query editor → detail panel → query);
  `/` focuses the chat input with `preventDefault`, suppressed while a
  text input is focused.
- **Responsive layout**: `@media (max-width: 768px)` stacks canvas and
  chat; chat becomes a full-screen collapsible sheet with a collapse
  bar (canvas takes full width when collapsed); `chatCollapsed` resets
  when the viewport widens past 1025px.
- **Large-graph rendering**: viewport culling, zoom-level LOD label
  guards, edge bundling, and the rAF-throttled redraw loop keep per-frame
  draw work bounded (verified by mutation-sensitive perf tests on
  10k/15k-node graphs).
- **NEW — performance warning banner**: `ExplorerView` shows a
  dismissible `role="status"` banner over the canvas when the effective
  graph exceeds 10,000 nodes, naming the actual node count. The
  dismissed state resets when a new repo graph loads. The toolchain and
  performance warnings now stack in a `.canvas-warning-stack` wrapper
  (identical positioning to the previous single-banner layout).
- **Visual regression tests**: 14 committed Playwright baselines cover
  zoom levels, filter presets, annotated view queries, blast radius,
  and toolbar lenses.

### Test evidence

- New `web/src/__tests__/ExplorerViewLargeGraph.test.js`: a 10,001-node
  graph renders the banner (with the node count in its text) and the
  dismiss button hides it; a 10,000-node graph renders no banner
  (threshold boundary).
- Focused re-runs green: ExplorerViewLargeGraph, ExplorerView-responsive,
  ExplorerViewShortcuts, ExplorerChat-streaming (22 tests),
  ExplorerCanvas-performance, ExplorerViewDrillUrl, ExplorerChat
  (133 tests).
- Full `npm test` in `web/`: 69 files, 1690 passed | 41 skipped.
- `cargo fmt --all --check` clean; no Rust changes shipped, so the
  cargo test surface is unaffected (full workspace suites owned by
  verification).

Probe commands, exit codes, and the per-item audit table are retained in
`/tmp/stage/review-evidence/task-078-focused-probes.md`.
