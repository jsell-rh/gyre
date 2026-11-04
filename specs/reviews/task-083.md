# Review — task-083 (Canonical Navigation — Scope-Aware Content Routing)

Spec: `specs/system/ui-navigation.md` §2 (Workspace Home), §3 (Repo Mode), §7 (URL Structure), §10 (Cross-Workspace View), via the task's required-behavior items (scope-aware content routing, no restored sidebar).
Candidate under review: `f6c6402d8ef7a1581fcf738fbb01bdbb4a0de204` on branch `pipeline/task-083/adfa1e7c46f34f84ac8adc621df2108a-3`, diffed against assigned base `ed57c723631e7ad99b8983cf127fdca3eefb1a9a`. Evidence: `/tmp/stage/review-evidence/` (README.md indexes every log).

Verdict: **not approved** — one disqualifying production defect (F1) plus two smaller regressions (F2, F3). The scope-aware routing work itself is largely real and well-tested (details below); the defects are collateral damage from the same diff.

## Findings

### F1 (disqualifying) — Architecture node drill-down crashes: `NodeDetailPanel` import removed but component still used

`web/src/components/ExplorerView.svelte` — the diff removes `import NodeDetailPanel from '../lib/NodeDetailPanel.svelte';` (dropped in task-listed commit `8ee005df`, still missing through merge head `a387ad98` and candidate `f6c6402d`) while the component is still instantiated at line 1675 in the shared canvas detail area (`{:else if detailNode}` branch).

Reproduction (evidence: `probe-nodedetail.log`, `vite-build.log`):
- Runtime: mount ExplorerView at repo scope, fire the canvas `onNodeDetail` callback (any node click) → `ReferenceError: NodeDetailPanel is not defined` thrown from ExplorerView.svelte:1675. The node detail panel — the §3 "Architecture → Spec" drill-down ("Double-click any graph node → detail panel shows which spec(s) govern this node") — is broken at repo and workspace scope.
- The committed rebuilt dist (`8a4a42a8`) ships the dangling global: `web/dist/assets/index-BPlFwZSI.js` contains `NodeDetailPanel(ba, {...})` with no binding. `vite build` exits 0 with no warning (Rollup treats the unresolved identifier as an implicit global), so the break is silent at build time.
- The full frontend suite (65 files, 1598 passed — matches the Shipped claim; re-run in this review) stays green because no test renders the node-detail branch. Not test inflation by intent, but the missing coverage hides a crash.

Fix: restore the import (one line). The task explicitly says "Reuse working behavior" — this deleted it.

### F2 (moderate) — drawer/sidebar Briefing is dead navigation when already on the Architecture tab

`web/src/components/ExplorerView.svelte:731-745` — `archSubTab` is internal state initialized from `?subTab=` in a `$effect` that only tracks `scopeType`, so it runs on mount/scope-change only. `App.svelte`'s `repoArchSubTab` flows child→parent (`onArchSubTabChange`) but is never passed back down. When the user is already on the Architecture tab (Graph sub-tab) and clicks Briefing in the drawer/sidebar, `goToRepoTab('architecture', { subTab: 'briefing' })` rewrites the URL to `?subTab=briefing` and moves the drawer highlight, but ExplorerView is not remounted and its internal sub-tab stays `'graph'` — URL and visible content diverge (evidence: `probe-subtab.log`: `arch-briefing visible: false | graph subtab still active: true | URL: ?subTab=briefing`). The AppShell test only covers the from-another-tab path (fresh mount), which is why the suite stays green. This violates the acceptance item "deep links and back navigation retain scope" for this state.

### F3 (minor) — merge-queue ordering regression in `loadMergeQueue`

`web/src/components/WorkspaceHome.svelte` (~line 961) — the refactor dropped `.sort((a, b) => (a.position ?? a.priority ?? 0) - (b.position ?? b.priority ?? 0))` from the `mergeQueueItems` chain with no replacement. The sidebar merge-queue widget renders `mergeQueueItems.slice(0, 5)` with `#{i + 1}` position badges (line 1908), so displayed queue order now depends on raw API response order instead of queue position.

## Verified working (no findings)

- **Server-side repo-scoped briefing** (`crates/gyre-server/src/api/graph.rs`): genuine enforcement — `?repo_id=` on GET briefing validated for existence (404) and workspace membership (403); `briefing_ask` carries an ABAC caller-scope guard on the request-body `repo_id` (403; JWT-admin bypass test included), plus the same 404/403 validation; `assemble_briefing` filters MRs, tasks, cross-workspace links, spec-assertion notifications, and completed agents (via `agent.repo_id`, workspace-orchestrator agents correctly excluded under a filter); grounding JSON and `sources` are repo-scoped; MCP resource path unchanged (workspace scope). Focused run: `api::graph::tests` 28/28 passed. Mutation probe (filter disabled in the GET handler) makes `briefing_get_repo_filter_scopes_and_validates_at_http_level` fail — the regression tests are real, not self-confirming.
- **Inbox repo scope**: exact client-side `repo_id` match excludes `repo_id: null` workspace-only notifications (test asserts the exclusion); workspace filter is server-side via `?workspace_id=`, which `users.rs::get_my_notifications` genuinely applies.
- **Cross-workspace view (§10)**: five sections from real APIs; `goToEntityDetail` resolves the entity's repo, derives the owning workspace, and enters it only when present in the caller's membership list (error toast otherwise — no fabricated scope identity); spec rows carry `workspace_id`/`repo_id` for attribution; tenant settings/agent-rules stay gated by `userIsAdmin` (App.svelte:1684-1685).
- **Workspace home (§2)**: all section loaders query the selected workspace's id behind the `wsLoadGen` generation guard (single bump per scope change; stale success and error results discarded); Agent Rules merges `scope=Workspace&scope_id=:id` with all `scope=Global` rules, grouped by kind with scope badges and lock on required; failed lookup surfaces as error + Retry.
- **Repo mode (§3/§7)**: `goToRepo` defaults to Specs; all five tabs URL-addressable incl. multi-segment spec paths and entity detail routes; popstate restores workspace/repo/tab; Architecture Graph|Briefing sub-tab exists with deep link (modulo F2).
- **Shell intact**: no sidebar restored; mobile drawer navigation tests pass.
- **Mechanical checks** at the candidate head: `check-task-commit-attribution.sh`, `check-forged-scope-fields.sh`, `check-arch.sh`, `check-inert-enforcement.sh` all OK; no exemptions added. (The frontmatter `commits:` restoration to the assigned 16-entry list in `f6c6402d` drops `8a4a42a8`/`a387ad98` again, but `8a4a42a8` touches only `web/dist`, which is outside the attribution check's product-surface scope, so no mechanical violation; noting for verifier scoping awareness.)

— Reviewer, 2026-11-04
