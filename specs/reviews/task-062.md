# Review: task-062 — View Query Grammar Core Types & Computed Reference Resolver

Spec: `view-query-grammar.md` §1–3. Commit: `ce4845cc` (task-file only; implementation predates it in `d7940e85`).

## Round 1

Scope verified:
- All 17 §3 computed references parse, validate (`validate_computed_expression`), and resolve
  (`resolve_computed_expression_inner`, `crates/gyre-domain/src/view_query_resolver.rs`).
  Edge-direction semantics match spec: `$callers`=incoming Calls BFS, `$callees`=outgoing,
  `$implementors`=incoming Implements sources, `$fields`=incoming FieldOf sources,
  `$descendants`=outgoing Contains, `$ancestors`=incoming Contains, set ops recurse correctly.
- Rust `ViewQuery`/`Scope`/`Emphasis` cover all seven §2 primitives; TypeScript
  `web/src/lib/types/view-query.ts` mirrors them field-for-field (Scope union, Emphasis,
  EdgeFilter, Zoom, Annotation).
- `cargo test -p gyre-domain view_query_resolver`: **115 passed, 0 failed** (verified with
  gcc/mold linker override; workspace `.cargo/config.toml` pins `clang` which is absent in this
  environment — environmental, not a code defect). Frontend `npm test` not runnable here
  (`vitest` not installed in node_modules) — could not verify the 53 validator tests.

Findings:

- [x] **F1 — Dead code: `compute_test_fragility_count` never called.**
  `crates/gyre-domain/src/view_query_resolver.rs:443-470` defines a private
  `compute_test_fragility_count(node_id, ...)` marked `#[allow(dead_code)]` with rationale
  "Kept for potential use in per-node metric queries." It is referenced nowhere in `crates/`
  or `web/` (grep confirms only the definition). Its single-node fragility computation is fully
  superseded by `compute_all_test_fragility` (the batch version used at line 1528 and in
  `$where(test_fragility, ...)`). Speculative dead code retained behind an `#[allow]` is a
  defect under the dead-code flaw class and violates clean-cutover (remove obsolete code).
  Fix: delete `compute_test_fragility_count` and its `#[allow(dead_code)]` attribute.

## Round 2

- [x] **F1 resolved.** Deleted `compute_test_fragility_count` (private, `#[allow(dead_code)]`)
  from `crates/gyre-domain/src/view_query_resolver.rs`. Its single-node computation was
  fully superseded by `compute_all_test_fragility` (used at the `$where(test_fragility, ...)`
  and `$test_fragility(...)` sites). `bfs_traverse` remains referenced by
  `compute_all_test_fragility`, so no cascading dead code. `cargo test -p gyre-domain
  view_query_resolver`: 115 passed, 0 failed. `cargo build -p gyre-domain`: clean, zero warnings.

## Round 3

F1 confirmed resolved: `compute_test_fragility_count` is gone; grep across `crates/`
finds no residual references; `bfs_traverse` remains live via `compute_all_test_fragility`.
`cargo test -p gyre-domain --lib view_query_resolver`: 115 passed, 0 failed (clang+mold
available in this environment). New spec-fidelity finding below.

- [x] **F2 — `$test_reachable`/`$test_unreachable`/`$test_fragility` traverse Implements+RoutesTo, contradicting spec's "via Calls".**
  Spec `view-query-grammar.md` §3 defines these references with an explicit edge qualifier:
  `$test_reachable — nodes reachable from any test function **via Calls**`,
  `$test_unreachable — complement`, and `$test_fragility(node) — count of distinct **test paths**
  reaching this node`. The §2 `test_gaps` scope is likewise "Nodes NOT reachable from any test
  function". The implementation defines
  `const TEST_REACHABILITY_EDGES: &[EdgeType] = &[EdgeType::Calls, EdgeType::Implements, EdgeType::RoutesTo]`
  (`crates/gyre-domain/src/view_query_resolver.rs:399-400`) and uses it in
  `compute_test_reachable` (line 405) and `compute_all_test_fragility` (line 441). These power
  `$test_reachable` (line 994), `$test_unreachable` (lines 973-991), the `test_gaps` scope, and
  `$test_fragility` (line 1494) plus `$where(test_fragility, ...)` (line 1041) and the dry_run
  metric population (lines 2109, 2170). A node reachable from a test only via an `Implements` or
  `RoutesTo` edge (not `Calls`) is classified as test-reachable and is excluded from test-coverage
  gaps — the opposite of what the spec specifies. The code carries a rationale comment
  (trait dispatch / HTTP endpoint tests), but that is a reinterpretation of the spec, not an
  amendment. Per the goal's rule 4 ("Never implement around a spec by reinterpreting it"): either
  restrict the traversal to `EdgeType::Calls` to match §3, or amend `view-query-grammar.md` §3 via
  spec lifecycle to define test reachability over the three-edge set (and update the §2 `test_gaps`
  description accordingly). Add a test that exercises an Implements/RoutesTo-only path so the chosen
  semantics are pinned.

