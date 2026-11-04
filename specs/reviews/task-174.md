# Review — task-174 (Explorer scope views — tenant workspace cards, workspace graph, repo architecture)

Spec: `specs/system/ui-layout.md` §5 "Explorer at Each Scope" (Tenant/Workspace/Repo scope subsections), as amended by `specs/system/ui-navigation.md` ("Relationship to Existing Specs": Code sub-tab moved to a peer repo-mode tab; repo creation lives in workspace home Repos section, not Admin).
Commit under review: `65f30e2a` (product surface; candidate `cceb7bc8` adds only task-bookkeeping commits on top of the assigned base `2427ccc1`).
Verdict: **approved**.

## Round 1

Probes (evidence under `/tmp/stage/review-evidence/`; `npm ci` used so the locked toolchain ran):

- `npx vitest run ExplorerViewWorkspaceGraph WorkspaceHomeArchitecture ExplorerViewScope DependencyGraph` → **49 passed** (02-focused-tests.txt, exit 0).
- `npx vitest run WorkspaceCards WorkspaceHome WorkspaceHomeSections RepoMode RepoModeHeader CrossWorkspaceHome ExplorerView-responsive` → **169 passed, 41 skipped** (03-adjacent-tests.txt, exit 0).
- `npx vitest run ExplorerCanvas NodeDetailPanel` (repo-scope surfaces this task claims pre-existing) → **192 passed** (04-repo-scope-tests.txt, exit 0).
- `npm run build` → green; rebuilding produced **zero diff** against the committed `web/dist`, so the shipped bundle is current with the reviewed source (05-build.txt).
- Mutation probes (restored immediately; tree verified clean): disconnecting `onNodeDblClick={enterRepo}` in ExplorerView → **2 failures** in the new suite; disconnecting `onEnterRepo={handleArchEnterRepo}` in WorkspaceHome → **2 failures** in the rewritten suite (06-mutation-probes.txt). The new tests gate the real wiring; they are not self-confirming.

Verified against the acceptance criteria:

- **Tenant scope cards**: `WorkspaceCards.svelte` (pre-existing, unchanged) renders name, repo count, active agents, budget % bar, trust-level badge, Enter Workspace, responsive `auto-fill` grid; data from `GET /workspaces` + per-workspace budget/repos enrichment. The production tenant surface (`CrossWorkspaceHome`) carries the equivalent stats. Suite green.
- **Workspace-scope graph**: `ExplorerView.svelte` workspace branch now renders a `DependencyGraph` canvas with every workspace repo as a node (isolated repos included), cross-repo dependency edges from the real server endpoint `GET /api/v1/workspaces/:id/dependency-graph` (`api/mod.rs:581` → `dependencies.rs:291`), and external edge endpoints as nodes so cross-workspace edges resolve. Response shapes match the client (`GraphEdgeResponse` serde-renames `edge_type`→`type`; `source`/`target` are repo ids keyed to `GraphNode.repo_id`) — no fabricated or stubbed data path (07-api-wiring.txt). Dependency-fetch failure is non-blocking with a visible warning while repo nodes still render (tested).
- **Empty state**: exact spec text ("No repos in this workspace yet" / "Create a repo to start.") with a create link and no canvas (tested).
- **Single- vs double-click**: single-click opens a repo metadata detail panel with no scope change (test asserts `onEnterRepo` and `api.repoGraph` uncalled); double-click performs a real scope change through the host shell — `WorkspaceHome` → `onSelectRepo(repo, 'architecture')` → `App.goToRepo` → URL `/workspaces/:slug/r/:repo/architecture` with breadcrumb update. A repo from another workspace is refused with an error toast (scope containment, tested). Keyboard-reachable "Explore architecture" button performs the same drill; an in-place crate-graph fallback exists for host-less renders.
- **Repo scope**: pre-existing `ExplorerCanvas` boundary view with boundary→modules→types drill-down (`#drill=` deep-linking) and `NodeDetailPanel` spec linkage (`spec_path`/governed-by) + churn metrics (`churn_count_30d`); unchanged by this diff and covered by the 192-test ExplorerCanvas/NodeDetailPanel suites re-run green here.
- **Code sub-view**: correctly superseded — `ui-navigation.md` line 611 moves the Architecture/Code control-bar toggle to a peer repo-mode tab; `RepoMode` already renders `code` → `ExplorerCodeTab`. Not adding the toggle follows the superseding spec rather than reinterpreting it.
- **Control bar (lens/view/search/ask) at workspace scope** is absent from this diff but is explicitly task-178's coverage row ("Canvas + Controls"), not task-174's; no scope creep either way.

Minor (non-blocking, recorded for completeness):

- The empty-state "Create a repo" button calls the `goToWorkspaceHome` context, which resets the workspace home to its inbox section rather than targeting the Repos section where creation actually lives (there is no repos-section expand signal in the current plumbing). In the embedded production surface the click is therefore a weak affordance — it keeps the user on the page that contains the Repos section but does not surface it. The spec's literal "link to Admin" is itself superseded (repo creation moved to workspace home Repos section), and the button does navigate to that page, so the criterion "empty state with a create link" is met; a follow-up could add a repos-section signal analogous to `archExpandSignal`.
- `ExplorerView`'s `getContext('goToRepo')` fallback is dead code in production — no component ever sets a `goToRepo` context (only `goToRepoTab`). Harmless: the `onEnterRepo` prop is the wired path in the only production mount.
