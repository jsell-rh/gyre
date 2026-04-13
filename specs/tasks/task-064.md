---
title: "View Query Grammar — Interaction Context, Composition Examples & Integration"
spec_ref: "view-query-grammar.md §10–16"
depends_on:
  - task-063
progress: not-started
coverage_sections:
  - "view-query-grammar.md §10 Interaction Context"
  - "view-query-grammar.md §11 Composition Examples"
  - "view-query-grammar.md §12 Blast radius (interactive)"
  - "view-query-grammar.md §13 Test coverage gaps"
  - "view-query-grammar.md §14 High-risk untested code"
  - "view-query-grammar.md §15 Authentication concept"
  - "view-query-grammar.md §16 Relationship to Other Specs"
commits: []
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