## Round 4

Task was flipped back to `ready-for-review` without addressing F2 — R3 raised F2 but
never updated the task frontmatter's `progress` field to `needs-revision` (the R3 commit
`9ca7ea3d` touched only the review file). No code commit or spec amendment landed after
R3: `git log` shows the last resolver commit is the F1 fix (`e482bbf3`/`30a46847`), and the
working tree is clean.

F2 remains OPEN and unchanged:
- `crates/gyre-domain/src/view_query_resolver.rs:399-400` still defines
  `TEST_REACHABILITY_EDGES = &[EdgeType::Calls, EdgeType::Implements, EdgeType::RoutesTo]`,
  consumed at lines 429 (`compute_test_reachable`) and 455 (`compute_all_test_fragility`).
- `specs/system/view-query-grammar.md` §3 line 31 still reads
  `$test_reachable — nodes reachable from any test function **via Calls**` (no amendment).

The three-edge traversal continues to contradict the spec's single-edge (`Calls`) definition
for `$test_reachable`/`$test_unreachable`/`$test_fragility` and the §2 `test_gaps` scope. A
node reachable from a test only via `Implements`/`RoutesTo` is still misclassified as
test-reachable and wrongly excluded from coverage gaps. Resolution requires EITHER restricting
`TEST_REACHABILITY_EDGES` to `&[EdgeType::Calls]` to match §3, OR amending §3 (and the §2
`test_gaps` row) via spec lifecycle to define test reachability over the three-edge set — plus
a test that pins an Implements/RoutesTo-only path so the chosen semantics are enforced.
Setting `progress: needs-revision`.

## Round 5

F2 resolved by conforming to the spec (rule 4: follow the spec, do not reinterpret).
`TEST_REACHABILITY_EDGES` restricted to `&[EdgeType::Calls]` in
`crates/gyre-domain/src/view_query_resolver.rs`; the doc comment now cites
`view-query-grammar.md` §3 "via Calls" and explains the exclusion of Implements/RoutesTo/Contains.
This single constant powers `compute_test_reachable` and `compute_all_test_fragility`, so
`$test_reachable`, `$test_unreachable`, `$test_fragility`, `$where(test_fragility, ...)`,
the `test_gaps` scope, and dry-run metric population all now use Calls-only reachability.
New test `test_reachability_is_calls_only_not_implements_or_routes_to` pins the semantics:
a node reachable from a test only via `Implements` and one only via `RoutesTo` are both
classified test-unreachable (present in `$test_unreachable`, absent from `$test_reachable`)
and accrue zero `$test_fragility` count, while the Calls-reached node is reachable with
fragility 1. `cargo test -p gyre-domain --lib view_query_resolver`: 116 passed, 0 failed.

## Round 6

F2 verified resolved end-to-end on the Rust side: `TEST_REACHABILITY_EDGES` is
`&[EdgeType::Calls]` (`view_query_resolver.rs:400`) with a doc comment citing §3 "via
Calls", consumed by `compute_test_reachable` and `compute_all_test_fragility`, pinning
test `test_reachability_is_calls_only_not_implements_or_routes_to` present, and no
stale three-edge references remain anywhere in Rust. Re-ran
`cargo test -p gyre-domain --lib view_query_resolver`: 116 passed, 0 failed.

