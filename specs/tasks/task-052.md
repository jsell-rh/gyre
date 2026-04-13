---
title: "Breaking Change Impact Analysis Modal"
spec_ref: "dependency-graph.md §UI (line 232), §Impact Analysis"
depends_on: 
  - task-020
  - task-022
progress: complete
review: specs/reviews/task-052.md
coverage_sections: []
commits: 
  - 3cd4d3e9
---

## Spec Excerpt

From `dependency-graph.md` §UI:

> **Impact analysis modal:** before merging a breaking change, show the full blast radius with cascade test results.

From `dependency-graph.md` §Impact Analysis:

> Before merging a change, the Workspace Orchestrator (or a human) can query:
>
> "If I merge this change to Repo B, what is the blast radius?"
>
> Response:
>   Direct dependents: Repo A, Repo C, Repo F
>   Transitive dependents: Repo G (depends on Repo A), Repo H (depends on Repo C)
>   Total blast radius: 5 repos
>
>   Breaking: yes (semver major)
>   Cascade test results: not yet run
>
>   Dependent repo health:
>     Repo A: pinned at v1.4.0 (1 version behind), tests passing
>     Repo C: pinned at v1.2.0 (3 versions behind, STALE), tests unknown
>     Repo F: pinned at v1.5.0 (current), tests passing

## Current State

**Backend:** `GET /api/v1/repos/:id/blast-radius` exists (registered at `mod.rs:534`) and returns transitive dependents via BFS. However:
- It does NOT include cascade test results (TASK-022 adds cascade testing)
- It does NOT include per-dependent repo health (version drift, test status) — TASK-021 adds drift tracking
- `GET /api/v1/dependencies/breaking` and `POST /api/v1/dependencies/breaking/:id/acknowledge` do not exist yet (TASK-020)

**Frontend:** No impact analysis modal exists. The merge queue UI (`PipelineOverview.svelte`) shows MRs as a list (TASK-049 will add DAG visualization). There is no pre-merge blast radius display.

## Implementation Plan

1. **New component** — `web/src/components/ImpactAnalysisModal.svelte`:
   - Modal dialog triggered from merge queue or MR detail
   - Header: "Impact Analysis: {repo_name}" with breaking change badge
   - Content sections:
     - **Blast Radius Summary:** "5 repos affected (2 direct, 3 transitive)"
     - **Dependency Tree:** visual tree of direct → transitive dependents
     - **Per-Repo Health:** table with columns: Repo, Pinned Version, Current Version, Drift, Test Status, Cascade Result
     - **Cascade Test Status:** if cascade tests ran (TASK-022), show pass/fail per dependent repo; if not yet run, show "Not run" with a "Trigger Cascade Tests" button

2. **Trigger points:**
   - In the merge queue (PipelineOverview or TASK-049 DAG): "Impact Analysis" button on MRs that contain breaking changes
   - In MR detail (DetailPanel): "Check Impact" button when the MR has been flagged as a breaking change
   - The modal opens, fetches `GET /repos/:id/blast-radius`, and displays the analysis

3. **Acknowledgment flow:**
   - For `block` policy workspaces, the modal shows: "Merge blocked until all dependents acknowledge"
   - Each dependent repo row has an "Acknowledge" button (calls `POST /dependencies/breaking/:id/acknowledge` from TASK-020)
   - Once all dependents are acknowledged, the merge block is lifted

4. **Graceful degradation:**
   - If TASK-020 endpoints are not available, show blast radius without breaking change details
   - If TASK-022 cascade tests are not available, show "Cascade tests: not configured"
   - If TASK-021 version drift data is not available, show "--" for drift columns

5. **Tests:**
   - Component test: modal renders with mock blast radius data
   - Component test: dependency tree displays direct and transitive dependents
   - Component test: per-repo health table renders with version drift and test status
   - Component test: acknowledge button calls correct endpoint
   - Component test: graceful degradation when optional data is unavailable

## Acceptance Criteria

- [ ] `ImpactAnalysisModal.svelte` renders blast radius summary and dependency tree
- [ ] Per-repo health table shows: repo name, pinned version, current version, drift, test status
- [ ] Cascade test results shown when available (pass/fail per dependent repo)
- [ ] "Trigger Cascade Tests" button shown when cascade tests have not run
- [ ] Modal accessible from merge queue and MR detail for breaking-change MRs
- [ ] Acknowledgment flow: "Acknowledge" button per dependent repo for `block` policy
- [ ] Graceful degradation when breaking change or cascade test data is unavailable
- [ ] Component tests verify rendering and interaction
- [ ] `npm test` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `specs/system/dependency-graph.md` §UI and §Impact Analysis for the full requirements
3. Read `web/src/components/PipelineOverview.svelte` for the merge queue UI (trigger point)
4. Read `web/src/lib/DetailPanel.svelte` for the MR detail view (second trigger point)
5. Read `web/src/lib/api.js` for existing blast-radius and dependency API calls
6. Verify `GET /api/v1/repos/:id/blast-radius` exists in `crates/gyre-server/src/api/mod.rs`
7. Check if TASK-020 endpoints exist before using them; implement graceful fallback
8. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- `3cd4d3e9` feat(ui): add breaking change impact analysis modal (TASK-052)
