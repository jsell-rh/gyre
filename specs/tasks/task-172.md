---
title: "Editor Split layout component for spec and meta-spec editing"
spec_ref: "ui-layout.md §2 Editor Split"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "ui-layout.md §Editor Split"
commits: ["471563d7d218003d6d2dbbab3ba0598e801d6f3a", "1ae8ca350439f07c5c75dcfbb7704dbf0ce96021", "a53ade326e99a7954fe976a84981e3df0212465b", "80925c2145b51e64ddb69ef1b4fb900c44ee0a89", "3688b5cddbfddbed2f4cdc158f03ce35bb172951"]
---

## Spec Excerpt

The Editor Split layout (ui-layout.md §2) is used by meta-specs preview loop and spec editing with preview. It has two panels:

**Left panel** — spec or meta-spec editor with two modes:
- Manual edit: standard markdown editor with toolbar
- LLM chat: inline input below editor. User types instruction → LLM produces draft revision shown as inline diff block with Accept/Edit/Dismiss buttons

**Right panel** — preview of architectural impact:
- Architecture tab (default): structural diff from knowledge graph — added/modified/removed nodes
- Code Diff tab: traditional line-level diff of agent implementation on throwaway branch

The detail panel "pops out to full width" (Pop Out mechanism from §2 Split layout) and switches to Editor Split when the user clicks "Preview" in the spec detail panel.

Three states: Editing → Preview Running → Preview Complete (with Iterate option).

## Implementation Plan

1. **EditorSplit.svelte component** (`web/src/components/`):
   - Two-panel layout: left (editor, 50%) + right (preview, 50%)
   - Left panel: markdown textarea with toolbar + LLM chat input
   - Right panel: tabs for Architecture (default) and Code Diff
   - Inline diff block component with Accept/Edit/Dismiss buttons
   - State machine: editing → preview_running → preview_complete

2. **Pop Out integration**:
   - The existing DetailPanel's "Pop Out" mechanism should transition to EditorSplit
   - Back/Esc returns to normal detail panel view

3. **LLM draft revision display**:
   - Inline diff block component showing suggested changes
   - Accept: applies changes to editor content (in-memory, not committed)
   - Edit: copies suggested text into editor
   - Dismiss: removes suggestion

4. **Preview states**:
   - Editing: editor active, preview panel shows target spec selector
   - Preview Running: editor locked, progress indicators per spec
   - Preview Complete: Architecture delta + Code Diff tabs

5. **Tests**:
   - Component tests for state transitions
   - Accept/Edit/Dismiss button behaviors
   - Pop-out and return navigation

## Acceptance Criteria

- [x] EditorSplit component renders two-panel layout
- [x] Left panel supports manual edit and LLM chat modes
- [x] Right panel has Architecture and Code Diff tabs
- [x] Inline diff blocks with Accept/Edit/Dismiss work correctly
- [x] State machine handles editing → running → complete → iterate cycle
- [x] Pop Out from DetailPanel transitions to EditorSplit
- [x] Back/Esc returns to normal view
- [x] Tests pass

## Shipped

EditorSplit (`web/src/lib/EditorSplit.svelte`, 1494 lines) implements the §2
Editor Split layout as a reusable component with two contexts:

- **Spec context** — mounted by DetailPanel (`context="spec"`, pop-out via the
  Preview button at DetailPanel.svelte:3647 → `openEditorSplit` :68; Back/Esc →
  `closeEditorSplit` collapses the panel). Left pane: markdown textarea with
  `bind:content`, Save with `base_sha` optimistic concurrency + 409 conflict
  dialog (overwrite/discard), ConcurrentEditBanner presence, and the inline LLM
  chat (Ctrl/Cmd+Enter → `POST /repos/:id/specs/assist` SSE stream with
  partial/complete event handling). The draft revision renders as an inline
  diff block: **Accept** applies the diff ops in-memory without saving,
  **Edit** copies the suggested text into the editor, **Dismiss** drops it
  (§3 steps 4–6). Right pane: Architecture (default — real nodes/edges from
  `repoGraph` + `graphPredict` ghost overlays on ArchPreviewCanvas) and Code
  Diff (`SpecDiffView` line-level diff from the thorough-preview result) tabs.
  State machine editing → preview_running → preview_complete with
  `thoroughPreview` + `taskStatus` polling (10 s interval, 300 s timeout,
  cancel available), Iterate returns to editing with results retained.
