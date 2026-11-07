# Review: task-083 — Canonical Navigation — Scope-Aware Content Routing

Candidate `9af70150d9e25c0b8a01ab9f394c0fe58033d645` on base `a884329d71182b59aba3868a9291f2ee2f4282a0`.
Evidence retained under `/tmp/stage/review-evidence/` (see README.md there).

## Scope of the diff

22 files: server-side repo-scoped briefing (`api/graph.rs` +629: `?repo_id=`
filter on GET briefing and `briefing/ask` body with 404/403 validation and a
G6 per-repo ABAC handler guard; `assemble_briefing` filters MRs, tasks,
cross-workspace links, spec-assertion notifications, and completed agents by
repo binding; MCP delegate unchanged), and web-side scope-aware routing
(`App.svelte` cross-workspace `goToEntityDetail` owning-workspace switch,
Architecture Graph|Briefing sub-tab, unified `wsLoadGen` stale-response guard
in `WorkspaceHome`, server-side workspace filter + repo filter in `Inbox`,
repo-scoped `Briefing`, `CrossWorkspaceHome` notifications-shape fix, plus
WorkspaceSettings Repos tab / TenantSettings Workspaces tab and tests).

## What was verified (independent evidence)

- Focused frontend suites (after `npm ci`): AppShell/Briefing/ExplorerViewScope/
  Inbox 147 passed; TenantSettings/WorkspaceHomeSections/WorkspaceSettings/
  CrossWorkspaceHome/WorkspaceHomeRulesFailure/ScopeTransitions 171 passed
  (41 pre-existing skips); WorkspaceHome/WorkspaceHomeArchitecture/
  WorkspaceHomeScopeDrill/RepoModeHeader 54 passed. 0 failures.
- Rust: `cargo test -p gyre-server --lib briefing` 24/0, `--lib api::graph::tests`
  28/0, `--test graph_integration` 37/0.
- Clippy changed-line gate (the check that failed the prior verification round):
  `cargo clippy -p gyre-server --tests` JSON cross-referenced against the
  changed lines of the two changed .rs files → 0 hits. The `graph.rs:2462`
  `unused_must_use` is fixed (`.expect(...)` inside the `block_in_place`
  closure) and `briefing_ask_repo_scope_abac_denied_returns_403` passes.
- Mutation probes prove the new tests are real: disabling `repo_match` in
  `assemble_briefing` fails both repo-filter tests; removing the Inbox repo
  filter fails the repo-scope filtering test. Both mutations restored.
- `scripts/check-task-commit-attribution.sh` OK. No sidebar or legacy
  breadcrumb control introduced; canonical shell intact (AppShell suite).
- §10 admin gates (tenant settings / tenant agent-rules) remain
  `userIsAdmin`-gated; unknown owning workspace surfaces an error toast
  (no fabricated scope identity) in `goToEntityDetail`.
- The §4 stale-response guard is genuine: one `wsLoadGen` bump per workspace
  change, every loader discards superseded results; the pre-existing
  WorkspaceHomeRulesFailure test still covers the rules path, and Inbox and
  Briefing carry equivalent guards.

## Findings (not approved)

- **F1 [test]: CrossWorkspaceHome `myNotifications` shape fix has no regression
  test.** The fix (server returns `{notifications:[...]}`; the old
  `data?.items` read always rendered an empty Decisions section in `/all`) is
  real production behavior, but every mock in
  `web/src/__tests__/CrossWorkspaceHome.test.js` resolves `myNotifications`
  to a bare array. Mutation probe: reverting the fix passes all 36 tests. The
  task acceptance prescribes a regression "with real API arguments and
  rendered results" for demonstrated missing behavior — this one was
  demonstrated and fixed in this task without a pinning test.

- **F2 [code]: `?subTab` deep-link param stripped on Architecture mount, URL
  stops matching content.** `ExplorerView.svelte`'s initial-read `$effect`
  deletes the `?subTab` param from the URL after reading it, contradicting
  `App.svelte`'s `setRepoArchSubTab` invariant ("mirrored into the ?subTab=
  URL param so deep links always match the rendered content") and
  `goToRepoTab`'s param-retention branch. Probe: fresh mount with
  `.../architecture?subTab=briefing` fires the sub-tab switch but leaves
  `location.search === ''`; reload/copy of the URL then lands on Graph.
  The AppShell assertion `window.location.search === '?subTab=briefing'`
  passes only because `RepoMode.svelte` is stubbed in that suite, so it does
  not represent real-app URL behavior. Fix: stop deleting the param in the
  effect (App already maintains it) or re-set it after the initial read.

## Notes (no action required from this task)

- `goToEntityDetail`'s silent `if (!currentRepo) return;` when repo resolution
  fails is pre-existing at base, not introduced here; this task actually
  improved error surfacing (unknown-workspace toast).
- The committed `web/dist` predates the candidate's frontend changes; dist
  regeneration is a publication-stage step (repo history: "build(web):
  regenerate verified assets"), and a fresh `vite build` of the candidate
  succeeded in the prior verification round.
- The WorkspaceSettings "Repos" tab and TenantSettings "Workspaces" tab go
  beyond §2/§10's literal section lists but implement HSI §1.5 Admin rows
  whose deeper behaviors the task authority section preserves; they are real
  (API-backed, tested) and do not restore any superseded navigation element.
