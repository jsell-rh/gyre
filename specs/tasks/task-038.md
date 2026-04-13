---
title: "Explorer Keyboard Shortcuts"
spec_ref: "explorer-implementation.md §Phase 4: Polish, system-explorer.md §1"
depends_on: 
  - task-029
progress: complete
coverage_sections: []
commits: []
---

## Spec Excerpt

From `explorer-implementation.md` §Phase 4:

> - Keyboard shortcuts (Escape to clear query, / to focus chat)

From `system-explorer.md` §1:

> **Search** (`/`) → Find any entity by name, type, or spec within the canvas. Results highlight on the canvas.

From `ui-layout.md` §2:

> Closing the panel (Esc or ✕) closes the panel, main content returns to full-width.

## Current State

**Implemented.** All acceptance criteria met:
- `ExplorerView.svelte` line 1026: `<svelte:window onkeydown={onWindowKeydown}>` global listener
- Lines 892-899: Escape cascade clears `activeViewQuery = null`
- Lines 901-910: `/` key focuses `.chat-input` element
- Line 890: `isTyping` check prevents shortcuts when input/textarea focused
- `ExplorerCanvas.svelte` also has Cmd+K (global search), Tab/Shift+Tab (node cycling), Enter (select in search)

## Implementation Plan

1. In `ExplorerView.svelte`, add a `keydown` event listener (on `window` or the explorer container):

   | Key | Action |
   |---|---|
   | `Escape` | Clear active view query; if detail panel is open, close it; if trace mode is active, exit trace mode |
   | `/` | Focus the chat input in `ExplorerChat.svelte` (prevent default to avoid typing `/` in the input) |
   | `?` | Show keyboard shortcut help overlay (optional, low priority) |

2. Ensure shortcuts do NOT fire when:
   - A text input/textarea is focused (check `document.activeElement` tagName)
   - A modal is open

3. In `ExplorerChat.svelte`, expose a `focus()` method or use a bindable ref that `ExplorerView` can call when `/` is pressed.

4. In `ExplorerCanvas.svelte`, expose a `clearQuery()` method or accept an `onClearQuery` callback that `ExplorerView` triggers on Escape.

5. Add test verifying shortcut behavior (mock keydown events, check focus state and query clearing).

## Acceptance Criteria

- [ ] Escape clears the active view query and returns canvas to default state
- [ ] Escape closes the detail panel if it's open
- [ ] Escape exits trace mode if active
- [ ] `/` focuses the chat input (without typing "/" into it)
- [ ] Shortcuts do not fire when a text input is focused
- [ ] Test verifies shortcut behavior

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `web/src/components/ExplorerView.svelte` for the component structure
3. Read `web/src/lib/ExplorerChat.svelte` for the chat input element
4. Read `web/src/lib/ExplorerCanvas.svelte` for the active query state
5. Use `svelte:window` or `onMount` with `addEventListener` for keyboard handling
6. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

Implemented as part of the explorer-canvas branch prior to task decomposition.
