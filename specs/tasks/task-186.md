---
title: "Ghost Overlays — structural prediction visualization on canvas"
spec_ref: "system-explorer.md §3"
depends_on: [task-178, task-185]
progress: complete
coverage_sections:
  - "system-explorer.md §3. Inline Spec Editing with Progressive Preview"
commits: ["ae93a98c38edc5b7da1d0e9c5318b75d78188282", "a88f1a6ddfa2fe25d28d7d09876e2002da9b6466", "fe77459041d8f40fdf77bee5384dc4f696a7cb96", "2e6b190d8db841a5b62b99a0f4152ecc93e733cb", "44310fa1fcf02864c93460b3496601f12b89d516", "343ff8c8c4d5a55504571c5c3a53abba9280da5f", "364023298aeddd7b445ff9f4d98bf596b7ce8bf1"]
---

## Spec Excerpt

system-explorer.md §3 defines Inline Spec Editing with Progressive Preview. The core feature is the **ghost overlay** — a fast, probabilistic feedback loop (2-5 seconds) that shows predicted structural impact on the architecture canvas as the user edits a spec.

**The Edit → Preview Flow (three tiers):**

1. **Instant (milliseconds)** — Link graph impact:
   - Which specs are connected, which repos implement this spec, how many graph nodes are governed
   - Shown as a subtle info bar above the editor

2. **Fast (2-5 seconds)** — Structural prediction via ghost overlays:
   - LLM analyzes spec diff against knowledge graph via `POST /api/v1/repos/{id}/graph/predict`
   - Predicts: new types/traits to add, existing types to modify, dependencies to change
   - Shown as **ghost nodes** on the canvas — dotted outlines, yellow highlights, strikethroughs
   - Canvas updates live as user edits (debounced)

3. **Thorough (minutes, on-demand)** — Full preview:
   - Click [Preview] to spawn agent on throwaway branch
   - Real code produced, architecture canvas updates with actual extracted nodes
   - Diff viewer: existing vs. preview code

**Ghost Overlay Visual Language:**

| Visual | Meaning |
|---|---|
| Dotted outline, green fill | New element predicted to be added |
| Yellow highlight on existing node | Existing element predicted to change |
| Red strikethrough on existing node | Existing element predicted to be removed |
| Dotted edge, green | New relationship predicted |
| Pulsing border | Currently being previewed (agent running) |
| Solid border (after preview) | Confirmed by actual implementation |

**Predict endpoint** (already exists): `POST /api/v1/repos/{id}/graph/predict` — request: `{spec_path, draft_content}`, response: `{predictions: [{action, node_type, name, confidence, reason, ...}], affected_specs, affected_repos, estimated_agent_cost}`. Predictions include confidence scores (higher confidence = more opaque ghost).

## Implementation Plan

1. **Ghost node rendering** (`GhostOverlay.svelte`):
   - SVG layer rendered on top of (or alongside) the regular canvas nodes
   - Ghost nodes: dotted border, semi-transparent fill, color by action (green=add, yellow=modify, red=remove)
   - Ghost edges: dotted stroke, green for new relationships
   - Opacity scales with confidence score (0.3 at 50% confidence, 1.0 at 100%)
   - Each ghost node shows a tooltip with the prediction reason

2. **Prediction trigger logic**:
   - When user edits a spec (in Editor Split or Spec tab), debounce changes (1-2 seconds)
   - On debounce fire, call `POST /repos/{id}/graph/predict` with current draft content
   - While prediction is loading, show a subtle "Predicting..." indicator
   - On response, render ghost nodes on the canvas

3. **Instant tier — link graph impact bar**:
   - When spec editing begins, immediately compute:
     - Connected specs (from spec links API: `GET /specs/:path/links`)
     - Repos implementing this spec
     - Number of graph nodes governed by this spec
   - Show as a compact info bar above the editor

