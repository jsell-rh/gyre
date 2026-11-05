---
title: "View Query Grammar — Interaction Context, Composition Examples & Integration"
spec_ref: "view-query-grammar.md §10–16"
depends_on:
  - task-063
progress: complete
coverage_sections:
  - "view-query-grammar.md §10 Interaction Context"
  - "view-query-grammar.md §11 Composition Examples"
  - "view-query-grammar.md §12 Blast radius (interactive)"
  - "view-query-grammar.md §13 Test coverage gaps"
  - "view-query-grammar.md §14 High-risk untested code"
  - "view-query-grammar.md §15 Authentication concept"
  - "view-query-grammar.md §16 Relationship to Other Specs"
commits: ["3a61cb9619705ea94cc192e6e9461074f99d57df", "e9c6e76b2b42e6333cc1152ac05f814ef81fb09e", "3b1c93fe6f3498f4d5ab67a1cd0e48fd564dd916", "8f65014884ec0a5173d695f1d4610c7e35a1c65e"]
---

## Spec Excerpt

**§10 Interaction Context:** When the LLM is invoked, it receives the current canvas state:
```json
{
  "canvas_state": {
    "selected_node": { "id": "uuid", "name": "TaskPort", "node_type": "interface" },
    "zoom_level": 2.3,
    "visible_tree_groups": ["gyre-server", "gyre-domain"],
    "active_filter": "calls",
    "active_lens": "structural",
    "active_query": { "type": "blast_radius", "node": "TaskPort" }
  },
  "recent_interactions": [
    { "action": "click", "node": "TaskPort::find_by_id" },
    { "action": "zoom_in", "to_level": 2.3 },
    { "action": "query", "preset": "blast_radius" }
  ]
}
```

**§11–15 Composition Examples:** Four canonical view queries that exercise the full grammar:
1. **Blast radius (interactive)**: `focus` scope with `$clicked`, tiered_colors, incoming calls+implements, `"fit"` zoom
2. **Test coverage gaps**: `test_gaps` scope, red highlight, 0.3 dim
3. **High-risk untested code**: `filter` scope with `$intersect($where(complexity, '>', 20), $test_unreachable)`, heat map
4. **Authentication concept**: `concept` scope with seed nodes, blue highlight, `"fit"` zoom

**§16 Relationship:** Extends `ui-layout.md` §4. Implements `system-explorer.md` §4 (Concept Views), §8 (Conversational Exploration). Consumed by `explorer-canvas.md` (renderer). Produced by LLM endpoints, preset buttons, user-composed JSON.

## Implementation Plan

1. **Canvas state serialization**: Verify that `ExplorerChat.svelte` sends the `canvas_state` object with all fields from §10 (`selected_node`, `zoom_level`, `visible_tree_groups`, `active_filter`, `active_lens`, `active_query`) and `recent_interactions` array when sending messages over WebSocket.

2. **Composition example integration tests**: Write integration tests that:
   - Construct each of the 4 canonical view queries from §11–15
   - Resolve them against a synthetic knowledge graph
   - Verify the result set is correct (node counts, specific nodes included/excluded)
   - Verify emphasis is applied correctly
   - Verify annotation templates resolve

3. **Blast radius preset**: Verify the "Blast Radius (click)" system default view uses `$clicked` interactive binding correctly. Each click should re-run the BFS traversal.

4. **Test coverage gaps preset**: Verify `test_gaps` scope correctly identifies nodes not reachable from test functions via `Calls` edges.

5. **High-risk untested code**: Verify `$intersect` + `$where` + `$test_unreachable` compose correctly.

6. **Concept view**: Verify `concept` scope with `seed_nodes` and `expand_edges` works.

## Acceptance Criteria

- [ ] Canvas state includes all §10 fields when sent to the agent
- [ ] `recent_interactions` array tracks click, zoom, and query actions
- [ ] Integration test: blast radius query resolves correctly against synthetic graph
- [ ] Integration test: test coverage gaps query identifies unreachable functions
- [ ] Integration test: high-risk untested code query intersects complexity + test gaps
- [ ] Integration test: authentication concept query expands from seed nodes
- [ ] `cargo test --all` passes
- [ ] `cd web && npm test` passes

## Agent Instructions

Read `specs/system/view-query-grammar.md` §10–16. This task is primarily about verifying the end-to-end integration of the grammar primitives built in task-062 and task-063.

Focus areas:
1. Check `web/src/lib/ExplorerChat.svelte` for canvas_state serialization when sending WebSocket messages
2. Check `web/src/components/ExplorerView.svelte` for how `explorerCanvasState` is constructed and passed to chat
3. Write Rust integration tests that construct each composition example query, resolve it against a test graph, and assert correct results
4. Check that the system default views (in `saved_views.rs` or seeded in migrations) match the §11–15 query shapes

## Shipped

Verified the task-062/063 grammar primitives end-to-end, closed the §10 wiring defects between the frontend canvas and the agent, and added the integration evidence for the canonical composition examples. Three review rounds found and fixed real defects; all repairs are in the history below.

### §10 Interaction Context — full shape, fresh, end to end

