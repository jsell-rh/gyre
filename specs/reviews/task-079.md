# Review — task-079 (Explorer Unit Tests — View Query Resolver, Graph Summary & Saved Views)

Spec: `explorer-implementation.md` §30 (Testing / Unit Tests).
Comparison base: `ad417f8a5d34d8299065ea2f2d1585d47128b401` → candidate `dc56ab82f1a5338d315588b2f312e1500fc6dd4d`.

## Round 1

### Diff scope

Test-only delta (+750/−2): 476 added lines in the resolver's `mod tests`
(hunk starts at :5967, entirely inside the test module), 212 added lines in
`crates/gyre-server/tests/graph_integration.rs` (one new test), and the
task-079 shipped record. Zero deleted source lines, zero production-code
changes. The 3 task-068 tests (`test_graph_summary_call_graph_metrics`,
`test_is_call_graph_complete`, `test_dry_run_call_graph_incomplete_warning`)
are in the base, not this task's delta.

### Verified working

- **Merge losslessness (independently recomputed).** Parent test-fn counts:
  base 127, task-079 checkpoint 138, merge 141. `comm` of each parent against
  the merge is empty (nothing lost), `uniq -d` empty (no duplicates). The
141 = 124 shared + 14 task-079 additions + 3 task-068 additions — the shipped
claim ("141 unique tests, 0 lost, 0 duplicates") is exact.
- **Every new test cross-checked against the production path it exercises**
(Rust execution is transport-blocked in this sandbox — see below — so
verification was static, assertion-by-assertion):
  - Heat metrics: `count_incoming_calls` filters `EdgeType::Calls` only, so the
    fixture's `Contains` edge is excluded (hot=3, sink=1); `complexity` and
    `test_coverage` use `and_then(Option)` so missing-data nodes are excluded
    from `node_metrics`; `churn` always present; `test_fragility` counts
    distinct test nodes via per-test BFS over `TEST_REACHABILITY_EDGES =
    [Calls]` (shared=2, single=1, orphan=0 via `unwrap_or(&0)`); trace metrics
    (`span_duration` is a registered KNOWN_HEAT_METRIC) warn "requires OTLP
    trace data" and return `Some(0.0)` placeholders — never fabricated values.
  - `tiered_colors`: the resolver's pass-through contract is `node_depths`;
    `bfs_traverse_with_depths` seeds the start node at depth 0, and the canvas
    colors `tiered_colors[min(depth, len-1)]` from `queryMatchedWithDepth`
    (ExplorerCanvas.svelte:2188) — the test verifies that exact contract
    (clicked=0, direct=1, transitive=2) with the system-default blast-radius
    query shape.
  - Callouts: exact→prefix→substring precedence verified in
    `dry_run_with_call_graph`; prefix resolves silently, substring pushes the
    "resolved via substring match" precision warning.
  - Narrative: substring resolution, "Unresolved narrative steps" warning
    naming each missing node, and order-uniqueness rejection
    ("Duplicate narrative step order") in `ViewQuery::validate`.
  - `$selected` with no selection: interactive branch pushes the "click a
    node" hint and guards off the "Scope matched 0 nodes" double report;
    resolve produces no spurious "Focus node not found" for the empty
    resolved name; with a selection, focus BFS resolves and interactive
    scopes are excluded from the partial-match warning path.
  - Highlight: `is_valid_color` accepts hex (3/4/6/8), named CSS ("tomato" is
    in the list), and `rgb()` functions; invalid colors produce an error
    naming `highlight.matched`.
  - Groups: matching is gated on `result_set.contains(id)` — a pattern that
    also matches an out-of-scope node cannot pull it into the group.
  - Saved-views ownership (graph_integration.rs): `created_by` is
    `auth.agent_id` on create; update/delete guard `!is_admin &&
    created_by != auth.agent_id` → 403 "Only the view creator or an Admin can
    …"; the system-view guard fires first ("System default views cannot be
    deleted"); `ViewResponse` serializes `created_by`/`is_system`; the
    agent-registration + `auth_token` pattern matches the established
    `test_link_node_to_spec_requires_developer_role` fixture. Crucially the
    test includes the control case — agent B succeeds create/update/delete on
    its own view via the same endpoints — which kills the "403 for an
    unrelated reason" false-positive class.
  - Expected values in the new tests are hand-computed from fixture topology,
    not mirrored from implementation output; all referenced symbols, fields,
    and signatures exist (compile-level audit done via the sources).
- **Acceptance-criteria audit (base ∪ candidate):** scope types
  all/focus/test_gaps (base), emphasis highlight/heat/tiered_colors (new),
  group+callout edge cases (base + new), all dry-run warning conditions (base
  + new interactive/no-double-report), graph summary fixtures (base + task-068),
  BFS test-gap detection (base), saved-views CRUD complete (base + new
  ownership). The task instruction was to audit existing coverage and fill
  gaps only — no duplicate tests were added.
- **Mechanical gates:** rustfmt `--check` on the resolver exit 0;
  `check-self-confirming-tests.sh`, `check-mirrored-logic-tests.sh`,
  `check-assertionless-tests.sh`, `check-task-commit-attribution.sh` all pass;
  the 2 `check-conditional-test-guards.sh` hits are pre-existing in
  `web/src/__tests__/WorkspaceSettings.test.js`, untouched by this diff.

### Transport restriction (infrastructure, not a defect)

crates.io and static.crates.io are connection-refused (curl exit 7,
selective filter — registry.npmjs.org is reachable); CARGO_HOME does not
exist, no registry cache, no vendored sources, no target dir.
`cargo build -p gyre-domain --offline` fails at the first dependency. Rust
test execution is therefore deferred to remote verification with concrete
checks: `cargo test -p gyre-domain --lib view_query_resolver` (expect 141
pass) and `cargo test -p gyre-server --test graph_integration
test_saved_views` (expect all pass), plus the full `cargo test --all` gate.
The diff touches no web code, so the web suite is unchanged by this
candidate; the full `cd web && npm test` gate remains a verification-stage
gate.

### Verdict (round 1)

**approved** — the candidate is a genuine, lossless, test-only delivery for
§30: every new test asserts specific, hand-computed expected values against
real production behavior (verified line-by-line against the resolver and
saved-views handler), the merge preserves both parents' suites exactly, and
no production code, exemptions, or unrelated files changed. Execution
evidence is deferred to remote verification under the documented transport
restriction; the static audit found no hollowness, no mirrored logic, and no
assertionless or conditionally-skipped tests in the delta.
