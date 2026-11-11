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

The §9 meta-specs preview loop runs through the shared EditorSplit (task-172)
in `context="meta-spec"`, mounted by the MetaSpecs workspace view:

- **State 1 (Editing):** left panel is the persona/principle editor plus the
  LLM chat input and Preview/Publish actions; right panel is the
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

### §9 State 1 LLM chat — workspace-scoped assist

Closes the round-1 finding that the production §9 surface (workspace
meta-specs, no repo context) rendered the State 1 chat permanently disabled:

- **Server** — `POST /api/v1/workspaces/:id/meta-specs/assist`
  (`specs_assist::assist_meta_spec`, f51cc2296): workspace-scoped LLM assist
  using the same SSE `{diff, explanation}` wire protocol as the repo-scoped
  `/repos/:id/specs/assist` (protocol logic extracted into a shared
  `assist_sse_events` helper, not duplicated). Effective content is the
  caller's `draft_content` (the in-editor draft) or, when only
  `meta_spec_id` is given, the stored registry prompt. Real behavior:
  per-user/workspace 10 req/60 s rate limit, 503 when the LLM is
  unconfigured, 404 for unknown workspace/meta-spec, 403 when a
  Workspace-scoped meta-spec belongs to another workspace, 400 when neither
  draft nor id is given, cost-entry + per-call budget recording keyed to
  the workspace. ABAC resolver entry `meta_spec`/`generate` added to
  `abac_middleware.rs` (no exemption-file growth).
- **Frontend** — EditorSplit enables the chat when `repoId` OR (meta-spec
  context + `workspaceId`); the §9 surface dispatches to
  `api.metaSpecsAssist(workspaceId, {instruction, draft_content,
  meta_spec_id})`. The dead `specsAssistGlobal` 404 stand-in was removed
  from `api.js`.

### This round: merge integration with base 22ca8fd8 (task-175)

- Merged assignment base 22ca8fd805dc3a7ef98b938672d270c14217bffa
  (task-175 Specs/Inbox layouts) as commit 4361d5501. The integration
  points — `web/src/lib/api.js`, `web/src/locales/en.json` (both modified
  by task-175 on the base side and by this task), and
  `crates/gyre-server/src/api/specs_assist.rs` (task-175 added ~405 lines
  to the module carrying `assist_meta_spec`) — were verified: §9 strings,
  `metaSpecsAssist`, and `assist_meta_spec` all present at HEAD.
- Focused frontend tests on the merged tree: EditorSplit + MetaSpecs +
  lineDiff — 169 passed; Inbox (task-175 surface sharing the merged
  api.js/en.json) — 74 passed. `SKIP_WEB_BUILD=1 cargo check -p
  gyre-server` — clean. Evidence:
  `/tmp/stage/review-evidence/round4-verification.log`.
- Server tests for the verifier: `cargo test -p gyre-server specs_assist`
  (covers `assist_meta_spec_*` and the task-175 assist tests in the same
  module); full `cd web && npm test`.

### web/dist publication

Per the current controller contract for this assignment, the executor
restores the committed web/dist from the assignment base and the final
integration stage rebuilds and publishes those assets remotely from this
branch's web/src (which carries the entire §9 preview loop:
`locked-diff`, `pending_changes`, `metaSpecsAssist` all grep-positive in
web/src at HEAD). No sandbox dist rebuild was performed, per the explicit
assignment instruction.
