---
title: "Concept Views — cross-cutting architecture views by concept"
spec_ref: "system-explorer.md §4"
depends_on: [task-178]
progress: ready-for-review
coverage_sections:
  - "system-explorer.md §4. Concept Views"
commits: ["a44ad8886416bd9de4defed02be1ad35cf8eb11e", "188b9f9482edeeb0550990fc15761137eef9dc76", "362807160d14e186ac329eb185fab33754efc6f7", "39ba4fb6968847e2eb226cc5ae6e9e38394f7834"]
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
This round is the rebase repair against current main; no product code
changed — the previously reviewed implementation was preserved through
the merge and re-verified on the merged tree.

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

### This round: rebase repair onto current main

The published PR conflicted with main (merge_state DIRTY, repair
category rebase). Resolved by merging main `3b734b1af` (task-168 repo
lifecycle) into this branch as merge commit `3257e9fb8`: all source
files merged cleanly — the only conflicted paths were generated
web/dist assets, resolved to the candidate's verified build (final
integration rebuilds and publishes those remotely). Verified the
staged tree against both parents: vs main, the full task-182
implementation is retained; vs the candidate, only main-side task-168
changes appear. Both ABAC route-registry and migration-version checks
pass on the merged tree (the concepts routes resolve; migration 000062
is not duplicated).

### Test evidence

Focused matrix, run green on the merged tree (HEAD `3257e9fb8`),
evidence: `/tmp/stage/review-evidence/focused-repair-round3.log`:

- `web` vitest: ExplorerConceptViews (8 tests) plus the five other
  merge-affected suites (ExplorerViewNavigableArchitecture,
  filter-categories, generated-view, view-query-scope, RepoSettings,
  WorkspaceSettings) — 215 tests passed. ExplorerConceptViews pins the
  manifest-concept fade against actual canvas draw output, the
  end-to-end Ask flow, the saved-ad-hoc-concept reload round trip, the
  multi-select → right-click → dialog → save persistence assert, and
  dialog validation/error paths.
- `cargo check -p gyre-server -p gyre-domain -p gyre-common` — clean.
- `cargo test -p gyre-common` — 124 pass.
- `cargo test -p gyre-domain` — 486 pass (478 lib + 8 integration).
- Mechanical checks (exit 0): check-arch.sh, check-abac-route-registry.sh,
  check-migration-versions.sh, check-in-memory-state-stores.sh,
  check-fabricated-scope-defaults.sh.

Full workspace suites, architecture checks, all-target Clippy, and
GitHub CI are owned by the independent verifier. Server tests for the
verifier: `cargo test -p gyre-server graph`, plus full gyre-domain and
gyre-common suites.