- **Meta-spec context** (`context="meta-spec"`, §9 preview loop) — target spec
  selector checklist (Preview disabled until a target is selected), Preview
  via `previewPersona` + `previewPersonaStatus` polling (1.5 s) with per-spec
  progress indicators, architecture_diff line parsing (`+`/`~` prefixes) into
  ghost overlays, specs_diff into the Code Diff tab, Iterate/Publish hooks.

This repair assignment restored the original task contract after the prior
attempt drifted into task-210's attribution ledger (its 69a63669 edited
task-210's frontmatter on the task-172 branch). That edit is reverted — the
branch's specs/tasks/task-210.md is byte-identical to base. The revert
restores a pre-existing upstream condition: origin/main head (8c2d1775)
itself fails the attribution pre-commit hook because a781ede2
(`feat(task-210)`, 28 product-surface files) is missing from task-210's
frontmatter on main (verified in a clean worktree at base, gate exit 1).
That drift belongs to task-210's scope on main, not this branch; it is
recorded here for the reviewer to route to the right task. Within this
branch's own contract: task-172.md keeps the original contract sections
byte-identical to base, coverage matrix row #13 (ui-layout.md §Editor Split)
marked `implemented` with evidence, no exemptions or verifier changes.

**Test evidence** (2026-10-09, this session; artifacts under
/tmp/stage/review-evidence/):
- `npx vitest run src/__tests__/EditorSplit.test.js src/__tests__/DetailPanel.test.js`
  → **66/66 passed** (EditorSplit 37: rendering, contexts, Back/Esc, content
  binding, LLM streaming + Accept/Edit/Dismiss, save + base_sha + 409 conflict
  + overwrite, concurrent-edit banner, spec preview immediate + taskStatus
  polling + cancel, iterate retention, meta-spec selector + previewPersona
  with/without persona_id; DetailPanel 29 including the 5 pop-out tests).
