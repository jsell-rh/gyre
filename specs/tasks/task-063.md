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
commits: ["08f616e0e4d84fd91280fd3437cdeb253a4c5d4a", "ff416a1ff75435442b6192b81731b599b026e784", "ac5f41f6f609089f36492a37fbc2f1634d58bbf0", "7f5f8278c035e1006fcb0dc74d5ed83af6614a36", "3c8b331eba71ff231f076767885948db731bab49"]
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

Recovered-interrupted-assignment continuation. The implementation itself was complete and
independently reviewed through four rounds (`specs/reviews/task-063.md` R1–R4; R4 set
`progress: complete` on the F1/F2/F3 repairs). This round re-verified the recovered tree and
re-ran the focused gates on it:

- **Scope resolution, all 6 types**: Rust resolver `crates/gyre-domain/src/view_query_resolver.rs`
  (Diff :722–818; scope unit tests incl. 3 Diff tests) — `cargo test -p gyre-domain --lib
  view_query_resolver` → **116 passed, 0 failed** (gcc linker override; workspace mold/clang
  absent — environmental). Client resolution `web/src/lib/ExplorerCanvas.svelte:1940–2076`
  covers all/focus/filter/test_gaps/concept/diff, reads the real `GraphNodeResponse` fields
  (`created_sha`/`last_modified_sha`/`created_at`/`last_modified_at`) and mirrors server Diff
  semantics (`~epoch` half-open temporal, ≥7-char SHA prefix, from-exclusion).
- **Emphasis/edges/zoom/annotation/interactive bindings**: verified present in the working
  tree at the R4-cited lines; `web/src/lib/ExplorerCanvas.svelte` and
  `web/src/__tests__/ExplorerCanvas.test.js` are byte-identical to the R4-verified commit
  `656c1281` (`git diff 656c1281..HEAD -- <files>` is empty; the web/src delta since then is
  other tasks' no-sidebar/WorkspaceHome work, which does not touch these files).
- **Mutation probe re-run on this tree**: deleting the `all`-scope branch fails its targeted
  test ("all scope: {{count}} resolves to total node count"); source restored clean after
  the probe (evidence: `/tmp/stage/review-evidence/task-063-r5/`).
- **Focused suites**: ExplorerCanvas.test.js → **139 passed, 0 failed**. Full `npm test`:
  1533 passed, 8 failed, 41 skipped — all 8 failures reproduce **worse or equal on the clean
  base commit `8c2d1775`** (ghost-overlay timeouts 2 vs 1; ExplorerCanvas-performance
  timeouts 10 vs 5, in an isolated worktree with locked deps), i.e. pre-existing
  sandbox-load flakes in files this task never touched, not regressions.
- **Dist policy**: `web/dist` was restored to the base state by round 6 (`18811787`) after
  the recovered checkpoint's committed rebuild failed the whitespace gate — the bundle's
  minified vendor svelte-i18n whitespace-char class ends a line in a literal tab/newline
  byte, so any diff-introduced rebuild trips `git diff --check` (rc=2, verification finding
  `68edd07c`; the base's own `index-fzyK9GaC.js` carries the same pattern but no
  diff-introduced lines). Task branches ship no dist rebuilds; CI builds from source
  (`web/src` keeps the R4-verified fixes). On this HEAD `git diff 6bf777a6..HEAD --
  web/dist/` is empty and `git diff --check 6bf777a6..HEAD` → rc=0.
- **Attribution repair (this round's only source-tree change)**: `a781ede2`
  (`feat(task-210): Repair verified failure on main cd1c5f044e49`, product surface:
  `crates/gyre-server/src/api/admin.rs`, `web/src/*`, landed via base merge) was missing
  from `specs/tasks/task-210.md` `commits:` frontmatter, failing
  `scripts/check-task-commit-attribution.sh` on main itself. Recorded the full SHA in
  task-210's frontmatter — the script's prescribed repair, mirroring the identical recording
  already shipped on sibling pipeline branches (e.g. `f4fae4e2`, task-095). No exemption
  entries were added; check now passes on this tree.

No task-063 production code required changes this round — the recovered candidate was
already the R4-approved implementation; the deliverables are the re-verification evidence
above and the attribution-gate repair.

## Shipped (round 7 — recovered-checkpoint continuation, attribution repair)

Continuation of the interrupted `b36df580` assignment on merged tree `65bd78c0` (base
`6bf777a6`). No task-063 production code changed this round; the implementation remains
byte-identical to the R4-verified state (`git diff 656c1281..HEAD -- web/src/lib/
ExplorerCanvas.svelte web/src/__tests__/ExplorerCanvas.test.js web/src/lib/
view-query-validator.js crates/gyre-domain/src/view_query_resolver.rs` is empty).
Evidence: `/tmp/stage/review-evidence/task-063-r7/`.

- **Whitespace gate**: `git diff --check 6bf777a6..HEAD` → rc=0 (round-6 repair `18811787`
  still holds; dist delta vs base is empty).
- **Attribution repair (this round's only other-file change)**: merge base `6bf777a6`
  (`feat(task-200)`, product surface `crates/gyre-common/src/message.rs`,
  `crates/gyre-server/src/api/messages.rs`, `crates/gyre-server/src/mcp.rs`) is main's own
  HEAD and was missing from `specs/tasks/task-200.md` `commits:` frontmatter — same
  drift class as rounds 5/6 (`a781ede2`/task-210, `f4acb4eb`/task-189): a ship commit
  cannot contain its own future SHA, so the recording is necessarily a follow-up and none
  landed on main. Recorded the full SHA in task-200's frontmatter; the gate was failing on
  this tree before (rc=1) and passes after (rc=0). No exemption entries added.
- **Record repair**: the previous interrupted checkpoint deleted the round-6 ledger but
  left the round-5 "Dist freshness" bullet claiming committed `index-DJOFnUtw.js` — false
  on any post-`18811787` tree. Replaced with the dist-policy bullet above so the record
  matches the tree.
- **Focused suites on this HEAD**: `cargo test -p gyre-domain --lib view_query_resolver`
  → 116 passed / 0 failed; ExplorerCanvas.test.js (`npm ci` with locked deps first) →
  139 passed / 0 failed.
- **Transport restriction** (unchanged): TCP listener unsupported in this sandbox
  (`capabilities.json`: `Operation not supported`, errno 95); no server/browser probe
  attempted — host verification and GitHub CI own the transport checks.