4. **Thorough tier — full preview integration**:
   - [Preview] button spawns an agent on a throwaway branch
   - While agent runs, ghost nodes show pulsing border
   - When preview completes, ghost nodes become solid (confirmed by real implementation)
   - Architecture tab in Editor Split shows actual graph delta (from task-172)
   - Code Diff tab shows line-level changes

5. **Canvas integration**:
   - Ghost nodes must be positioned relative to existing nodes
   - New nodes: placed near their predicted parent/related node
   - Modified nodes: overlay on existing node position
   - Removed nodes: overlay with strikethrough on existing position
   - Ghost nodes are interactive (hoverable for tooltip) but don't trigger navigation

6. **Tests**:
   - Ghost nodes render with correct visual styles per action type
   - Confidence score maps to opacity correctly
   - Debounced prediction calls fire after edit pause
   - Ghost overlay clears when editing stops or spec is saved
   - Preview mode transitions ghosts from pulsing to solid

## Acceptance Criteria

- [ ] Ghost nodes render on canvas with correct visual language (green=add, yellow=modify, red=remove)
- [ ] Ghost edges render as dotted green lines for new relationships
- [ ] Confidence scores control ghost opacity
- [ ] Ghost tooltips show prediction reason
- [ ] Debounced prediction calls to `POST /repos/:id/graph/predict`
- [ ] Instant tier: link graph impact bar shows connected specs, repos, governed nodes
- [ ] Thorough tier: [Preview] spawns agent, ghosts pulse during execution, become solid on completion
- [ ] Ghost overlay clears when editing ends
- [ ] Tests pass

## Agent Instructions

Read `system-explorer.md` §3 "Inline Spec Editing with Progressive Preview" for the full specification. The predict endpoint is already implemented — check `crates/gyre-server/src/api/graph.rs` for the `predict_graph` handler and its request/response types. The route is at `POST /api/v1/repos/:id/graph/predict` (verified in `mod.rs`). The ghost overlay visual language must match the table in the spec exactly. The Editor Split (task-172) and LLM-Assisted Spec Editing (task-185) handle the left-side editing; this task focuses on the right-side canvas ghost visualization. For the preview agent spawn, check the agent spawn API in `api/spawn.rs`. Ghost node positioning should use a simple heuristic: place new nodes near their predicted parent or in available space near related existing nodes.

## Shipped

The §3 progressive-preview loop runs end to end with real data on every
 tier: instant link-graph impact, debounced LLM structural prediction rendered
 as the spec's ghost visual language, and a thorough preview that really spawns
 an implementation agent on a throwaway branch whose completion MR carries the
 code diff. The implementation was squashed onto this branch as
 `verify(task-186): reconcile candidate with main` (c052a61b) with its
 verified asset build (fe774590); this round repairs the CI failure that
 blocked the candidate (see F4 below).

### Ghost visual language (shared model + SVG layer)

- `web/src/lib/ghostOverlay.js` — pure model shared by every surface:
  prediction normalization (the §3 wire contract `{action, node_type, name,
  node_id, confidence, reason, changes, parent_trait}` plus the legacy
  `{nodeId, type}` overlay shape), the confidence→opacity map (0.3 at 50%,
  1.0 at 100%, linear between the anchors, faint floor below), positioning
  (modify/remove overlay their existing node by id or name; add ghosts near
  the predicted `parent_trait`, staggered, or in deterministic free space),
  and dotted-green ghost-edge derivation for predicted new relationships.
- `web/src/lib/GhostOverlay.svelte` — the §3 table exactly: dotted outline +
  green fill for adds, yellow highlight for modifies, red dotted outline +
  red strikethrough for removes, dotted green edges, opacity scaled by
  confidence, hover/focus tooltip showing the prediction reason, confidence
  and changes, pulsing border while a preview agent runs
  (`ghost-previewing`), solid border after real implementation
  (`ghost-confirmed`). Ghosts are hoverable but carry no click handlers —
  they never trigger canvas navigation.
