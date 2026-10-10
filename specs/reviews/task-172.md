# Review — task-172 (Editor Split layout component for spec and meta-spec editing)

Spec: `ui-layout.md` §2 "Editor Split" (plus §3 LLM-Assisted Spec Editing steps 4-6 and §9 Meta-specs Preview Loop Layout as the interaction references named by the task).
Commits under review (base `c9b0a6f91f6ca5a5bc638cf9b885179419a154b9` → candidate `28fb853dee468a527fd14c326a9783673370986e`): the six frontmatter commits plus process/checkpoint commits; product surface is `web/src/lib/EditorSplit.svelte`, `web/src/__tests__/EditorSplit.test.js`, `web/src/locales/en.json`, `web/dist/`, `specs/coverage/system/ui-layout.md` row 13 + SUMMARY, `specs/tasks/task-172.md`.
Verdict: **approved**.

Evidence under `/tmp/stage/review-evidence/` (fresh run at candidate head 28fb853d; earlier-round artifacts from the implementer were not trusted).

## Probes run

- `npm ci` (169 packages, locked), then focused suites:
  `vitest run src/__tests__/EditorSplit.test.js src/__tests__/DetailPanel.test.js` → **66/66 passed** (EditorSplit 37, DetailPanel 29 including the 5 pop-out tests). Reproduced twice.
- **Bug-injection**: injected `if (true) return c;` into `applyDiffOps` (disables Accept's content application) → the upstream Accept test fails (`expected '# Auth\n\nExisting body.\n' to contain '## Error Handling'`). Source restored, tree clean. The suite is sensitive to the specced behavior, not self-confirming.
- **Accept/Edit/Dismiss diff-op semantics beyond the shipped tests**: `replace` and `remove` ops verified (section rewrite/deletion with following sections preserved) — the upstream suite only exercised `add`. Multi-suggestion curation: dismiss → re-ask → new block, exactly one pending block at a time.
- **SSE parser robustness**: frames split at arbitrary byte boundaries (mid-`data:` line), multi-event reads, `complete` without `explanation` falling back to streamed partial text, and `event: error` → toastError with no suggestion block. Matches the server's actual emitter (`specs_assist.rs` partial/complete/error events with `{diff, explanation}` payloads).
- **State machine**: editing → preview_running (editor locked, per-spec progress, cancel) → preview_complete (Iterate) → iterate returns to editing with Architecture/Code Diff results retained. taskStatus polling with fake timers (2 polls to completion), cancel mid-run, thoroughPreview rejection → fallback toast + editable editor (not stuck), taskStatus `failed` → error toast + editing state.
- **Esc/Back**: Esc during editing, during an in-flight never-resolving LLM stream, and during preview_running all call `onClose` exactly once. Unmount (the parent's actual close path: `closeEditorSplit` → `showEditorSplit=false`) deterministically stops the poll interval ($effect cleanup verified with fake timers over 25 s).
- **LLM failure paths**: non-ok assist response and network rejection both surface toastError; streaming flag resets (aria-busy=false), input not disabled, new instruction re-enables send — no stuck state.
- **Meta-spec context**: target-spec selector with Preview disabled until selection; `previewPersona` called with `persona_id`/`spec_paths`/`content` exactly as the server's `PreviewRequest` defines; without `persona_id` when specPath null; missing workspaceId does not hang or fire a request.
- **Manual edit mode**: plain markdown textarea with live content binding; verified onChange propagation.
- `vite build` reproduces the committed `web/dist` byte-exactly (no diff after build).
- i18n: all 50 `editor_split.*` keys defined; zero missing, zero dead.
- `scripts/check-task-commit-attribution.sh` exit 0; `check-arch.sh` OK.
- Full web suite: 1539 passed / 6 failed — all failures in ExplorerCanvas performance/ghost-overlay tests, which the candidate does not touch; the same failures reproduce at base `c9b0a6f9` under parallel load, and both suites pass in isolation at base and candidate. Pre-existing flakiness, not a regression (see full-suite-note.txt).

## Verified working (no findings)

- Two-panel layout (left editor + LLM chat, right preview) rendered by a single reusable component; Architecture tab default with aria-selected=true, Code Diff tab showing `SpecDiffView` line-level diff.
- LLM chat wiring matches the real endpoints: `POST /repos/:id/specs/assist` request `{spec_path, instruction, draft_content}` (server `SpecsAssistRequest`), SSE parsing matching `specs_assist.rs`; Save via `POST /repos/:id/specs/save` with `base_sha` + `overwrite` (server 409 conflict body fields `current_content`/`submitted_content`/`diff` consumed by the dialog; overwrite/discard/close resolvers verified).
- Meta-spec preview loop against the real `POST/GET /workspaces/:id/meta-specs/preview(/:id)` endpoints (`meta_specs.rs`): request fields match `PreviewRequest`; status polling consumes `specs[].path/status` and `state`; `applyPreviewResult` accepts both the response shapes the endpoints can produce. Note the current server responds `state: "complete"` immediately with `structural_impact`/`blast_radius` (no `architecture_diff`/`specs_diff`), so the Architecture tab falls back to the fast `graph/predict` overlays — spec-consistent: §2 explicitly designates graph/predict ghosts as the Phase-1 fast preview and the agent-run thorough delta as Phase 2, and §9's three-state loop is driven by the polling contract which is honored.
- Pop Out: DetailPanel "Preview" button → `openEditorSplit` (expands to full width, replaces detail panel content); Back button and Esc → `closeEditorSplit` returns to the normal detail panel view. Entity change resets the split. The URL deep-link params are managed by DetailPanel's existing pop-out machinery (`closeEditorSplit` strips `detail`/`expanded`).
- Coverage: row 13 marked `implemented` with SUMMARY updated; only task-172 rows touched; no verifier/exemption files modified.
- No production code outside the component/test/i18n/dist/coverage/task-file surface was touched.

## Notes for the verifier (infrastructure, not defects)

- This sandbox cannot create TCP listeners (capabilities.json errno 95), so no live gyre-server/browser run was performed. Host verification / GitHub CI (`web-build`) should exercise: spec detail panel → Edit tab → Preview → EditorSplit renders two-pane; LLM assist posts to `/repos/:id/specs/assist`; Preview invokes `/repos/:id/graph/thorough-preview` (endpoint not yet implemented server-side — the component's documented graceful fallback to fast prediction fires, per commit afe2d543's original design); meta-spec context via `MetaSpecs` (the §9 layout row 37 is task-176's scope; EditorSplit's meta-spec context is the reusable component side, tested at the component level).
- The `graph/thorough-preview` endpoint has no server implementation at this head (pre-existing since afe2d543 introduced the client call); the component handles its absence with the specced fast-prediction fallback, so this is not a task-172 defect but is recorded for the Phase-2 thorough-preview owner.
