# Review — task-038 (Explorer Keyboard Shortcuts)

Spec: `explorer-implementation.md` §Phase 4 ("Keyboard shortcuts (Escape to clear query, / to focus chat)"), `system-explorer.md` §1, `ui-layout.md` §2.
Candidate: `1db791c3439d579ac4d2532956ef4241c0e192b1` (base `4d6759a0b`). Commit under review: `91abab172` (test-only; the shortcut implementation itself shipped with the explorer-canvas branch and is in the base).
Verdict: **approved**.

## Scope of the diff

Base→candidate touches exactly two files: `web/src/__tests__/ExplorerViewShortcuts.test.js` (new, 369 lines) and `specs/tasks/task-038.md` (frontmatter bookkeeping + `## Shipped` notes with `###` subheadings, per the durable contract finding). No production code changed — consistent with the shipped claim that this round closed the one missing acceptance criterion ("Test verifies shortcut behavior"). Commit `91abab172` is an ancestor of the candidate and listed in the frontmatter `commits:`.

## What was verified

Production handler (`ExplorerView.svelte` `onWindowKeydown`, wired via `<svelte:window onkeydown>` at line 1164): Escape cascade (spec editor → query editor → detail panel → clear `activeViewQuery`), `/` focuses `.chat-input` with `preventDefault()`, and an isTyping guard (INPUT/TEXTAREA/SELECT/contentEditable) on both branches.

The test suite renders the full ExplorerView tree (graph load, ExplorerCanvas with mocked canvas 2d context, ExplorerChat with mocked WebSocket brought to `ready`) and dispatches real keydown events, so the assertions exercise the production event wiring, not an extracted copy of the handler. The `TRACE_QUERY` fixture was diffed against the production "Trace from here" action (`ExplorerCanvas.svelte` `contextMenuAction('trace')`, lines 4485-4494) — identical shape, so the trace-mode test drives a query the product actually produces; trace activity is asserted via the numbered step badges (`_traceStep` rendering) on mocked `fillText` calls, not just the annotation title.

## Probes (evidence in /tmp/stage/review-evidence)

- `npm ci` in `web/` (locked versions), then `npx vitest run src/__tests__/ExplorerViewShortcuts.test.js` → **6/6 passed, exit 0** (`task-038-focused-test.txt`, `task-038-clean-rerun.txt`).
- Mutation testing (temporary edits to `ExplorerView.svelte`, restored after each; tree verified clean at candidate `1db791c34` afterward):
  - **A** — Escape no longer clears `activeViewQuery`: 2 tests fail (query clear, trace exit). `task-038-mutation-A.txt`
  - **B** — `/` loses `preventDefault()` and its isTyping guard: 2 tests fail (focus+preventDefault, no-focus-steal-while-typing). `task-038-mutation-B.txt`
  - **C** — Escape no longer closes the detail panel: 1 test fails. `task-038-mutation-C.txt`
  - **D** — Escape isTyping guard removed: 1 test fails (Escape in text input must not clear query). `task-038-mutation-D.txt`

Every acceptance criterion is covered by at least one test that kills a real production mutation, so the suite is not self-confirming:

| Criterion | Test | Killing mutation |
|---|---|---|
| Escape clears active view query | test 3 | A |
| Escape closes detail panel | test 5 | C |
| Escape exits trace mode | test 4 | A |
| `/` focuses chat input, no "/" typed | test 1 | B |
| Shortcuts suppressed while typing | tests 2, 6 | B, D |
| Test verifies shortcut behavior | the suite itself | A–D |

No findings. The `?` help overlay remains unimplemented but is explicitly optional/low-priority in the task plan and is not an acceptance criterion.
