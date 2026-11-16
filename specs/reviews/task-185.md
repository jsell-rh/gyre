# Review — task-185 (LLM-Assisted Spec Editing — inline suggestions with Accept/Edit/Dismiss)

Spec: `specs/system/ui-layout.md` §3 "LLM-Assisted Spec Editing".
Candidate under review: `274e39aa` (base `a4c4c012`); product commits `bd4ee690`
and `ee9c6c63`, both ancestors of HEAD and both recorded in the task's
`commits:` frontmatter at the candidate.
Verdict: **approved** — no findings. Frontend-only change; every acceptance
criterion is backed by shipped production code and by tests that demonstrably
fail when the behavior is broken.

## Probes run (evidence: `/tmp/stage/review-evidence/`)

- `cd web && npm ci` → exit 0.
- `npx vitest run src/__tests__/specAssist.test.js src/__tests__/EditorSplit.test.js src/__tests__/DetailPanel.test.js`
  → **113/113 passed** (`task-185-review-vitest.log`).
- `npx vitest run src/__tests__/DetailPanelChat.test.js src/__tests__/MetaSpecs.test.js`
  (adjacent suites mounting the same components) → **115/115 passed**
  (`task-185-adjacent-vitest.log`).
- Mutation probe 1: reverted EditorSplit's suggestion *append* to the old
  single-suggestion *replace* → the "multiple suggestions coexist" test
  **failed** (1 failed; `task-185-mutation1.log`). Source restored.
- Mutation probe 2: dropped the separator newline in `applyReplace`
  (specAssist.js) → the "replace substitutes the section body but keeps the
  header line" test **failed** (1 failed; `task-185-mutation2.log`). Source
  restored.
- `scripts/check-task-commit-attribution.sh` timed out in this sandbox
  (full-history walk, 120 s cap); manual verification performed instead — the
  verifier should run it to completion.

## What was verified

- **Shared production implementation, no diverging copies.** Both §3 host
  surfaces (DetailPanel spec edit tab and EditorSplit left pane, spec and
  meta-spec contexts) now consume one set of modules: `SpecChatInput.svelte`
  (recipient indicator "Edit spec: … ▸", Ctrl/Cmd+Enter submit, send button
  with streaming loading state, progressive `partial` rendering with
  `data-testid="llm-streaming"`), `SpecSuggestion.svelte` (per-suggestion
  inline diff block with Accept/Edit/Dismiss), and `specAssist.js`
  (`streamAssist`, `applyDiffOps`, `suggestionText`). The previously
  duplicated inline SSE loops, diff-application code, and markup in both
  hosts were deleted, not left as dead copies.
- **Wire protocol matches the server.** `streamAssist` consumes exactly what
  `crates/gyre-server/src/api/specs_assist.rs` `assist_sse_events`/`sse_error_stream`
  emit: `event: partial` with `{"text":…}` chunks, `event: complete` with
  `{diff, explanation}`, `event: error` with `{"error":…}`. Non-OK statuses,
  missing bodies, and `error` events throw; a stream truncated before
  `complete` yields no suggestion (pinned by a dedicated test) — no
  masquerading.
- **Multiple suggestions coexist (the §3 behavior this task finished).**
  Each completed assist appends `{id, diff, explanation}` to `llmSuggestions`;
  Accept applies only that block's diff in-memory, Edit appends only that
  block's `suggestionText`, Dismiss removes only that block. Entity switches
  in DetailPanel reset the list. Proven by tests AND by mutation probe 1
  (reverting to replace semantics fails the suite).
- **Diff-op semantics per spec.** `add` inserts after the `path` header line
  (or line-range fallback, or document end); `replace` substitutes the section
  body keeping the header; `remove` deletes header+body; `"L15-L22"`/`"L7"`
  line-range fallback with clamping; unresolvable anchors and unknown ops are
  no-ops. The round-1 defect (replaced body fusing with the next header) is
  fixed with a separator newline plus trailing-newline preservation, and the
  fix is pinned by tests that fail without it (mutation probe 2).
- **Save workflow.** Save posts `POST /api/v1/repos/:id/specs/save` with
  `{spec_path, content, message, base_sha?}` and surfaces the created MR id in
  the success toast (`detail_panel.spec_saved` / `editor_split.spec_saved`).
  Accept never triggers a save call (asserted in both hosts' tests) — the LLM
  never writes directly.
- **Request shape.** DetailPanel test asserts the exact assist URL
  (`/api/v1/repos/repo-abc/specs/assist`) and body
  `{spec_path, instruction, draft_content}`; `api.js` `specsAssist` is a POST
  read via `ReadableStream` (the spec-sanctioned SSE approach for POST bodies).
- **Test quality.** No self-confirming tests found: the EditorSplit streaming
  test gates the second SSE read so the intermediate progressive-render state
  is observed while the stream is open (input disabled, partial text visible)
  before `complete` lands; the DetailPanel tests assert fetch URLs/bodies
  verbatim, in-memory-ness of Accept (zero `/specs/save` calls), and the MR id
  in the toast. Both mutations I ran were killed.
- **No dangling state.** Old `llmSuggestion`/`findSectionEnd`/inline CSS
  removed from both hosts; all i18n keys used by the new components exist in
  `web/src/locales/en.json`. No Rust files touched, so no backend/arch/ABAC
  surface changed.

## Non-blocking observations (recorded for completeness)

- `SpecSuggestion.svelte` uses the `editor_split.*` i18n namespace even when
  hosted by DetailPanel — cosmetic only; all keys exist and render.
- `streamAssist` does not `reader.cancel()` on the error path — a harmless
  unused-stream remainder; the server closes the response.
- The agent did not regenerate `web/dist/` — per checkpoint rules the
  deterministic verifier owns the bundle rebuild; recorded separately, not a
  source defect.
- The coverage matrix row (`ui-layout.md` §LLM-Assisted Spec Editing, row 19)
  still reads `task-assigned`; per repo convention the flip to `implemented`
  lands in the controller's process commit after approval (cf. task-171).
