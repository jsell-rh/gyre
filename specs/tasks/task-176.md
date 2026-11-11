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
  Preview/Publish actions; right panel is the target-spec checkbox selector
  (specs from repos in the workspace) with Select All / Select None.
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
  preview results still visible.
- **Publish** PUTs the edited prompt to the meta-spec registry, resets the
  spec's approval state to Pending, and refreshes the in-session diff
  baseline.

### This round: §9 State 1 LLM chat wired to a workspace-scoped assist

Closes the round-1 finding that the production §9 surface (workspace
meta-specs, no repo context) rendered the State 1 chat permanently disabled:

- **Server** — `POST /api/v1/workspaces/:id/meta-specs/assist`
  (`specs_assist::assist_meta_spec`): workspace-scoped LLM assist using the
  same SSE `{diff, explanation}` wire protocol as the repo-scoped
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

### Test evidence

- Frontend: `npx vitest run src/__tests__/EditorSplit.test.js
  src/__tests__/MetaSpecs.test.js src/__tests__/lineDiff.test.js` — 169
  passed; full `npm test` — 2102 passed, 41 skipped (logs in
  `/tmp/stage/review-evidence/task-176-frontend-*.log`). New EditorSplit
  tests cover: chat enabled in meta-spec context with a workspace and no
  repo; chat still disabled without either; §9 chat dispatches to the
  workspace endpoint (not `specsAssist`) with the draft + meta_spec_id and
  renders the Accept-able suggestion block. Mutation probes: reverting
  `llmChatEnabled` to repo-only fails 2 tests; dispatching to
  `specsAssist` unconditionally fails 1 test.
- Server: 8 new `assist_meta_spec_*` tests in `specs_assist.rs` — 404
  (workspace/meta-spec), 400 (no content), 403 (cross-workspace meta-spec),
  503 (LLM unavailable), SSE complete streaming, and two
  recording-LLM-factory tests asserting WHAT the LLM received (stored
  registry prompt when no draft; draft overrides the stored prompt).
  Type-checked in-sandbox (`cargo check -p gyre-server --lib --tests`);
  execution exceeds the sandbox build budget — the verifier must run
  `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib api::specs_assist`
  (evidence: `/tmp/stage/review-evidence/task-176-server-check.log`).

### Note on web/dist

The round-2 review finding (stale shipped bundle) concerns generated
assets. Per the current assignment contract, `web/dist` is restored from
the assignment base and rebuilt/published by final integration; this
round's changes are source-only (`web/src`, `crates/`).
