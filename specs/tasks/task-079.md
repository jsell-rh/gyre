---
title: "Explorer Unit Tests — View Query Resolver, Graph Summary & Saved Views"
spec_ref: "explorer-implementation.md §30"
depends_on:
  - task-065
  - task-068
progress: complete
coverage_sections:
  - "explorer-implementation.md §30 Unit Tests"
commits: ["e4dd7a62751896b1ad02fc978bc610e65dae134d", "4058b618fb52c49f2b4bbb7179526d415f7fe2c3"]
---

## Shipped

Explorer unit tests for `explorer-implementation.md` §30, delivered on the retained
source checkpoint (contract repair `requirements_changed`). The checkpoint
(`4058b618f`, rustfmt-repaired in `e4dd7a627`) carried the full test additions;
this round resolved the pending merge against the task-068 base `ad417f8a5`
(`d3ccb81ad`) — both sides had appended distinct tests to the resolver's test
module, and the resolution keeps both suites losslessly (141 unique tests, 0 lost
from either parent, 0 duplicates) while adopting task-068's resolver API changes
(`compute_graph_summary` 5-arg signature, `dry_run_with_call_graph`, per-pass
call-edge metrics) with all cross-crate call sites consistent. No production code
changed in this task beyond the merge adaptation.

### Test coverage shipped

- **View query resolver** (`crates/gyre-domain/src/view_query_resolver.rs`): scope
types `all`/`filter`/`focus` (depth/direction)/`test_gaps`/`concept`/`diff`;
emphasis heat metrics (`incoming_calls` excluding non-Calls edges,
`complexity`/`churn`/`test_coverage` with missing-data exclusion,
`test_fragility` distinct-test counting, trace metrics warn-and-zero — never
fabricate OTLP values), `tiered_colors` BFS `node_depths`, highlight color
validation (invalid rejected naming `highlight.matched`; hex/named/rgb accepted);
callout exact/prefix/substring matching with precision warnings; narrative
resolution counts, unresolved-step warnings naming each missing node,
duplicate-order rejection; `$selected`/`$clicked` with and without a selection;
group patterns restricted to the scope's result set (groups are overlays, not
scope extensions).
- **Dry-run warnings**: empty scope, cluttered/many-node scope, broad group,
unresolved callouts, partial match, unresolved narrative steps, interactive
no-selection hint (no spurious empty-scope double report), call-graph
incomplete gating (only on positive knowledge + calls traversal).
- **Graph summary**: node/edge counts, top types by fields, top functions by
calls, test-coverage counts, per-pass `Calls`-edge provenance metrics,
`is_call_graph_complete` marker-vs-extraction ordering.
- **Test coverage gap detection**: BFS-from-tests reachability
(`test_computed_test_unreachable*`, `test_intersect_where_and_test_unreachable`,
`test_scope_test_gaps*`).
- **Saved views** (`crates/gyre-server/tests/graph_integration.rs`): CRUD,
system-default seeding, system-view delete protection, and ownership enforcement
(two distinct Agent callers in one tenant — non-creator update AND delete return
403 ownership-specific while the same endpoints succeed for the creator).

### Test evidence

- Merge-resolution proof, rustfmt parse/format check (exit 0), and the acceptance
criteria audit: `/tmp/stage/review-evidence/` (`EVIDENCE.md`, `merge-resolution.log`,
`rustfmt-check.log`).
- Web spot check (this round, locked `npm ci`): `vitest run ExplorerCanvas.test.js
ExplorerViewScope.test.js` — **155 passed, 0 failed** (`web-spot-check.log`).
- `cargo test -p gyre-domain --lib view_query_resolver` (138 tests at the
checkpoint state) and `cargo test -p gyre-server --test graph_integration
test_saved_views` are **not executable in this sandbox**: crates.io egress is
connection-refused (curl exit 7, selective filter — github.com and
registry.npmjs.org reachable) with no registry cache or vendored sources
(`cargo-transport-restriction.log`). The merge adds only the 3 approved task-068
tests and moves no tokens (rustfmt exit 0). Re-execution deferred to remote
verification.
- Full `cargo test --all` and `cd web && npm test` are verification-stage gates
and were not repeated here.

## Spec Excerpt

Unit tests from `explorer-implementation.md` §Testing:

- **View query resolver:** scope, emphasis, groups, callouts, narrative resolution
- **Dry-run warning generation** — overly broad groups, unresolved nodes, empty scopes
- **Graph summary computation** — node/edge counts, top types by fields, top functions by calls
- **Test coverage gap detection** — BFS from test nodes to find unreachable functions
- **Saved views CRUD** — create, read, update, delete, system defaults

## Implementation Plan

1. **Audit existing tests:**
   - Check `crates/gyre-domain/src/view_query_resolver.rs` for existing unit tests
   - Check `crates/gyre-server/src/api/saved_views.rs` for existing tests (already has several)
   - Check `web/src/__tests__/ExplorerCanvas.test.js` and related test files
   - Identify gaps between spec requirements and existing test coverage

2. **View query resolver tests** (in `gyre-domain`):
   - Scope resolution: `all`, `focus` (with depth/direction), `test_gaps`
   - Emphasis resolution: `highlight`, `heat` (with metric/palette), `tiered_colors`
   - Group resolution: named groups with node name patterns, `$selected`/`$clicked` references
   - Callout resolution: specific node names with labels
   - Narrative resolution: ordered annotations
   - Edge case: empty scope, unresolvable node names, `$selected` without selection

3. **Dry-run warning tests:**
   - Scope matched 0 nodes → warning
   - Scope matched >200 nodes → warning
   - Group matched >20 nodes → "too broad" warning
   - Unresolved callout names → warning with unresolved list
   - All resolved → no warnings

4. **Graph summary tests:**
   - Correct node type counts from a fixture graph
   - Correct edge type counts
   - Top types sorted by field count
   - Top functions sorted by incoming call count
   - Test coverage metrics (test nodes, reachable, unreachable)

5. **Saved views tests** — verify existing tests in `saved_views.rs` cover:
   - CRUD operations (already tested)
   - System default view seeding (already tested)
   - Ownership checks on update/delete
   - System views cannot be deleted (already tested)
   - Add any missing edge case tests

## Acceptance Criteria

- [ ] View query resolver has tests for all scope types (all, focus, test_gaps)
- [ ] Emphasis resolution tests for highlight, heat, tiered_colors
- [ ] Group and callout resolution tests with edge cases
- [ ] Dry-run warning generation tests for all warning conditions
- [ ] Graph summary computation tests with fixture data
- [ ] Test coverage gap detection tested (BFS from test nodes)
- [ ] Saved views CRUD tests verified complete
- [ ] All new tests pass: `cargo test --all` and `cd web && npm test`

## Agent Instructions

Start by auditing existing test coverage — many of these tests may already exist. Read `crates/gyre-domain/src/view_query_resolver.rs`, `crates/gyre-server/src/api/saved_views.rs` (has extensive tests), and `web/src/__tests__/ExplorerCanvas.test.js`. Only add tests for gaps, not duplicates. Use the existing test fixture patterns (check `crates/gyre-server/tests/` for integration test helpers). For the view query resolver, create small graph fixtures with known nodes/edges and verify resolver output.
