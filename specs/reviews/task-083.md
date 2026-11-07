# Review: task-083 — Canonical Navigation — Scope-Aware Content Routing

Candidate `b7b4183687a47a2d7a84577e27d429c3a402a03d` against assigned base
`233664cf81cbb1189b2aa00f76e80c8445745955`. Review-repair round for the two
findings raised at review checkpoint `716f4c48` (candidate `9af70150d`).

## Round 1 (this review) — Resolution of prior findings

Both prior findings are fixed with real, mutation-verified work. Evidence for
every probe (commands, output, exit status) is under
`/tmp/stage/review-evidence/README.md`.

- [x] **F1 (test gap): myNotifications envelope regression.** Commit
  `7e5cdaba` adds two tests to `CrossWorkspaceHome.test.js` that mock the real
  server envelope (`{notifications:[...], limit, offset}` from
  `users.rs` get_my_notifications) and assert the decision items render and
  the empty state is not shown for a non-empty array. Mutation probe:
  reverting `CrossWorkspaceHome.svelte:286` to the pre-fix
  `data = Array.isArray(data) ? data : (data?.items ?? []);` fails exactly
  those 2 tests (2 failed | 36 passed). The tests are real regressions, not
  self-confirming.
- [x] **F2 (code): `?subTab` deep-link param stripped after Architecture
  mount.** Commit `7c2b9757` removes the `history.replaceState` param
  deletion from ExplorerView's initial-read `$effect` (App's
  `setRepoArchSubTab` already maintains the param per docs/ui.md
  "deep-linkable via ?subTab=briefing") and adds a regression test in
  `ExplorerViewScope.test.js` rendering the real ExplorerView at repo scope
  with an App-mimicking `onArchSubTabChange`; the param survives the mount.
  Mutation probe: restoring the deletion fails exactly that test
  (1 failed | 7 passed).

## Independent verification of the task contract

- **Repo mode (§3/§7):** `goToRepo(repo, tab = 'specs')` (App.svelte:356) —
  Specs is the default landing tab; tabs URL-addressable; repo-scoped
  Briefing narrowed server-side with HTTP-level tests
  (`briefing_ask_repo_scope_abac_denied_returns_403`,
  `briefing_ask_unknown_repo_returns_404`,
  `briefing_get_repo_filter_scopes_and_validates_at_http_level`,
  `briefing_repo_filter_narrows_sections_to_that_repo`,
  `briefing_repo_filter_completed_agents_by_agent_binding` — all pass under
  `cargo test -p gyre-server --lib api::graph`, 28/28). Repo Decisions
  (Inbox.svelte:122–126) filters on exact `repo_id === repoId`, so
  workspace-only notifications (`repo_id: NULL`) never appear under a repo.
- **Cross-workspace (§10):** `goToEntityDetail` (App.svelte:416) resolves the
  entity's repo, derives the owning workspace from `workspace_id`/repo
  lookup, switches `currentWorkspace` only when the workspace is in the
  caller's membership (error toast otherwise — no fabricated scope
  identity), persists the selection, and reloads scoped data.
- **Workspace home (§2):** `wsLoadGen` generation guard (WorkspaceHome.svelte)
  covers every section loader; delayed old-scope responses are discarded.
- **Canonical shell intact:** `NoSidebar.test.js` passes; the superseded
  6×3 sidebar matrix is not restored.
- **Contract prose:** the `233664cf..b7b41836` diff on `specs/tasks/task-083.md`
  changes only `progress:`, `commits:`, and the appended `## Shipped`
  operational section — the assigned contract text is byte-identical.

## Fixed-state test results

- CrossWorkspaceHome + ExplorerViewScope: 46/46 passed.
- Broad navigation suites (AppShell, CrossWorkspaceHome, ExplorerViewScope,
  ExplorerViewDrillUrl, ExplorerView-responsive, RepoMode, RepoModeHeader):
  174/174 passed.
- Remaining touched suites (Inbox, Briefing, WorkspaceHomeSections,
  WorkspaceHomeScopeDrill, WorkspaceHomeRulesFailure, ScopeTransitions,
  NoSidebar, WorkspaceDrawerSectionNav, TenantSettings, WorkspaceSettings):
  214 passed / 41 skipped / 0 failed.
- `cargo test -p gyre-server --lib api::graph`: 28/28 passed.

Files that appear in `9af70150d..b7b41836` but not in the authoritative
`233664cf..b7b41836` diff (trace.rs, mem.rs, otlp_receiver.rs, task-087
specs, DetailPanel/ExplorerCanvas/NodeDetailPanel, web/dist) arrived via the
merge of the assigned base (`51ceb1a02`) — base lineage, not candidate work.

## Verdict

No open findings. Both prior findings are repaired with mutation-sensitive
regression tests; the task contract behaviors are independently verified in
source and by focused probes. Approved.