- [-] [process-revision-complete] **F3 — F2 fix not applied exhaustively: frontend still resolves §3 computed references over the old three-edge set, with comments now falsely claiming backend parity.**

  The F2 fix commit `e828ff39` touched only `crates/gyre-domain/src/view_query_resolver.rs`
  plus the review/task files — no `web/` files (verified: all four task-062 commits
  `ce4845cc`/`30a46847`/`e482bbf3`/`e828ff39` show zero `web/` paths in `--stat`).
  `web/src/lib/ExplorerCanvas.svelte` contains a parallel frontend implementation of the
  §3 computed references, and it still uses the pre-F2 three-edge traversal:

  - `ExplorerCanvas.svelte:1709` — `FRONTEND_TEST_EDGES = new Set(['calls', 'implements',
    'routes_to'])` under the comment "Edge types traversed for test reachability — matches
    backend TEST_REACHABILITY_EDGES", which is now **false**: backend is Calls-only.
  - `ExplorerCanvas.svelte:2031` — the `test_gaps` scope block re-declares a local
    `TEST_REACHABILITY_EDGES = new Set(['calls', 'implements', 'routes_to'])` with the
    comment "Match backend TEST_REACHABILITY_EDGES: calls, implements, routes_to" —
    quoting the backend constant's old contents, now deleted backend-side.

  Affected frontend functions: `computeTestUnreachable()` (1712), `computeTestReachable()`
  (1731), `computeAllTestFragility()` (1846, `FRONTEND_TEST_EDGES` at 1862), and the inline
  `test_gaps` scope block (2026–2048). All traverse Implements+RoutesTo. These power the
  frontend resolution of `$test_reachable`/`$test_unreachable` (via `resolveComputed`,
  1933–1934), `$test_fragility`/`$where(test_fragility, ...)` (1962, 2205–2208), and the
  `test_gaps` scope (2026). A node reachable from a test only via `Implements` or
  `RoutesTo` renders as test-covered in the canvas but is a coverage gap in server-side
  dry_run — contradicting spec §3 "via Calls" (view-query-grammar.md line 31), §2
  `test_gaps` ("Nodes NOT reachable from any test function", line 46), and §3 line 17
  ("All computations are deterministic" — the same reference resolves differently in the
  two surfaces).

  Corroborating evidence that this is staleness, not intentional divergence: the frontend
  test helper `resolveQueryMatch` in `web/src/__tests__/ExplorerCanvas.test.js:348-373`
  already uses Calls-only (`et === 'calls'`, `nb.edgeType !== 'calls'`) for `test_gaps`,
  mirroring the post-F2 backend semantics, while the production `ExplorerCanvas.svelte`
  code does not.

  Filed under task-062 (not task-063): the frontend `resolveComputed` implements the §3
  computed references — task-062's exact coverage section ("view-query-grammar.md §3 1.
  Computed References", task frontmatter line 10) — and the false "matches backend"
  comments are a direct stale-reference consequence of the F2 fix commit. The `test_gaps`
  scope block specifically overlaps task-063's §4 territory, but its local edge set and
  comment duplicate the same stale backend claim, so it is included here.

  Fix shape: restrict both sets to `new Set(['calls'])`, update the comments to cite
  spec §3 "via Calls", and add a frontend test case mirroring the Rust pinning test (an
  Implements-only or RoutesTo-only node must be untested/gap, not covered). Note: no
  existing frontend test exercises the production `computeTest*` helpers (grep of
  `web/src/__tests__` for `computeTestUnreachable|computeTestReachable|computeAllTestFragility`
  returns nothing) — the frontend test mirror uses its own re-implementation, so the
  production code path is untested either way; the pinning test must call the real code
  path or re-home the logic so it is testable.

  Not a task-062 finding: `npm test` in `web/` currently reports 17 failing tests (3
  files: `ExplorerCanvas-performance`, `ExplorerViewAskViewSpec`, `FlowRenderer`,
  `MoldableViewNodeTypeFilter`), all in files untouched by any task-062 commit —
  pre-existing failures outside this task's scope.

Setting `progress: needs-revision` (F3 open).

## Round 7

