---
title: "Explorer Keyboard Shortcuts"
spec_ref: "explorer-implementation.md §Phase 4: Polish, system-explorer.md §1"
depends_on: 
  - task-029
progress: ready-for-review
coverage_sections: []
commits: ["91abab172"]
---

## Spec Excerpt

From `explorer-implementation.md` §Phase 4:

> - Keyboard shortcuts (Escape to clear query, / to focus chat)

From `system-explorer.md` §1:

> **Search** (`/`) → Find any entity by name, type, or spec within the canvas. Results highlight on the canvas.

From `ui-layout.md` §2:

> Closing the panel (Esc or ✕) closes the panel, main content returns to full-width.

## Shipped

The shortcut implementation shipped with the explorer-canvas branch
(`ExplorerView.svelte` `onWindowKeydown`, wired via
`<svelte:window onkeydown>`). This assignment closed the missing
acceptance criterion "Test verifies shortcut behavior" with
`web/src/__tests__/ExplorerViewShortcuts.test.js` (commit `91abab172`),
which renders the full ExplorerView tree and exercises the real event
wiring.

### Behavior

- **Escape cascade** (`ExplorerView.svelte` `onWindowKeydown`): closes
  the spec editor first, then the query editor, then the detail panel,
  then clears `activeViewQuery` — the same state that trace mode ("Trace
  from here") is an instance of (an interactive view query with
  `_trace: true`), so Escape exits trace mode.
- **`/`** focuses the `.chat-input` chat input with `preventDefault()`
  so no "/" is typed; falls back to the concept search input when the
  chat is hidden.
- **isTyping guard**: shortcuts are suppressed when the event target is
  an INPUT/TEXTAREA/SELECT/contentEditable element.
- `ExplorerCanvas.svelte` additionally handles Cmd+K (global search),
  Tab/Shift+Tab (node cycling), and its own canvas-scoped Escape cascade
  (search overlay → context menu → active query → breadcrumb → fit).

### Test evidence

`ExplorerViewShortcuts.test.js` (6 tests, all passing):

- `/` focuses the chat input (after bringing the mocked WS session to
  `ready` so the input is enabled) and `defaultPrevented` is true.
- `/` while the concept search input is focused: no preventDefault, no
  focus steal.
- Escape clears the active view query set through the Risk heat map
  toolbar button; the annotation disappears and the toolbar overlay flag
  resets.
- Escape exits trace mode: the manual query editor runs the exact query
  object the "Trace from here" context action produces; the canvas draws
  the numbered step badges (1/2 — asserted on the mocked canvas context
  draw calls) while the trace is active, and after Escape the redraw
  still draws node labels but no step badges. The cascade is also
  verified: the first Escape closes only the query editor.
- Escape closes the detail panel opened through the canvas-scoped
  search flow (canvas `/` → search → result click → detail panel).
- Escape inside a text input does not clear the active view query.

Mutation checks (see `/tmp/stage/review-evidence/task-038-mutation-checks.txt`):
removing the `activeViewQuery`/`detailNode` Escape branches, disabling
the `/` handler, and dropping the isTyping guard each fail the
corresponding test(s). Explorer-surface regression: 313 tests across 7
explorer test files pass.

## Acceptance Criteria

- [x] Escape clears the active view query and returns canvas to default state
- [x] Escape closes the detail panel if it's open
- [x] Escape exits trace mode if active
- [x] `/` focuses the chat input (without typing "/" into it)
- [x] Shortcuts do not fire when a text input is focused
- [x] Test verifies shortcut behavior

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `web/src/components/ExplorerView.svelte` for the component structure
3. Read `web/src/lib/ExplorerChat.svelte` for the chat input element
4. Read `web/src/lib/ExplorerCanvas.svelte` for the active query state
5. Use `svelte:window` or `onMount` with `addEventListener` for keyboard handling
6. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- `91abab172` — test(task-038): verify explorer keyboard shortcuts
  (implementation itself shipped with the explorer-canvas branch prior to
  task decomposition)
