# Review — task-079 (Explorer Unit Tests — View Query Resolver, Graph Summary & Saved Views)

Spec: `specs/coverage/system/explorer-implementation.md` §30 "Unit Tests" (spec §Testing: view query resolver scope/emphasis/groups/callouts/narrative, dry-run warnings, graph summary, test-coverage gap detection, saved views CRUD).
Commits under review: `4058b618` (recovered interrupted-run source) + `e4dd7a62` (rustfmt repair of finding 1a62c878), candidate `ae3fcb06`, base `1d9547de`.
Verdict: **approved** (round 1).

## Scope

Diff is test-only: +476 lines inside `mod tests` of `crates/gyre-domain/src/view_query_resolver.rs` (14 new unit tests), +212 lines in `crates/gyre-server/tests/graph_integration.rs` (`test_saved_views_ownership_enforcement`), and task-079.md shipped notes. Zero production code and zero web code changed.

## Evidence (logs under /tmp/stage/review-evidence/)

- `cargo test -p gyre-domain --lib view_query_resolver` → **138 passed, 0 failed**; full `--lib` → **414 passed, 0 failed**; `--test view_query_composition` → **8 passed, 0 failed**. Note: the shipped notes claimed these were not executable in the implementer's sandbox (crates.io refused); in this review sandbox the executor's cargo wrapper supplied the closure and every suite ran for real — the independent run confirms the claimed counts.
- `cargo test -p gyre-server --test graph_integration test_saved_views` → **2 passed, 0 failed** (CRUD + new ownership test).
- **Mutation probes (all killed, each reverted immediately, tree verified clean after):**
  1. `count_incoming_calls` counting all edge types instead of `Calls` only → `test_heat_emphasis_incoming_calls_values` FAILED.
  2. Group resolution dropping the `result_set.contains(id)` restriction → `test_group_resolution_restricted_to_result_set` FAILED ("group must not include out-of-scope nodes").
  3. Empty-scope handling losing the `$clicked`/`$selected` interactive exemption → `test_selected_no_selection_warns_interactive_not_empty` FAILED.
  4. `delete_view` ownership check disabled → ownership test FAILED at "non-creator must not delete another agent's view".
  5. `update_view` ownership check disabled → FAILED at "non-creator must not update another agent's view".
  The tests are not self-confirming, mirrored, or stub-backed: every asserted behavior is computed by real production code and breaks loudly when that code is disabled.
- Static cross-checks: heat metric exclusions (non-`Calls` edges, missing complexity/coverage data), callout exact→prefix→substring resolution with precision warning, narrative unresolved-name warnings and duplicate-order rejection in `ViewQuery::validate`, `highlight.matched` color validation, group-scope restriction, interactive empty-scope hint — all verified line-by-line against the resolver and `gyre-common/src/view_query.rs`. Ownership test verified against `saved_views.rs`: 403 messages ("Only the view creator or an Admin…", "System default views cannot be deleted"), system-check-before-ownership ordering, `created_by = auth.agent_id`, 201/200/204 status codes; agent tokens resolve to `UserRole::Agent` (auth.rs) so both callers are genuine non-Admin non-creators, and the test's own-view control proves the 403s are ownership-specific rather than general denial. `RegisterAgentResponse`'s `#[serde(flatten)]` matches the test's top-level field reads.
- Audit of "already present" claims: scope types all/filter/focus(incoming+BFS)/test_gaps/concept/diff, dry-run warnings (empty/cluttered/broad-group/many-nodes/invalid-computed), graph summary (counts, top types by fields, top functions by calls, test-coverage counts), test-gap BFS (`test_computed_test_unreachable` ×3, `test_coverage_gaps_identifies_unreachable_functions`), saved views CRUD + system-default seeding — all exist and pass. No duplicate tests were added for covered behavior.
- Gates: `check-rustfmt-diff.py 1d9547de` → clean at the candidate (durable finding 1a62c878 from 112e98af repaired in e4dd7a62); `check-conditional-test-guards.sh` passed. `check-mirrored-logic-tests.sh` / `check-self-confirming-tests.sh` / `check-assertionless-tests.sh` exit 2 identically at the base commit in this sandbox — pre-existing infra issue, not candidate-related; the direct awk detection on the changed file is clean and the mutation probes supersede the heuristics.
- Web spot check (no web code in diff): `npm ci` + `vitest run ExplorerCanvas.test.js ExplorerViewScope.test.js` → **155 passed, 0 failed**, matching the shipped notes.

## Notes (non-blocking)

- `test_tiered_colors_emphasis_populates_node_depths`: `node_depths` is populated by focus-scope resolution regardless of the `tiered_colors` setting, so the emphasis field in that test is decorative. The tiered_colors acceptance criterion is nevertheless genuinely covered: `view_query_composition.rs`'s blast-radius tests assert node depths for a tiered_colors query end-to-end, and `ViewQuery::validate` covers tiered_colors format. No action required.
- The shipped notes' "not executable in this sandbox" cargo claims were conservative — this round's executor wrapper provided the dependency closure and all claimed results reproduced exactly.
- Full `cargo test --all` and full `cd web && npm test` remain verification-stage gates; everything task-scoped ran green here.

## Verdict

Approved. The task closes the §30 gaps with real, mutation-hardened tests, adds no production changes, and every acceptance criterion is backed by passing, meaningful coverage.
