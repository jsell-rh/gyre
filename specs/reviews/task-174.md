# Review — task-174 (Explorer scope views — tenant workspace cards, workspace graph, repo architecture)

Spec: `specs/system/ui-layout.md` §5 "Explorer at Each Scope" (Tenant/Workspace/Repo scope sections).
Candidate: `31bb2977e0d02cb7ac6b7e5a85afafa710927668` (merge of base `fefde2946f3a0af1f212588f54591516fee0f7b2`, second merge round; task commits `65f30e2a0`, `c64bccf4b`).
Verdict: **approved**.

## What was reviewed

Diff base→candidate touches only the frontend surface plus its task record: `ExplorerView.svelte` (workspace-scope realized-architecture canvas replacing the repo-card list), `DependencyGraph.svelte` (double-click drill prop, scope-toggle hiding, taller node boxes with type/endpoint count sub-labels), `WorkspaceHome.svelte` (Architecture section embeds the workspace-scoped ExplorerView, drill routed through `onSelectRepo(repo, 'architecture')` → `App.goToRepo`), `en.json`, `docs/ui.md`, `specs/tasks/task-174.md`, and six test files. No Rust, script, or production-spec changes.

## Independent evidence (full logs: /tmp/stage/review-evidence/)

- **Full frontend suite**: `npx vitest run` after `npm ci` → 90 files, **1942 passed / 41 skipped**, exit 0. Skips are pre-existing (no `.skip` added in the diff).
- **Focused suites** matching the shipped claim all green: ExplorerViewWorkspaceGraph (new, 9), WorkspaceHomeScopeDrill (rewritten as a real-component integration test), ExplorerViewDrillUrl (4 URL-behavior assertions, real spied `history.pushState`), WorkspaceHomeArchitecture (14), ExplorerDefaultViews/ExplorerViewLensUrl (upstream tests correctly adapted to the canvas surface), DependencyGraph, ExplorerCanvas, ExplorerCanvasDrillDown, NodeDetailPanel, WorkspaceCards, RepoMode, CrossWorkspace*, AppShell, etc.
- **Mutation probe (test strength)**: temporarily setting `onNodeDblClick={null}` in ExplorerView made 4 tests fail across ExplorerViewWorkspaceGraph and WorkspaceHomeScopeDrill (both host-drill tests, the in-place fallback test, and the external-repo refusal test); file restored, tree verified clean. The double-click drill tests are load-bearing, not self-confirming.
- **Production build**: `vite build` to a temp outDir succeeds.
- **Mechanical checks**: `check-arch.sh`, `check-task-commit-attribution.sh`, and all frontend test-quality checks (mirrored-logic, tautological, conditional-guard, phantom-API, dead-test-code, exhaustive-ui-state, noop-callbacks, stale-mechanism-claims, self-contradicting-comments, identical-baselines) pass. `check-dead-components.sh` reports 4 violations — verified byte-identical at the base (pre-existing, not introduced).
- **API wiring**: `GET /api/v1/workspaces/:id/dependency-graph` exists (api.js:527, api/mod.rs:582, dependencies.rs `get_workspace_dependency_graph` — pre-existing TASK-046 route, ABAC-exempt at frozen baseline; not touched by this candidate). The workspace-scope fetch is asserted to pass the scope's workspace id.

## Acceptance criteria

- **Workspace cards (repo count, agents, budget %, trust)** — pre-existing `WorkspaceCards.svelte`, unchanged, tests assert each field. Note (pre-existing, not a candidate defect): the component's mount path via ExplorerView's tenant branch was equally absent from production callers at the base (`git grep` at fefde294 shows ExplorerView rendered only from RepoMode); tenant workspace cards are served by `CrossWorkspaceHome` in the current navigation model.
- **Workspace-scope graph: repos as nodes, dependency edges** — real implementation: every workspace repo is a node (isolated repos included), edges from the workspace dependency-graph endpoint, external edge endpoints rendered as nodes, fetch failure non-blocking (warning chip, nodes still render). Verified by tests plus code read.
- **Empty state** — exact spec text ("No repos in this workspace yet." / "Create a repo to start.") with a create action; no canvas rendered. Spec's "link to Admin" is resolved to workspace-home navigation per ui-navigation.md's supersession of the Admin/sidebar model (the New Repo form lives on workspace home); disclosed in the shipped notes.
- **Single-click vs double-click** — single-click opens a repo-metadata detail panel with no scope change (asserted: no `onEnterRepo`, no `repoGraph` fetch); double-click is a real scope change through the host shell (`onSelectRepo(repo,'architecture')` → `App.goToRepo` → mode/tab/pushState → URL `/workspaces/:slug/r/:repo/architecture`), closes the panel, and refuses cross-workspace repos with an error toast (scope containment). A keyboard-reachable "Explore architecture" button performs the same drill; without a host shell the view falls back to the in-place crate-level drill with `?repo=` deep-linking and popstate sync (task-173 machinery preserved through the merge).
- **Repo scope C4 L2 + drill-down, detail panel spec linkage/churn** — pre-existing ExplorerCanvas/NodeDetailPanel, untouched by the diff, suites green.
- **Code sub-view toggle** — superseded by ui-navigation.md:611 ("The Explorer's Code sub-tab … is moved to a separate repo-mode tab"); Code is a peer RepoMode tab. Spec-conformant supersession, documented.
- **Tests pass** — full suite green.

## Notes (non-blocking)

- Committed `web/dist` is byte-identical to the base's and does not contain the task-174 UI strings. This is the pipeline's designed artifact flow: `pipeline-artifacts.py` restores base dist during conflict resolution and rebuilds/commits it at integration (`build(web): regenerate verified assets`), so it is not a candidate defect. The current source builds cleanly (verified).
- Merge resolution was inspected against both parents: the upstream task-088/089 surfaces (view selector, Generated Views, arch sub-tabs) are preserved and unioned with the workspace canvas; the upstream repo-card grid's HSI §3 Level 1 node content (type/endpoint counts) is retained as in-node sub-labels, and `docs/ui.md` was updated to match. Upstream tests were adapted to the specced surface rather than deleted.