[-] [process-revision-complete] **F3 resolved.** Frontend test-reachability
semantics extracted into `web/src/lib/test-reachability.js` and aligned with
spec §3 (commit `cbcbd6ab`, "refactor(web): extract test-reachability to module,
align frontend with spec §3"):

- `TEST_REACHABILITY_EDGES = new Set(['calls'])` with a doc comment citing
  §3 "via Calls" and explaining the exclusion of Implements/RoutesTo/Contains —
  now genuinely matching the backend constant
  (`view_query_resolver.rs:400`, `&[EdgeType::Calls]`).
- `ExplorerCanvas.svelte` no longer contains `FRONTEND_TEST_EDGES` or the local
  three-edge `TEST_REACHABILITY_EDGES` (grep: zero hits). It imports
  `buildAdjacency`, `computeTestReachable`, `computeTestUnreachable`,
  `computeTestFragilityCounts`, `computeTestGaps` from the shared module
  (imports at lines 11–17, adjacency at 1622, delegates at 1701–1707/1815,
  `test_gaps` scope at 1969–1973). ~100 lines of inline three-edge traversal
  removed.
- Pinning tests exercise the real production code path:
  `web/src/__tests__/test-reachability.test.js` (7 tests) imports the module
  consumed by ExplorerCanvas and mirrors the Rust pinning test — Implements-only
  and RoutesTo-only nodes are NOT test-reachable, appear in `computeTestGaps`,
  and accrue zero fragility; Calls chains (incl. transitive) are reachable;
  fragility counts distinct tests once each. All 7 pass
  (`npx vitest run src/__tests__/test-reachability.test.js`).

Cross-surface parity verified: `scripts/check-cross-surface-parity.sh` passes;
all JS hygiene checks pass (`check-mirrored-logic-tests-js.sh`,
`check-tautological-assertions-js.sh`, `check-phantom-test-apis-js.sh`, etc.).
`cargo test -p gyre-domain --lib view_query_resolver`: 116 passed, 0 failed.
Full `cd web && npm test`: 1492 passed, 17 failed — the same 17 pre-existing
failures documented in Round 6 (`ExplorerCanvas-performance` flake,
`ExplorerViewAskViewSpec`, `FlowRenderer`, `MoldableViewNodeTypeFilter`), all
in files untouched by task-062 commits. Remaining `routes_to` references in
ExplorerCanvas.svelte (lines 4297/4322) are blast-radius/focus-scope view
queries, unrelated to test reachability.

Setting `progress: ready-for-review` (no open findings).

## Round 8

F3 verified resolved — adversarially, including collateral damage from the fix commit
`cbcbd6ab` and cross-surface semantic parity beyond the edge set:

- `web/src/lib/test-reachability.js` is genuinely Calls-only
  (`TEST_REACHABILITY_EDGES = new Set(['calls'])`, doc comment cites §3), pure over
  (nodes, adjacency), and consumed by ExplorerCanvas for `computeTestUnreachable`/
  `computeTestReachable`/`computeAllTestFragility` (via `computeTestFragilityCounts`) and
  the `test_gaps` scope. Grep of `ExplorerCanvas.svelte` for `FRONTEND_TEST_EDGES` and the
  three-edge `TEST_REACHABILITY_EDGES`: zero hits. The stale "matches backend" comments are
  gone. `computeTestFragility(nodeName)` correctly delegates to the cached
  `computeAllTestFragility()` and returns set-membership (count > 0), matching the backend
  `$test_fragility(...)` semantics at `view_query_resolver.rs:1485-1500`.
- Pinning tests exercise the real production module (not a re-implementation): 7/7 pass
  (`npx vitest run src/__tests__/test-reachability.test.js`).
- `cargo test -p gyre-domain --lib view_query_resolver`: 116 passed, 0 failed.
  `scripts/check-cross-surface-parity.sh`: passes.
- Full `cd web && npm test`: 1492 passed, 17 failed — exactly the R6 baseline
  (`MoldableViewNodeTypeFilter` ×5, `FlowRenderer` ×6, `ExplorerViewAskViewSpec` ×5,
  `ExplorerCanvas-performance` ×1), all in files untouched by task-062 commits. No new
  failures attributable to `cbcbd6ab`.

However, the R7 "all checks verified" sweep missed two defects in the code `cbcbd6ab`
actually touched. Findings:

- [x] **F4 — `cbcbd6ab` deleted `filterEdge`'s `'dependencies'` case: unexplained
  out-of-scope deletion inconsistent with its surviving `filterOpacity` counterpart.**

  The F3 fix's stated scope was extracting test-reachability semantics (commit message:
  "extract test-reachability to module, align frontend with spec §3"). But the diff also
  removed, from `web/src/lib/ExplorerCanvas.svelte`, the line

  `case 'dependencies': return et === 'depends_on' || et === 'calls';`

  from `filterEdge` (function at :1611) — collateral damage from a hunk whose real target
  was the adjacency builder directly below it. Current state:

  - `filterEdge` (`:1611-1619`) has no `'dependencies'` case → falls to
    `default: return true`, so under `filter='dependencies'` **all** edges render,
    including `contains`/`governed_by`/`renders`/etc., instead of only
    `depends_on`/`calls`.
  - `filterOpacity` (`:1596-1608`) **still has** `case 'dependencies': return 0.1;`
    (:1606) — the node-dimming counterpart to a filter whose edge case no longer exists.
    Pre-fix (`git show cbcbd6ab^`), both functions handled `'dependencies'`.

  Severity context (why this is filed rather than noted): `'dependencies'` is currently
  unreachable as a filter value — `explorerFilter` in `ExplorerView.svelte:48` is
  `$state('all')` with no setter found anywhere in `web/` (toolbar presets were removed
  per the comment at `ExplorerCanvas.svelte:5010`), and MoldableView instantiates
  ExplorerCanvas without a `filter` prop. So no user-visible behavior changes today. But
  this is exactly the fix-introduced-regression flaw class: a fix commit deleted adjacent
  behavior it did not own, with no justification in the message, no test covering
  `filterEdge`, and leaving the codebase in an inconsistent state (one half of the
  `'dependencies'` filter survives) that will misfire the moment any caller sets the
  filter. Note `filterEdge` is a real rendering-path function — called from `drawEdges`
  at :3465 — not dead code.

  Fix: restore `case 'dependencies': return et === 'depends_on' || et === 'calls';` to
  `filterEdge` (or, if `'dependencies'` is confirmed dead as a filter value, delete the
  `filterOpacity` case too and document why — either way the two functions must agree).
  A regression test for `filterEdge`'s filter→edge-type mapping would prevent recurrence.

