# Review: task-083 — Canonical Navigation — Scope-Aware Content Routing

Candidate: `d6739afcc1b6f07d7f866d3d8a0dca766241b16a` (branch
`pipeline/task-083/df1ef9f39fac472a91bf0d780b83e8d3-4`), base
`9bef21a41729a6b696b48a587b1f2e619d0f6ff2`. Head commit `d6739afc` only
records the `commits:` frontmatter; all task work is in checkpoint commit
`13d856c8`.

## What was verified (independent evidence)

All probe commands, sources, and outputs are under `/tmp/stage/review-evidence/`
(`task-083-focused-frontend.txt`, `task-083-focused-settings-tabs.txt`,
`task-083-cargo-fetch.txt`, `task-083-cargo-graph-tests.txt`,
`task-083-scratch-probe-source.test.js`, `task-083-scratch-probe-result.txt`).

- **Focused frontend navigation suite** (17 files: Inbox, api,
  BidirectionalNav, ScopeTransitions, NoSidebar, CrossWorkspaceHome,
  WorkspaceHome ×5, WorkspaceDrawerSectionNav, RepoMode ×2,
  ExplorerViewScope, ExplorerViewDrillUrl, Briefing) — 266 passed / 0 failed
  (41 pre-existing skips). Re-ran on the exact candidate after `npm ci`;
  matches the interrupted review's saved evidence.
- **Changed settings-tab tests** (AppShell, TenantSettings,
  WorkspaceSettings) — 164 passed / 0 failed.
- **Rust server-side briefing narrowing** (`cargo test -p gyre-server
  api::graph::tests`, via `/tmp/stage/bin/cargo` after fetching 393 crates) —
  28 passed / 0 failed, including the new repo-scope guards:
  HTTP-level 403 (cross-workspace repo) / 404 (unknown repo), per-section
  repo filtering (MRs, tasks, cross-workspace links, spec-assertion
  notifications, completed agents via `agent.repo_id`), and the G6 ABAC
  caller-scope guard on `briefing/ask` request-body `repo_id`. These are real
  kill tests — they exercise the HTTP handlers, not internals only.
- **§4 stale-response guard**: `WorkspaceHomeRulesFailure.test.js` ("keeps
  workspace B rules when workspace A finishes loading after navigation")
  exercises the shared `wsLoadGen` generation guard; the guard is uniformly
  applied to every section loader in `WorkspaceHome.svelte` (verified in the
  diff: arch, rules, repos, decisions, specs, tasks, MRs, agents, budget,
  dep-health, merge queue, activity).
- **§2/§3 quality**: Agent Rules merges real
  `getMetaSpecs({scope:'Workspace',scope_id})` + `getMetaSpecs({scope:'Global'})`
  with error+Retry (kill test present); Inbox repo scope filters on exact
  `repo_id` with workspace-only (`repo_id: null`) items excluded (kill test
  present, asserts rendered card count); Briefing repo scope passes
  `repo_id` to GET and `ask` (kill tests present); the drawer Briefing item
  at repo scope stays in the repo (Architecture sub-tab) — test updated and
  passing. No sidebar, no legacy breadcrumb control, no
  Architecture-as-default routing introduced.
- **§10 in-session entry (scratch probe A, PASS)**: rendered App with a saved
  workspace, navigated to `/all` via the workspace selector, clicked a ws-2
  decision item → URL `/workspaces/platform/r/checkout/mrs/mr-1`, title
  "checkout — Platform | Gyre": the owning-workspace switch, repo entry, and
  entity-detail URL all work when `/all` is reached client-side. Probe source
  saved; scratch file deleted (tree clean).

## R1 Findings

- [ ] **F1 (code — §10 deep-link/refresh dead click):** `web/src/App.svelte`
  — on a fresh load or refresh of `/all` (a §7-deep-linkable route, also the
  redirect target for legacy `/repos/:uuid/explorer` URLs), the mount
  `cross_workspace` branch sets `mode` but never assigns or restores
  `currentWorkspace` (the `parsed.slug` and entrypoint `else` branches that
  read `localStorage` are both skipped because `/all` parses with
  `slug: 'all'`). `goToEntityDetail` then early-returns on
  `if (!currentWorkspace) return;` (App.svelte:417), so every aggregated
  item click in the cross-workspace dashboard — decision bodies
  (`.decision-body-clickable`), Specs table rows, and activity entity links
  — is a **silent no-op**. Reproduced with scratch probe B: fresh `/all`
  load, click the same decision item that works in probe A → URL stays
  `/all`, title unchanged, no error surfaced. This violates the §10 required
  behavior "opening an item must enter its actual owning scope" on the
  deep-link path, and the silent no-op also contradicts §4's "lookup/request
  errors must surface as errors, rather than successful empty data". The
  base had the same dead path, but this task's core deliverable is the §10
  attribution-link entry, and the Shipped section claims it unconditionally.
  Fix sketch: in `goToEntityDetail`, resolve the owning workspace from
  `d.workspace_id ?? entityRepo?.workspace_id` even when `currentWorkspace`
  is null (the `workspaces` membership list is already loaded at mount), or
  restore `currentWorkspace` from `localStorage`/membership when booting at
  `/all`.

- [ ] **F2 (test — missing regression for the §10 owning-scope entry):** the
  primary new behavior of this task — `goToEntityDetail` switching
  `currentWorkspace` to the item's owning workspace, persisting the
  selection, and reloading scoped data — has **no regression test**. The
  `CrossWorkspaceHome.test.js` suite stops at section rendering and
  `onSelectWorkspace`; no test asserts the decision/spec/activity click
  path's API arguments (`api.repo(...)`, workspace switch) and rendered
  result (URL/title). The task contract requires "add a regression only for
  a demonstrated missing behavior, with real API arguments and rendered
  results" — the base demonstrably lacked the workspace switch (it resolved
  repos only within `currentWorkspace`), so this regression was warranted.
  Probe A (saved at
  `/tmp/stage/review-evidence/task-083-scratch-probe-source.test.js`)
  demonstrates a feasible test; note it would currently fail on the F1
  deep-link path unless the test drives `/all` via the selector.

- [ ] **F3 (minor, cosmetic):** `web/src/components/WorkspaceHome.svelte`
  template renders `<span class="rule-version">` (line 1686) but the new CSS
  block defines `.rules-version` (line 5716) — dead selector, the rule
  version text is unstyled. No functional impact.

## Deferred to verification (not run here — sandbox scope)

- Full frontend suite, workspace-wide cargo test/clippy, architecture
  scripts, exact-head CI (the review wrapper rejects workspace-wide cargo).
- Playwright e2e / browser probes (host-dependent; see
  `/tmp/stage/capabilities.json`).

## Verdict

Not approved this round: F1 is a reachable, silent failure of the §10
acceptance item "Cross-workspace attribution links enter the item's owning
workspace/repo" (deep-link/refresh path), and F2 leaves the task's primary
new §10 behavior without any regression coverage. The §2/§3/§7 work and the
server-side repo-scoped Briefing narrowing are genuinely implemented and
well tested (28/28 Rust, 266/0 + 164/0 focused frontend).
