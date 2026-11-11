---
title: "Meta-specs Preview Loop layout — editor split states and preview workflow"
spec_ref: "ui-layout.md §9"
depends_on: [task-172]
progress: ready-for-review
coverage_sections:
  - "ui-layout.md §9. Meta-specs Preview Loop Layout"
commits: ["91fe1e86da29a2c68cdb5cf32137da69b36e3132", "6e3bb1a7a474d6d333f9aa59a7924807770ead20", "eee86b3e66c5330eeee84e72a4a5fde76a19a87c"]
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

The §9 meta-specs preview loop runs through the shared EditorSplit (task-172)
in `context="meta-spec"`, mounted by the MetaSpecs workspace view
(`MetaSpecs.svelte`):

- **State 1 (Editing):** left panel is the persona/principle editor plus the
  Preview/Publish actions; right panel is the target-spec checkbox selector
  (specs from repos in the workspace) with Select All / Select None.
- **State 2 (Preview Running):** the editor is locked and renders the
  pending-changes diff — an LCS line diff of the edited prompt against the
  published registry prompt (`+`/`-` markers, add/remove summary). The right
  panel shows per-spec ◐ running / ✓ complete indicators from the real
  preview-status poll plus aggregate progress, with a [Cancel Preview]
  action (button and Esc) that aborts the running preview.
- **State 3 (Preview Complete):** the right panel has [Architecture]
  (default) and [Code Diff] tabs rendering the server-computed
  PreviewResponse fields: structural impact (scope, affected spec count,
  blast radius with affected repos and pin reasons) and per-spec statuses
  from the throwaway-branch agent runs.
- **Iterate** returns to State 1 with the selector restored AND the last
  preview results still visible (`showResultTabs` in `EditorSplit.svelte`
  retains the Architecture/Code Diff tabs below the selector).
- **Publish** PUTs the edited prompt to the meta-spec registry, resets the
  spec's approval state to Pending, and refreshes the in-session diff
  baseline so the next preview diffs against the newly published prompt.

### Test evidence

At source HEAD `e8782818` (product commits `eee86b3e`, `6e3bb1a`,
`91fe1e86`), this round re-ran and recorded:

- `cd web && npm ci && npx vitest run --reporter=verbose
  src/__tests__/EditorSplit.test.js src/__tests__/MetaSpecs.test.js
  src/__tests__/lineDiff.test.js` — **166/166 passed, exit 0** (log:
  `/tmp/stage/review-evidence/task-176-focused-tests.log`), including the
  §9 tests for state transitions, locked-editor pending-changes diff,
  preview cancellation, architecture delta display, publish-baseline
  refresh, and Iterate retention.
- Mutation probe: reverting `showResultTabs` to the pre-fix logic (dropping
  `|| Boolean(metaPreviewResult)`) fails exactly the §9 test "meta-spec
  Iterate returns to editing with the selector AND the last preview results
  still visible" (1 failed | 46 passed; log:
  `/tmp/stage/review-evidence/task-176-mutation-showResultTabs.log`). The
  fix was restored and the suites re-run green.

### web/dist status (round-2 review finding)

The committed `web/dist` at HEAD is byte-identical to the assigned base
(`git diff --shortstat fbf1a63b HEAD -- web/dist` is empty), so it does not
contain the §9 UI. Per the current executor contract for this assignment,
generated `web/dist` files are restored from the assignment base at
checkpoint and are rebuilt and published remotely by final integration —
this round deliberately contains no `build(web)` commit. The §9 production
UI ships in source (`web/src/lib/EditorSplit.svelte`, `lineDiff.js`,
`MetaSpecs.svelte`, `locales/en.json`; marker greps in
`/tmp/stage/review-evidence/task-176-round3-verification.log`). Dist
regeneration command for final integration: `cd web && npm ci && npm run
build`; post-build check: `git grep -c 'locked-diff' -- web/dist` ≥ 1.

### Known limitation

The State 1 LLM chat input is rendered but disabled in the workspace-level
meta-specs surface: EditorSplit requires a repo context for chat and the
API layer has no workspace-scoped assist endpoint (the repo-scoped
`specs/assist` is what produces inline suggestion blocks). This is outside
this task's acceptance criteria and blocked by the existing repo-scoped
API surface; wiring a workspace-level assist is follow-up work.