- [x] **F5 — Depth caps diverge between backend resolver and frontend module for
  `$test_reachable`/`$test_fragility`: same reference resolves differently per surface,
  violating determinism (§3 line 17).**

  Backend (`crates/gyre-domain/src/view_query_resolver.rs`):

  - `compute_test_reachable` caps BFS at depth 100 (`if depth > 100 { continue; }`, :421).
  - `compute_all_test_fragility` traverses with `bfs_traverse(..., 20, ...)` — depth cap
    20 (:453-460), used by both `$where(test_fragility, ...)` and `$test_fragility(node)`.

  Frontend (`web/src/lib/test-reachability.js`): `computeTestReachable` (:42-55) and
  `computeTestFragilityCounts` (:80-100) are unbounded BFS (visited-set prevents infinite
  loops on cycles, so no hang — but no depth cap).

  Consequence: on a Calls chain longer than 20 hops from any test (deep call stacks are
  exactly what fragility is for), the frontend reports fragility ≥ 1 while the backend
  dry-run reports 0 — `$where(test_fragility, '>', 0)` highlights a different node set in
  the canvas than in the server-resolved query result. Same class of cross-surface
  divergence F3 was filed under, one level deeper than the edge set: the module's header
  claims to implement §3, and §3 says "All computations are deterministic". Chains
  longer than 100 hops diverge for `$test_reachable`/`test_gaps` the same way.

  This is pre-existing on the node-iteration side (the deleted inline BFS was also
  unbounded) — but `cbcbd6ab` is the commit that consolidated these traversals into a
  module explicitly presented as the frontend counterpart of the backend resolver, so
  aligning the caps belonged in it.

  Fix: pass the caps through — e.g. `computeTestReachable(nodes, adjacency, maxDepth =
  100)` and `computeTestFragilityCounts(nodes, adjacency, maxDepth = 20)` mirroring the
  backend constants, with a comment cross-referencing `view_query_resolver.rs`; add a
  pinning test with a >20-hop chain (frontend count must be 0, matching backend).

  Not filed (checked and defused): the new module does not filter `n.deleted_at` /
  `e.deleted_at` where the backend does (`active_nodes` at resolver.rs:553,
  `build_adjacency` at :226). Both graph store adapters already exclude tombstoned
  rows server-side — SQLite `list_nodes`/`list_edges` filter `deleted_at IS NULL`
  (`gyre-adapters/src/sqlite/graph.rs:412/:475`), and `mem_graph.rs` does the same — so
  deleted nodes/edges never reach the canvas through `/repos/{id}/graph`, and the
  backend's own `deleted_at` filters are defense-in-depth for the resolver's internal
  callers. No observable divergence exists on any current data path; the module's
  parity claim ("matches backend TEST_REACHABILITY_EDGES") is scoped to the edge set and
  remains true.

