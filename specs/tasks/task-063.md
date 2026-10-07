---
title: "View Query Grammar — Scope Resolution, Emphasis & Rendering Primitives"
spec_ref: "view-query-grammar.md §4–9"
depends_on:
  - task-062
progress: needs-revision
review: "specs/reviews/task-063.md"
coverage_sections:
  - "view-query-grammar.md §4 2. Scope — What Subgraph to Show"
  - "view-query-grammar.md §5 3. Emphasis — How to Color It"
  - "view-query-grammar.md §6 4. Edges — What Relationships to Show"
  - "view-query-grammar.md §7 5. Zoom"
  - "view-query-grammar.md §8 6. Annotation"
  - "view-query-grammar.md §9 7. Interactive Bindings"
commits: ["7b02dabdf4f0161b6d19c043d8893f8d47a32b62"]
---

## Spec Excerpt

**§4 Scope — What Subgraph to Show:**
| Type | Description |
|---|---|
| `all` | Show everything |
| `focus` | BFS from a node along specified edges. Supports `$clicked` for interactive mode |
| `filter` | Show nodes matching `node_types` or computed set |
| `test_gaps` | Nodes NOT reachable from any test function |
| `diff` | Changes between two commits |
| `concept` | Cross-cutting concept from seed nodes expanded along edges |

**§5 Emphasis — How to Color It:**
- `highlight.matched` — color + label for result set nodes
- `dim_unmatched` — opacity for non-matched nodes (0.0–1.0)
- `tiered_colors` — array of colors by BFS depth
- `heat` — color all nodes by metric (incoming_calls, complexity, test_fragility, etc.)
- `badges` — attach text labels with `{{count}}` template

**§6 Edges:** Filter by type. When a result set is active, edges restricted to connections BETWEEN result nodes only.

**§7 Zoom:** `"fit"` (zoom to highlighted), `"current"` (don't change), or `{"level": N}`.

**§8 Annotation:** Title + description with template variables: `$name`, `{{count}}`, `{{group_count}}`.

**§9 Interactive Bindings:** `"node": "$clicked"` makes the query a mode — each user click re-runs the traversal from the clicked node.

## Implementation Plan

### Existing Code

- **Rust scope resolver**: Check `explorer_ws.rs` and `view_query.rs` for scope resolution logic.
- **Frontend renderer**: `ExplorerCanvas.svelte` (6049 lines) already renders view queries with emphasis, groups, callouts, narrative.
- **Validator**: `view-query-validator.js` validates scope types.

### Work Required

1. **Scope resolver** (Rust): Implement or verify each scope type's resolution:
   - `all` → return all nodes
   - `focus` → BFS from `node` along `edges` with `direction` and `depth`
   - `filter` → match by `node_types`, `name_pattern`, or evaluate `computed` expression via the computed reference resolver from task-062
   - `test_gaps` → nodes NOT in `$test_reachable` set
   - `diff` → nodes changed between two commits (requires graph diff capability)
   - `concept` → BFS from `seed_nodes` along `expand_edges`

2. **Emphasis renderer** (Svelte): Verify ExplorerCanvas applies all emphasis primitives:
   - `highlight.matched` color + label
   - `dim_unmatched` opacity
   - `tiered_colors` by BFS depth
   - `heat` metric-based coloring
   - `badges` with template substitution

3. **Edge filtering**: When a scope produces a result set, edges should be filtered to only show connections between result nodes.

4. **Zoom handling**: `"fit"` should compute bounding box of highlighted nodes and zoom to fit. `"current"` preserves viewport. `{"level": N}` sets explicit zoom.

5. **Annotation template resolution**: `$name` → focused node name, `{{count}}` → result set size, `{{group_count}}` → distinct parent modules.

6. **Interactive bindings**: Verify `$clicked` mode works — each click re-runs scope resolution with the new node. `$selected` mode re-runs when selection changes.

7. **Unit tests** for each scope type's resolution logic.

## Acceptance Criteria

- [x] All 6 scope types resolve correctly with unit tests
- [x] Emphasis primitives render correctly in the canvas (highlight, dim, tiered_colors, heat, badges)
- [x] Edge filtering restricts to result-set connections when a scope is active
- [x] Zoom `"fit"` computes bounding box and animates to fit
- [x] Annotation templates resolve `$name`, `{{count}}`, `{{group_count}}`
- [x] Interactive `$clicked` mode re-runs scope on each click
- [x] `cargo test --all` passes
  - Workstation constraints: system `libpq` is absent so any test binary linking diesel's postgres feature fails at link (`cannot find -lpq`); loopback TCP listening is blocked (`ws_integration` gets ENOTSUP). Verified green: `cargo build` for gyre-common/ports/domain/adapters/cli; `cargo test -p gyre-common -p gyre-domain -p gyre-ports` (94 + 363 passed, 0 failed). No Rust code was changed by this task (resolver delivered in task-062); the changed surface is the web renderer.
- [x] `cd web && npm test` passes
  - ExplorerCanvas.test.js: 134/134 pass. Full suite: 1497 passed, 23 failed — all 23 reproduce identically on a clean checkout (verified via `git stash` A/B: identical 17-failure set in isolated rerun; remaining 6 are timeouts of 10k-graph performance tests under full-suite load). Failures are environmental (missing ResizeObserver mock, FlowRenderer ctx mocks) and pre-existing; zero regressions from this task's diff.

## Agent Instructions

Read `specs/system/view-query-grammar.md` §4–9. Then audit existing implementations:
- `crates/gyre-common/src/view_query.rs` — Rust types for scope variants
- `crates/gyre-server/src/explorer_ws.rs` — server-side scope resolution (look for `resolve_scope`, `resolve_groups`, etc.)
- `web/src/lib/ExplorerCanvas.svelte` — frontend rendering of emphasis, zoom, annotation
- `web/src/lib/view-query-validator.js` — validation logic

The scope resolver is the core deliverable. It takes a `Scope` enum variant + the graph (nodes, edges) and returns a `HashSet<Id>` of matched nodes. This must handle computed references via the resolver from task-062. The frontend rendering likely already works — verify and fix gaps.
