---
title: "Editor Split layout component for spec and meta-spec editing"
spec_ref: "ui-layout.md §2 Editor Split"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "ui-layout.md §Editor Split"
commits: ["471563d7d218003d6d2dbbab3ba0598e801d6f3a", "1ae8ca350439f07c5c75dcfbb7704dbf0ce96021", "a53ade326e99a7954fe976a84981e3df0212465b", "80925c2145b51e64ddb69ef1b4fb900c44ee0a89", "3688b5cddbfddbed2f4cdc158f03ce35bb172951"]
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

EditorSplit (`web/src/lib/EditorSplit.svelte`, 1494 lines) implements the §2
Editor Split layout as a reusable component with two contexts:

- **Spec context** — mounted by DetailPanel (`context="spec"`, pop-out via the
  Preview button at DetailPanel.svelte:3647 → `openEditorSplit` :68; Back/Esc →
  `closeEditorSplit` collapses the panel). Left pane: markdown textarea with
  `bind:content`, Save with `base_sha` optimistic concurrency + 409 conflict
  dialog (overwrite/discard), ConcurrentEditBanner presence, and the inline LLM
  chat (Ctrl/Cmd+Enter → `POST /repos/:id/specs/assist` SSE stream with
  partial/complete event handling). The draft revision renders as an inline
  diff block: **Accept** applies the diff ops in-memory without saving,
  **Edit** copies the suggested text into the editor, **Dismiss** drops it
  (§3 steps 4–6). Right pane: Architecture (default — real nodes/edges from
  `repoGraph` + `graphPredict` ghost overlays on ArchPreviewCanvas) and Code
  Diff (`SpecDiffView` line-level diff from the thorough-preview result) tabs.
  State machine editing → preview_running → preview_complete with
  `thoroughPreview` + `taskStatus` polling (10 s interval, 300 s timeout,
  cancel available), Iterate returns to editing with results retained.
- **Meta-spec context** (`context="meta-spec"`, §9 preview loop) — target spec
  selector checklist (Preview disabled until a target is selected), Preview
  via `previewPersona` + `previewPersonaStatus` polling (1.5 s) with per-spec
  progress indicators, architecture_diff line parsing (`+`/`~` prefixes) into
  ghost overlays, specs_diff into the Code Diff tab, Iterate/Publish hooks.

This repair assignment restored the original task contract after the prior
attempt drifted into task-210's attribution ledger (its 69a63669 edited
task-210's frontmatter on the task-172 branch). That edit is reverted — the
branch's specs/tasks/task-210.md is byte-identical to base. The revert
restores a pre-existing upstream condition: origin/main head (8c2d1775)
itself fails the attribution pre-commit hook because a781ede2
(`feat(task-210)`, 28 product-surface files) is missing from task-210's
frontmatter on main (verified in a clean worktree at base, gate exit 1).
That drift belongs to task-210's scope on main, not this branch; it is
recorded here for the reviewer to route to the right task. Within this
branch's own contract: task-172.md keeps the original contract sections
byte-identical to base, coverage matrix row #13 (ui-layout.md §Editor Split)
marked `implemented` with evidence, no exemptions or verifier changes.

**Test evidence** (2026-10-09, this session; artifacts under
/tmp/stage/review-evidence/):
- `npx vitest run src/__tests__/EditorSplit.test.js src/__tests__/DetailPanel.test.js`
  → **66/66 passed** (EditorSplit 37: rendering, contexts, Back/Esc, content
  binding, LLM streaming + Accept/Edit/Dismiss, save + base_sha + 409 conflict
  + overwrite, concurrent-edit banner, spec preview immediate + taskStatus
  polling + cancel, iterate retention, meta-spec selector + previewPersona
  with/without persona_id; DetailPanel 29 including the 5 pop-out tests).
- `scripts/check-task-commit-attribution.sh` → **FAIL on this branch, same
  pre-existing upstream failure as base** (verified in a clean worktree at
  base 8c2d1775: exit 1). Cause: a781ede2 (`feat(task-210)`) is on main but
  absent from task-210's frontmatter — drift that predates this branch.
  Fixing it from here would repeat the contract violation; the gate is a
  pre-commit hook, not a CI job, and this branch adds no new
  task-labeled surface commit (all product commits are recorded in this
  task's frontmatter).

**Sandbox limitation:** TCP listener `accept()` unsupported (errno 95,
/tmp/stage/capabilities.json), so no live server/browser probe here. The
component behavior is covered by the jsdom component suites above; exact-head
GitHub CI remains mandatory for the built bundle.


**Fresh-round re-verification (2026-10-09, this sandbox):** inherited the
repaired implementation at 67139daf and re-proved it in a clean environment:
`npm ci` (169 locked packages) then EditorSplit + DetailPanel suites
**66/66 passed**; `npx vite build` reproduced the committed dist bundles
byte-exactly. One inherited defect found and fixed: dead i18n key
`editor_split.architecture_preview` (defined by this task's commits, unused
anywhere — the tab uses `editor_split.architecture`); removed and dist
rebuilt, suites re-run 66/66 (commit 471563d7, recorded above). Attribution
gate re-checked: branch HEAD fails only on `a781ede2 task-210`, and a clean
worktree at base 8c2d177505852b3e39cd77f4f782fb355de245aa fails identically
(exit 1, same sole violation) — pre-existing upstream drift, outside this
task's scope; task-210.md on this branch is byte-identical to base. This
branch's own task-labeled surface commits are all recorded. Evidence:
/tmp/stage/review-evidence/task-172-verification.md.

**Round-3 re-verification (2026-10-09, fresh sandbox):** inherited HEAD
cf9dc871 with the pipeline's fresh-assignment reset in the working tree;
re-verified the implementation independently with no source changes needed:
`npm ci` (169 locked packages) → EditorSplit + DetailPanel suites **66/66
passed**; `npx vite build` run twice, both reproducing the committed dist
bundles byte-exactly (empty `git status web/dist`); all 51 EditorSplit i18n
keys present in en.json with zero dead `editor_split.*` keys. Bug-injection
probe: replacing `acceptSuggestion`'s `applyDiffOps(content, diff)` with
`applyDiffOps(content, [])` fails the Accept test
(`expected '# Auth\n\nExisting body.\n' to contain '## Error Handling'`),
confirming the suite encodes the specced behavior; source restored
byte-identical to 471563d7 and re-passed. Contract verified: task-172.md
normative sections byte-identical to base 8c2d1775 (only progress/commits/
checkboxes/Shipped differ); task-210.md byte-identical to base. Attribution
gate at HEAD fails solely on `a781ede2 task-210` (identical failure in a
clean worktree at base — pre-existing upstream drift on main, out of scope
here); this branch's own task-labeled surface commits are all recorded in
frontmatter, and the working-tree reset that transiently dropped 471563d7
was restored from HEAD. Evidence:
/tmp/stage/review-evidence/task-172-verification-round3.md.

## Agent Instructions

Read `ui-layout.md` §2 "Editor Split" and §9 "Meta-specs Preview Loop Layout" for the full interaction design including all three states. Check the existing `MetaSpecs.svelte` component — it may already have partial editor functionality. The EditorSplit is used by BOTH spec editing and meta-spec editing; build it as a reusable component. The meta-spec preview loop layout (§9) shows the exact wireframes for States 1-3.