- `scripts/check-task-commit-attribution.sh` → **FAIL on this branch, same
  pre-existing upstream failure as base** (verified in a clean worktree at
  base 8c2d1775: exit 1). Cause: a781ede2 (`feat(task-210)`) is on main but
  absent from task-210's frontmatter — drift that predates this branch.
  Fixing it from here would repeat the contract violation; the gate is a
  pre-commit hook, not a CI job, and this branch adds no new
  task-labeled surface commit (all product commits are recorded in this
  task's frontmatter).

**Sandbox limitation:** TCP listener `accept()` unsupported (errno 95,
/tmp/stage/capabilities.json), so no live server/browser probe here. The
component behavior is covered by the jsdom component suites above; exact-head
GitHub CI remains mandatory for the built bundle.


**Fresh-round re-verification (2026-10-09, this sandbox):** inherited the
repaired implementation at 67139daf and re-proved it in a clean environment:
`npm ci` (169 locked packages) then EditorSplit + DetailPanel suites
**66/66 passed**; `npx vite build` reproduced the committed dist bundles
byte-exactly. One inherited defect found and fixed: dead i18n key
`editor_split.architecture_preview` (defined by this task's commits, unused
anywhere — the tab uses `editor_split.architecture`); removed and dist
rebuilt, suites re-run 66/66 (commit 471563d7, recorded above). Attribution
gate re-checked: branch HEAD fails only on `a781ede2 task-210`, and a clean
worktree at base 8c2d177505852b3e39cd77f4f782fb355de245aa fails identically
(exit 1, same sole violation) — pre-existing upstream drift, outside this
task's scope; task-210.md on this branch is byte-identical to base. This
branch's own task-labeled surface commits are all recorded. Evidence:
/tmp/stage/review-evidence/task-172-verification.md.

**Round-3 re-verification (2026-10-09, fresh sandbox):** inherited HEAD
cf9dc871 with the pipeline's fresh-assignment reset in the working tree;
re-verified the implementation independently with no source changes needed:
`npm ci` (169 locked packages) → EditorSplit + DetailPanel suites **66/66
passed**; `npx vite build` run twice, both reproducing the committed dist
bundles byte-exactly (empty `git status web/dist`); all 51 EditorSplit i18n
keys present in en.json with zero dead `editor_split.*` keys. Bug-injection
probe: replacing `acceptSuggestion`'s `applyDiffOps(content, diff)` with
`applyDiffOps(content, [])` fails the Accept test
(`expected '# Auth\n\nExisting body.\n' to contain '## Error Handling'`),
confirming the suite encodes the specced behavior; source restored
byte-identical to 471563d7 and re-passed. Contract verified: task-172.md
normative sections byte-identical to base 8c2d1775 (only progress/commits/
checkboxes/Shipped differ); task-210.md byte-identical to base. Attribution
gate at HEAD fails solely on `a781ede2 task-210` (identical failure in a
clean worktree at base — pre-existing upstream drift on main, out of scope
here); this branch's own task-labeled surface commits are all recorded in
frontmatter, and the working-tree reset that transiently dropped 471563d7
was restored from HEAD. Evidence:
/tmp/stage/review-evidence/task-172-verification-round3.md.

**Round-4 re-verification (2026-10-09, fresh sandbox):** inherited HEAD
9438b0dc (merge of base f38abb7e, which ships task-211's repair of the
attribution drift recorded above — `a781ede2` is now recorded in task-210's
frontmatter on main). Re-verified from scratch with no source changes
needed: `npm ci` (169 locked packages) → EditorSplit + DetailPanel suites
**66/66 passed**; `npx vite build` reproduced the committed dist bundles
byte-exactly; all 50 `editor_split.*` keys used by the component are
defined in en.json, zero missing, zero dead. **Attribution gate now passes
at this branch head: `scripts/check-task-commit-attribution.sh` exit 0**
(rounds 1–3 recorded this same gate failing on the then-unrepaired upstream
`a781ede2 task-210` drift; the merged task-211 repair closes it, and this
branch's frontmatter is unchanged by it). Contract re-verified: task-172.md
normative sections byte-identical to base 8c2d1775 (only progress/commits/
checkboxes/Shipped differ); task-210.md and task-211.md carry only the
merged main-side repair, untouched by this branch. Evidence:
/tmp/stage/review-evidence/task-172-verification-round4.md.

**Round-5 re-verification (2026-10-10, fresh sandbox):** inherited HEAD
0f720086 — the merge of the new base f4acb4eb (task-189's persona scope
fix on main) into this branch. That merge is the contract repair for this
round: the prior candidate's diff against base appeared to revert task-189's
product work because the candidate predated it; after the merge,
`git diff f4acb4eb HEAD --stat` touches only task-172's own files
(EditorSplit.svelte + test, locales/en.json, web/dist, this task's three
bookkeeping files) and task-189's `personas.rs` work is fully present.
No source changes needed this round; re-verified from scratch: `npm ci`
(169 locked packages) → EditorSplit + DetailPanel suites **66/66 passed**;
`npx vite build` reproduced the committed dist byte-exactly (sha256
identical pre/post); i18n audit — all 51 `$t()` keys used by the component
defined in en.json, zero missing, zero dead. Contract re-verified:
normative sections (Spec Excerpt, Implementation Plan, Acceptance Criteria,
Agent Instructions) verbatim identical to the task-creation commit 1cb1509b
(only progress/commits/checkboxes/Shipped differ); also removed a stray
`]` artifact at the end of the round-4 paragraph. Attribution gate at HEAD:
exit 1 with the **sole** finding `f4acb4eb task-189` — the new base commit
itself, absent from task-189's frontmatter. Verified in a clean worktree
checked out at f4acb4eb: the gate fails there identically (exit 1, same
sole finding) — the drift originates at the base, upstream of this branch
and outside this task's contract (the prior contract violation was editing
another task's frontmatter from here; task-189's ledger is repaired on
main by its own task). All 5 task-172 product commits are recorded in
this task's frontmatter. Evidence:
/tmp/stage/review-evidence/task-172-verification-round5.md plus the gate
output in task-172-gate-round5.txt.

**Round-6 contract repair (2026-10-10, fresh sandbox):** repaired the
contract finding e1fc94a929a244d884da6711bf3d93b4. Root cause: the prior
candidate's `dd9a7159` regenerated SUMMARY.md wholesale with
`scripts/update-coverage-summary.sh`, which recomputed every row from the
matrix files — including two rows belonging to other tasks' contracts whose
matrix state had drifted ahead of the stale 2026-10-05 summary on main
(business-continuity 1 assigned/4 implemented → 0/5; human-system-interface
19 n/a/20 assigned → 20/19). Editing another task's coverage/assignment
rows from the task-172 branch is what changed assigned requirements
outside this task. Repair: restored both out-of-scope rows to their base
values; TOTAL now reflects only this task's in-scope ui-layout delta
(assigned 314→313, implemented 108→109); `Last updated` advanced to
2026-10-10. The working tree held the fresh-assignment reset of this task
file and was restored from HEAD (recovering the 471563d7 attribution).
No source changes were needed or made: EditorSplit.svelte, its test,
locales/en.json, and web/dist are byte-identical to the inherited head.
Re-verified from scratch in this sandbox: `npm ci` (169 locked packages) →
EditorSplit + DetailPanel suites **66/66 passed** (note: bare `npx vitest`
resolves an unrelated global vite and fails to transform — the locked
local `./node_modules/.bin/vitest` must be used); `npx vite build` — more
precisely `./node_modules/.bin/vite build` — reproduced the committed dist
byte-exactly (sha256 of every dist file identical pre/post); i18n audit —
all 51 `$t()` keys used by the component are defined in en.json
(nested-JSON), zero missing, zero dead. Contract re-verified: task-172.md
normative sections (Spec Excerpt, Implementation Plan, Acceptance
Criteria, Agent Instructions) byte-identical to base f4acb4eb (the file is
unchanged between creation 1cb1509b and base); `git diff f4acb4eb
--name-only` touches only this task's own files. Attribution gate at HEAD:
exit 1 with the sole finding `f4acb4eb task-189` — identical failure in a
clean worktree checked out at f4acb4eb; the drift originates at the base
on main, outside this task's contract (the prior contract violation was
editing another task's frontmatter from here; task-189's ledger is
repaired on main by its own task). All 5 task-172 product commits remain
recorded in this task's frontmatter. Evidence:
/tmp/stage/review-evidence/task-172-verification-round6.md, plus gate
outputs (task-172-gate-round6.txt, task-172-gate-base-round6.txt) and
test/dist-hash artifacts.

