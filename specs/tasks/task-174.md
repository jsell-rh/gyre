---
title: "Explorer scope views — tenant workspace cards, workspace graph, repo architecture"
spec_ref: "ui-layout.md §5"
depends_on: [task-170]
progress: not-started
coverage_sections:
  - "ui-layout.md §5. Explorer at Each Scope"
  - "ui-layout.md §Tenant Scope — Workspace Cards"
  - "ui-layout.md §Workspace Scope — Realized Architecture"
  - "ui-layout.md §Repo Scope — Architecture Detail"
commits: []
---

## Spec Excerpt

ui-layout.md §5 defines how the Explorer renders at each scope level:

**Tenant Scope — Workspace Cards**: Not a canvas. Grid of cards with workspace name, repo count, active agents, budget %, trust level. Data from `GET /api/v1/workspaces` + `GET /api/v1/workspaces/:id/budget`.

**Workspace Scope — Realized Architecture**: Default Boundary View. Graph canvas showing repos as nodes, cross-repo dependencies as edges. Empty state if no repos. Double-click repo → drill to crate level. C4 "Container" level.

**Repo Scope — Architecture Detail**: Default Boundary View (C4 Level 2 — crates/packages). Drill-down to modules (Level 3) and types (Level 4). Detail panel shows type/function detail with spec linkage, churn metrics. Code sub-view accessed via tab in control bar. When Code active, canvas-specific controls hidden.

## Implementation Plan

1. **Tenant scope — WorkspaceCards**:
   - Already exists as `WorkspaceCards.svelte`
   - Verify data population: repo count, agent count, budget %, trust level per card
   - Ensure click → enter workspace, layout is responsive grid

2. **Workspace scope — Realized Architecture**:
   - Enhance `ExplorerView.svelte` for workspace scope
   - Show repos as top-level nodes with cross-repo dependency edges
   - Empty state: "No repos in this workspace yet" with create link
   - C4 Container level mapping
   - Double-click repo → scope change to repo

3. **Repo scope — Architecture Detail**:
   - Boundary View at C4 Level 2 (crates/packages)
   - Progressive drill-down: boundary → modules → types
   - Detail panel with spec linkage, churn metrics, provenance
   - Code sub-tab integration per spec: [Architecture] [Code] toggle in control bar

4. **Control bar behavior**:
   - When Architecture active: canvas + lens/view/search/ask controls
   - When Code active: hide canvas controls, show only tab toggle
   - Code tab uses Full-Width layout with table views

5. **Tests**:
   - Workspace cards render with correct data
   - Scope transition from workspace → repo on double-click
   - Empty state displayed correctly

## Acceptance Criteria

- [ ] Workspace cards show repo count, agent count, budget %, trust level
- [ ] Workspace-scope graph renders repos as nodes with dependency edges
- [ ] Empty state shown for zero-repo workspaces
- [ ] Repo-scope shows C4 Level 2 with progressive drill-down
- [ ] Detail panel shows spec linkage and churn metrics
- [ ] Code sub-view toggles canvas controls visibility
- [ ] Tests pass

## Agent Instructions

Read `ui-layout.md` §5 for the full scope-level specifications. The existing `WorkspaceCards.svelte` and `ExplorerView.svelte` are the starting points. Check the graph API endpoints: `GET /api/v1/repos/:id/graph` for repo-scope, workspace-level graph endpoint for workspace-scope. The C4 drill-down uses the existing graph's `depth` concept. Verify that double-click produces scope changes (URL update, breadcrumb change) vs single-click (detail panel only).
