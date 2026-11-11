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
in `context="meta-spec"`, mounted by the MetaSpecs workspace view:

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
  preview results still visible (Architecture/Code Diff tabs retained
  below the height-capped selector).
- **Publish** PUTs the edited prompt to the meta-spec registry, resets the
  spec's approval state to Pending, and refreshes the in-session diff
  baseline so the next preview diffs against the newly published prompt.

### Test evidence

- `npx vitest run src/__tests__/EditorSplit.test.js
  src/__tests__/MetaSpecs.test.js` — 153 passed, exit 0 (this round, at HEAD
  e8782818 after `npm ci` from the lockfile; logs in
  /tmp/stage/review-evidence/task-176-focused-tests.log and
  task-176-round-notes.md).
- Mutation probe re-run this round: reverting `showResultTabs` to pre-fix
  behavior fails the §9 Iterate-retention test at its meta-impact
  assertion (task-176-mutation-probe.log); source restored and the full
  focused suites re-run green. The test kills the prior round's real
  defect (Iterate dropping results from the DOM), it does not mirror the
  implementation.
- Preview cancellation, state transitions, architecture delta display,
  and 404-rejection handling are covered in the same suites.

### Known limitation

The State 1 LLM chat input is rendered but disabled in the workspace-level
meta-specs surface: EditorSplit requires a repo context for chat and the
API layer has no workspace-scoped assist endpoint (the repo-scoped
`specs/assist` is what produces inline suggestion blocks). This is outside
this task's acceptance criteria and blocked by the existing repo-scoped
API surface; wiring a workspace-level assist is follow-up work.

### Round note

Contract-restoration round (repair 0cef13045ec944f1822c51937337f51f-1,
prior repair 938fa20b45b34af2af2243843334c681): the executor restored the
assigned task document and web/dist from the assignment base, so this
round changes no production code — it re-verifies the existing §9
implementation on this branch (focused suites + mutation probe above) and
re-records these notes. Regarding the prior review's dist-staleness
finding: under this assignment's runtime contract the executor restores
generated web/dist files from the base and final integration rebuilds and
publishes those assets remotely, so dist regeneration is deliberately not
performed here; every §9 feature (locked-diff, `pending_changes` locale,
`showResultTabs` Iterate retention, publish baseline refresh) is present
in web/src at HEAD and will be carried into the published bundle by the
final integration build. The acceptance criteria above are left in their
assigned contract form; satisfaction is judged by the independent
reviewer and deterministic gates, not by editing the protected text.
