# Review: task-064 — View Query Grammar: Interaction Context, Composition Examples & Integration

Spec: `view-query-grammar.md` §10–16 (task rows 10–16 of `specs/coverage/system/view-query-grammar.md`).
Candidate: `e40d9ec31266ae3f3ab02590ffeb43e16152d1cc` (task commits `3b1c93fe`, `8f650148`),
base `0cb71015861e41b082183b89e9a1924808ad114a`. Round 4 (this file); rounds 1–3 recorded in the
task history — both prior durable findings (LLM-boundary drop of `zoom_level`/`active_filter`;
zoom-arming never disarming) were re-verified as fixed this round.

## Independently verified (evidence: /tmp/stage/review-evidence/task-064-round4/)

- **Prior finding 1 fixed (LLM boundary)**: `build_canvas_context_json()` (explorer_ws.rs) now
  surfaces `zoom_level` (omitted when 0/unset) and `active_filter` in the `<canvas_state>` block;
  system prompt documents both. Unit tests `test_canvas_context_json_surfaces_section_10_fields`
  and `test_system_prompt_documents_section_10_fields` pin it. `cargo test -p gyre-server --lib --
  explorer_ws:: saved_views::` → 51 passed, 0 failed; `--test explorer_ws_integration` → 8 passed.
- **Prior finding 2 fixed (zoom arming)**: `ZoomInteractionTracker` (canvas-state.js) with
  arm/disarm semantics; every programmatic camera source in ExplorerCanvas.svelte (auto-fit,
  data-change refit, drill, tree-group zoom, breadcrumb, query fit/level, search fit,
  zoom-to-node) calls `disarmZoomTracking()`; wheel/keyboard/pinch call `armZoomTracking()`.
  Independent real-module probe (`zoom-tracker-probe.mjs`, 3 scenarios: round-2 scenario,
  re-arm semantics, mid-gesture disarm) — all pass: programmatic fit after user zoom 1.0→1.5
  leaves the entry at `to_level 1.5`; re-armed significant zoom starts a NEW entry without
  rewriting the frozen one; small nudges record nothing.
- **§11–15 composition tests are real**: `crates/gyre-domain/tests/view_query_composition.rs`
  uses the byte-exact spec JSON per example, asserts membership AND exclusions, BFS depths,
  heat metrics, matched-edge restriction, annotation template resolution, `$clicked` mode
  re-resolution, and partial-seed warnings. `cargo test -p gyre-domain --test
  view_query_composition` → 8 passed, 0 failed.
- **System default views**: "High-Risk Untested Code" added with the exact §14 shape
  (`saved_views.rs`), shape-pinned by `test_high_risk_untested_uses_intersect_where_test_unreachable`
  and resolution-tested. `api::explorer_views` tests updated to 7 defaults → 16 passed.
  Omitting a §15 system default (repo-specific seeds) is reasonable and documented.
- **Wire format + web tests**: `toWireCanvasState()` normalizes to the §10 snake_case shape at
  the single send site; `npx vitest run src/__tests__/canvas-state.test.js` → 20 passed.
  Committed `web/dist` rebuilt from the fixed source (toWireCanvasState/ZoomInteractionTracker/
  visible_tree_groups/allowCoalesce symbols present in the bundle).
- **Rust types**: `InteractionRecord` extended with `to_level`/`preset`
  (gyre-common view_query.rs, custom Deserialize paths both covered).

## Round-4 finding (blocks approval)

- **§10 `zoom_level`/`visible_tree_groups` staleness** (`web/src/lib/canvas-state.js` +
  `web/src/lib/ExplorerCanvas.svelte`): the only writer of `canvasState.zoom_level` and
  `canvasState.visible_tree_groups` is `syncCanvasState()` (ExplorerCanvas.svelte:775-790),
  invoked solely from `onLensChange()` (:765-769). The zoom `$effect` (:4969-4973) keeps only
  `canvasState.zoom` fresh, and `toWireCanvasState()` prefers a numeric `zoom_level` over the
  live `zoom`. Consequence: after one lens toggle, the agent forever receives the zoom level
  snapshotted at toggle time — directly contradicting the (correct) `recent_interactions`
  zoom entries (LLM told `zoom_level: 1` while the user is at 3.0 and the trail records
  `zoom_in to_level 3.0`). `visible_tree_groups` is similarly frozen after tree-group
  expand/collapse. Probe: `wire-staleness-probe.mjs` (real module) — DEFECT reproduced.
  Fix: prefer the live `zoom` in `toWireCanvasState` (or drop the `zoom_level` snapshot from
  `syncCanvasState` / refresh `zoom_level` in the `$effect`), and refresh
  `visible_tree_groups` when groups change. Note `active_filter` is additionally limited by
  `explorerFilter` being dead state in ExplorerView (never reassigned from `'all'`) — pre-existing
  at base and truthful while no filter UI exists, but worth noting for §10 fidelity.

Everything else in the task contract is satisfied with real implementations and hard tests; the
single finding above is a narrow, easily-fixed defect in the assigned §10 section.
