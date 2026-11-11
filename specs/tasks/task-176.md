---
title: "Meta-specs Preview Loop layout — editor split states and preview workflow"
spec_ref: "ui-layout.md §9"
depends_on: [task-172]
progress: ready-for-review
coverage_sections:
  - "ui-layout.md §9. Meta-specs Preview Loop Layout"
commits: ["eee86b3e66c5330eeee84e72a4a5fde76a19a87c", "6e3bb1a7a474d6d333f9aa59a7924807770ead20", "91fe1e86da29a2c68cdb5cf32137da69b36e3132", "f51cc2296bf96aeb0a151c7b19c55e73faf5e9b0"]
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
  published registry prompt, eee86b3e); the right panel shows per-spec
  ◐ running / ✓ complete indicators from the real preview-status poll plus
  aggregate progress, with a [Cancel Preview] action (button and Esc).
- **State 3 (Preview Complete):** the right panel has [Architecture]
  (default) and [Code Diff] tabs rendering the server-computed
  PreviewResponse fields (structural impact, blast radius, per-spec
  statuses from the throwaway-branch agent runs).
- **Iterate** returns to State 1 with the selector restored AND the last
  preview results still visible (91fe1e86, `showResultTabs`).
- **Publish** PUTs the edited prompt to the meta-spec registry, resets the
  spec's approval state to Pending, and refreshes the in-session diff
  baseline so a subsequent preview shows no stale pending diff (6e3bb1a).

### §9 State 1 LLM chat — workspace-scoped assist

The production §9 surface (workspace meta-specs, no repo context) has a
working State 1 chat (f51cc2296):

- **Server** — `POST /api/v1/workspaces/:id/meta-specs/assist`
  (`specs_assist::assist_meta_spec`): workspace-scoped LLM assist using
  the same SSE `{diff, explanation}` wire protocol as the repo-scoped
  `/repos/:id/specs/assist` (shared `assist_sse_events` helper). Effective
  content is the caller's `draft_content` (the in-editor draft) or, when
  only `meta_spec_id` is given, the stored registry prompt. Real behavior:
  per-user/workspace rate limit, 503 when the LLM is unconfigured, 404 for
  unknown workspace/meta-spec, 403 when a Workspace-scoped meta-spec
  belongs to another workspace, 400 when neither draft nor id is given,
  cost-entry + per-call budget recording keyed to the workspace. ABAC
  resolver entry `meta_spec`/`generate` in `abac_middleware.rs`.
- **Frontend** — EditorSplit enables the chat when `repoId` OR (meta-spec
  context + `workspaceId`); the §9 surface dispatches to
  `api.metaSpecsAssist(workspaceId, {instruction, draft_content,
  meta_spec_id})` and renders the inline diff suggestion with
  Accept/Edit/Dismiss.

### Verification

- Focused frontend suites on the current tree (`npm ci`, locked versions;
  `npx vitest run src/__tests__/EditorSplit.test.js
  src/__tests__/MetaSpecs.test.js`): **2 files, 156 tests passed, exit 0** —
  covering the §9 state machine (editing → running → complete → iterate
  back to editing), preview cancellation, locked-editor pending-changes
  diff, Architecture/Code Diff tabs, meta-spec Iterate results retention,
  and the workspace-scoped chat (incl. honest 404-failure path). Evidence:
  `/tmp/stage/review-evidence/task176-r6-vitest.log`.
- `web/src` at HEAD is byte-identical to the previously vetted round-4
  candidate 5b1d8540 (`git diff --stat 5b1d8540 HEAD -- web/src` empty);
  §9 markers grep-positive (`locked-diff` ×18 in EditorSplit.svelte,
  `metaSpecsAssist` in api.js, `pending_changes`, `showResultTabs`).
- Server tests for the independent verifier (gyre-server compile exceeds
  this sandbox's budget): `cargo test -p gyre-server api::specs_assist`
  (covers `assist_meta_spec_unknown_workspace_returns_404`,
  `assist_meta_spec_requires_draft_or_meta_spec_id`,
  `assist_meta_spec_unknown_meta_spec_returns_404`,
  `assist_meta_spec_rejects_meta_spec_from_another_workspace`,
  `assist_meta_spec_with_draft_streams_sse_complete`), plus full
  `cd web && npm test`.

### web/dist publication

Per the current controller contract for this assignment, the executor
restores the committed web/dist from the assignment base and the final
integration stage rebuilds and publishes those assets remotely from this
branch's web/src — the rounds 1–5 review prescription that the worker
rebuild and commit the bundle is superseded (each such rebuild was dropped
again by the next base merge). No sandbox dist rebuild was performed this
round, per the explicit assignment instruction; the §9 implementation
ships in the source commits listed above.
