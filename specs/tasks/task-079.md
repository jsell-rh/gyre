---
title: "Explorer Unit Tests — View Query Resolver, Graph Summary & Saved Views"
spec_ref: "explorer-implementation.md §30"
depends_on:
  - task-065
  - task-068
progress: ready-for-review
coverage_sections:
  - "explorer-implementation.md §30 Unit Tests"
commits: ["4058b618fb52c49f2b4bbb7179526d415f7fe2c3"]
---

## Shipped

Explorer unit tests for `explorer-implementation.md` §30, completing the source recovered from the interrupted run (commit 4058b618) and re-verified across review rounds. An audit found most spec-required tests already present (scope types `all`/`filter`/`focus`/`test_gaps`/`concept`/`diff`, dry-run warnings for empty/cluttered/broad-group/unresolved-callout scopes, graph summary counts + top types by fields + test-coverage counts, group exact/substring matching, saved views CRUD + system-default seeding + system-view delete protection). The gaps were closed with real tests; no production code changed anywhere in this task. Acceptance is recorded here rather than by editing the assigned criteria list (contract repair 34ae63f2).

### New tests

- **View query resolver** (`crates/gyre-domain/src/view_query_resolver.rs`): heat emphasis metric values for `incoming_calls` (non-Calls edges excluded), `complexity`/`churn`/`test_coverage` (missing-data nodes excluded where the metric requires data), `test_fragility` (distinct-test counting), trace metrics warn-and-zero (never fabricate OTLP values); `tiered_colors` populates BFS `node_depths`; highlight color validation (invalid rejected naming `highlight.matched`; hex/named/rgb accepted); callout prefix match (clean) vs substring match (precision warning); narrative resolution counts, unresolved-step warnings naming each missing node, duplicate-order rejection; `$selected` with no selection yields the interactive hint without a spurious empty-scope warning, `$selected` with a selection resolves focus; group patterns restricted to the scope's result set (groups are overlays, not scope extensions).
- **Saved views ownership** (`crates/gyre-server/tests/graph_integration.rs`, `test_saved_views_ownership_enforcement`): two distinct Agent-role callers in one tenant — non-creator update AND delete of another agent's view return 403 with the ownership-check message, while the same endpoints succeed for the caller's own view (proving the 403 is ownership-specific, not general access denial); `created_by` records the creating agent; system default views reject deletion with the system-view message. The test reads the registration response via its flattened top-level fields (`RegisterAgentResponse` uses `#[serde(flatten)]`).

### Test evidence

- Source integrity this round: `git diff 93013f3d -- crates/ web/` is empty — the product surface is byte-identical to the checkpoint that the attempt-2 round verified; the only checkout change was the executor-restored assigned task document.
- `cargo test -p gyre-domain --lib view_query_resolver` — **138 passed, 0 failed**, executed in the attempt-2 sandbox at this exact source state. In this round's sandbox it is **not executable**: crates.io egress (index.crates.io and static.crates.io) is connection-refused (curl exit 7) and the box has no cargo registry cache or vendored sources, so the dependency closure cannot be fetched — a selective block (github.com and registry.npmjs.org reachable), an infrastructure restriction for remote verification, not a code defect. Logs: `/tmp/stage/review-evidence/focused-tests.log`, `/tmp/stage/review-evidence/EVIDENCE.md`.
- `cargo test -p gyre-server --test graph_integration test_saved_views` — not executable in either sandbox (same transport restriction). In lieu of execution, every assertion was cross-checked line-by-line against the `saved_views.rs` handlers (ownership/system-view 403 messages and ordering, `created_by` assignment, status codes, ABAC exemption + agent-token auth preconditions). Re-execution is deferred to the remote verification environment.
- Web spot check re-executed this round (task adds no web code): `npm ci` then `vitest run ExplorerCanvas.test.js ExplorerViewScope.test.js` — **155 passed, 0 failed**, matching the attempt-2 result. Log: `/tmp/stage/review-evidence/web-spot-check.log`.
- Full `cargo test --all` and `cd web && npm test` are verification-stage gates and were not repeated here.

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