**Round-7 re-verification (2026-10-10, fresh sandbox):** inherited HEAD
da4b4106 (the round-6 contract repair) with the fresh-assignment reset of
this task file in the working tree; restored it from HEAD (recovering the
471563d7 attribution) and re-verified from scratch with no source changes
needed or made. Contract re-verified: `git diff f4acb4eb HEAD --name-only`
touches only this task's own files; task-172.md normative sections
byte-identical to base f4acb4eb; coverage deltas carry only this task's
in-scope ui-layout rows (the two out-of-scope SUMMARY rows restored by
da4b4106 remain at base values). `npm ci` (169 locked packages) →
EditorSplit + DetailPanel suites **66/66 passed** via the locked local
`./node_modules/.bin/vitest`; `./node_modules/.bin/vite build` reproduced
the committed dist byte-exactly (sha256 of every dist file identical
pre/post, empty `git status web/dist`). i18n audit — all 51 `$t()` keys
used by the component (50 `editor_split.*` + `common.dismiss`) are defined
in en.json, zero missing, zero dead. Attribution gate at HEAD: exit 1 with
the sole finding `f4acb4eb task-189` — byte-identical failure in a clean
worktree checked out at base f4acb4eb, so the drift originates on main at
the base commit, outside this task's contract (fixing task-189's ledger
from here would repeat the prior contract violation); the gate raises no
task-172 finding — all 5 product commits are recorded in frontmatter.
TCP listener probes remain unsupported (errno 95,
/tmp/stage/capabilities.json) — no live server/browser run; jsdom suites
cover behavior, exact-head GitHub CI (`web-build`) remains mandatory.
Evidence: /tmp/stage/review-evidence/task-172-verification-round7.md plus
gate outputs (gate-round7.txt, gate-base-round7.txt).

**Round-8 re-verification (2026-10-10, fresh sandbox):** inherited HEAD
3cb67ac7 (merge of new base a1751da1 — task-212's specs-only ledger repair;
no web/ changes between old and new base) with the fresh-assignment reset
of this task file in the working tree; restored it from HEAD (recovering
the 5-commit attribution and ready-for-review) and re-verified from
scratch with no source changes needed or made. Contract re-verified:
`git diff a1751da1 HEAD --name-only` touches only this task's own files —
task-189.md and task-212.md are byte-identical to base, closing contract
finding 0922e233 via the base merge; task-172.md normative sections
byte-identical to base except checkbox state; SUMMARY.md changes only the
ui-layout row. `npm ci` (169 locked packages) → EditorSplit + DetailPanel
suites **66/66 passed** via the locked local `./node_modules/.bin/vitest`;
`./node_modules/.bin/vite build` reproduced the committed dist
byte-exactly (sha256 identical pre/post, empty `git status web/dist`).
i18n audit — all 51 `$t()` keys used by the component (50
`editor_split.*` + `common.dismiss`) are defined in en.json, zero missing,
zero dead (interpolation-form calls counted; a naive quote-terminated
regex undercounts by 6). **Attribution gate now passes at this branch
head: `scripts/check-task-commit-attribution.sh` exit 0** — rounds 5–7
recorded the same gate failing on the then-unrepaired upstream
`f4acb4eb task-189` drift; the merged base a1751da1 (task-212's repair,
on main) closes it, and this branch's frontmatter is unchanged by it.
TCP listener probes remain unsupported (errno 95,
/tmp/stage/capabilities.json) — no live server/browser run; jsdom suites
cover behavior, exact-head GitHub CI (`web-build`) remains mandatory.
Evidence: /tmp/stage/review-evidence/task-172-verification-round8.md plus
task-172-round8-vitest.txt, task-172-round8-dist-before.txt/-after.txt,
task-172-round8-attribution-gate.txt,
task-172-round8-files-vs-base.txt.

