# Review — task-186 (Ghost Overlays — structural prediction visualization on canvas)

Spec: `specs/system/system-explorer.md` §3 "Inline Spec Editing with Progressive Preview".
Candidate: `4b9fdd8d25d0b591d16ce35a549173c3a0deb5e6` (base `c66ee2ac`). Task commits:
`343ff8c8` (implementation), `44310fa1` (F1/F2 repair), `2e6b190d` (F3 repair), plus
process/record commits `918d3b83`, `1988ebf0`, `9188fc29`, `f5f7cdde`, `64fff764`,
`4b9fdd8d` — all recorded in the task's `commits:` frontmatter.
Verdict: **approved** (round 3; round-1 findings F1/F2 and round-2 finding F3 verified
repaired, no new findings). Evidence: `/tmp/stage/review-evidence/task-186-round3.md`.

## Round 3 (this review)

Probes run:

- `cd web && npm ci` then `npx vitest run` on the five touched suites:
  ghostOverlay (26) + GhostOverlay (16) → 42 passed; EditorSplit (62) +
  ArchPreviewCanvas + DetailPanel → 138 passed.
- Mutation probes (each restored afterwards; `git status` confirms no source
  modification survives):
  - completion condition regressed to fictional `'completed'` → 3 tests fail;
  - failure condition regressed to fictional `'failed'` → 2 tests fail;
  - confidence→opacity map flattened to a constant → 7 tests fail;
  - debounce removed (synchronous prediction per effect run) → the
    "one call after the edit pause, not per keystroke" test fails.
  All four §3-relevant production behaviors are load-bearing for the suite.
- `/tmp/stage/bin/cargo check -p gyre-server --tests` → clean (9m12s).
- Mechanical guards all pass: abac-route-registry, arch, mem-port-contracts,
  in-memory-state-stores, inert-enforcement, scope-literal-defaults,
  forged-scope-fields (87 handlers), byte-slice-truncation,
  relative-path-defaults, mcp-write-tools, abac-exempt-handlers,
  task-commit-attribution.
- Line-by-line read of the diff: `graph.rs` (+749 incl. 4 new tests),
  `ghostOverlay.js`, `GhostOverlay.svelte`, `EditorSplit.svelte`,
  `ArchPreviewCanvas.svelte`, `DetailPanel.svelte`, `api/mod.rs`,
  `abac_middleware.rs`, `explorer_views.rs`, `docs/api-reference.md`,
  `en.json`, and the three new/extended test files.

Verified working:

- **Ghost visual language matches the §3 table exactly.** `GhostOverlay.svelte`
  renders dotted outline + green fill for adds, yellow for modifies, red dotted
  outline + red strikethrough for removes, dotted green edges for predicted
  relationships, opacity scaled by confidence (`ghostOpacity`: 0.3 @ 50%,
  1.0 @ 100%, linear between anchors, 0.15 floor below), pulsing border while a
  preview agent runs, solid border after completion. Ghosts are hover/focus
  targets for the reason tooltip and carry no click handlers — a dedicated test
  asserts non-navigation.
- **Fast tier is real.** `EditorSplit.svelte` debounces edits (1.2 s, timer reset
  per keystroke, suppressed while a thorough preview runs and after save), fires
  `POST /repos/:id/graph/predict` with `{spec_path, draft_content}`, shows the
  `Predicting…` chip while in flight, and normalizes the response's predictions
  through the shared `ghostOverlay.js` model. Server side, the spec-edit path of
  `predict_graph` appends the §3 output contract to the LLM system prompt and
  computes `affected_specs`/`affected_repos` from real spec-links and
  knowledge-graph queries — the new server test asserts the contract actually
  reached the LLM via `MockLlmAdapter::captured_system_prompts`.
- **Instant tier is real.** The impact bar computes connected specs from
  `GET /specs/:path/links`, repos implementing the spec (inbound `implements`
  links + repos whose graphs govern the spec's nodes), and the governed-node
  count from the repo graph; each segment degrades independently.
- **Thorough tier is real.** New `POST /repos/:id/graph/thorough-preview`
  (route registered in `api/mod.rs`, ABAC resolver mapping `graph:write` in
  `abac_middleware.rs`) validates input, creates a preview `Task` carrying the
  draft (`labels: ["spec-preview"]`, sanitized throwaway branch
  `preview/<spec-stem>-<id8>`), spawns the agent through the shared
  `spawn_agent_core`, and rolls the preview task back when the spawn fails
  (test drives a genuinely failing spawn via a dangling workspace ref and
  asserts the task store is empty). `pollPreviewTask` matches exactly the real
  `task_status_str` vocabulary (`review`/`done` complete, `blocked`/`cancelled`
  fail); completion resolves the agent's MR and renders its real hunks in the
  Code Diff tab; ghosts pulse during execution and become solid on completion.
- **Prior findings repaired.** F1: branch assertion now expects
  `preview/search-md-` (matches the sanitizer's `.`→`-` map). F2: missing-field
  case asserts 422 (axum 0.7.9 `Json<T>` data error), in-handler empty-field
  validations still 400. F3: production poller and every thorough-preview test
  use only the real vocabulary, with a `blocked`/`cancelled` failure-path test;
  mutation probes confirm both branches are load-bearing.
- **Tests are not inflated.** Debounce test fires three keystrokes and asserts
  zero calls inside the window plus exactly one call after it; opacity tests pin
  the anchors; the MR-diff test mocks realistic hunk shapes and asserts the
  rendered diff content; the failure test asserts error banner, unlocked editor,
  no iterate button, and no MR fetch.

Non-blocking observations (recorded, no action required):

- `web/dist/` working tree has uncommitted generated changes that pre-existed
  this review (no build ran here; no postinstall script). Per assignment,
  `web/dist` is verifier-owned and rebuilt after integration.
- `estimated_agent_cost` is a documented deterministic formula
  ($0.05 + $0.10 × prediction count) rather than a measured cost; the spec
  calls the field an estimate and the formula is documented in code and
  `docs/api-reference.md`.
- A transient predict failure sets `lastPredictedContent`, so it is not retried
  until the next edit — documented best-effort behavior (§3: predictions are
  feedback, not a gate).

Verifier-owed (gyre-server test compile is verifier-owned per the 4 GiB budget):

```
cargo test -p gyre-server --lib api::graph::tests::predict_graph_spec_edit_returns_ghost_contract_and_link_context
cargo test -p gyre-server --lib api::graph::tests::thorough_preview_creates_task_and_spawns_agent_on_throwaway_branch
cargo test -p gyre-server --lib api::graph::tests::thorough_preview_rejects_missing_fields_and_unknown_repo
cargo test -p gyre-server --lib api::graph::tests::thorough_preview_rolls_back_preview_task_when_spawn_fails
cargo test -p gyre-server --lib api::graph::tests::predict_graph
cargo test -p gyre-server --lib api::spawn
```

`cargo check -p gyre-server --tests` is clean at this candidate; all four new
tests were read line-by-line and drive the real router with real fixtures.
