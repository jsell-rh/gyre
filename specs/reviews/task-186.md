# Review — task-186 (Ghost Overlays — structural prediction visualization on canvas)

Spec: `specs/system/system-explorer.md` §3 "Inline Spec Editing with Progressive Preview".
Candidate: `918d3b832f9ec912a58e3dfc88a321e7fe9a65e3` (base `6f5f408289122bba4a7e2495b6fdabe807543485`, product commit `343ff8c8`).
Verdict: **needs-revision** — two blocking findings (F1, F2), both in the new server tests, both provable without running the test binary; the frontend implementation itself is verified clean.

## Probes run (evidence: `/tmp/stage/review-evidence/`)

- `cd web && npm ci` (locked versions), then:
  - `npx vitest run src/__tests__/ghostOverlay.test.js src/__tests__/GhostOverlay.test.js` → 42/42 passed.
  - `npx vitest run src/__tests__/EditorSplit.test.js` → 60/60 passed.
  - Full frontend suite `npx vitest run` → 124 files, 2485 passed, 41 skipped.
- Mutation probes (each restored; `git status` clean afterwards):
  - A: `ghostOpacity` → constant `1` → 5 tests fail across both ghost test files (opacity tests are real guards).
  - B: remove the 1.2 s debounce in `EditorSplit.svelte` → the "prediction is debounced — one call after the edit pause, not per keystroke" test fails.
  - C: remove `derivedGhosts = []` from the save handler → the "ghost overlay clears when the spec is saved" test fails.
- Mechanical gates, all PASS: `check-arch`, `check-abac-route-registry`, `check-mcp-write-tools`, `check-abac-exempt-handlers` (87 handlers), `check-byte-slice-truncation`, `check-relative-path-defaults`, `check-in-memory-state-stores`, `check-inert-enforcement`.
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` — clean (cold, 6m28s).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` — clean (3m50s). gyre-server test-binary compilation deferred to the verifier per assignment budget rules; F1/F2 below are proven statically (exact source semantics + the pinned axum 0.7.9 registry sources), so no binary run was needed to establish them.

## Verified clean (frontend + API surface)

- **Ghost visual language matches the §3 table exactly.** `GhostOverlay.svelte` + shared `ghostOverlay.js`: dotted outline + green fill for adds (`#22c55e`), yellow for modifies (`#eab308`), red dotted + red strikethrough for removes (`#ef4444`), dotted green edges for predicted relationships, pulsing border while a preview agent runs (`ghost-previewing` keyframes), solid border after confirmation (`ghost-confirmed` overrides the presentation-attribute dasharray). Component tests assert each visual per action; mutation A confirms the opacity tests bite.
- **Confidence → opacity** implements the documented anchors (0.3 @ 50 %, 1.0 @ 100 %, linear, faint 0.15 floor below), with clamping for percent-form/garbage inputs.
- **Tooltips** show name, action, confidence %, reason and changes on hover and keyboard focus; ghost nodes carry no click handlers (asserted).
- **Fast tier**: `EditorSplit` debounces edits (1.2 s) into one `POST /repos/:id/graph/predict` call per pause with `{spec_path, draft_content}`, shows a `Predicting…` chip in flight, renders response predictions as ghosts. Mutation B confirms the debounce guard. Server side, the spec-edit prompt now carries the §3 output contract (action/node_type/name/node_id/confidence/reason/changes/parent_trait) — asserted via `MockLlmAdapter::captured_system_prompts`, a real prompt-level check, not mirrored logic — and the response adds `affected_specs` (real spec-links store query, both directions, deduped) and `affected_repos` (real per-workspace repo scan for live graph nodes governed by the spec). The §8 explorer-ask caller is unchanged.
- **Instant tier**: impact bar above the editor renders connected specs (spec links API), repos implementing (inbound `implements` links + graph-governed repo) and governed node count, each segment degrading independently. Tested against mocked API shapes.
- **Thorough tier**: new `POST /repos/:id/graph/thorough-preview` — ABAC `graph:write` registered in the resolver (not the frozen exemption file), request validation, preview `Task` carrying the draft, real agent spawn via the shared `spawn_agent_core` (same ABAC/budget/worktree/JWT path as `/agents/spawn`), compensating task rollback on spawn failure, and a `process_launched` flag that never fabricates completion. `EditorSplit` polls with the real task status vocabulary (`review`/`done` = complete), resolves the completion MR by task/agent linkage and renders its real diff hunks in the Code Diff tab; ghosts pulse during the run and turn solid on completion (tested end-to-end with mocked transport).
- **Canvas integration**: modify/remove ghosts overlay their existing node (id, else name match); add ghosts sit near the predicted `parent_trait` (staggered) or in deterministic free space; ghost positions extend the canvas bounds; ghosts render even for specs with no graph linkage. The legacy `{nodeId, type}` overlay shape still normalizes through the shared model (DetailPanel arch tab uses it too).
- Route + resolver mapping + `docs/api-reference.md` rows added; no new exemptions anywhere; `estimated_agent_cost` is a documented deterministic estimate over the real prediction count (no pre-execution cost source exists — acceptable, noted below).

