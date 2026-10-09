---
title: "Editor Split layout component for spec and meta-spec editing"
spec_ref: "ui-layout.md §2 Editor Split"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "ui-layout.md §Editor Split"
commits: ["1ae8ca350439f07c5c75dcfbb7704dbf0ce96021", "a53ade326e99a7954fe976a84981e3df0212465b", "80925c2145b51e64ddb69ef1b4fb900c44ee0a89", "3688b5cddbfddbed2f4cdc158f03ce35bb172951", "2fa7ca25070b32716b8f8383b617ba28ec1e1c20"]
---

## Spec Excerpt

The Editor Split layout (ui-layout.md §2) is used by meta-specs preview loop and spec editing with preview. It has two panels:

**Left panel** — spec or meta-spec editor with two modes:
- Manual edit: standard markdown editor with toolbar
- LLM chat: inline input below editor. User types instruction → LLM produces draft revision shown as inline diff block with Accept/Edit/Dismiss buttons

**Right panel** — preview of architectural impact:
- Architecture tab (default): structural diff from knowledge graph — added/modified/removed nodes
- Code Diff tab: traditional line-level diff of agent implementation on throwaway branch

The detail panel "pops out to full width" (Pop Out mechanism from §2 Split layout) and switches to Editor Split when the user clicks "Preview" in the spec detail panel.

Three states: Editing → Preview Running → Preview Complete (with Iterate option).

## Implementation Plan

1. **EditorSplit.svelte component** (`web/src/components/`):
   - Two-panel layout: left (editor, 50%) + right (preview, 50%)
   - Left panel: markdown textarea with toolbar + LLM chat input
   - Right panel: tabs for Architecture (default) and Code Diff
   - Inline diff block component with Accept/Edit/Dismiss buttons
   - State machine: editing → preview_running → preview_complete

2. **Pop Out integration**:
   - The existing DetailPanel's "Pop Out" mechanism should transition to EditorSplit
   - Back/Esc returns to normal detail panel view

3. **LLM draft revision display**:
   - Inline diff block component showing suggested changes
   - Accept: applies changes to editor content (in-memory, not committed)
   - Edit: copies suggested text into editor
   - Dismiss: removes suggestion

4. **Preview states**:
   - Editing: editor active, preview panel shows target spec selector
   - Preview Running: editor locked, progress indicators per spec
   - Preview Complete: Architecture delta + Code Diff tabs

5. **Tests**:
   - Component tests for state transitions
   - Accept/Edit/Dismiss button behaviors
   - Pop-out and return navigation

## Acceptance Criteria

- [x] EditorSplit component renders two-panel layout
- [x] Left panel supports manual edit and LLM chat modes
- [x] Right panel has Architecture and Code Diff tabs
- [x] Inline diff blocks with Accept/Edit/Dismiss work correctly
- [x] State machine handles editing → running → complete → iterate cycle
- [x] Pop Out from DetailPanel transitions to EditorSplit
- [x] Back/Esc returns to normal view
- [x] Tests pass

## Shipped

EditorSplit (`web/src/lib/EditorSplit.svelte`) implements the full §2 Editor
Split layout as a reusable component used by spec editing (DetailPanel pop-out,
pre-existing integration, unchanged) and available for meta-spec editing
(`context="meta-spec"`):

- **Two-panel layout:** markdown editor left, architecture preview right;
  Back button and Esc call `onClose` (DetailPanel collapses back to the
  normal detail view on both).
- **Manual edit:** textarea with content binding (`bind:content`), Save
  button posts `specs/save` with `base_sha` optimistic concurrency and a
  409 conflict dialog (overwrite/discard).
- **LLM chat mode:** inline input below the editor (Ctrl/Cmd+Enter),
  streams SSE from `POST /repos/:id/specs/assist`; the draft revision
  renders as an inline diff block with **Accept** (applies diff ops
  in-memory, not committed), **Edit** (copies suggested text into the
  editor), **Dismiss** (removes the suggestion) — §3 steps 4-6.
- **Right panel tabs:** Architecture (default — real knowledge-graph
  nodes/edges from `repoGraph` + `graphPredict` ghost overlays on
  `ArchPreviewCanvas`) and Code Diff (line-level `SpecDiffView` of the
  agent implementation from the preview result).
- **Preview state machine:** editing → preview_running → preview_complete.
  Spec context: `thoroughPreview` (throwaway branch) + `taskStatus`
  polling. Meta-spec context (§9): target spec selector, `previewPersona`
  + `previewPersonaStatus` polling with per-spec progress indicators,
  Iterate returns to editing with results still visible, Cancel aborts.

Recovered from the interrupted checkpoint (1ae8ca35): finished the dead
`previewTaskId` variable removal (0 references remain), rebuilt `web/dist`
which still contained the stale pre-cleanup bundle, and re-ran all affected
suites.

**Test evidence** (2026-10-09, saved under /tmp/stage/review-evidence/):
- `npx vitest run src/__tests__/EditorSplit.test.js` → **37/37 passed**
  (rendering, contexts, close callbacks, Accept/Edit/Dismiss, save +
  conflict + overwrite, concurrent-edit banner, spec preview immediate +
  taskStatus polling, meta-spec selector + previewPersona with/without
  persona_id, iterate retention, cancel).
- `npx vitest run src/__tests__/DetailPanel.test.js` → **29/29 passed**,
  including the 5 "Editor Split pop-out" tests (Preview click expands and
  mounts EditorSplit, Back collapses, Esc closes, editor + arch preview
  side by side).
- Combined run: **66/66 passed**. `npx vite build` → success; dist
  committed.

**Sandbox limitation:** TCP listener `accept()` unsupported (errno 95), so
no live server/browser probe here. Exact HTTP checks for host verification
are recorded in /tmp/stage/review-evidence/task-172-summary.md; exact-head
GitHub CI remains mandatory.

## Agent Instructions

Read `ui-layout.md` §2 "Editor Split" and §9 "Meta-specs Preview Loop Layout" for the full interaction design including all three states. Check the existing `MetaSpecs.svelte` component — it may already have partial editor functionality. The EditorSplit is used by BOTH spec editing and meta-spec editing; build it as a reusable component. The meta-spec preview loop layout (§9) shows the exact wireframes for States 1-3.
