---
title: "Explorer Unit Tests — View Query Resolver, Graph Summary & Saved Views"
spec_ref: "explorer-implementation.md §30"
depends_on:
  - task-065
  - task-068
progress: ready-for-review
coverage_sections:
  - "explorer-implementation.md §30 Unit Tests"
commits: ["477c69538b99a3d093e292b12c9aa951742a76a7", "39adce730a183792089f04905f1af498375a80e8", "f63d1e5e0bc033e496f4d26f24b43372c73c6bea"]
---

## Shipped

Explorer unit tests for `explorer-implementation.md` §30. An audit of the existing suites found most spec-required tests already present (scope types `all`/`filter`/`focus`/`test_gaps`/`concept`/`diff`, dry-run warnings for empty/many-node/broad-group/unresolved-callout scopes, graph summary counts + top types by fields + test-coverage counts, group exact/substring matching, saved views CRUD + system-default seeding + system-view delete protection). The gaps found were closed with real tests, and the one test that had never been executed (the previous run was cut off by a network error before running it) was fixed and verified.

### New tests

- **View query resolver** (`crates/gyre-domain/src/view_query_resolver.rs`, +~460 lines across commits 39adce73/477c6953): heat emphasis metric values for `incoming_calls` (non-Calls edges excluded), `complexity`/`churn`/`test_coverage` (missing-data nodes excluded where spec requires), `test_fragility` (distinct-test counting), trace metrics warn-and-zero (never fabricate OTLP values); `tiered_colors` populates BFS `node_depths`; highlight color validation (invalid rejected naming `highlight.matched`, hex/named/rgb accepted); callout prefix match (clean) vs substring match (precision warning); narrative resolution counts, unresolved-step warnings naming each missing node, duplicate-order rejection; `$selected` with no selection yields the interactive hint without a spurious empty-scope warning, `$selected` with a selection resolves focus; group patterns restricted to the scope's result set (groups are overlays, not scope extensions).
- **Saved views ownership** (`crates/gyre-server/tests/graph_integration.rs`, `test_saved_views_ownership_enforcement`): two distinct Agent-role callers in one tenant — non-creator update AND delete of another agent's view return 403 with the ownership-check message, while the same endpoints succeed for the caller's own view (proving the 403 is ownership-specific); `created_by` records the creating agent; system default views reject deletion with the system-view message.

### Fix during this run

`test_saved_views_ownership_enforcement` panicked on first execution: it read `body["agent"]["id"]` from the agent registration response, but `RegisterAgentResponse` flattens `AgentResponse` to the top level (`#[serde(flatten)]`). Fixed the test to read `body["id"]` (commit f63d1e5e). No production code changed anywhere in this task.

### Test evidence

- `cargo test -p gyre-domain view_query_resolver` — **138 passed, 0 failed** (includes all new tests).
- `cargo test -p gyre-server --test graph_integration saved_views` — **2 passed, 0 failed** (`test_saved_views_crud`, `test_saved_views_ownership_enforcement`).
- Evidence saved under `/tmp/stage/review-evidence/`.
- Full `cargo test --all` and `cd web && npm test` gates are owned by the verification stage; this task added no web code.

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

- [x] View query resolver has tests for all scope types (all, focus, test_gaps)
- [x] Emphasis resolution tests for highlight, heat, tiered_colors
- [x] Group and callout resolution tests with edge cases
- [x] Dry-run warning generation tests for all warning conditions
- [x] Graph summary computation tests with fixture data
- [x] Test coverage gap detection tested (BFS from test nodes)
- [x] Saved views CRUD tests verified complete
- [x] All new tests pass: `cargo test --all` and `cd web && npm test`

## Agent Instructions

Start by auditing existing test coverage — many of these tests may already exist. Read `crates/gyre-domain/src/view_query_resolver.rs`, `crates/gyre-server/src/api/saved_views.rs` (has extensive tests), and `web/src/__tests__/ExplorerCanvas.test.js`. Only add tests for gaps, not duplicates. Use the existing test fixture patterns (check `crates/gyre-server/tests/` for integration test helpers). For the view query resolver, create small graph fixtures with known nodes/edges and verify resolver output.