- **Wire format**: `ExplorerChat.svelte` previously sent the raw internal canvas state (camelCase `selectedNode`), which the server's serde `CanvasState` silently dropped. `web/src/lib/canvas-state.js` `toWireCanvasState()` now normalizes to the exact §10 shape (selected_node, zoom_level, visible_tree_groups, active_filter, active_lens, active_query, recent_interactions) at the single send site.
- **Freshness** (round-4 repair): `zoom_level` and `visible_tree_groups` used to be snapshotted only by `syncCanvasState()` on lens toggles, so both went stale afterwards — the agent could receive `zoom_level: 1.0` while `recent_interactions` correctly reported `zoom_in to_level: 3.0`. Now `toWireCanvasState()` prefers the live `zoom` (kept fresh by the canvas zoom-sync effect; snake-only normalized inputs still pass through), `syncCanvasState()` no longer snapshots those fields, and a reactive effect on `rootLayoutNodes` refreshes `visible_tree_groups`/`visibleGroups` whenever the root tree groups change (data reloads, layout-mode switches, drill navigation), guarded to write only on change.
- **`active_query`** is synced from `activeViewQuery` via an ExplorerView `$effect`; **query actions** record as `{action: "query", preset|detail}` for saved-view loads, LLM-generated views, user-composed JSON, and interactive `$clicked` re-runs.
- **Zoom actions** record as `{action: "zoom_in"|"zoom_out", to_level}` for user-initiated zoom only. `ZoomInteractionTracker` (canvas-state.js) owns the armed/disarmed state machine: wheel/keyboard/pinch handlers arm it; every programmatic zoom source (initial auto-fit and data-change refit, drill transitions, breadcrumb navigation, query-driven `"fit"`/level zoom, search-result fits, zoom-to-node) calls `disarm()`, which re-baselines without recording. Coalescing is gesture-scoped, so a pre-disarm entry stays frozen at the level the user actually reached instead of being rewritten by programmatic zoom (the round-2 review finding). Rust side: `InteractionRecord` carries `to_level`/`preset`.
- **LLM boundary** (round-3 repair): the `<canvas_state>` block built for the LLM previously dropped `zoom_level` and `active_filter` even though the server deserialized them. The block is now built by `build_canvas_context_json()` (explorer_ws.rs) and surfaces both fields; the system prompt documents them. Regression tests assert the exact §10 JSON survives deserialization AND reaches the LLM context.

### §11–15 Composition examples — 8 integration tests

`crates/gyre-domain/tests/view_query_composition.rs`: each canonical query is the byte-exact JSON from the spec, deserialized into `ViewQuery`, validated, and resolved via `dry_run` against synthetic graphs. Assertions cover result-set membership AND exclusions, tiered BFS depths (§12), heat metrics (§14), edge restriction to result nodes, and annotation template resolution (`$name`, `{{count}}`). Includes interactive-mode semantics: the same `$clicked` query re-resolved from a different selected node produces that node's blast radius, and with no selection it warns "click a node". Concept-scope tests cover bidirectional per-seed expansion (union, per-seed minimum depths), disconnected-subgraph exclusion, and partial-seed warnings.

### System default views

- "Blast Radius (click)" (§12) and "Test Coverage Gaps" (§13) matched the spec shapes — verified by resolution tests.
- Added "High-Risk Untested Code" (exact §14 shape: `$intersect($where(complexity, '>', 20), $test_unreachable)` + complexity heat) to `system_default_views()`, seeded per-repo and per-workspace.
- No system default for §15 (Authentication concept): seed nodes are repo-specific, so a universal preset is meaningless; the concept scope is reachable via LLM queries, concept search, and user-composed JSON (all tested).

### Committed dist

Each round that changed `web/src` rebuilt the committed `web/dist` in the same source, so `SKIP_WEB_BUILD=1` deployments serve the fixed SPA. The current bundle contains the §10 wire fields, the `ZoomInteractionTracker` logic, and the round-4 freshness fix (live-zoom preference and the reactive visible-tree-groups effect verified as minified symbols in the bundle).

### Test evidence (re-verified this round; logs in /tmp/stage/review-evidence/task-064-round6)

Round-6 verification repair: the controller `checks.sh` run failed only on the changed-lines rustfmt check for `explorer_ws.rs` (lines 2715–2716, 4004–4034); every other gate in that run passed (clippy changed-lines clean, all lint scripts OK, web build succeeded with pre-existing warnings only). Fixed with `rustfmt --edition 2021` on that file — whitespace-only reflow, no semantic change; pre-existing formatting drift in files outside this task's diff was left untouched. Evidence in `task-064-round6/log.txt`.

- Rust: `cargo test -p gyre-server --lib -- explorer_ws::` — 42 passed, 0 failed (includes the LLM-boundary §10 tests: canvas-context JSON surfacing zoom_level/active_filter, system-prompt documentation, plus §10 deserialization).
- Rust: `cargo test -p gyre-server --test explorer_ws_integration` — 8 passed, 0 failed.
- Rust: `cargo test -p gyre-domain --test view_query_composition` — 8 passed, 0 failed.
- Web: `npx vitest run src/__tests__/canvas-state.test.js` — 21 passed, 0 failed (wire normalization with live-zoom preference and the round-4 staleness regression — wire zoom must agree with the interaction trail — zoom/query recording, coalescing, ZoomInteractionTracker cases).
- Web: `npx vitest run` on ExplorerCanvas/ExplorerCanvas-performance/ExplorerChat/ExplorerViewScope/ExplorerViewDrillUrl — 275 passed, 0 failed (component suites touching the changed canvas state plumbing).
- Node probe (`wire-staleness-probe.mjs`, real module): the round-4 review scenario — stale `zoom_level: 1.0` snapshot alongside live `zoom: 3.0` — serializes `zoom_level: 3` matching the trail's `to_level: 3.0`; PASS.
- Full workspace suites (`cargo test --all`, full web vitest, arch/Clippy/CI) are owned by deterministic verification.
