# Review — task-083 (Canonical Navigation — Scope-Aware Content Routing)

Spec: `specs/system/ui-navigation.md` §2 (Workspace Home), §3 (Repo Mode), §7
(URL table), §10 (Cross-Workspace View); the deeper HSI §2–§12 behaviors as
amended by the task's Authority and Scope note.
Candidate: `2499d20b4dcb84f9e58b615327a9de794005dc19` (branch
`pipeline/task-083/6984bf7c41cf4914af7d78ba2c211b3f-1`), assigned base
`4d6759a0bfd637e899bbf65e725f91b07f745c69`.
Verdict: **approved** (round covering the `084040ecc` /all page-load repair
and the `c38c8e003` specs-scope repair; earlier-round findings from the
`9ab2a0e0` and `1d994c8f` rounds are closed and independently re-verified
below). Evidence: `/tmp/stage/review-evidence/` (README + raw outputs).

## Verification performed (candidate, unmodified)

- `npm ci` then `npx vitest run` (full web suite): **65 files / 1665 passed,
  41 skipped, 0 failed**. Navigation-focused subset (17 files incl.
  CrossWorkspaceEntry, CrossWorkspaceHome, WorkspaceHome*,
  WorkspaceDrawerSectionNav, ScopeTransitions, AppShell, RepoMode*, Briefing,
  Inbox, ExplorerViewScope): 438 passed / 0 failed.
- `cargo test -p gyre-server --lib api::specs::tests` → 52 passed / 0 failed;
  `api::graph::tests` → 28 passed / 0 failed.
- `scripts/check-arch.sh` and `scripts/check-abac-route-registry.sh` pass.
- All 7 commits listed in the task frontmatter exist on the branch; the
  assigned contract prose is byte-identical to the assignment (only
  `progress:`, `commits:`, and the appended `## Shipped` operational section
  differ).

## Mutation probes (isolated worktree, each restored)

Every mutation was killed by exactly the test claiming the behavior:

1. Restoring the old `if (!currentWorkspace) return;` early return in
   `goToEntityDetail` → both `CrossWorkspaceEntry.test.js` tests fail.
2. Disabling the `?workspace_id=` filter in `list_specs` →
   `list_specs_honors_workspace_id_scope` fails.
3. Reverting the `/all` Specs section to the unscoped
   `specsForWorkspace(null)` → `fetches specs per accessible workspace, not
   the unscoped list` fails.
4. Ignoring the validated `?repo_id=` in GET briefing →
   `briefing_get_repo_filter_scopes_and_validates_at_http_level` fails.
5. Disabling the `wsLoadGen` stale-response guard in WorkspaceHome →
   `keeps workspace B rules when workspace A finishes loading after
   navigation` fails.

## Findings against the acceptance criteria

- **Cross-workspace attribution links enter the owning workspace/repo.**
  Verified: `goToEntityDetail` resolves the owner from
  `data.workspace_id ?? entityRepo.workspace_id`, membership-checks it (no
  fabricated scope identity), persists the selection, and enters
  `/workspaces/:slug/r/:repo/...` — proven by the real-App regression test
  and mutation 1. The /all page-load path (currentWorkspace null) works; an
  undeterminable owner surfaces a rendered `role=alert` error toast, stays at
  `/all`, and persists nothing.
- **Workspace sections and repo tabs issue queries for their selected
  scope.** Verified in code: every §2 section loader in WorkspaceHome passes
  `workspace.id` (myNotifications + client filter, workspaceRepos,
  specsForWorkspace, meta-specs Workspace scope, workspaceGraph, tasks/MRs/
  agents queries, getWorkspaceBriefing); §10 sections fetch per accessible
  workspace (`Promise.allSettled`, attribution backfilled); the `/all` Specs
  aggregation is per-workspace, server-enforced (`?workspace_id=` filter with
  repo-ownership resolution — mutation 2), not the unscoped list (mutation 3).
- **Repo-mode entry defaults to Specs; deep links and back navigation retain
  scope.** `goToRepo(repo, tab='specs')`; `parseUrl`/`urlFor` implement the
  §7 table including multi-segment spec paths and entity detail routes;
  `handlePopstate` restores workspace, repo (repoIdCache + API fallback), and
  tab; the Architecture `?subTab=briefing` deep link is retained after mount
  (repair `7c2b9757`, covered by ExplorerViewScope tests).
- **Delayed old-scope responses cannot replace current-scope content.**
  Single `wsLoadGen` bump per workspace change; every loader discards stale
  success AND failure results (mutation 5 kills the regression). Inbox and
  Briefing carry equivalent per-component generation guards.
- **Scoped request failures are visible and recoverable.** All six §2
  sections and all five §10 sections render `role=alert` error states (Rules
  with Retry; WorkspaceHomeRulesFailure proves failure ≠ empty success);
  backend repo filters 404 unknown repos and 403 cross-workspace repos;
  `briefing/ask` applies a per-repo ABAC check to the caller-supplied body
  `repo_id` before accepting the scope (JWT-admin bypass covered by
  `briefing_ask_repo_scope_abac_denied_returns_403`). Repo Decisions filter
  on exact `repo_id` so workspace-only notifications never appear under a
  repo.
- **Canonical shell / mobile navigation intact.** No permanent sidebar
  (NoSidebar.test.js passes); drawer section navigation at repo scope stays
  in the repo (Briefing → Architecture sub-tab, not workspace home); mobile
  drawer tests pass. The superseded 6×3 matrix is not restored.
- **No side effects:** specs/coverage/ is untouched by the diff; task
  acceptance checkboxes remain unchecked in the contract prose.

## Non-blocking observations

- `goToEntityDetail` still ends with a pre-existing `if (!currentRepo) return;`
  silent no-op when an entity's repo cannot be resolved at all (deleted repo /
  API failure) — present verbatim at the assigned base and outside this
  task's diff; the owner-workspace resolution added here handles its failure
  case with an error toast. Worth a follow-up task if the silent path is ever
  observed in practice.

## Verdict

Approved. The task contract's required behaviors are implemented as real
production code (server-side scope filters with 403/404 validation and ABAC
gating, membership-checked scope switching, generation-guarded loaders,
error-rendering sections), and every claim is backed by a regression test
that demonstrably fails when the behavior is disabled.
