---
task: task-064
spec_ref: view-query-grammar.md §10–16
candidate: f64953916c33b827f122ed8c54da447e6d0c3a11
base: 0cb71015861e41b082183b89e9a1924808ad114a
round: 6 (independent review)
verdict: approved
---

# Review — task-064 View Query Grammar §10–16

Independent review of the exact candidate against the task contract and
`specs/system/view-query-grammar.md` §10–16. Probe evidence and mutation-test
logs: `/tmp/stage/review-evidence/task-064-r6/`.

## What was verified (independently re-run, not trusted from labels)

**§10 Interaction Context.**
- Wire format: `ExplorerChat.svelte:501` is the single `canvas_state` send site and
  routes through `toWireCanvasState()` (`web/src/lib/canvas-state.js`), which emits
  the §10 snake_case shape (selected_node, zoom_level, visible_tree_groups,
  active_filter, active_lens, active_query, recent_interactions) and drops
  rendering-only camelCase fields. The server structs (`CanvasState`,
  `SelectedNodeInfo`, `InteractionRecord`) accept this shape; the exact §10 JSON
  deserialization is regression-tested (`test_canvas_state_section_10_full_deserialization`).
- Freshness: `ExplorerCanvas.svelte:4969` keeps `canvasState.zoom` live via a
  reactive effect and `:4975` refreshes `visible_tree_groups` from
  `rootLayoutNodes` (write-on-change guard); `toWireCanvasState` prefers the live
  zoom over any stale `zoom_level` snapshot. Verified in source AND in the
  committed `web/dist` bundle (minified code for both the live-zoom ternary and
  the guarded visible-tree-groups effect found in `index-CJm3c8Zx.js`, which
  `index.html` references; the superseded chunk is fully removed).
- recent_interactions: click (legacy string form, normalized server-side to
  `{action, node, detail}`), zoom (`ZoomInteractionTracker` armed/disarmed state
  machine — wheel/keyboard/pinch arm it; every programmatic camera source found
  in the diff calls `disarm()`), and query actions (`{action:'query', preset|detail}`
  for saved-view loads, LLM views, user-composed JSON, interactive re-runs).
  `InteractionRecord` carries `to_level`/`preset` with serde defaults.
- LLM boundary: `build_canvas_context_json()` (explorer_ws.rs) surfaces
  `zoom_level` and `active_filter` in addition to the previously-surfaced fields;
  the system prompt documents them.

**§11–15 Composition examples.** 8 integration tests in
`crates/gyre-domain/tests/view_query_composition.rs` use the byte-exact spec JSON
against synthetic graphs with concrete membership AND exclusion assertions
(disconnected subgraphs, Implements-does-not-confer-test-reachability, strict
`>` boundary at complexity 20), tiered BFS depths, heat metrics, edge
restriction, and `$name`/`{{count}}` annotation resolution; interactive `$clicked`
mode semantics (re-run from a different node; no-selection warning) and concept
per-seed minimum depths are covered.

**System default views.** §14 "High-Risk Untested Code" added to
`system_default_views()` with the exact spec shape; count assertions updated to 7;
shape and resolution tests added. §12/§13 defaults already matched.

**Probe results (all re-run this round):** domain composition 8/8; explorer_ws lib
38/38; saved_views+explorer_views lib 29/29; WS integration 8/8; gyre-common
111/111; web canvas-state 21/21; five touched component suites 275/275.

**Mutation probes (test-sensitivity proof):**
1. `$test_unreachable` disabled (empty set) in the resolver →
   `high_risk_untested_intersects_complexity_and_test_gaps` and
   `system_default_view_shapes_resolve` FAIL. The tests are not vacuous.
2. `zoom_level` dropped from `build_canvas_context_json` →
   `test_canvas_context_json_surfaces_section_10_fields` FAILS.
Both mutations were restored; tree verified clean and HEAD at the candidate SHA.

## Non-blocking notes

- **Workspace-level REST seeding is seed-once** (`seed_system_views_if_needed`
  early-returns when any system view exists), so the new §14 default will not be
  added to pre-existing workspaces through that path. This early-return is
  pre-existing base behavior (verified in 0cb71015), the WS repo-level seeding is
  per-name self-healing (explorer_ws.rs:1403), and the §14 query remains reachable
  via LLM queries, repo-level views, and user-composed JSON — so it does not gate
  this task. Worth a follow-up if workspace-list parity matters.
- Click interactions ride the wire as legacy strings (`click:Foo(function)`)
  normalized server-side into `{action, node, detail}` — semantically §10-conformant
  at the LLM boundary, though not the object form shown in the spec example.
- Cosmetic: `build_canvas_context_json` in explorer_ws.rs carries a copy-pasted
  doc-comment line ("Fallback when no LLM is configured…") that belongs to the
  function below it.

## Conclusion

All acceptance criteria are satisfied with real production behavior and tests
that demonstrably fail when that behavior is disabled. Approved.
