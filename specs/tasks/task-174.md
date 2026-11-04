---
title: "Explorer scope views — tenant workspace cards, workspace graph, repo architecture"
spec_ref: "ui-layout.md §5"
depends_on: [task-170]
progress: ready-for-review
coverage_sections:
  - "ui-layout.md §5. Explorer at Each Scope"
  - "ui-layout.md §Tenant Scope — Workspace Cards"
  - "ui-layout.md §Workspace Scope — Realized Architecture"
  - "ui-layout.md §Repo Scope — Architecture Detail"
commits: ["65f30e2a"]
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

## Shipped

Explorer scope views per ui-layout.md §5, implemented at all three scope levels with production wiring.

### Behavior

- **Tenant scope (verified pre-existing)**: `WorkspaceCards.svelte` renders the card grid with workspace name, repo count, active agent count, budget % bar, and trust level badge per card. Data from `GET /api/v1/workspaces` + `GET /api/v1/workspaces/:id/budget` (plus workspace repos for counts, batched 4 at a time). Enter Workspace navigates through the host shell.
- **Workspace scope (new)**: `ExplorerView.svelte` workspace branch now renders the realized-architecture graph canvas instead of a repo list. Every workspace repo is a node (including repos with zero dependencies), with cross-repo dependency edges from `GET /api/v1/workspaces/:id/dependency-graph`; external repos referenced by edges render as nodes so cross-workspace edges resolve. This is the C4 "Container" level. Zero-repo workspaces get the spec empty state: "No repos in this workspace yet. Create a repo to start." with a create-repo link. Single-click a repo node opens a repo metadata detail panel (name, description, default branch, created/updated, repo id) without changing scope. Double-click drills to crate level as a real scope change: the host shell navigates to repo mode's Architecture tab (URL `/workspaces/:slug/r/:repo/architecture`) via the `onEnterRepo` prop; a keyboard-reachable "Explore architecture" button in the detail panel performs the same drill; when no host shell is available the view falls back to loading the repo's crate-level graph in place with a back-to-repos control. A dependency-graph fetch failure is non-blocking (repo nodes still render, warning chip shown).
- **Production reachability**: the workspace home Architecture section (ui-navigation.md §2) now embeds the workspace-scoped ExplorerView (embedded mode, page header suppressed) instead of the previous unscoped merged code graph, and wires the repo drill through `onSelectRepo(repo, 'architecture')` -> `App.goToRepo` so the URL and breadcrumb update. Double-clicking a repo from another workspace is refused with an error toast (scope containment).
- **Repo scope (verified pre-existing)**: ExplorerCanvas boundary view with progressive drill-down (boundary -> modules -> types via Contains edges, breadcrumb with `#drill=` URL deep-linking) and NodeDetailPanel showing spec linkage (spec_path / governed-by) and churn metrics (churn_count_30d bar and label). Unchanged by this task; covered by the existing ExplorerCanvas (132 tests) and NodeDetailPanel (60 tests) suites.
- **Code sub-view**: superseded per ui-navigation.md's relationship table ("The Explorer's Code sub-tab (Architecture/Code toggle in the control bar) is moved to a separate repo-mode tab"). Code is a peer repo-mode tab (`RepoMode` 'code' -> `ExplorerCodeTab`) and was already implemented; no control-bar toggle was added, per the superseding spec.

### Test evidence

- New `web/src/__tests__/ExplorerViewWorkspaceGraph.test.js` (9 tests, all passing): repos-as-nodes rendering including isolated and external repos; dependency edges rendered; spec empty-state message with create link and no canvas; single-click opens the repo metadata panel and asserts no scope change; double-click invokes `onEnterRepo` with the repo identity, closes the detail panel, and performs no in-place fetch; detail-panel drill button; in-place fallback calls `api.repoGraph`; dependency-fetch failure keeps repo nodes with a warning; dependency graph fetched scoped to the workspace id.
- `WorkspaceHomeArchitecture.test.js` rewritten for the new section wiring (14 tests, all passing): expanded section renders the embedded workspace-scoped ExplorerView (scope type + workspace id asserted on the stub), repo drill routes through `onSelectRepo(repo, 'architecture')`, external-workspace repo is refused with toast, plus the retained collapsed-by-default / toggle / aria assertions.
- `ExplorerViewScope.test.js` api mock extended with `workspaceDependencyGraph`.
- Re-run green: WorkspaceCards, DependencyGraph, ExplorerViewScope, ExplorerView-responsive, WorkspaceHome, WorkspaceHomeSections, WorkspaceHomeRulesFailure, WorkspaceDrawerSectionNav, RepoMode, RepoModeHeader, AppShell, NoSidebar, BidirectionalNav, CrossWorkspaceHome (313 tests across the focused set). `npm run build` (vite production build) passes.
