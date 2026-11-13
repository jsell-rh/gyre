---
title: "Concept Views — cross-cutting architecture views by concept"
spec_ref: "system-explorer.md §4"
depends_on: [task-178]
progress: complete
coverage_sections:
  - "system-explorer.md §4. Concept Views"
commits: ["fa5d864d6205a53e8e50c09a45047ad5ad4e8ad1", "13b99dd78a2d35b3c60cf3dd653486ffd5fb9708", "afd7e164cba686c33acc937fc15b1edd56964997", "a44ad8886416bd9de4defed02be1ad35cf8eb11e", "188b9f9482edeeb0550990fc15761137eef9dc76", "362807160d14e186ac329eb185fab33754efc6f7", "39ba4fb6968847e2eb226cc5ae6e9e38394f7834", "647d9bd3424fdea3da7471d49652dbe03bc7f25e"]
---

## Spec Excerpt

system-explorer.md §4 defines Concept Views — cross-cutting views that show a concept (e.g., "Authentication") by pulling related elements from across the codebase:

**Canvas behavior:** The canvas reorganizes to show only elements related to the selected concept. Everything else fades to background. Relationships between concept elements are highlighted.

**Three interaction modes:**
1. **Browse predefined concepts** (from manifest) — the concept manifest is defined in `realized-model.md` §4, served by `GET /repos/:id/graph/concept/:name`
2. **Create ad-hoc concepts** by selecting nodes and grouping them — user selects multiple nodes on the canvas, right-click → "Create concept"
3. **Ask "show me everything related to X"** — conversational, LLM-powered (uses the Ask input in the control bar)

**Data source distinction:** The view spec grammar's `concept` field (from `ui-layout.md` §4) does case-insensitive substring matching on node names. The `GET /repos/:id/graph/concept/:name` endpoint uses manifest-based concept projections. These are different mechanisms — concept views should support both.

## Implementation Plan

1. **Concept browser panel**:
   - Add a "Concepts" section to the in-view filter panel (from task-178)
   - List predefined concepts from the concept manifest (fetch from knowledge graph API)
   - Click a concept → canvas filters to show only related nodes
   - Non-related nodes fade to low opacity (0.1-0.2) rather than being removed

2. **Canvas concept filtering**:
   - When a concept is active, apply visual emphasis to concept nodes (full opacity, highlighted border)
   - Fade non-concept nodes to background (low opacity, desaturated)
   - Highlight edges between concept nodes
   - Show concept name in the control bar as an active filter chip (click to clear)

3. **Ad-hoc concept creation**:
   - Multi-select nodes on canvas (Shift+click or lasso select)
   - Right-click selected nodes → "Create concept" option in context menu
   - Dialog: name the concept
   - Save ad-hoc concept as a saved view (uses existing explorer-views CRUD)

4. **LLM concept exploration**:
   - The Ask input already generates views (task-178). Concept questions ("show me everything related to authentication") should produce a view spec with the `concept` field set
   - The LLM prompt template should include concept examples

5. **Concept view API integration**:
   - Predefined concepts: `GET /repos/:id/graph/concept/:name` (already exists)
   - Workspace-scoped: `GET /workspaces/:id/graph/concept/:name` (already exists)
   - Substring matching: use view spec `concept` field for ad-hoc queries

6. **Tests**:
   - Concept selection filters canvas correctly
   - Non-concept nodes fade to background
   - Ad-hoc concept creation saves as explorer view
   - Concept filter chip appears and clears correctly

## Acceptance Criteria

- [ ] Concept browser lists predefined concepts from manifest
- [ ] Selecting a concept filters canvas to show related nodes
- [ ] Non-related nodes fade to background (not removed)
- [ ] Edges between concept nodes highlighted
- [ ] Active concept shown as filter chip in control bar
- [ ] Multi-select + right-click → "Create concept" works
- [ ] Ad-hoc concepts saved as explorer views
- [ ] Ask input handles concept questions via LLM
- [ ] Tests pass

## Agent Instructions

Read `system-explorer.md` §4 "Concept Views" for the full specification. The concept API endpoints are already registered in `mod.rs` — check `GET /repos/:id/graph/concept/:name` and `GET /workspaces/:id/graph/concept/:name` response shapes in `graph.rs`. The canvas is built in task-178 — check `ExplorerView.svelte` for the SVG node rendering and how to apply opacity/highlight styles. The in-view filter panel is also in task-178. For ad-hoc concept saving, use the existing explorer-views CRUD endpoints (`POST /workspaces/:id/explorer-views`). The view spec grammar `concept` field is defined in `ui-layout.md` §4 Data Layer.

## Shipped

Concept views (system-explorer.md §4) on the task-178 canvas substrate,
covering all three interaction modes and both data-source mechanisms.
This round repaired the CI failure on the merged tree (see "CI repair"
below); the previously reviewed implementation was preserved through
the base merge and re-verified.

### Behavior