Setting `progress: needs-revision` (F4, F5 open; F3 remains resolved).

## Round 9

F4 and F5 resolved by commit `decb0353` ("fix(web): resolve task-062 F4/F5 —
filterEdge dependencies case, backend depth caps"):

- **F4**: the filter→edge-type mapping is extracted to
  `web/src/lib/canvas-filters.js` (`edgePassesFilter`) with the restored
  `case 'dependencies': return et === 'depends_on' || et === 'calls';` —
  byte-faithful to the pre-`cbcbd6ab` mapping (verified against
  `git show cbcbd6ab^:web/src/lib/ExplorerCanvas.svelte`). `filterEdge`
  (`ExplorerCanvas.svelte:1614-1616`) delegates to it, and the module header
  documents that `filterOpacity`'s node-dimming cases must stay in agreement —
  the two functions now handle the same filter values. Regression test
  `web/src/__tests__/canvas-filters.test.js` pins every filter value's edge
  set, included and excluded types (the F4 contrast: `dependencies` must NOT
  pass `contains`/`governed_by`/`renders`).
- **F5**: `web/src/lib/test-reachability.js` exports
  `REACHABLE_MAX_DEPTH = 100` and `FRAGILITY_MAX_DEPTH = 20` with doc comments
  cross-referencing `view_query_resolver.rs:421` (compute_test_reachable depth
  cap) and `:457` (bfs_traverse(..., 20, ...)). `computeTestReachable`,
  `computeTestUnreachable`, `computeTestGaps`, and `computeTestFragilityCounts`
  all take `maxDepth` parameters defaulting to the backend-mirroring constants,
  so a reference resolves identically on both surfaces (§3 "All computations
  are deterministic"). Pinning tests in
  `web/src/__tests__/test-reachability.test.js`: a >20-hop Calls chain accrues
  zero fragility at the default cap and a >100-hop chain is unreachable and a
  coverage gap, both recovered by raising the cap.

Verified: `cargo test -p gyre-domain --lib view_query_resolver`: 116 passed,
0 failed. `npx vitest run src/__tests__/test-reachability.test.js
src/__tests__/canvas-filters.test.js`: 16 passed, 0 failed. Full
`cd web && npm test`: 1501 passed, 17 failed — the exact pre-existing R6/R8
baseline (5 `ExplorerViewAskViewSpec` + 6 `FlowRenderer` + 5
`MoldableViewNodeTypeFilter` + 1 `ExplorerCanvas-performance`), all in files
untouched by task-062 commits. `scripts/check-cross-surface-parity.sh` passes;
`check-arch`, `check-mirrored-logic-tests(-js)`, `check-dead-test-variables-js`,
`check-conditional-test-guards`, `check-tautological-assertions-js`,
`check-phantom-test-apis-js`, `check-dead-test-code`,
`check-comparative-test-claims-js`, `check-assertionless-tests`,
`check-aspirational-test-names`, `check-self-confirming-tests`,
`check-self-contradicting-test-comments-js`, `check-stale-mechanism-claims-js`
all pass.

Process note: the original fix commit was recorded in the task frontmatter as
`c08e3011`, a commit parented on the pre-merge base that became unreachable
after rebasing onto `main` (`9df46795`). The identical fix landed as
`decb0353` (web files byte-identical, verified by `git diff c08e3011
decb0353 -- web/` = empty), and the frontmatter `commits` list now points at
the reachable SHA, following the task-092 precedent
(`e1e23df9`/`79e216bb`).

Correction (pre-merge): the R9 replacement SHAs (`4a16b621`, `23a59fa1`,
`14e153ab`, `77389ee6`) are themselves unreachable from `worker/task-062`
HEAD — the branch carries byte-equivalent duplicates (`2bf5eb20`, `2d0cff8f`,
`7364e938`, `a7303e58`; verified: `git diff` per pair is empty for `web/` and
`crates/`, differing only in an unrelated task-077 `ui-navigation.md` coverage
edit present on the branch side). The frontmatter `commits` list now points at
the branch-ancestral SHAs. Fourth recorded instance of the SHA-drift flaw
class on this task; the underlying cause is committing doc updates against a
detached/pre-merge base instead of the worker branch tip.

Setting `progress: ready-for-review` (no open findings).

## Round 10

Adversarial re-verification of the F4/F5 resolutions plus a fix-class-exhaustion
sweep over the remaining frontend computed-reference code.

- **F4 (re-verified)**: `web/src/lib/canvas-filters.js` `edgePassesFilter`
  carries the restored `dependencies` case (`depends_on` || `calls`);
  `ExplorerCanvas.svelte:1614-1616` `filterEdge` delegates to it;
  `filterOpacity` (:1597-1610) keeps the aligned node-dimming cases. The 7-test
  regression suite `canvas-filters.test.js` pins every filter's included and
  excluded edge types.
- **F5 (re-verified, boundary-level)**: backend `compute_test_reachable`
  (resolver.rs:405-436) inserts at depth ≤100 (`if depth > 100 { continue; }`
  before insert); frontend `computeTestReachable` (test-reachability.js:53-68)
  uses `depth > maxDepth` with `REACHABLE_MAX_DEPTH = 100` — identical
  boundary, node at exactly 100 reachable on both. Backend
  `compute_all_test_fragility` (rs:441-466) traverses via
  `bfs_traverse(..., 20, ...)`, where `bfs_traverse_with_depths` (rs:257-307)
  inserts the start at 0 and expands only while `d < depth` — nodes at depth
  ≤20 included, and the test node's own `visited` seed means it counts toward
  its own fragility. Frontend `computeTestFragilityCounts` (js:96-117) seeds
  `reached = new Set([tn.id])` and gates expansion on
  `depth >= maxDepth` (`FRAGILITY_MAX_DEPTH = 20`) — identical boundary and
  identical self-inclusion. Pinning tests at
  `test-reachability.test.js:147-182`: 21-hop chain → zero fragility at cap 20
  (node at exactly 20 still counted); 101-hop chain → unreachable at cap 100
  (node at exactly 100 reachable); raising caps recovers the deep nodes.
- **Wiring**: `ExplorerCanvas.svelte:11-17` imports `buildAdjacency`,
  `computeTestFragilityCounts`, `computeTestGaps`, and the reachability pair
  from `./test-reachability.js`; delegation at :1697-1703, fragility cache at
  :1805-1814, `test_gaps` scope at :1965-1969. `TESTABLE_TYPES` frontend set
  matches the backend `$test_unreachable` NodeType filter (rs:978-988);
  reachable-set non-filtering matches the backend on both surfaces.
- **`$test_fragility(node)` contract (triaged, not a finding)**: frontend
  `computeTestFragility` (:1816-1823) returns set membership (`count > 0` →
  node id), byte-matching the backend contract at rs:1485-1503, which likewise
  returns a membership set and populates real counts only into dry-run
  `node_metrics` for `$where(test_fragility, ...)`.
- **Pre-existing divergences (checked, defused — predate task-062)**: frontend
  `computeWhere` (:1705-1736) resolves raw node properties with a string
  fallback where the backend (rs:998-1088) special-cases `node_type` /
  `visibility` / `spec_confidence` and computes graph-derived metrics
  (`incoming_calls`, `test_fragility`, `risk_score`) via traversal; frontend
  `computeGovernedBy` (:1674-1692) matches name/spec_path without the
  backend's (rs:1299-1371) directory-boundary/exact-path rules or
  spec-node-inclusion; frontend set-op operand fallbacks
  (`a ?? b ?? new Set()` at :1910-1927) vs backend strict empty-set on
  unparseable operands. All three blocks originate in `d7940e85` (the
  pre-task-062 explorer-canvas commit) and are untouched by every task-062
  commit (verified: `git show <commit> -- ExplorerCanvas.svelte` matches none
  of these symbols outside diff context). Per the R1–R9 scope precedent
  (findings filed only where task-062 commits introduced or touched the
  divergent code, e.g. F4/F5 vs the untouched-by-F5 parts in R9's "not filed"
  notes), these are task-063 territory (frontend §3 parity), not task-062
  regressions. The spec's own `$where` example (complexity > 20, a serialized
  GraphNode field) resolves identically on both surfaces.

Verified: `cargo test -p gyre-domain --lib view_query_resolver`: 116 passed,
0 failed. `npx vitest run src/__tests__/test-reachability.test.js
src/__tests__/canvas-filters.test.js`: 16 passed, 0 failed.

No open findings. Setting `progress: complete`.
