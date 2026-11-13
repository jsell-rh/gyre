---
title: "Concept Views — cross-cutting architecture views by concept"
spec_ref: "system-explorer.md §4"
depends_on: [task-178]
progress: complete
coverage_sections:
  - "system-explorer.md §4. Concept Views"
commits: ["da395b72500cbaabd640bbfa237d157e6b59748f", "376a3f6fc7086289d330cd4b8ca950a1ef0f17d4", "647d9bd3424fdea3da7471d49652dbe03bc7f25e", "fa5d864d6205a53e8e50c09a45047ad5ad4e8ad1", "13b99dd78a2d35b3c60cf3dd653486ffd5fb9708", "afd7e164cba686c33acc937fc15b1edd56964997", "a44ad8886416bd9de4defed02be1ad35cf8eb11e", "188b9f9482edeeb0550990fc15761137eef9dc76", "362807160d14e186ac329eb185fab33754efc6f7", "39ba4fb6968847e2eb226cc5ae6e9e38394f7834", "7b56049a8b1fb365af30e91ec9662b399c914f25"]
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
mechanisms; this round closed the last CI failure on the merged tree
(round 15 of the repair pipeline: 4 stale e2e visual baselines).

### CI repair (this round)

The four "Filter presets visual regression" baselines
(filter-endpoints / filter-types / filter-calls / filter-dependencies)
still recorded the pre-task-182 rendering in which every tree-group
container dims to 0.15 during a view query — the old `treeGroupHasMatch`
recursion compared `cln.parentTreeGroup === ln` by object reference
across Svelte 5 `$state` proxies and never matched, so even containers
holding matched nodes were dimmed. The task-182 fix compares by id, so
containers with matched children keep full opacity (their leaves'
`dim_unmatched` fade is unchanged).

Repair: the baselines were regenerated from the actual screenshots CI
run 43366519906 captured at the approved head (same runner class as the
gate). Assertion strength is unchanged — same tests, same
PRESET_SNAPSHOT_OPTIONS (maxDiffPixelRatio 0.02, threshold 0.05), and
the behavioral canvas-pixel change-count assertions in
filter_preset_buttons_show_correct_subsets are untouched.

### Concept views (previously shipped, re-verified)

- **Concept browser (manifest mechanism):** `GET /repos/:id/graph/concepts`
  lists a repo's manifest-defined concepts in the in-view filter panel's
  Concepts section; selecting one fetches its
  `GET /repos/:id/graph/concept/:name` projection and activates the
  concept view.
- **Canvas concept emphasis:** concept nodes keep full draw opacity and
  gain a highlighted border; everything else fades to background at 0.15
  (drawn, never removed). Edges between concept nodes are highlighted;
  other edges dim with their endpoints. The active concept shows as a
  clearable control-bar chip.
- **Ad-hoc concepts:** Shift+Click multi-select + right-click → "Create
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

### Test evidence

- Local reproduction of the CI failure then verification of the repair:
  `npx playwright test tests/e2e/explorer-visual.spec.js` → 16 passed
  (exit 0) with the updated baselines; before the update the same four
  tests failed at the same diff-ratio signatures as CI (0.74/0.17/0.73/
  0.76). Logs and pixel-analysis evidence:
  /tmp/stage/review-evidence/task-182-ci-repair.md.
- No source changes this round: the repair diff is the four baseline
  PNGs only; web/dist restored to HEAD (executor republishes generated
  assets).
- Server tests for the verifier: none new this round; the head's Rust and
  vitest suites already passed in CI run 43366519906 (61/65 e2e; the 4
  failures are the baselines repaired here).
