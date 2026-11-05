# Review — task-174: Explorer scope views (tenant workspace cards, workspace graph, repo architecture)

**Candidate:** `99db792af3fc61d7d8d7e122698eae216c888ba5` (base `110180fbb248d6fa408492c0f9a3ec4a5a724372`)
**Reviewer round:** independent review, 2026-11-05
**Verdict:** approved (see /tmp/stage/verdict.json)

## Scope of diff

Frontend only: `ExplorerView.svelte` (workspace-scope realized-architecture branch), `DependencyGraph.svelte` (`onNodeDblClick`, `showScopeToggle`), `WorkspaceHome.svelte` (Architecture section now embeds the workspace-scoped ExplorerView), `en.json` locales, five test files (one new), rebuilt `web/dist`, plus `specs/tasks/task-174.md`. The data endpoint `GET /api/v1/workspaces/:id/dependency-graph` is pre-existing (task-170 lineage) with an ABAC `RouteResourceMapping` entry — unchanged by this diff.

## Acceptance criteria — evidence

1. **Workspace cards (repo count, agent count, budget %, trust level)** — pre-existing `WorkspaceCards.svelte` verified in code (budget/agent/repo enrichment from `GET /workspaces` + `/:id/budget`, trust Badge); `WorkspaceCards.test.js` green.
2. **Workspace-scope graph: repos as nodes, dependency edges** — `ExplorerView` renders `DependencyGraph` fed by `wsRepos` (scoped `api.repos({workspaceId})`) unioned with edge-referenced external repos (dedup by `repo_id`); edges from `api.workspaceDependencyGraph(wsId)`. `ExplorerViewWorkspaceGraph.test.js` (9 tests) asserts nodes incl. isolated + external repos, edge count, scoped fetch.
3. **Empty state** — exact spec text ("No repos in this workspace yet. Create a repo to start.") with create link; test asserts message, link, and no canvas. The link routes to workspace home rather than "Admin" — consistent with ui-navigation.md's amendment of repo-lifecycle.md §1 (repo management moved from Admin to workspace home Repos section, where the create form lives).
4. **Repo scope C4 L2 with progressive drill-down** — pre-existing ExplorerCanvas (boundary → modules → types, breadcrumb, `#drill=` deep-links); `ExplorerCanvas.test.js` + `NodeDetailPanel.test.js` re-run green on this tree (196 tests).
5. **Detail panel spec linkage + churn** — pre-existing NodeDetailPanel (`spec_path`/governed-by, `churn_count_30d` bar); unchanged.
6. **Code sub-view / control-bar toggle** — correctly superseded: ui-navigation.md "Amends" table explicitly moves ui-layout.md §5's Architecture/Code control-bar toggle to a peer repo-mode tab; verified `RepoMode` renders `ExplorerCodeTab` for the `code` tab. No toggle added, per the superseding spec.
7. **Tests pass** — focused re-run: 30 (new/rewritten drill suites) + 210 (regression set incl. RepoMode, AppShell, ScopeTransitions, CrossWorkspaceHome) + 196 (ExplorerCanvas/NodeDetailPanel), all green after `npm ci`.

## Independent probes (evidence: /tmp/stage/review-evidence/)

- **Mutation sensitivity:** temporarily replaced DependencyGraph's `ondblclick` wiring with `undefined` → 7 failures across `ExplorerViewWorkspaceGraph`, `WorkspaceHomeScopeDrill`, `ExplorerViewDrillUrl` (double-click drill, URL pushState/popstate, external-refusal). The drill tests are behavior-sensitive, not self-confirming. File restored; tree clean.
- **Production build:** `npm run build` exits 0; post-build `git status web/dist` clean — the committed dist reproduces byte-identically from the merged sources (no stale bundle).
- **Integration depth:** `WorkspaceHomeScopeDrill.test.js` drives real WorkspaceHome → real ExplorerView → real DependencyGraph through single-click (detail panel, no scope change), double-click (`onSelectRepo(repo, 'architecture')`), and external-repo refusal (toast, no navigation).

## Scope containment

Workspace repo list fetched with `workspaceId`; dependency graph scoped server-side to the workspace; external repos render read-only and their drill is refused with an error toast in `handleArchEnterRepo`. Mount guard prevents ExplorerView embedding with a null workspace.

## Merge-repair check

Base's task-173 drill-URL machinery (`selectRepo` pushState `?repo=`, mount-time restore, popstate sync, `backToRepoList`) preserved and coexists with the new workspace graph; `ExplorerViewDrillUrl.test.js` adapted to drive via double-click on `dep-node-*` with all four URL assertions retained and passing.

## Notes (non-blocking)

- The workspace-scope control bar (lens/view/search/ask below the canvas) named in ui-layout.md §5 is not part of this task's acceptance criteria; coverage row 7 ("Canvas + Controls") assigns it to task-178.
- In the embedded workspace-home context, the empty-state create button navigates to the page the user is already on (harmless no-op); standalone it correctly routes to workspace home where repo creation lives.

## Verdict

Approved. All acceptance criteria are met by production code with behavior-sensitive tests; no fakery, no unsafe scope handling, no test inflation found.
