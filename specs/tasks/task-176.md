---
title: "Meta-specs Preview Loop layout — editor split states and preview workflow"
spec_ref: "ui-layout.md §9"
depends_on: [task-172]
progress: ready-for-review
coverage_sections:
  - "ui-layout.md §9. Meta-specs Preview Loop Layout"
commits: ["f51cc2296bf96aeb0a151c7b19c55e73faf5e9b0", "91fe1e86da29a2c68cdb5cf32137da69b36e3132", "6e3bb1a7a474d6d333f9aa59a7924807770ead20", "eee86b3e66c5330eeee84e72a4a5fde76a19a87c", "e20c04c8eb8474ed90bb9d4aebb94a7da9c3ed3e", "95c85a3528df4e3ace92c2e7def6fc284f0e3440"]
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

### §9 State 1 LLM chat — workspace-scoped assist

- **Server** — `POST /api/v1/workspaces/:id/meta-specs/assist`
  (`specs_assist::assist_meta_spec`, f51cc2296): workspace-scoped LLM
  assist using the same SSE `{diff, explanation}` wire protocol as the
  repo-scoped `/repos/:id/specs/assist` (shared `assist_sse_events`
  helper). Effective content is the caller's `draft_content` (the
  in-editor draft) or, when only `meta_spec_id` is given, the stored
  registry prompt. Real behavior: per-user/workspace 10 req/60 s rate
  limit, 503 when the LLM is unconfigured, 404 unknown
  workspace/meta-spec, 403 cross-workspace meta-spec, 400 when neither
  draft nor id is given, cost-entry + per-call budget recording keyed to
  the workspace. ABAC resolver entry `meta_spec`/`generate`
  (`abac_middleware.rs:462`), no exemption growth.
- **Frontend** — EditorSplit enables the chat when `repoId` OR (meta-spec
  context + `workspaceId`); the §9 surface dispatches to
  `api.metaSpecsAssist(workspaceId, {instruction, draft_content,
  meta_spec_id})`.

### This round: base merge + rustfmt repair

- Resolved the in-progress merge of assignment base
  3af80bd21a8a3452420e014e645eefdfd137f9be (task-158 tenant-scope
  meta-specs) as e20c04c8e. Only conflict: `docs/api-reference.md` —
  combined this task's `/workspaces/{id}/meta-specs/assist` row with
  task-158's updated meta-specs-registry rows (Tenant scope, `force`
  propagation).
- Fixed the round-5 verification failure — rustfmt on changed lines of
  `crates/gyre-server/src/api/specs_assist.rs` — as 95c85a352 (restyling
  only). All four Rust files changed on this branch now pass
  `rustfmt --check`.
- §9 integrity on the merged tree: `locked-diff` ×18 and
  `showResultTabs` ×3 in `EditorSplit.svelte`, `metaSpecsAssist` in
  `api.js`, `lineDiff.js`/`lineDiff.test.js` present, assist route
  registered (`api/mod.rs:444`) with its ABAC resolver entry.
- Focused tests on the merged tree (npm ci, locked deps): EditorSplit +
  MetaSpecs (156) and lineDiff (13) — 169 passed, same count as round 5.
  `SKIP_WEB_BUILD=1 cargo check -p gyre-server` — clean (6m 20s).
  Evidence: `/tmp/stage/review-evidence/task-176-round6-rustfmt-cargo.log`.

### Server tests for the verifier

- `cargo test -p gyre-server specs_assist` (covers `assist_meta_spec_*`)
- `cargo test -p gyre-server meta_specs`
- `cd web && npm test`

### web/dist

Per the assignment contract, web/dist is restored from the assignment
base by the executor and rebuilt/published remotely from this branch's
  web/src, which carries the entire §9 preview loop.
