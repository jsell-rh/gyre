---
title: "Editor Split layout component for spec and meta-spec editing"
spec_ref: "ui-layout.md §2 Editor Split"
depends_on: []
progress: complete
coverage_sections:
  - "ui-layout.md §Editor Split"
commits: ["a4c8b6bd80ecbbe4ce43edc666f3b5bbe766d808", "a44ba473711eb46d11bfc6348bc166051144481c", "4b80b93bc68116b0d54d8696fdd3bc78d6931069", "f27a4ef5b2b77f0469c6b5b1100d2156efa597e3", "33b4241d762be547f88f134fe1a4bff84d54cfc8", "471563d7d218003d6d2dbbab3ba0598e801d6f3a", "1ae8ca350439f07c5c75dcfbb7704dbf0ce96021", "a53ade326e99a7954fe976a84981e3df0212465b", "80925c2145b51e64ddb69ef1b4fb900c44ee0a89", "3688b5cddbfddbed2f4cdc158f03ce35bb172951"]
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

EditorSplit (`web/src/lib/EditorSplit.svelte`) implements the §2 Editor
Split layout as one reusable component used by **both production
contexts**:

- **Spec context** — mounted by DetailPanel (`context="spec"`; pop-out via
  the Preview button in DetailPanel.svelte → `openEditorSplit`, Back/Esc →
  `closeEditorSplit` returns to the normal detail panel view). Left pane:
  markdown textarea with toolbar, Save with `base_sha` optimistic
  concurrency and 409-conflict dialog, concurrent-edit banner, and the
  inline LLM chat (Ctrl/Cmd+Enter → `POST /repos/:id/specs/assist` SSE
  stream). The draft revision renders as an inline diff block: **Accept**
  applies the diff ops in memory without saving, **Edit** copies the
  suggested text into the editor, **Dismiss** drops the suggestion. Right
  pane: Architecture (default, graph + ghost overlays) and Code Diff tabs.
  State machine editing → preview_running → preview_complete via
  `thoroughPreview` + `taskStatus` polling; Iterate returns to editing
  with results retained.
- **Meta-spec context** (`context="meta-spec"`, §9 preview loop) —
  mounted in production by the MetaSpecs workspace view
  (MetaSpecs.svelte, mounted from App.svelte for `mode === 'agent_rules'`),
  which passes `targetSpecs`, `onpublish` (persona registry Publish) and
  `onPreviewStateChange` (locks the persona selector while a preview
  runs, §9 State 2). The previous parallel §9 implementation inside
  MetaSpecs — including its simulated "complete" fallback that fabricated
  State 3 when previewPersona failed — was removed: one production
  implementation of the §9 loop. Preview runs `previewPersona` +
  `previewPersonaStatus` polling and renders the real PreviewResponse
  fields (structural_impact, blast_radius, per-spec statuses); a failed,
  unavailable or timed-out preview returns to editing with an error
  banner and never presents State 3.
- **Esc semantics** — one Escape does exactly one thing. While the
  EditorSplit pop-out is open, DetailPanel's panel-div keydown handler
  returns early (it sits on the bubbling path before EditorSplit's window
  handler, so previously one Esc both closed the split and popped detail
  history via `onback()` — the double-fire). While a preview is running,
  Esc cancels the preview (§9 State 2) rather than closing the split.
  While the 409 conflict dialog is open it owns Esc.
- **i18n** — all `editor_split.*` keys are referenced by the component;
  the 22 `meta_specs.workspace.*` keys made dead by the consolidation were
  removed, one new `editor_split.no_specs_available` empty-state key
  added.
- **Coverage** — `specs/coverage/system/ui-layout.md` row #13 marked
  `implemented` (both production contexts noted); no verifier/exemption
  files touched.

### Review-repair rounds

- **a44ba473** (review 70971f1e): SpecConflictDialog open-dialog guard for
  Esc, stale dist rebuilt, debug console.log removed, assertionless
  scratch test deleted, nine `md_*_aria` i18n keys wired as toolbar
  aria-labels, Shipped bookkeeping made truthful.
- **a4c8b6bd** (review 248c3a60, durable findings from 8fdde267):
  (1) Esc double-fire fixed with the DetailPanel `showEditorSplit` guard
  and pinned by a bubbling-Escape-from-textarea test with an `onback`
  spy; (2) the §9 meta-spec context is now reached from production via
  MetaSpecs mounting EditorSplit (was component-level only, with
  MetaSpecs carrying a divergent parallel implementation); (3) the suite
  now exercises real deployment paths — thoroughPreview rejecting with
  the 404 produced by every real spec-context Preview (no server route),
  previewPersona/previewPersonaStatus fed the exact
  PreviewResponse/PreviewStatusResponse shapes from meta_specs.rs, and
  Esc fired as a bubbling event inside the DetailPanel panel div. Also
  fixed en route: the preview-error banner now renders in every
  right-pane state (it was invisible in the meta-spec selector branch a
  failed preview returns to).

### Test evidence (artifacts under /tmp/stage/review-evidence/)

`npm ci`, then `./node_modules/.bin/vitest run src/__tests__/EditorSplit.test.js
src/__tests__/DetailPanel.test.js src/__tests__/MetaSpecs.test.js` →
**174/174 passed** at this head (EditorSplit 42 — including the real
PreviewResponse-shape rendering, thoroughPreview-404 and previewPersona-404
honest-failure paths, and Esc-cancels-running-preview; DetailPanel 30
including the bubbling-Esc double-fire regression test; MetaSpecs 102 —
workspace suite rewritten against the EditorSplit-backed §9 loop,
tenant/repo/accessibility suites unchanged). Adjacent suites that render
DetailPanel/RepoMode/MetaSpecs transitively (18 files) all pass.
`web/dist` rebuilt from this source: one clean bundle set, zero conflict
markers, zero debug statements, byte-identical on a second build.
Attribution gate `scripts/check-task-commit-attribution.sh` exit 0 at this
head. TCP listener probes are unsupported in this sandbox (errno 95,
/tmp/stage/capabilities.json), so no live server/browser run; exact-head
GitHub CI (`web-build`) remains mandatory after independent review.