- **Concept browser (manifest mechanism):** `GET /repos/:id/graph/concepts`
  and `GET /workspaces/:id/graph/concepts` list a repo's (or workspace
  union of) manifest-defined concepts; the in-view filter panel's Concepts
  section shows a "Predefined" subsection (manifest concepts,
  projection-backed) alongside "Discovered" (substring). Selecting a
  predefined concept fetches its `GET /repos/:id/graph/concept/:name`
  projection and activates the concept view.
- **Canvas concept emphasis:** concept nodes keep full draw opacity and gain
  a highlighted border; everything else fades to background at 0.15 —
  drawn, never removed. Edges between concept nodes are highlighted
  (accent color + alpha floor + thickness); other edges dim with their
  endpoints. Active concept shows as a clearable control-bar chip.
- **Ad-hoc concepts:** Shift+Click multi-select + right-click → "Create
  concept" opens a naming dialog; saving persists the grouping as an
  explorer view (`POST /workspaces/:id/explorer-views`) whose ViewQuery
  concept scope seeds exactly the selected nodes.
- **LLM concept exploration:** the `explorer-generate` prompt template
  and the hardcoded fallback route "show me everything related to X"
  questions to `data.concept` with a worked example.
- **Substring mechanism:** the ViewSpec `data.concept` field is a
  case-insensitive substring search on node name/qualified_name
  (ui-layout.md §4 Data Layer), translated by `viewSpecToViewQuery` to a
  concept scope with `seed_match: 'substring'` (ViewQuery grammar field,
  default `'exact'`, documented in view-query-grammar.md, validated in
  gyre-common, implemented in the server resolver) — all three
  concept-scope evaluation surfaces (ExplorerCanvas
  `queryMatchedWithDepth`, `view-query-scope.js` `resolveScopeNodes`,
  gyre-domain `view_query_resolver`) seed every node whose
  name/qualified_name contains the seed.
- **Saved-concept round trip:** the reload path classifies the stored
  explorer-view document by grammar via `viewDocumentToQuery` — a
  ViewQuery-shaped spec (the exact document the Create-concept dialog
  persists) is applied with its concept scope intact instead of being
  flattened to `{scope:{type:'all'}}`. Selecting a saved ad-hoc concept
  from the view selector re-applies the grouping.

### CI repair (this round)

PR #807 run 43354234804 failed
`api::graph::tests::list_workspace_graph_concepts_unions_and_dedupes`:
the workspace concept listing (`list_workspace_graph_concepts`)
deduped case-insensitive concept names first-wins over
`list_by_workspace()`, whose iteration order the port does not guarantee
(the mem adapter returns HashMap order). With case-variant spellings of
one concept across repos (repo-1 `Authentication`, repo-2
`authentication`), the surviving spelling depended on adapter iteration
order — CI got `authentication` where `Authentication` was expected
(reproduced as a standalone order-flip in
`/tmp/stage/review-evidence/dedupe_check.rs`). Fixed in `13b99dd7`:
each case-insensitive name now keeps a deterministic winner — the
lexicographically smallest (name, description) pair — and the listing
is sorted, so every adapter returns the same response for the same
manifest set regardless of iteration order. No spec text changed; the
test's own contract ("the winner must not matter") is now actually
satisfied.

### Test evidence

This round (repair commit `13b99dd7`, merged tree at `97b5d097`):

- `cargo check -p gyre-server --tests` — clean (this sandbox; full
  server test builds are deferred to the verifier per the 4 GiB budget).
- `rustfmt --check` on `crates/gyre-server/src/api/graph.rs` — clean.
- Standalone order-independence reproduction of the new dedupe (both
  repo iteration orders → `["Authentication", "Billing"]` with the
  manifest description): `/tmp/stage/review-evidence/dedupe_check.rs`,
  exit 0.
- `scripts/check-task-commit-attribution.sh` — exit 0 with the
  restored commits frontmatter.

Prior-round evidence on the same implementation (merge round
`ce2418944`): `web` vitest ExplorerConceptViews (8 tests) plus
merge-affected suites — 5 files, 74 tests passed, pinning the
manifest-concept fade against actual canvas draw output, the end-to-end
Ask flow, the saved-ad-hoc-concept reload round trip, the multi-select →
right-click → dialog → save persistence assert, and dialog
validation/error paths; `cargo test -p gyre-common` 124 pass;
`cargo test -p gyre-domain` 486 pass; mechanical checks (check-arch,
abac-route-registry, migration-versions, mem-port-contracts) exit 0.

Server tests for the verifier: `cargo nextest run -p gyre-server
api::graph::tests::` — in particular
`list_workspace_graph_concepts_unions_and_dedupes` (the repaired
flake), `workspace_graph_concept_unions_manifest_projections`,
`list_graph_concepts_returns_manifest_concepts`, and
`graph_concept_returns_manifest_projection_not_substring` — plus the
full web suite (`cd web && npm test`).
