---
title: "Concept Views — cross-cutting architecture views by concept"
spec_ref: "system-explorer.md §4"
depends_on: [task-178]
progress: not-started
coverage_sections:
  - "system-explorer.md §4. Concept Views"
commits: []
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
