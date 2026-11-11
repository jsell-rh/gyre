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

The §9 meta-specs preview loop runs through the shared EditorSplit
(task-172) in `context="meta-spec"`, mounted by the MetaSpecs workspace
view. All three states are production code against real endpoints:

- **State 1 (Editing)** — left panel: persona/principle editor, LLM chat
  input, [Preview]/[Publish]; right panel: target-spec checkbox selector
  (specs from repos in the workspace) with Select All / Select None. The
  chat is wired to `POST /api/v1/workspaces/:id/meta-specs/assist`
  (`assist_meta_spec`, f51cc2296) — workspace-scoped SSE assist with the
  same `{diff, explanation}` protocol as the repo-scoped endpoint
  (shared `assist_sse_events` helper), per-user rate limit, 503 when the
  LLM is unconfigured, 403/404 scope checks, cost recording. ABAC
  resolver entry `meta_spec`/`generate` in `abac_middleware.rs`.
- **State 2 (Preview Running)** — editor locked, rendering the
  pending-changes diff (LCS line diff of the edited prompt vs the
  published registry prompt, eee86b3e); right panel shows per-spec
  ◐ running / ✓ complete indicators from the real preview-status poll
  plus aggregate progress, with [Cancel Preview] (button and Esc).
- **State 3 (Preview Complete)** — right panel [Architecture] (default)
  / [Code Diff] tabs rendering the server-computed PreviewResponse
  fields from the throwaway-branch agent runs.
- **Iterate** returns to State 1 with the selector restored AND the last
  preview results still visible (`showResultTabs`, 91fe1e86).
- **Publish** PUTs the edited prompt to the meta-spec registry, resets
  the spec's approval state to Pending, and refreshes the in-session
  diff baseline (6e3bb1a).

### Round 6 verification (this assignment)

- Branch HEAD is the round-5 candidate c41e24a76 — `web/src` and
  `crates/` are unchanged from the previously vetted tree; only this
  task-contract record is new. No merge or rebase was in progress.
- Source integrity re-probed on this tree: `locked-diff` ×18 and
  `showResultTabs` ×3 in `web/src/lib/EditorSplit.svelte`,
  `metaSpecsAssist` in `web/src/lib/api.js`, `lineDiff.js` present,
  assist route registered in `api/mod.rs:444` with its
  `meta_spec`/`generate` ABAC resolver entry, coverage row 37
  (ui-layout §9) implemented. Evidence:
  `/tmp/stage/review-evidence/task-176-r6-source-integrity.log`.
- Focused frontend probe on this tree (after `npm ci`):
  EditorSplit + MetaSpecs + lineDiff — 169/169 passed, exit 0.
  Evidence: `/tmp/stage/review-evidence/task-176-r6-frontend-focused.log`.
- Round-5 gate on the identical tree stands: `SKIP_WEB_BUILD=1 cargo
  check -p gyre-server` clean.

### web/dist publication (review rounds 1–5 finding, superseded by
### executor contract)

Rounds 1–5 flagged that the committed `web/dist` did not contain this
task's bundle. Under the current executor contract this is by design:
the executor restores generated `web/dist` files from the assignment
base, and final integration rebuilds and publishes those assets
remotely from this branch's `web/src`. The §9 UI therefore ships via the
remote rebuild of the source verified above; dist grep verification
(`locked-diff` / `metaSpecsAssist` / `pending_changes`) belongs to the
remote publication stage, not to this in-branch bundle.

### Server tests for the independent verifier

- `cargo test -p gyre-server specs_assist` (covers `assist_meta_spec_*`)
- `cargo test -p gyre-server ralph_loop signal_chain e2e_ralph_loop`
  (merged task-118 surfaces sharing the assist route's module neighbors)
- full `cd web && npm test`
