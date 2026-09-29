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
