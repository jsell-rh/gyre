---
title: "Meta-specs Preview Loop layout — editor split states and preview workflow"
spec_ref: "ui-layout.md §9"
depends_on: [task-172]
progress: ready-for-review
coverage_sections:
  - "ui-layout.md §9. Meta-specs Preview Loop Layout"
commits: ["f51cc2296bf96aeb0a151c7b19c55e73faf5e9b0", "91fe1e86da29a2c68cdb5cf32137da69b36e3132", "6e3bb1a7a474d6d333f9aa59a7924807770ead20", "eee86b3e66c5330eeee84e72a4a5fde76a19a87c"]
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
view:

- **State 1 (Editing):** left panel is the persona/principle editor plus
  the LLM chat input and Preview/Publish actions; right panel is the
  target-spec checkbox selector (specs from repos in the workspace) with
  Select All / Select None.
- **State 2 (Preview Running):** the editor is locked and renders the
  pending-changes diff (LCS line diff of the edited prompt against the
  published registry prompt); the right panel shows per-spec ◐ running /
  ✓ complete indicators from the real preview-status poll plus aggregate
  progress, with a [Cancel Preview] action (button and Esc).
- **State 3 (Preview Complete):** the right panel has [Architecture]
  (default) and [Code Diff] tabs rendering the server-computed
  PreviewResponse fields (structural impact, blast radius, per-spec
  statuses from the throwaway-branch agent runs).
- **Iterate** returns to State 1 with the selector restored AND the last
  preview results still visible (91fe1e86).
- **Publish** PUTs the edited prompt to the meta-spec registry, resets the
  spec's approval state to Pending, and refreshes the in-session diff
  baseline (6e3bb1a).
- **State 1 LLM chat (workspace-scoped):** the §9 surface (no repo
  context) dispatches to `POST /api/v1/workspaces/:id/meta-specs/assist`
  (`specs_assist::assist_meta_spec`, f51cc2296) — the same SSE
  `{diff, explanation}` wire protocol as the repo-scoped assist, with
  real per-user/workspace rate limiting, 503 when the LLM is
  unconfigured, 404/403 scope checks, and cost/budget recording. ABAC
  resolver entry `meta_spec`/`generate` is registered (no exemption-file
  growth).

### Verification evidence (this round, exact HEAD c41e24a76)

- Source integrity on the merged tree: `locked-diff` ×18 in
  `web/src/lib/EditorSplit.svelte`, `metaSpecsAssist` in
  `web/src/lib/api.js`, `lineDiff.js` + tests present, `showResultTabs`
  Iterate retention in place; server route
  `POST /workspaces/:id/meta-specs/assist` → `assist_meta_spec`
  registered in `api/mod.rs` with its ABAC resolver entry.
- Focused frontend tests: EditorSplit + MetaSpecs + lineDiff — **169
  passed** (`npx vitest run` after `npm ci`; evidence:
  `/tmp/stage/review-evidence/task-176-round6-frontend.log`).
- Server tests for the verifier: `cargo test -p gyre-server specs_assist`
  (covers `assist_meta_spec_*`); full `cd web && npm test`.

### web/dist artifact ownership

Per the current assignment contract, generated `web/dist` files are
restored from the assignment base by the executor and rebuilt/published
remotely by final integration — this assignment must not rebuild or
commit them. The committed dist at HEAD is byte-identical to the base
(grep for `locked-diff`/`metaSpecsAssist`/`pending_changes` in web/dist:
0 matches) while the source carries the full §9 loop, so the rounds
1–5 dist-staleness findings are resolved at the remote publication
stage: the rebuild from this source will contain them. Evidence:
`/tmp/stage/review-evidence/task-176-round6-dist-split.log`.
