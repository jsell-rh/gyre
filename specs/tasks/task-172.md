---
title: "Editor Split layout component for spec and meta-spec editing"
spec_ref: "ui-layout.md §2 Editor Split"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "ui-layout.md §Editor Split"
commits: ["a44ba473711eb46d11bfc6348bc166051144481c", "4b80b93bc68116b0d54d8696fdd3bc78d6931069", "f27a4ef5b2b77f0469c6b5b1100d2156efa597e3", "33b4241d762be547f88f134fe1a4bff84d54cfc8", "471563d7d218003d6d2dbbab3ba0598e801d6f3a", "1ae8ca350439f07c5c75dcfbb7704dbf0ce96021", "a53ade326e99a7954fe976a84981e3df0212465b", "80925c2145b51e64ddb69ef1b4fb900c44ee0a89", "3688b5cddbfddbed2f4cdc158f03ce35bb172951"]
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

- [ ] EditorSplit component renders two-panel layout
- [ ] Left panel supports manual edit and LLM chat modes
- [ ] Right panel has Architecture and Code Diff tabs
- [ ] Inline diff blocks with Accept/Edit/Dismiss work correctly
- [ ] State machine handles editing → running → complete → iterate cycle
- [ ] Pop Out from DetailPanel transitions to EditorSplit
- [ ] Back/Esc returns to normal view
- [ ] Tests pass

## Agent Instructions

Read `ui-layout.md` §2 "Editor Split" and §9 "Meta-specs Preview Loop Layout" for the full interaction design including all three states. Check the existing `MetaSpecs.svelte` component — it may already have partial editor functionality. The EditorSplit is used by BOTH spec editing and meta-spec editing; build it as a reusable component. The meta-spec preview loop layout (§9) shows the exact wireframes for States 1-3.

## Shipped

EditorSplit (`web/src/lib/EditorSplit.svelte`) implements the §2 Editor Split
layout as one reusable component with two contexts:

- **Spec context** — mounted by DetailPanel (`context="spec"`; pop-out via the
  Preview button in DetailPanel.svelte → `openEditorSplit`, Back/Esc →
  `closeEditorSplit` returns to the normal detail panel view). Left pane:
  markdown textarea with toolbar, Save with `base_sha` optimistic concurrency
  and 409-conflict dialog, concurrent-edit banner, and the inline LLM chat
  (Ctrl/Cmd+Enter → `POST /repos/:id/specs/assist` SSE stream, partial and
  complete events). The draft revision renders as an inline diff block:
  **Accept** applies the diff ops to the editor content in memory without
  saving, **Edit** copies the suggested text into the editor, **Dismiss**
  drops the suggestion. Right pane: Architecture (default — nodes/edges from
  `repoGraph` plus `graphPredict` ghost overlays on ArchPreviewCanvas) and
  Code Diff (`SpecDiffView` line-level diff from the thorough-preview result)
  tabs. State machine editing → preview_running → preview_complete via
  `thoroughPreview` + `taskStatus` polling (10 s interval, 300 s timeout,
  cancelable); Iterate returns to editing with the results retained.
- **Meta-spec context** (`context="meta-spec"`, §9 preview loop) — target spec
  selector checklist (Preview disabled until a target is selected), Preview
  via `previewPersona` + `previewPersonaStatus` polling (1.5 s) with per-spec
  progress indicators, architecture_diff parsing into ghost overlays,
  specs_diff into the Code Diff tab, Iterate/Publish hooks.
- **i18n** — all 67 `editor_split.*` keys in `web/src/locales/en.json` are
  referenced by the component (the nine `md_*_aria` keys are wired as the
  toolbar buttons' aria-labels/titles via `$t()`); no missing or dead keys.
- **Coverage** — `specs/coverage/system/ui-layout.md` row #13 (Editor Split)
  marked `implemented` with the SUMMARY row updated; no other coverage rows
  or verifier/exemption files touched.

### Review-repair round (commit a44ba473)

All six findings from review 70971f1e were fixed at merge head d59d5a06:

- **Esc regression** (`SpecConflictDialog.svelte`): the window keydown
  handler consumed every Escape (stopPropagation/cancelBubble) even when the
  dialog was closed, so EditorSplit's Back/Esc in the DetailPanel pop-out
  never fired. Fixed with an open-dialog guard (`if (!conflict) return;`);
  Esc is consumed only while the dialog is open. The previously failing test
  "Esc key closes EditorSplit and collapses panel" now passes.
- **Stale dist**: `web/dist/index.html` contained git merge-conflict markers
  and two tracked bundle sets. Rebuilt from this source with `vite build` —
  one clean bundle set, zero conflict markers, and a fresh rebuild to a
  temp outDir is byte-identical (`diff -r` clean).
- **Debug leftover**: removed `console.log('DEBUG EditorSplit...')` from
  `handleKeydown`; the production bundle contains no match.
- **Test pollution**: deleted the assertionless
  `web/src/__tests__/__debug-esc.test.js` scratch test.
- **Dead i18n keys**: the nine `editor_split.md_*_aria` keys are now wired
  as localized toolbar aria-labels/titles, pinned by a new test.
- **Task bookkeeping**: this Shipped section replaces the earlier notes,
  whose "66/66" and "dist byte-exact" claims were false at the reviewed
  head.

### Test evidence (artifacts under /tmp/stage/review-evidence/)

`npm ci`, then `./node_modules/.bin/vitest run src/__tests__/EditorSplit.test.js
src/__tests__/DetailPanel.test.js src/__tests__/SpecConflictDialog.test.js`
→ **72/72 passed** at this head (EditorSplit 38 — rendering, contexts,
Back/Esc, content binding, toolbar aria-labels, LLM streaming +
Accept/Edit/Dismiss, save with base_sha + 409 conflict + overwrite,
concurrent-edit banner, spec preview with taskStatus polling + cancel,
iterate retention, meta-spec selector + previewPersona with and without
persona_id; DetailPanel 29 including the 5 pop-out tests; SpecConflictDialog
5). Attribution gate `scripts/check-task-commit-attribution.sh` exit 0 at
this head. TCP listener probes are unsupported in this sandbox (errno 95,
/tmp/stage/capabilities.json), so no live server/browser run; exact-head
GitHub CI (`web-build`) remains mandatory after independent review.