- `ArchPreviewCanvas.svelte` renders the layer on top of its nodes (any input
  shape normalized once), keeps per-node ghost markers, includes ghost
  positions in the canvas bounds, and renders ghosts even for specs with no
  graph linkage yet (empty state only when no nodes AND no ghosts).

### Fast tier (debounced prediction)

`EditorSplit.svelte` fires `POST /repos/:id/graph/predict` with
`{spec_path, draft_content}` 1.2 s after every edit pause (one call per
pause, not per keystroke), shows a `Predicting…` chip while in flight, and
renders the response's predictions as ghosts. Server side, the spec-edit path
of `predict_graph` appends the §3 output contract to the LLM system
prompt (so predictions come back with action/confidence/reason) and returns
real context fields: `affected_specs` (other side of every spec link touching
the spec), `affected_repos` (workspace repos whose graphs contain live nodes
governed by the spec) and a deterministic `estimated_agent_cost`. The §8
explorer-ask caller of the shared prediction core is unchanged (no contract).

### Instant tier (link-graph impact bar)

The moment editing begins, `EditorSplit` computes and renders a compact bar
above the editor (`impact-bar`): connected specs from `GET /specs/:path/links`,
repos implementing the spec (inbound `implements` link repos + repos whose
graphs govern the spec's nodes), and the governed node count from the repo
graph. Each segment degrades independently; none blocks editing.

### Thorough tier (agent preview)

- New `POST /api/v1/repos/:id/graph/thorough-preview` (ABAC `graph:write`,
  route + resolver mapping registered): validates the request, creates a
  preview `Task` carrying the draft in its description
  (`labels: ["spec-preview"]`, `spec_path`, throwaway `preview/<spec>-<id>`
  branch), and spawns an implementation agent through the shared
  `spawn_agent_core` (same worktree/JWT/compute-target/budget path as
  `POST /agents/spawn`). Returns `{repo_id, task_id, agent_id, branch,
  process_launched}`; a failed spawn rolls the preview task back (no
  orphaned Backlog work for the scheduler).
- `EditorSplit` polls the task with exactly the real wire vocabulary that
  `GET /api/v1/tasks/{id}` serializes (`task_status_str`:
  backlog/in_progress/review/done/blocked/cancelled): `review`/`done` =
  complete, `blocked`/`cancelled` = failed. It keeps the ghost canvas
  visible under the progress strip so ghosts pulse while the agent runs, and
  on completion resolves the agent's MR (`GET /merge-requests?repository_id=`,
  matched by task/agent) and renders its real diff
  (`GET /merge-requests/:id/diff` hunks) in the Code Diff tab; ghosts switch
  to solid/confirmed. A preview that fails, 404s, or times out returns to
  editing with an error banner — never a fabricated complete state.
- The ghost overlay clears when the spec is saved (and the saved draft is
  not re-predicted until the next edit).

### Test evidence

- `web/src/__tests__/ghostOverlay.test.js` (26 tests) — action mapping,
  opacity anchors and clamping, normalization (both wire shapes,
  idempotency, drops), positioning heuristics, ghost-edge derivation.
- `web/src/__tests__/GhostOverlay.test.js` (16 tests) — §3 visual language
  per action (colors, dotted strokes, strikethrough), confidence opacity,
  tooltip show/hide/focus with reason + changes, pulse/solid modes,
  non-navigation.
- `web/src/__tests__/EditorSplit.test.js` (62 tests) — §3 describe block:
  impact bar segments, debounced single predict call after an edit pause,
  Predicting… indicator lifecycle, ghost rendering from a prediction
  response (colors/edge/tooltip), ghost clearing on save without
  re-prediction, pulse→solid transition across a thorough preview,
  completion MR diff rendering, and the thorough-tier failure path
  (`blocked`/`cancelled` → error banner, editor unlocked, no MR fetch).
  Every thorough-preview mock uses the real task-status vocabulary.
- Full frontend suite after the F3 repair: 124 files, 2487 passed, 41
  skipped (locked deps via `npm ci`).
