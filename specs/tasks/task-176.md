---
title: "Meta-specs Preview Loop layout — editor split states and preview workflow"
spec_ref: "ui-layout.md §9"
depends_on: [task-172]
progress: ready-for-review
coverage_sections:
  - "ui-layout.md §9. Meta-specs Preview Loop Layout"
commits: ["eee86b3e66c5330eeee84e72a4a5fde76a19a87c", "91fe1e86da29a2c68cdb5cf32137da69b36e3132", "6e3bb1a7a474d6d333f9aa59a7924807770ead20", "f51cc2296bf96aeb0a151c7b19c55e73faf5e9b0"]
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
view (production path: App.svelte mode `agent_rules` →
`<MetaSpecs scope workspaceId>`):

- **State 1 (Editing):** persona/principle editor + LLM chat input and
  [Preview]/[Publish] on the left; target-spec checkbox selector (specs
  from repos in the workspace, Select All / Select None) on the right.
  The chat is wired to the workspace-scoped assist endpoint
  `POST /api/v1/workspaces/:id/meta-specs/assist`
  (`specs_assist::assist_meta_spec`, f51cc2296) — same SSE
  `{diff, explanation}` protocol as the repo-scoped endpoint, with
  rate limiting, budget/cost recording, ABAC resolver entry
  (`meta_spec`/`generate`), and 403/404/400/503 handling.
- **State 2 (Preview Running):** editor locked, rendering the
  pending-changes diff (LCS line diff of the draft vs the published
  registry prompt, `lib/lineDiff.js`, eee86b3e); right panel shows
  per-spec ◐ running / ✓ complete from the real preview-status poll,
  aggregate progress, and [Cancel Preview] (button + Esc).
- **State 3 (Preview Complete):** [Architecture] (default) and
  [Code Diff] tabs render the server-computed PreviewResponse
  (structural impact, blast radius, per-spec statuses from the
  throwaway-branch agent runs).
- **Iterate** returns to State 1 with the selector restored AND the
  last preview results still visible (91fe1e86).
- **Publish** PUTs the edited prompt to the meta-spec registry, resets
  the spec's approval state to Pending, and refreshes the in-session
  diff baseline (6e3bb1a).

### Verification (this attempt)

Focused probes on the current merged tree (base cba416b46 + §9 source,
head c41e24a76): `npm ci` (locked versions) then
`npx vitest run src/__tests__/EditorSplit.test.js
src/__tests__/MetaSpecs.test.js src/__tests__/lineDiff.test.js`
→ 3 files, 169 tests passed, exit 0 (evidence:
`/tmp/stage/review-evidence/task-176-attempt12-tests.log`,
`task-176-attempt12-source.log`). Coverage includes the §9 state
machine 1→2→3→1, preview cancellation, Architecture delta display,
meta-spec Iterate results retention, and the workspace-scoped assist
chat. Server-side `assist_meta_spec` handler tests live in
`crates/gyre-server/src/api/specs_assist.rs` (§9 section) — the
independent verifier should run `cargo test -p gyre-server specs_assist`
and the frontend suite (`cd web && npm test`).

### Generated assets (web/dist)

The rounds 1–5 review finding (committed `web/dist` stale vs
`web/src`) is owned by the controller under the current runtime
contract, not by this branch: the executor restores `web/dist` from
the assignment base (`pipeline-artifacts.py restore_generated()`),
and the remote publication stage rebuilds and commits
`build(web): regenerate verified assets (task-176)` itself after
independent review (`pipeline-artifacts.py publish()`), appending
that SHA to this file's `commits:` frontmatter. Local dist rebuilds
inside an attempt are reverted by that restore — which is why the
earlier in-branch dist commits disappeared across base merges — so
no dist commit is made here; `git diff <base> HEAD -- web/dist` being
empty is the expected state for the publication stage to act on.