## Findings (blocking)

- **F1 — server test asserts a branch prefix the sanitizer cannot produce** (`crates/gyre-server/src/api/graph.rs`, test `thorough_preview_creates_task_and_spawns_agent_on_throwaway_branch`, assert at line 3409). The handler's `spec_stem` sanitizer keeps only `[A-Za-z0-9_-]` and maps everything else (including `.`) to `-`, so spec_path `specs/system/search.md` produces branch `preview/search-md-<id8>`. The test asserts `branch.starts_with("preview/search.md-")` — false at position 15 (`.` vs `-`) — so the test fails at runtime. The spawn path itself succeeds under the identical fixture pattern in existing spawn.rs tests (201 asserted), so the test reaches and fails at this assertion. Repair: fix the expected prefix to `preview/search-md-` (or preserve `.` in the sanitizer) and actually execute the test.
- **F2 — server test asserts 400 for a missing required field; axum 0.7.9 returns 422** (`crates/gyre-server/src/api/graph.rs`, test `thorough_preview_rejects_missing_fields_and_unknown_repo`, third case). The handler extracts with stock `Json<ThoroughPreviewRequest>`; a JSON body missing `spec_path` is a serde `Category::Data` error → axum `JsonDataError` → **422 Unprocessable Entity** (axum 0.7.9 sources in the build registry; the pre-existing passing test `release.rs:441` documents exactly this convention). The empty-draft case in the same test reaches the handler (400), proving the middleware passes, so the missing-field case gets 422 and `assert_eq!(422-as-actual, 400)` fails. Repair: assert `UNPROCESSABLE_ENTITY`, or validate inside the handler and return `ApiError::InvalidInput`.

Both findings are test defects rather than production-code defects (the production behaviors — `preview/search-md-…` branch naming, 422 on a missing field — are defensible), but the task's "Tests pass" acceptance criterion is not met: the implementer deferred all server test execution to the verifier (task doc lists these exact tests) after running only `cargo check`, and two of the four new server tests provably fail. The verifier will run them and the round will fail there; better to fix now.

## Non-blocking observations (recorded, not requiring revision)

- `estimated_agent_cost` is `$0.05 + $0.10 × prediction-count`, documented as a deterministic estimate; there is no real pre-execution cost source (actual spend is recorded per agent at completion). Acceptable, but worth a spec-lifecycle note if a real cost model lands.
- Coverage matrix row (`system-explorer.md` row 11) remains `task-assigned | task-186`, consistent with the pipeline convention (the verifier regenerates matrices; the task-185 dependency row is likewise) — not a source defect.
- Ghost tooltips can render partially off-canvas for nodes near the top edge (translate `pos.y - nodeH - 6`); cosmetic.
- `web/dist/` untouched by the candidate — generated output is verifier-owned per the assignment; no observation beyond that the committed bundle predates this change (expected).

## Verifier must run (server, per assignment budget rules)

- `cargo test -p gyre-server --lib api::graph::tests::predict_graph_spec_edit_returns_ghost_contract_and_link_context` (expected: pass)
- `cargo test -p gyre-server --lib api::graph::tests::thorough_preview_creates_task_and_spawns_agent_on_throwaway_branch` (**expected: FAIL — F1**)
- `cargo test -p gyre-server --lib api::graph::tests::thorough_preview_rejects_missing_fields_and_unknown_repo` (**expected: FAIL — F2**)
- `cargo test -p gyre-server --lib api::graph::tests::thorough_preview_rolls_back_preview_task_when_spawn_fails` (expected: pass)
- `cargo test -p gyre-server --lib api::graph::tests::predict_graph` (pre-existing predict regressions)
- `cargo test -p gyre-server --lib api::spawn` (spawn-core regressions)
