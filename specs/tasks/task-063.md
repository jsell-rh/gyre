---
title: "View Query Grammar — Scope Resolution, Emphasis & Rendering Primitives"
spec_ref: "view-query-grammar.md §4–9"
depends_on:
  - task-062
progress: ready-for-review
coverage_sections:
  - "view-query-grammar.md §4 2. Scope — What Subgraph to Show"
  - "view-query-grammar.md §5 3. Emphasis — How to Color It"
  - "view-query-grammar.md §6 4. Edges — What Relationships to Show"
  - "view-query-grammar.md §7 5. Zoom"
  - "view-query-grammar.md §8 6. Annotation"
  - "view-query-grammar.md §9 7. Interactive Bindings"
commits: ["322b2973909bc9a556453a7d90b120f885bf58ff", "08f616e0e4d84fd91280fd3437cdeb253a4c5d4a", "ff416a1ff75435442b6192b81731b599b026e784", "ac5f41f6f609089f36492a37fbc2f1634d58bbf0", "7f5f8278c035e1006fcb0dc74d5ed83af6614a36", "3c8b331eba71ff231f076767885948db731bab49"]
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

- [ ] All 6 scope types resolve correctly with unit tests
- [ ] Emphasis primitives render correctly in the canvas (highlight, dim, tiered_colors, heat, badges)
- [ ] Edge filtering restricts to result-set connections when a scope is active
- [ ] Zoom `"fit"` computes bounding box and animates to fit
- [ ] Annotation templates resolve `$name`, `{{count}}`, `{{group_count}}`
- [ ] Interactive `$clicked` mode re-runs scope on each click
- [ ] `cargo test --all` passes
- [ ] `cd web && npm test` passes

## Agent Instructions

Read `specs/system/view-query-grammar.md` §4–9. Then audit existing implementations:
- `crates/gyre-common/src/view_query.rs` — Rust types for scope variants
- `crates/gyre-server/src/explorer_ws.rs` — server-side scope resolution (look for `resolve_scope`, `resolve_groups`, etc.)
- `web/src/lib/ExplorerCanvas.svelte` — frontend rendering of emphasis, zoom, annotation
- `web/src/lib/view-query-validator.js` — validation logic

The scope resolver is the core deliverable. It takes a `Scope` enum variant + the graph (nodes, edges) and returns a `HashSet<Id>` of matched nodes. This must handle computed references via the resolver from task-062. The frontend rendering likely already works — verify and fix gaps.

## Shipped

Recovered-interrupted-assignment continuation (repair id `423d57a0`, candidate `14088b68`).
The implementation is unchanged from the R4-approved state — no task-063 production code
required changes; this round verified the recovered tree end-to-end:

- **Tree integrity**: `web/src/lib/ExplorerCanvas.svelte` and
  `web/src/__tests__/ExplorerCanvas.test.js` are byte-identical to the R4-verified commit
  `656c1281` (diff 0 lines). All 5 frontmatter SHAs resolve. F1/F2/F3 repairs confirmed
  present: zero `last_commit_sha` in production source (3 occurrences in tests are
  explanatory comments), `type === 'all'` branch at ExplorerCanvas.svelte:1940.
- **Scope resolution, all 6 types**: Rust resolver `crates/gyre-domain/src/view_query_resolver.rs`
  (Diff :722–818, scope unit tests incl. 3 Diff tests) — `cargo test -p gyre-domain --lib
  view_query_resolver` → **116 passed, 0 failed** (gcc linker override; workspace mold/clang
  absent — environmental). Client resolution `ExplorerCanvas.svelte:1940–2076` covers
  all/focus/filter/test_gaps/concept/diff against the real `GraphNodeResponse` fields,
  mirroring server Diff semantics (`~epoch` half-open temporal, ≥7-char SHA prefix,
  from-exclusion).
- **Emphasis/edges/zoom/annotation/interactive bindings**: verified present at the
  R4-cited lines (dim :2117–2119, edge restriction :3575–3578, annotation :4992–5009,
  `$clicked` :4160–4171); coverage rows 4–9 spot-checked against the current tree —
  accurate, no drift.
- **Focused web suite**: after `npm ci` (locked deps, rc=0), `npx vitest run
  src/__tests__/ExplorerCanvas.test.js` → **139 passed, 0 failed**.
- **Dist policy**: task branches ship no dist rebuilds (whitespace gate finding
  `68edd07c`); `git diff 6bf777a6..HEAD -- web/dist/` is empty and
  `git diff --check 6bf777a6..HEAD` → rc=0. CI builds from source.
- **Attribution**: `scripts/check-task-commit-attribution.sh` → OK rc=0 on this tree;
  no exemption entries added.
- **Transport restriction** (unchanged): TCP listener unsupported in this sandbox
  (`capabilities.json` errno 95); no server/browser probe attempted — host verification
  and GitHub CI own transport checks.

Evidence: `/tmp/stage/review-evidence/task-063-r9/`.

### Round 10 (rebase repair `60487c84`)

Resolved the auto-merge conflict from rebasing onto base `73a31e0b`. The conflict in
`specs/tasks/task-200.md` was a single SHA in its `commits:` list: the HEAD side (from
recovery checkpoint `322b2973`) carried a malformed 38-char SHA; the incoming base carried
the valid 40-char `e44f11354629cf2dab7fd7846c8a0a0a1d2d9591` (resolves to `e44f1135`).
Resolved to the valid SHA; merge committed as `22c614f8`. The merge brought in only
`specs/tasks/` files (task-200, task-213, task-219) — the task-063 product surface is
byte-identical to the R4-verified candidate `14088b68` (empty diff on `crates/` +
`web/src/`). No task-063 production code changed this round; all 6 task-063 frontmatter
SHAs and all task-200 SHAs resolve post-merge. Focused suites re-run on the merged tree:
`cargo test -p gyre-domain --lib view_query_resolver` → 116 passed, 0 failed; `npm ci`
(rc=0) then `npx vitest run src/__tests__/ExplorerCanvas.test.js` → 139 passed, 0 failed;
attribution check rc=0; `git diff --check 6bf777a6..HEAD` rc=0; dist diff empty.

Evidence: `/tmp/stage/review-evidence/task-063-r10/`.
