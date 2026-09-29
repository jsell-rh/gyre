---
title: "LLM-Assisted Spec Editing — inline suggestions with Accept/Edit/Dismiss"
spec_ref: "ui-layout.md §3 LLM-Assisted Spec Editing"
depends_on: [task-172, task-171]
progress: not-started
coverage_sections:
  - "ui-layout.md §LLM-Assisted Spec Editing"
commits: []
---

## Spec Excerpt

ui-layout.md §3 "LLM-Assisted Spec Editing" defines the frontend interaction for spec editing assistance:

1. **User types change request** in the chat input (within the Spec tab or Editor Split): "Add a section on error handling for the timeout case"
2. **Frontend calls** `POST /api/v1/repos/:repo_id/specs/assist` — request: `{spec_path, instruction, draft_content?}`, response: `{diff: [{op, path, content}], explanation}`. This endpoint already exists.
3. **LLM produces a draft revision** shown as an inline diff block in the editor:
   ```
   ┌─ Suggested Change ──────────────────┐
   │  ## Error Handling                   │
   │  When the retry count exceeds...     │
   │  [Accept] [Edit] [Dismiss]           │
   └──────────────────────────────────────┘
   ```
4. **Accept:** applies change to editor content (in-memory, not committed). Save → commits to `spec-edit/*` branch via `POST /repos/:repo_id/specs/save`, auto-creates MR.
5. **Edit:** copies suggested text into editor for manual refinement.
6. **Dismiss:** removes the suggestion.

**Save workflow:** Commits to feature branch `spec-edit/<slug>-<hash>`. Auto-creates MR. Creates priority-2 "Spec pending approval" notification. After approval, MR auto-enqueued into merge queue.

**Multiple suggestions:** Each appears as a separate inline diff block. Human curates — LLM assists but never writes directly.

**Existing endpoints:**
- `POST /api/v1/repos/:repo_id/specs/assist` — LLM editing assistance (SSE stream)
- `POST /api/v1/repos/:repo_id/specs/save` — commit spec to feature branch + create MR

Both endpoints are already implemented in `specs_assist.rs`.

## Implementation Plan

1. **Spec chat input component** (`SpecChatInput.svelte`):
   - Text input with "Edit spec: ..." recipient indicator
   - Appears at the bottom of the Spec tab in the detail panel and in the Editor Split left panel
   - On submit, calls `POST /repos/:repo_id/specs/assist` with SSE streaming
   - Shows loading state while streaming

2. **Inline diff block component** (`SpecSuggestion.svelte`):
   - Renders the LLM's suggested changes as a styled diff block
   - Shows the section header (`path` field) and content
   - Three action buttons: Accept, Edit, Dismiss
   - Multiple suggestions can coexist as separate blocks
   - Accept → applies the diff op to the editor's in-memory content
   - Edit → copies content to editor at the appropriate position for manual editing
   - Dismiss → removes the suggestion block

3. **Diff application logic**:
   - Parse `diff` array from response: `{op: "add"|"remove"|"replace", path, content}`
   - `add`: insert content after the section identified by `path`
   - `remove`: delete content at section identified by `path`
   - `replace`: substitute section content with new `content`
   - `path` is a markdown section header or `"L15-L22"` line range fallback

4. **Save workflow integration**:
   - Save button calls `POST /repos/:repo_id/specs/save` with `{spec_path, content, message}`
   - Response: `{branch, mr_id}`
   - Show success notification with link to the created MR
   - If user has existing open `spec-edit/*` MR for same spec, appends commit to existing branch

5. **SSE streaming handling**:
   - Connect to SSE endpoint on submit
   - Render `partial` events progressively (explanation text)
   - On `complete` event, parse the full response and render suggestion blocks
   - On `error` event, show error message

6. **Tests**:
   - Chat input sends correct request to specs/assist
   - SSE partial events render progressively
   - Suggestion blocks render with Accept/Edit/Dismiss
   - Accept applies diff correctly to editor content
   - Save calls specs/save and shows success
   - Multiple suggestions rendered as separate blocks

## Acceptance Criteria

- [ ] Chat input with "Edit spec" recipient indicator in Spec tab and Editor Split
- [ ] Submitting instruction calls `POST /repos/:id/specs/assist` via SSE
- [ ] Explanation text streams progressively from `partial` events
- [ ] Suggested changes rendered as inline diff blocks
- [ ] Accept button applies change to editor content (in-memory)
- [ ] Edit button copies suggestion to editor for manual refinement
- [ ] Dismiss button removes the suggestion block
- [ ] Save button calls `POST /repos/:id/specs/save`, creates branch + MR
- [ ] Multiple suggestions coexist as separate blocks
- [ ] Tests pass

## Agent Instructions

Read `ui-layout.md` §3 "LLM-Assisted Spec Editing" for the full specification. The backend endpoints already exist — check `crates/gyre-server/src/api/specs_assist.rs` for `POST /repos/:id/specs/assist` and `POST /repos/:id/specs/save` request/response shapes. The routes are registered in `mod.rs` at `/api/v1/repos/:id/specs/assist` and `/api/v1/repos/:id/specs/save`. The Editor Split component is built in task-172 (`EditorSplit.svelte` or similar) — the spec chat input should integrate into the left panel of that component. For SSE handling in Svelte, use `EventSource` or `fetch` with `ReadableStream`. Check the existing SSE patterns in the web frontend (search for `text/event-stream` or `EventSource`).
