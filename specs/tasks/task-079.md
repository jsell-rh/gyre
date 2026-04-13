---
title: "Explorer Unit Tests — View Query Resolver, Graph Summary & Saved Views"
spec_ref: "explorer-implementation.md §30"
depends_on:
  - task-065
  - task-068
progress: not-started
coverage_sections:
  - "explorer-implementation.md §30 Unit Tests"
commits: []
---

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
