---
title: "Meta-specs Preview Loop layout — editor split states and preview workflow"
spec_ref: "ui-layout.md §9"
depends_on: [task-172]
progress: ready-for-review
coverage_sections:
  - "ui-layout.md §9. Meta-specs Preview Loop Layout"
commits: ["f51cc2296", "91fe1e86", "6e3bb1a7", "eee86b3e"]
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

The §9 meta-specs preview loop runs through the shared EditorSplit
(task-172) in `context="meta-spec"`, mounted by the MetaSpecs workspace
view (`web/src/components/MetaSpecs.svelte`):

- **State 1 (Editing):** left panel is the persona/principle editor plus
  the LLM chat input and Preview/Publish actions; right panel is the
  target-spec checkbox selector (specs from repos in the workspace) with
  Select All / Select None.
- **State 2 (Preview Running):** the editor is locked and renders the
  pending-changes diff (LCS line diff of the edited prompt against the
  published registry prompt, `web/src/lib/lineDiff.js`); the right panel
  shows per-spec ◐ running / ✓ complete indicators from the real
  preview-status poll plus aggregate progress, with a [Cancel Preview]
  action (button and Esc).
- **State 3 (Preview Complete):** the right panel has [Architecture]
  (default) and [Code Diff] tabs rendering the server-computed
  PreviewResponse fields (structural impact, blast radius, per-spec
  statuses from the throwaway-branch agent runs).
- **Iterate** returns to State 1 with the selector restored AND the last
  preview results still visible (91fe1e86).
- **Publish** PUTs the edited prompt to the meta-spec registry, resets the
  spec's approval state to Pending, and refreshes the in-session diff
  baseline so only post-publish edits show as pending (6e3bb1a7).
- **State 1 LLM chat** is wired to a real workspace-scoped server endpoint,
  `POST /api/v1/workspaces/:id/meta-specs/assist`
  (`assist_meta_spec` in `crates/gyre-server/src/api/specs_assist.rs`),
  sharing the SSE `{diff, explanation}` protocol with the repo-scoped
  assist via a common `assist_sse_events` helper; per-user/workspace rate
  limit, 503 when the LLM is unconfigured, 404/403 scoping (f51cc2296).
  Registered in the ABAC route resolver
  (`crates/gyre-server/src/abac_middleware.rs`).

### Test evidence

Focused vitest run at HEAD (c41e24a76, npm ci with locked deps first):

- `web/src/__tests__/EditorSplit.test.js` — 50 passed. Covers the state
  machine transitions (editing → preview_running → preview_complete →
  editing via Iterate), preview cancellation returning to editing,
  target-spec selector gating Preview, real PreviewResponse field
  rendering, and the workspace-scoped meta-spec assist chat.
- `web/src/__tests__/MetaSpecs.test.js` — 106 passed. Covers the §9 host:
  preview-loop mount, per-spec progress in running state, completion via
  real status poll (no fabricated state), failure returning to editing
  with an error banner, Iterate, Cancel Preview, Publish calling the
  registry API, Architecture/Code Diff tab switching, locked-editor
  pending-changes diff vs the published prompt, and post-publish baseline
  refresh.

Evidence log: `/tmp/stage/review-evidence/task176-r6-focused-tests.log`.

### Round-5 finding — web/dist artifact

The round-5 review correctly observed that committed `web/dist` at the
branch head equals the assignment base's bundle (no `locked-diff` /
`metaSpecsAssist` markers). Per this assignment's controller contract,
the executor restores `web/dist` from the assignment base and final
integration rebuilds and publishes those assets remotely; rebuilding or
committing `web/dist` is explicitly out of scope for this stage, so no
dist commit exists in this round either. The source implementation
(`web/src`, byte-identical to the round-4 vetted candidate) carries the
entire §9 surface and is covered by the passing tests above; dist
regeneration is owned by the publication stage.
