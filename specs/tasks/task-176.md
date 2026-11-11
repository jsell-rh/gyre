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

- [x] Meta-spec editing uses EditorSplit layout
- [x] Target spec selector with checkbox list in right panel
- [x] Preview triggers agent runs, shows per-spec progress
- [x] Editor locked during preview with diff display
- [x] Preview results show Architecture and Code Diff tabs
- [x] [Iterate] returns to editing with results visible
- [x] [Publish] triggers approval workflow
- [x] Tests pass for state transitions

## Agent Instructions

Read `ui-layout.md` §9 for the exact wireframes of all three states. Also read `meta-spec-reconciliation.md` §5 for the preview loop protocol. The existing `MetaSpecs.svelte` has the registry view and basic editing — this task adds the full EditorSplit preview workflow. Check the preview API endpoints: the spec mentions throwaway branches and agent spawning. Use the EditorSplit component created in task-172.

## Shipped

The §9 preview loop runs through the shared EditorSplit (task-172) in
`context="meta-spec"`, mounted by the MetaSpecs workspace view: State 1
target-spec checkbox selector + Preview/Publish; State 2 locked editor
showing the pending-changes diff vs the published registry prompt (LCS
line diff, `+`/`-` markers, add/remove summary) + per-spec ◐/✓ progress
+ Cancel Preview (button and Esc); State 3 Architecture (default)/Code
Diff tabs rendering the real PreviewResponse fields; Iterate returns to
editing with results retained; Publish → registry PUT resets approval
to Pending and refreshes the diff baseline so a post-publish preview
never diffs against a stale pre-publish prompt.

### Review repairs (prior rounds, intact at HEAD)

- **Iterate keeps results visible:** the meta-spec editing state
  previously rendered ONLY the target-spec selector, so Iterate
  (State 3 → State 1) removed the Architecture/Code Diff tabs from the
  DOM even though `metaPreviewResult` was retained in state. The
  right-pane chain now gives `preview_running` priority, renders the
  selector in the meta-spec editing state (height-capped via a
  `.stacked` modifier when results are retained), and renders the
  Architecture/Code Diff tabs whenever a preview result exists
  (`showResultTabs`). The Code Diff meta-spec status list is bound to
  `metaPreviewResult` instead of `previewState` so it survives Iterate.
  Spec-context behavior is unchanged. A meta-spec-context test runs a
  real-shape `previewPersona` poll to completion, clicks Iterate, and
  asserts the selector AND the retained results stay visible; it was
  verified to fail on the pre-fix component at `tab-architecture`.
- **Post-publish diff baseline:** Publish refreshes the in-session diff
  baseline from the registry write so the next preview diffs against
  the newly published prompt, not the stale pre-publish one.

### This round (contract restoration)

The executor restored the assigned task document to its contract form
(repair `58f1efcfece34880b5c8787dfac0e1a9`); this round changes no
production code — it re-verifies and re-records the existing fix.
`git diff 91fe1e86d HEAD -- web/src` is empty (verified this round),
confirming the Iterate results-visibility repair is intact at HEAD
`671873eea` after the base merge; the only web/dist difference vs the
assignment base is the executor's restoration of generated assets.
Focused vitest on `EditorSplit.test.js` + `MetaSpecs.test.js` (after
`npm ci` from the lockfile): 2 files, **153/153 passed**, exit 0.
Evidence: `/tmp/stage/review-evidence/task-176-focused-tests.log`.
`web/dist` regeneration is executor-owned (restored from the assignment
base; final integration rebuilds and publishes remotely), so no dist
commit is made here.

### Known limitation

The production §9 surface (workspace meta-specs page) mounts
EditorSplit without `repoId`, and `specs/assist` is repo-scoped, so the
State 1 LLM chat input renders but is disabled there.
Implementation-plan item 4 is reachable only in the repo-scoped spec
context (covered by the §3 Accept/Edit/Dismiss tests). Unblocking it
needs a workspace-scoped assist endpoint — follow-up work against the
API surface, not this UI task.

### Scope boundary

The §9 backend gaps the wireframe alludes to — agents actually spawned
on `preview/{preview_id}/*` throwaway branches, line-level code diffs,
DELETE cleanup — belong to the preview endpoint itself and are tracked
by task-206 against meta-spec-reconciliation.md coverage row 9
("Hollow (code-verified 2026-09-29)"). The UI consumes exactly the real
PreviewResponse/PreviewStatusResponse shapes the server sends today and
never fabricates a completed State 3.
