---
title: "Concept Views — cross-cutting architecture views by concept"
spec_ref: "system-explorer.md §4"
depends_on: [task-178]
progress: ready-for-review
coverage_sections:
  - "system-explorer.md §4. Concept Views"
commits: ["362807160d14e186ac329eb185fab33754efc6f7", "39ba4fb6968847e2eb226cc5ae6e9e38394f7834", "188b9f9482edeeb0550990fc15761137eef9dc76"]
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

- [x] Concept browser lists predefined concepts from manifest
- [x] Selecting a concept filters canvas to show related nodes
- [x] Non-related nodes fade to background (not removed)
- [x] Edges between concept nodes highlighted
- [x] Active concept shown as filter chip in control bar
- [x] Multi-select + right-click → "Create concept" works
- [x] Ad-hoc concepts saved as explorer views
- [x] Ask input handles concept questions via LLM
- [x] Tests pass

## Agent Instructions

Read `system-explorer.md` §4 "Concept Views" for the full specification. The concept API endpoints are already registered in `mod.rs` — check `GET /repos/:id/graph/concept/:name` and `GET /workspaces/:id/graph/concept/:name` response shapes in `graph.rs`. The canvas is built in task-178 — check `ExplorerView.svelte` for the SVG node rendering and how to apply opacity/highlight styles. The in-view filter panel is also in task-178. For ad-hoc concept saving, use the existing explorer-views CRUD endpoints (`POST /workspaces/:id/explorer-views`). The view spec grammar `concept` field is defined in `ui-layout.md` §4 Data Layer.

## Shipped

Concept views (system-explorer.md §4) on the task-178 canvas substrate,
covering all three interaction modes and both data-source mechanisms,
with the round-1 review findings and the round-1 verification findings
repaired.

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
- **Substring mechanism (review finding 1, repaired):** the ViewSpec
  `data.concept` field is a case-insensitive substring search on node
  name/qualified_name (ui-layout.md §4 Data Layer), not an exact seed
  name. `viewSpecToViewQuery` now translates it to a concept scope with
  `seed_match: 'substring'` — a new ViewQuery grammar field (default
  `'exact'`, documented in view-query-grammar.md, validated in
  gyre-common, implemented in the server resolver) — and all three
  concept-scope evaluation surfaces (ExplorerCanvas
  `queryMatchedWithDepth`, `view-query-scope.js` `resolveScopeNodes`,
  gyre-domain `view_query_resolver`) seed EVERY node whose
  name/qualified_name contains the seed. The Ask concept path
  (`data.concept: 'auth'` over a graph with no node named "auth") now
  fades the canvas to the matched nodes instead of resolving to a null
  scope (undimmed full graph). `data.depth: 0` pins `expand_depth: 0`
  (matched nodes only) per ui-layout.md §4.
- **Saved-concept round trip (review finding 2, repaired):** the reload
  path classifies the stored explorer-view document by grammar via the
  new `viewDocumentToQuery` — a ViewQuery-shaped spec (the exact document
  the Create-concept dialog persists; the server accepts and stores both
  grammars) is applied with its concept scope intact instead of being
  flattened to `{scope:{type:'all'}}` by the ViewSpec translator.
  Selecting a saved ad-hoc concept from the view selector now re-applies
  the grouping.

### Repair: merge onto the task-181 assignment base + verification findings

The candidate was merged onto the current assignment base (`d31906c5a`,
task-181 Risk Map). Two content conflicts, both resolved by union where
the features share a file:

- `ExplorerCanvas.svelte` import block: HEAD's `nodeCategory` import
  (the merged draw loop classifies fade via the `conceptNodeIds` prop,
  not `nodeMatchesConcept` container filtering) plus the base's
  `risk-map.js` import driving the merged-in Risk Map overlay.
- `docs/ui.md` Architecture Tab: task-181's expanded Risk Map overlay
  description on the lens-selector bullet, task-182's two-mechanism
  Concepts section detail on the filter-panel bullet.

Round-1 verification (`bash /tmp/stage/checks.sh`, exit 1) failed on two
changed-line findings, repaired in `188b9f948`:

- rustfmt: `crates/gyre-server/src/api/graph.rs` (manifest_concepts
  signature + concepts-listing test assert rewrapped).
- clippy::useless_vec: `gyre-domain/src/view_query_resolver.rs`
  substring seed-match tests passed `&vec![]` where `resolve_scope`
  takes `&[GraphEdge]` — replaced with `&[]`.

### Test evidence

- `web/src/__tests__/ExplorerConceptViews.test.js` (8 tests): concept
  browser listing; manifest-concept fade pinned against actual canvas
  draw output; end-to-end Ask flow (generate SSE → ViewSpec
  `data.concept: 'auth'` → canvas fade, matched nodes boosted to full
  opacity, non-concept tree groups at 0.15×, annotation rendered);
  saved-ad-hoc-concept reload round trip (view selector → concept scope
  re-applied, fade + annotation); multi-select → right-click → dialog →
  save asserting the persisted ViewQuery spec; dialog validation/error
  paths; edge-emphasis draw-path tests.
- `web/src/__tests__/generated-view.test.js` (22 tests): substring
  translation (`seed_match: 'substring'`), depth-0 pinning, the Ask-path
  translation + `resolveScopeNodes` end-to-end probe scenario,
  `viewDocumentToQuery` round trip (deep-copy, ViewSpec translation,
  non-object rejection).
- `web/src/__tests__/view-query-scope.test.js` (15 tests): substring
  seed matching (all-matches seeding, edge expansion from substring
  seeds, no-match null, exact default when `seed_match` is absent).
- Rust: `cargo test -p gyre-domain view_query_resolver` — 145 pass;
  `cargo test -p gyre-domain concept` — 10 unit + 2 integration pass;
  `cargo test -p gyre-common view_query` — 5 pass; `cargo check -p
  gyre-server` clean; `cargo clippy -p gyre-domain --all-targets`
  reports no useless_vec in view_query_resolver.rs (remaining crate
  warnings are pre-existing base-code lines outside this task's diff).
- Focused matrix re-run green on the merged tree: ExplorerConceptViews,
  ExplorerCanvas, filter-categories, RiskMap (base side),
  ExplorerCanvas.evaluative-lens (base side), view-query-scope,
  view-spec, view-query-validator, generated-view, ExplorerCanvas.flow,
  canvas-filters, ExplorerDefaultViews, ExplorerViewFlowTemplates,
  flow-templates, ExplorerGeneratedViews — 373 tests. Evidence:
  `/tmp/stage/review-evidence/frontend-focused-tests.log` and
  `rust-focused-tests.log`.
