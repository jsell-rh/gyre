---
title: "Meta-specs Preview Loop layout — editor split states and preview workflow"
spec_ref: "ui-layout.md §9"
depends_on: [task-172]
progress: ready-for-review
coverage_sections:
  - "ui-layout.md §9. Meta-specs Preview Loop Layout"
commits: ["eee86b3e66c5330eeee84e72a4a5fde76a19a87c", "6e3bb1a7a474d6d333f9aa59a7924807770ead20"]
---

## Spec Excerpt

ui-layout.md §9 defines the meta-specs preview loop layout using the Editor Split (§2):

**State 1 — Editing**: Left panel shows persona/principle editor + LLM chat input. Right panel shows target spec selector (checkbox list of specs from repos in workspace). [Preview] and [Publish] buttons.

**State 2 — Preview Running**: Left panel locked during preview (shows diff of changes). Right panel shows per-spec progress indicators (◐ running, ✓ complete). [Cancel Preview] button.

**State 3 — Preview Complete**: Left panel shows editor with changes + LLM suggestion blocks. Right panel has [Architecture] / [Code Diff] tabs. Architecture tab shows structural impact (+ added, ~ modified, = unchanged). [Iterate] returns to State 1 with results visible. [Publish] triggers approval flow.

The preview loop is the primary meta-spec interaction: edit → preview → iterate → publish. Each iteration spawns agents on throwaway branches.

## Implementation Plan

1. **Wire EditorSplit into MetaSpecs.svelte**:
   - Replace current meta-spec editor with EditorSplit component (from task-172)
   - Add target spec selector in right panel (State 1)
   - Connect to existing meta-spec API endpoints

2. **Preview workflow**:
   - [Preview] button triggers agents on throwaway branches (use existing preview API)
   - Show per-spec progress in right panel (State 2)
   - Lock editor during preview, show diff of pending changes
   - [Cancel Preview] aborts running agents

3. **Preview results display** (State 3):
   - Architecture tab: structural impact delta (added/modified/removed/unchanged nodes)
   - Code Diff tab: line-level diff from throwaway branches
   - [Iterate] returns to State 1 with results still visible
   - [Publish] commits change and triggers approval workflow

4. **LLM suggestion integration**:
   - LLM chat input produces inline diff blocks (via `specs/assist` endpoint)
   - Accept/Edit/Dismiss buttons per suggestion

5. **Tests**:
   - State machine transitions (1→2→3→1)
   - Preview cancellation
   - Architecture delta display

## Acceptance Criteria

- [ ] Meta-spec editing uses EditorSplit layout
- [ ] Target spec selector with checkbox list in right panel
- [ ] Preview triggers agent runs, shows per-spec progress
- [ ] Editor locked during preview with diff display
- [ ] Preview results show Architecture and Code Diff tabs
- [ ] [Iterate] returns to editing with results visible
- [ ] [Publish] triggers approval workflow
- [ ] Tests pass for state transitions

## Agent Instructions

Read `ui-layout.md` §9 for the exact wireframes of all three states. Also read `meta-spec-reconciliation.md` §5 for the preview loop protocol. The existing `MetaSpecs.svelte` has the registry view and basic editing — this task adds the full EditorSplit preview workflow. Check the preview API endpoints: the spec mentions throwaway branches and agent spawning. Use the EditorSplit component created in task-172.

## Shipped

The §9 preview loop runs through the shared EditorSplit (task-172) in
`context="meta-spec"`, mounted by the MetaSpecs workspace view: State 1
target-spec checkbox selector + Preview/Publish, State 2 locked editor
showing the pending-changes diff vs the published registry prompt
(LCS line diff, `+`/`-` markers, add/remove summary) + per-spec ◐/✓
progress + Cancel Preview, State 3 Architecture/Code Diff tabs rendering
the real PreviewResponse fields, Iterate returning to editing with
results retained, and Publish → registry PUT resetting approval to
Pending.

### What shipped

- **eee86b3e** — `web/src/lib/lineDiff.js` (LCS line diff +
  `diffSummary`), `EditorSplit.svelte`'s `baselineContent` prop (locked
  left pane renders the draft-vs-published-baseline diff during a
  preview, falling back to the verbatim draft when there is no baseline
  or no change), and `MetaSpecs.svelte`'s `selectedMsBaseline` tracking
  (set on load and on persona switch).
- **6e3bb1a7 (review repair 8f06c912)** — `publish()` in
  `MetaSpecs.svelte` now refreshes `selectedMsBaseline` (and the cached
  `wsMetaSpecs` entry's prompt) after a successful publish, so the §9
  publish → iterate → preview loop's locked pane never diffs against a
  stale pre-publish baseline: a preview with no post-publish edits
  shows the verbatim draft (no pending markers), and post-publish edits
  mark only the newly added lines as pending. Also restored
  `specs/coverage/system/ui-layout.md` row 37's Depth to 2 (H2 heading,
  matching bootstrap-coverage.sh derivation) and the file header to
  **Coverage: 2/38**.

### Test evidence

`npm ci`, then `./node_modules/.bin/vitest run src/__tests__/MetaSpecs.test.js
src/__tests__/EditorSplit.test.js src/__tests__/lineDiff.test.js` →
**165/165 passed** (163 prior + 2 new regression tests). Both new
tests were verified to fail on the unfixed tree — reproducing the
review's probe exactly: a publish-then-preview with no edits showed a
phantom `'+1 -0'` pending line, and a single post-publish edit line
showed `'+2 -0'` — and pass with the fix. `web/dist` rebuilt from this
source. Evidence retained at
`/tmp/stage/review-evidence/task-176-test-evidence.md` (repair-round
evidence appended).

### Scope boundary

The §9 backend gaps the wireframe alludes to — agents actually spawned
on `preview/{preview_id}/*` throwaway branches, line-level code diffs,
DELETE cleanup — belong to the preview endpoint itself and are tracked
by task-206 against meta-spec-reconciliation.md coverage row 9
("Hollow (code-verified 2026-09-29)"). The UI consumes exactly the real
PreviewResponse/PreviewStatusResponse shapes the server sends today
and never fabricates a completed State 3.