- Mutation probes on the repaired tests: regressing the completion
  condition to the fictional `'completed'` fails 3 tests; regressing the
  failure condition to the fictional `'failed'` fails 5 tests. Both real-
  vocabulary branches are now load-bearing for the suite.
- Server: `cargo check -p gyre-server --tests` clean at the F1/F2 repair
  round and again after the F4 repair; the mechanical guards pass (ABAC
  route registry, architecture lint, mem port contracts, inert enforcement,
  in-memory state stores).

### Deferred to the independent verifier (server test binaries)

Large gyre-server test compilation is verifier-owned per the assignment
budget. Execute:

- `cargo test -p gyre-server --lib api::graph::tests::predict_graph_spec_edit_returns_ghost_contract_and_link_context`
- `cargo test -p gyre-server --lib api::graph::tests::thorough_preview_creates_task_and_spawns_agent_on_throwaway_branch`
- `cargo test -p gyre-server --lib api::graph::tests::thorough_preview_rejects_missing_fields_and_unknown_repo`
- `cargo test -p gyre-server --lib api::graph::tests::thorough_preview_rolls_back_preview_task_when_spawn_fails`
- `cargo test -p gyre-server --lib api::graph::tests::predict_graph` (pre-existing predict regressions)
- `cargo test -p gyre-server --lib api::spawn` (spawn-core regressions — the thorough preview delegates to it)
- `cargo test -p gyre-server --lib api::users::tests::get_me_returns_recorded_auth_provider_info` (the F4 flake repair)

### Repair rounds

- **F1/F2 (review 74d337d3, commit 44310fa1)** — server test-assertion
  defects in `graph.rs`; production unchanged. F1: the spec-stem sanitizer
  maps `.` to `-`, so the throwaway branch is `preview/search-md-<id8>` and
  the test now expects that prefix. F2: axum 0.7.9 rejects a `Json<T>` body
  missing a required field with 422 before the handler runs; the
  missing-`spec_path` case now asserts 422 (in-handler empty-field
  validations still return 400). Ten unformatted `graph.rs` regions were
  fixed with `cargo fmt -p gyre-server`.
- **F3 (review 0b52dfad, commit 2e6b190d)** — the thorough-tier
  completion/failure detection was proven only through wire statuses the
  server never emits. Production `pollPreviewTask` now matches the real
  `task_status_str` vocabulary exactly (`review`/`done` complete,
  `blocked`/`cancelled` fail; the fictional `'completed'`/`'failed'` and
  their false "wire compatibility" comment are gone), every thorough-
  preview test mocks the real vocabulary (`in_progress` while running,
  `review`/`done` for completion), and a new failure-path test covers
  `blocked` and `cancelled`. Mutation probes confirm both branches are
  load-bearing (see above).
- **F4 (CI run 44025941598, commit a88f1a6d)** — the candidate failed
  `api::users::tests::get_me_returns_recorded_auth_provider_info` with
  `last_login_at` off by exactly one second (1794894780 vs 1794894779).
  Root cause: the test asserted strict equality between the
  `last_login_at` read at `GET /users/me` and the value read back after a
  `PUT /users/me`, but the PUT re-authenticates through `validate_jwt`,
  whose provisioning path re-stamps `last_login_at` on each auth (user
  spec: "set on each authentication") — so the values differ whenever the
  two requests straddle a wall-clock second boundary. Test-only defect;
  production unchanged. The assertion is now a two-sided wall-clock bound
  (GET value ≤ PUT value ≤ time sampled after the PUT response) and the
  PUT body spoofs `oidc_issuer`/`last_login_at` to keep the
  anti-forgery guarantee load-bearing: a client-set value fails the bound
  from either side, a cleared value fails the `expect`. The repair was
  type-checked with `cargo check -p gyre-server --tests` (clean); the
  test binary itself is deferred to the verifier (see list above).
  Evidence: `/tmp/stage/review-evidence/finding-F4-ci-flake-repair.md`.
