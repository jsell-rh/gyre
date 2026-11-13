---
title: "Concept Views — cross-cutting architecture views by concept"
spec_ref: "system-explorer.md §4"
depends_on: [task-178]
progress: complete
coverage_sections:
  - "system-explorer.md §4. Concept Views"
commits: ["a9e9903d62be6229c7c62634e897fe86c09f5995", "7b56049a8b1fb365af30e91ec9662b399c914f25", "da395b72500cbaabd640bbfa237d157e6b59748f", "376a3f6fc7086289d330cd4b8ca950a1ef0f17d4", "647d9bd3424fdea3da7471d49652dbe03bc7f25e", "fa5d864d6205a53e8e50c09a45047ad5ad4e8ad1", "13b99dd78a2d35b3c60cf3dd653486ffd5fb9708", "afd7e164cba686c33acc937fc15b1edd56964997", "a44ad8886416bd9de4defed02be1ad35cf8eb11e", "188b9f9482edeeb0550990fc15761137eef9dc76", "362807160d14e186ac329eb185fab33754efc6f7", "39ba4fb6968847e2eb226cc5ae6e9e38394f7834"]
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

Concept views (system-explorer.md §4) are implemented on the task-178
canvas substrate across all three interaction modes and both data-source
mechanisms. This round resolved the rebase conflict against current main
(durable finding `ef11cf4f`, PR 807 merge_state DIRTY) without changing
any task-182 source.

### Implementation (retained)

- **Concept browser (manifest mechanism):** `GET /repos/:id/graph/concepts`
  (and the workspace-scoped twin) lists a repo's manifest-defined concepts
  in the in-view filter panel's Concepts section; selecting one fetches its
  `GET /repos/:id/graph/concept/:name` projection and activates the
  concept view.
- **Canvas concept emphasis:** concept nodes keep full draw opacity and
  gain a highlighted border; everything else fades to background at 0.15
  (drawn, never removed). Edges between concept nodes are highlighted;
  other edges dim with their endpoints. The active concept shows as a
  clearable control-bar chip. Tree-group containers holding matched
  nodes keep full opacity (id-based `treeGroupHasMatch`, not object
  reference).
- **Ad-hoc concepts:** Shift+Click multi-select + right-click "Create
  concept" opens a naming dialog; saving persists the grouping as an
  explorer view whose ViewQuery concept scope seeds exactly the selected
  nodes.
- **LLM concept exploration:** the explorer-generate prompt template and
  the fallback route "show me everything related to X" questions to a
  ViewSpec with `data.concept` set.
- **Substring mechanism:** the ViewSpec `data.concept` field translates
  to a concept scope with `seed_match: 'substring'` (default 'exact'),
  implemented consistently across ExplorerCanvas, view-query-scope.js,
  and the server resolver.

### Rebase repair (this round)

The merge of assignment base `603cc2f7` (main's task-183 Architectural
Timeline) into the task branch was in progress with all conflicts
resolved but uncommitted. Conflicted paths were generated web/dist
assets only (kept at the incoming main state; the executor republishes
generated assets — not rebuilt here). Verified the combined tree:
main's timeline changes are purely additive to the task-182 concept
implementation (`ExplorerView.svelte` gains only the timeline history
fetcher; `ExplorerCanvas.svelte` gains only the
`timelineReconciliationEvents` prop pass; `api.js` gains only
`repoGraphAt`/`workspaceMessages`). The merge was concluded as commit
`601164b8`.

### Test evidence

- Merged-tree focused probe: `npx vitest run
  src/__tests__/ExplorerConceptViews.test.js
  src/__tests__/ExplorerCanvas.test.js
  src/__tests__/TimelineScrubber.test.js
  src/__tests__/timeline-utils.test.js
  src/__tests__/view-query-scope.test.js` → 249/249 passed, exit 0
  (after `npm ci` with locked versions). Evidence:
  /tmp/stage/review-evidence/task-182-rebase-repair.md.
- `cargo check -p gyre-server` on the merged tree → clean, exit 0.
- No source changes this round: the merge brought only main's
  already-CI-green code, additive to the retained task-182
  implementation. Prior-round evidence (e2e visual baselines) stands at
  the retained head.
- Server tests for the verifier: `cargo test -p gyre-server --test
  graph_integration` (concept endpoints:
  `test_graph_concept`,
  `test_full_graph_concept_query_param`,
  `test_workspace_graph_concept`,
  `test_workspace_graph_concept_not_found`), plus the standard full
  workspace suite.
