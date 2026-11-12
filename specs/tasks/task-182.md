---
title: "Concept Views — cross-cutting architecture views by concept"
spec_ref: "system-explorer.md §4"
depends_on: [task-178]
progress: complete
coverage_sections:
  - "system-explorer.md §4. Concept Views"
commits: ["188b9f9482edeeb0550990fc15761137eef9dc76", "362807160d14e186ac329eb185fab33754efc6f7", "39ba4fb6968847e2eb226cc5ae6e9e38394f7834"]
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
covering all three interaction modes and both data-source mechanisms,
with the round-1 review findings and the round-1 verification findings
repaired, and the round-2 contract repair applied (task bookkeeping
only — no product code changed this round).

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
  (ui-layout.md §4 Data Layer), not an exact seed name.
  `viewSpecToViewQuery` translates it to a concept scope with
  `seed_match: 'substring'` — a ViewQuery grammar field (default
  `'exact'`, documented in view-query-grammar.md, validated in
  gyre-common, implemented in the server resolver) — and all three
  concept-scope evaluation surfaces (ExplorerCanvas
  `queryMatchedWithDepth`, `view-query-scope.js` `resolveScopeNodes`,
  gyre-domain `view_query_resolver`) seed every node whose
  name/qualified_name contains the seed.
- **Saved-concept round trip:** the reload path classifies the stored
  explorer-view document by grammar via `viewDocumentToQuery` — a
  ViewQuery-shaped spec (the exact document the Create-concept dialog
  persists) is applied with its concept scope intact instead of being
  flattened to `{scope:{type:'all'}}` by the ViewSpec translator.
  Selecting a saved ad-hoc concept from the view selector re-applies the
  grouping.

### Round 2: contract repair

The round-1 checkpoint modified the protected task text (acceptance
criteria checkboxes), flagged as a contract repair. This round restores
the assigned task document's protected text verbatim (checkboxes left
unchecked, matching the repo convention — see task-178), re-records the
bookkeeping (progress, frontmatter commits), and rewrites these Shipped
notes under the exact `## Shipped` heading with `###` subheadings. No
product code changed. The candidate was re-merged onto the current
assignment base `e402e1ed6` (task-121 repo-access work; clean merge, no
conflicts with task-182 files) and the focused matrix was re-run green
on the merged tree (see Test evidence).

### Round 1: merge + verification repair (historical)

The candidate was merged onto the then-assignment base (`d31906c5a`,
task-181 Risk Map); two content conflicts resolved by union
(ExplorerCanvas.svelte import block, docs/ui.md Architecture Tab).
Round-1 verification failed on two changed-line findings, repaired in
`188b9f948`: rustfmt in `crates/gyre-server/src/api/graph.rs`
(manifest_concepts signature + concepts-listing test assert rewrapped),
and clippy::useless_vec in `gyre-domain/src/view_query_resolver.rs`
substring seed-match tests (`&vec![]` → `&[]`).

### Test evidence

Focused matrix, re-run green on the round-2 merged tree (HEAD
`2cd8c210c`), evidence: `/tmp/stage/review-evidence/focused-repair-round2.log`:

- `web` vitest: ExplorerConceptViews (8), generated-view (22),
  view-query-scope (15) — 45 tests passed. ExplorerConceptViews pins
  the manifest-concept fade against actual canvas draw output, the
  end-to-end Ask flow, the saved-ad-hoc-concept reload round trip, the
  multi-select → right-click → dialog → save persistence assert, and
  dialog validation/error paths.
- `cargo test -p gyre-common view_query` — 5 pass.
- `cargo test -p gyre-domain view_query_resolver` — 145 pass.
- `cargo test -p gyre-domain concept` — 10 unit + 2 integration pass.
- `cargo check -p gyre-server` — clean on the merged tree.

Full workspace suites, architecture checks, all-target Clippy, and
GitHub CI are owned by the independent verifier. Server tests for the
verifier: `cargo test -p gyre-server graph`, plus full gyre-domain and
gyre-common suites.
