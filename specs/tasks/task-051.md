# TASK-051: Spec Links — Impact Analysis View

**Spec reference:** `spec-links.md` §UI (line 189)  
**Depends on:** TASK-019 (spec dependents endpoint), TASK-045 (spec DAG visualization)  
**Progress:** `needs-revision`  
**Review:** [`specs/reviews/task-051.md`](../reviews/task-051.md)

## Spec Excerpt

From `spec-links.md` §UI:

> **Impact analysis view:** select a spec, see all transitive dependents. "If I change this spec, these 14 specs across 6 repos need review."

## Current State

**Backend:** The `GET /api/v1/specs/:path/dependents` endpoint does not exist yet — TASK-019 will create it. The existing `GET /api/v1/specs/graph` (registered at `mod.rs:345`) returns the full graph with edges, from which transitive dependents can be computed client-side.

**Frontend:** The SpecDashboard (`web/src/components/SpecDashboard.svelte`) has a graph view (TASK-045 will upgrade it to an interactive DAG). The `DetailPanel.svelte` has a "links" tab showing per-spec outbound/inbound links, but NO impact analysis — it shows only direct links, not transitive dependents.

There is no UI anywhere that answers "if I change this spec, what else needs review?"

## Implementation Plan

1. **Impact analysis panel in SpecDashboard:**
   - Add an "Impact Analysis" interaction to the spec DAG visualization (TASK-045)
   - When a user right-clicks or selects a spec node in the DAG and chooses "Analyze Impact":
     - Call `GET /api/v1/specs/:path/dependents` (from TASK-019)
     - Highlight all transitive dependent specs in the DAG
     - Dim non-dependent specs to 20% opacity
     - Show a summary panel: "{N} specs across {M} repos need review"
   - The summary panel lists each affected spec grouped by repo, with link type (depends_on, implements, extends)

2. **Impact analysis in spec detail links tab:**
   - Add an "Analyze Impact" button to the links tab in `DetailPanel.svelte`
   - When clicked, fetch `GET /api/v1/specs/:path/dependents`
   - Display a tree view: spec → direct dependents → transitive dependents
   - Each entry shows: spec path, link type, repo name, approval status
   - Total count header: "14 specs across 6 repos would need review"

3. **Cross-repo awareness:**
   - When dependents span multiple repos, group by repo with repo name as section header
   - Include workspace context for cross-workspace dependents
   - Show link type for each relationship (depends_on, implements, extends)

4. **Tests:**
   - Component test: impact analysis panel renders with mock dependents data
   - Component test: transitive dependents grouped by repo
   - Component test: DAG highlighting dims non-dependent nodes
   - Component test: empty state when no dependents exist

## Acceptance Criteria

- [ ] "Analyze Impact" interaction available in spec DAG visualization
- [ ] Selecting a spec highlights all transitive dependents in the DAG
- [ ] Summary panel shows "{N} specs across {M} repos need review"
- [ ] Affected specs listed grouped by repo with link types
- [ ] "Analyze Impact" button in DetailPanel links tab shows tree view
- [ ] Cross-repo dependents displayed with repo context
- [ ] Non-dependent nodes dimmed in DAG during analysis
- [ ] Component tests verify rendering and interaction
- [ ] `npm test` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `specs/system/spec-links.md` §UI for the impact analysis requirement
3. Read `web/src/components/SpecDashboard.svelte` for the graph view (and TASK-045 DAG if already implemented)
4. Read `web/src/lib/DetailPanel.svelte` (lines 3495–3590) for the existing links tab
5. Read `web/src/lib/api.js` for existing spec API calls
6. Verify `GET /api/v1/specs/:path/dependents` exists (created by TASK-019) before calling it
7. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- `798a0647` feat(ui): add spec links impact analysis view (TASK-051)
- `de992204` fix(ui): address TASK-051 R1 findings — impact analysis correctness
