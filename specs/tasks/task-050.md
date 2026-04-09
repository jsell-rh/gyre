# TASK-050: Repo Detail — Cross-Repo Dependency Section

**Spec reference:** `dependency-graph.md` §UI (lines 230–232)  
**Depends on:** TASK-020 (breaking change detection, for alerts), TASK-021 (version drift, for drift indicators)  
**Progress:** `not-started`

## Spec Excerpt

From `dependency-graph.md` §UI:

> **Repo detail page:** dependency section showing what this repo depends on and what depends on it. Version drift indicators. Breaking change alerts.

## Current State

The backend exposes the required data via existing endpoints:

- `GET /api/v1/repos/:id/dependencies` — outgoing code/spec/API dependencies (registered at `mod.rs:522–524`)
- `GET /api/v1/repos/:id/dependents` — repos that depend on this one (registered at `mod.rs:530–532`)
- `GET /api/v1/repos/:id/blast-radius` — transitive impact analysis (registered at `mod.rs:534`)

**No frontend component displays this data on the repo page.** `web/src/components/RepoMode.svelte` has no dependency section. The only dependency UI is the MR-level dependency display in `DetailPanel.svelte` (MR `depends_on` edges), which is unrelated to cross-repo code dependencies.

After TASK-020 and TASK-021:
- `GET /api/v1/dependencies/breaking` — unacknowledged breaking changes (to be created by TASK-020)
- `GET /api/v1/dependencies/stale` — stale dependencies (to be created by TASK-021)

## Implementation Plan

1. **New component** — `web/src/components/RepoDependencies.svelte`:
   - Two-column layout: "Dependencies" (outgoing) and "Dependents" (incoming)
   - Each entry shows: target/source repo name, dependency type badge (code/spec/api/schema/manual), version pinned, version drift indicator
   - Entries with `status = "Stale"` show a yellow warning badge with drift count
   - Entries with `status = "Breaking"` show a red alert badge with change description
   - Clicking a repo name navigates to that repo's detail page

2. **Integration into RepoMode.svelte:**
   - Add a collapsible "Dependencies" section to the repo detail view (alongside existing Architecture, Metrics sections)
   - Fetch dependency data on section expand (lazy load)
   - Show summary counts in the section header: "3 dependencies, 2 dependents, 1 stale"

3. **Blast radius summary:**
   - Add a "Show Impact" button that calls `GET /repos/:id/blast-radius`
   - Display as a tree: direct dependents → transitive dependents
   - Total blast radius count in the header

4. **Breaking change alerts:**
   - If any dependency has `status = "Breaking"`, show a prominent alert at the top of the section
   - Alert includes: which dependency has a breaking change, link to acknowledgment action
   - Depends on TASK-020 endpoints being available; gracefully degrade if not

5. **Version drift indicators:**
   - Each dependency row shows drift count when `version_drift > 0`
   - Visual: green (0 drift), yellow (1-2), orange (3+), red (exceeds policy max)
   - Depends on TASK-021 populating `version_drift`; show "--" if not available

6. **Tests:**
   - Component test: renders dependency list with mock data
   - Component test: stale/breaking badges render correctly
   - Component test: blast radius tree renders
   - Component test: empty state when no dependencies

## Acceptance Criteria

- [ ] `RepoDependencies.svelte` component renders outgoing dependencies and incoming dependents
- [ ] Each dependency shows type badge, version pinned, and version drift indicator
- [ ] Stale dependencies highlighted with yellow warning badge
- [ ] Breaking changes highlighted with red alert badge
- [ ] Clicking a repo name navigates to that repo's detail
- [ ] "Show Impact" button displays blast radius tree
- [ ] Section integrated into RepoMode.svelte as collapsible section
- [ ] Summary counts shown in section header
- [ ] Component tests verify rendering with mock data
- [ ] `npm test` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `specs/system/dependency-graph.md` §UI for the spec requirements
3. Read `web/src/components/RepoMode.svelte` for the existing repo detail layout
4. Read `web/src/lib/api.js` for existing dependency API calls (search for `dependencies`, `dependents`, `blast-radius`)
5. Read `web/src/components/WorkspaceHome.svelte` for the collapsible section pattern to reuse
6. Verify endpoints exist in `crates/gyre-server/src/api/mod.rs` before calling them
7. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

_(none yet)_