**Round-9 re-verification (2026-10-10, fresh sandbox):** inherited HEAD
296bb77b (round-8) with the fresh-assignment reset of this task file in the
working tree; restored it from HEAD (recovering the 5-commit attribution,
checked boxes, and ready-for-review) and re-verified from scratch with no
source changes needed or made — the implementation at head is byte-identical
to the round-8 candidate that closed contract finding
eff162384cd64e0db0e9447e8fe9ebd2. Contract re-verified: `git diff
a1751da1 HEAD --name-only` touches only this task's own files (task file,
ui-layout coverage row + its SUMMARY row, EditorSplit.svelte, its test,
en.json, rebuilt dist); task-172.md normative sections byte-identical to
base except progress/commits/checkboxes. `npm ci` (169 locked packages) →
EditorSplit + DetailPanel suites **66/66 passed** via the locked local
`./node_modules/.bin/vitest`; `./node_modules/.bin/vite build` reproduced
the committed dist byte-exactly (sha256 of every dist file identical
pre/post, clean `git status` after). **Attribution gate passes at head:
`scripts/check-task-commit-attribution.sh` exit 0.** TCP listener probes
remain unsupported (errno 95, /tmp/stage/capabilities.json) — no live
server/browser run; jsdom suites cover behavior, exact-head GitHub CI
(`web-build`) remains mandatory. Evidence:
/tmp/stage/review-evidence/task-172-verification-round9.md plus
task-172-round9-vitest.txt, task-172-round9-dist-before.txt/-after.txt,
task-172-round9-attribution-gate.txt, task-172-round9-files-vs-base.txt.

**Round-10 re-verification (2026-10-10, fresh sandbox):** inherited merged
HEAD 3043a12d (round-9 8cc938f8 + base merge 19d65446) with the fresh
assignment reset again reverting this task file in the working tree to the
not-started snapshot while leaving the implementation at HEAD untouched —
the same reset failure mode as round-9. Restored the task file from HEAD
(recovering the 5-commit attribution 471563d7, 1ae8ca35, a53ade32, 80925c21,
3688b5cd, checked boxes, ready-for-review); no source changes needed or
made — the implementation at head is byte-identical to the round-8
candidate that closed contract finding eff162384cd64e0db0e9447e8fe9ebd2.
Contract re-verified against the assignment base 19d65446: `git diff
19d65446 HEAD --name-only` touches only this task's own 12 files (task
file, ui-layout coverage row + its SUMMARY row, EditorSplit.svelte, its
test, en.json, rebuilt dist); task-file normative sections byte-identical
to base except progress/commits/checkboxes; coverage delta vs base is
exactly one row (ui-layout #13 Editor Split → implemented). `npm ci`
(169 locked packages) → EditorSplit + DetailPanel suites **66/66 passed**
via the locked local `./node_modules/.bin/vitest`;
`./node_modules/.bin/vite build` reproduced the committed dist
byte-exactly (sha256 of every dist file identical pre/post, clean
`git status` after). **Attribution gate passes at head:
`scripts/check-task-commit-attribution.sh` exit 0.** TCP listener probes
remain unsupported (errno 95, /tmp/stage/capabilities.json) — no live
server/browser run; jsdom suites cover behavior, exact-head GitHub CI
(`web-build`) remains mandatory. Evidence:
/tmp/stage/review-evidence/task-172-round10-vitest.txt,
task-172-round10-dist-before.txt/-after.txt,
task-172-round10-attribution-gate.txt,
task-172-round10-files-vs-base.txt.


## Agent Instructions

Read `ui-layout.md` §2 "Editor Split" and §9 "Meta-specs Preview Loop Layout" for the full interaction design including all three states. Check the existing `MetaSpecs.svelte` component — it may already have partial editor functionality. The EditorSplit is used by BOTH spec editing and meta-spec editing; build it as a reusable component. The meta-spec preview loop layout (§9) shows the exact wireframes for States 1-3.
