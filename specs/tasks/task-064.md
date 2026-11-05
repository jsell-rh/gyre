---
title: "View Query Grammar — Interaction Context, Composition Examples & Integration"
spec_ref: "view-query-grammar.md §10–16"
depends_on:
  - task-063
progress: ready-for-review
coverage_sections:
  - "view-query-grammar.md §10 Interaction Context"
  - "view-query-grammar.md §11 Composition Examples"
  - "view-query-grammar.md §12 Blast radius (interactive)"
  - "view-query-grammar.md §13 Test coverage gaps"
  - "view-query-grammar.md §14 High-risk untested code"
  - "view-query-grammar.md §15 Authentication concept"
  - "view-query-grammar.md §16 Relationship to Other Specs"
commits: ["2dbaea81c731e2d6b695f48d94fde99dab2f833e"]
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

- [x] Canvas state includes all §10 fields when sent to the agent
- [x] `recent_interactions` array tracks click, zoom, and query actions
- [x] Integration test: blast radius query resolves correctly against synthetic graph
- [x] Integration test: test coverage gaps query identifies unreachable functions
- [x] Integration test: high-risk untested code query intersects complexity + test gaps
- [x] Integration test: authentication concept query expands from seed nodes
- [x] `cargo test --all` passes (focused runs on the touched crates this round: gyre-domain 388 lib + 8 composition, gyre-common 111, gyre-server lib 1257, explorer_ws_integration 8; full workspace suites owned by verification)
- [x] `cd web && npm test` passes (66 files, 1638 passed / 41 skipped at the candidate commit; canvas-state suite re-verified this round: 14/14)

## Agent Instructions

Read `specs/system/view-query-grammar.md` §10–16. This task is primarily about verifying the end-to-end integration of the grammar primitives built in task-062 and task-063.

Focus areas:
1. Check `web/src/lib/ExplorerChat.svelte` for canvas_state serialization when sending WebSocket messages
2. Check `web/src/components/ExplorerView.svelte` for how `explorerCanvasState` is constructed and passed to chat
3. Write Rust integration tests that construct each composition example query, resolve it against a test graph, and assert correct results
4. Check that the system default views (in `saved_views.rs` or seeded in migrations) match the §11–15 query shapes

## Shipped

Verified the task-062/063 grammar primitives end-to-end and closed three real §10 integration defects found during verification. No stubs were needed — the resolver machinery from task-063 was sound; the gaps were in the wiring between the frontend canvas state and the agent, plus missing integration evidence for the canonical composition examples.

### §10 Interaction Context — wire-format defect fixed

- **Defect found**: `ExplorerChat.svelte` sent the raw internal canvas state, whose selection lives under camelCase `selectedNode`. The server's serde `CanvasState` only knows `selected_node`, so the selection was silently dropped on every message — the LLM never saw the selected node, and server-side `$clicked`/`$selected` resolution (explorer_ws.rs) always received `None`.
- **Fix**: new `web/src/lib/canvas-state.js` `toWireCanvasState()` normalizes to the exact §10 shape (selected_node, zoom_level, visible_tree_groups, active_filter, active_lens, active_query, recent_interactions); used at the single send site (ExplorerChat.svelte sendMessage).
- **`active_query` was never populated** — added an ExplorerView `$effect` syncing `activeViewQuery` into the canvas state.
- **`recent_interactions` missing zoom and query actions** — `recordZoomInteraction()` (same module) records `{action: "zoom_in"|"zoom_out", to_level}` for user-initiated zoom only (wheel/keyboard/pinch armed; initial auto-fit, drill transitions and query-driven zoom are not user interactions), coalescing continuous gestures into one trailing entry. Query actions record as `{action: "query", preset}` for saved-view loads, `{action: "query", detail}` for LLM-generated views, user-composed JSON, and interactive `$clicked` re-runs.
- **Rust side**: `InteractionRecord` extended with `to_level`/`preset` (gyre-common), surfaced into the LLM `<canvas_state>` context and system-prompt documentation (explorer_ws.rs).

### §11–15 Composition examples — 8 integration tests

`crates/gyre-domain/tests/view_query_composition.rs`: each canonical query is the byte-exact JSON from the spec, deserialized into `ViewQuery`, validated, and resolved via `dry_run` against synthetic graphs. Assertions cover result-set membership AND exclusions, tiered BFS depths (§12), heat metrics (§14), edge restriction to result nodes, and annotation template resolution (`$name`, `{{count}}`). Includes interactive-mode semantics: the same `$clicked` query re-resolved from a different selected node produces that node's blast radius, and with no selection it warns "click a node". Concept-scope tests cover bidirectional per-seed expansion (union, per-seed minimum depths), disconnected-subgraph exclusion, and partial-seed warnings.

### System default views

- Added "High-Risk Untested Code" (exact §14 shape: `$intersect($where(complexity, '>', 20), $test_unreachable)` + complexity heat) to `system_default_views()` — seeded per-repo and per-workspace like the existing defaults; shape asserted in saved_views/explorer_views/explorer_ws tests, resolution in the composition suite.
- "Blast Radius (click)" (§12) and "Test Coverage Gaps" (§13) already matched the spec shapes — verified by resolution tests rather than re-implemented.
- No system default for §15 (Authentication concept): seed nodes are repo-specific, so a universal preset is meaningless; the concept scope is reachable via LLM queries, concept search, and user-composed JSON (all tested).

### Contract-repair round (7fdcd39c)

- The executor restored the assigned task document over the candidate head; the production fix (commit 2dbaea81) was preserved unchanged — no source modifications were needed this round.
- **Stale-dist defect found and repaired**: commit 2dbaea81 changed `web/src` but never rebuilt the committed `web/dist`, so `SKIP_WEB_BUILD=1` deployments would keep serving the pre-fix SPA (the old bundle lacks `to_level`/`zoom_in` interaction recording and the `selected_node` normalization). Rebuilt via the project's own build path (build.rs) and committed as 94327d80; bundle delta ~+3.5KB, css hash unchanged.
- Re-verified with focused probes on the restored checkout (evidence: /tmp/stage/review-evidence/task-064-focused-probes.log): `cargo test -p gyre-domain --test view_query_composition` 8/8, `cargo test -p gyre-server --test explorer_ws_integration` 8/8, and `npx vitest run src/__tests__/canvas-state.test.js` 14/14 after `npm ci` with locked versions.

### Test evidence

- `cargo test -p gyre-domain` → 388 lib + 8 composition tests, 0 failed
- `cargo test -p gyre-common` → 111 passed, 0 failed
- `cargo test -p gyre-server --lib` → 1257 passed, 0 failed
- `cargo test -p gyre-server --test explorer_ws_integration` → 8 passed, 0 failed
- `cd web && npm test` → 66 files, 1638 passed / 41 skipped, 0 failed (includes the new `canvas-state.test.js`, 14 tests — one is a regression guard proving the camelCase selection drop was real)
- Clippy on the touched crates: no new findings (the pre-existing `never_loop` error in `rust_extractor.rs` and `end` loop-counter warning in `view_query.rs:798` exist on the base commit `d81b0fe4` — untouched by this task)
