---
title: "Canonical Navigation — Scope-Aware Content Routing"
spec_ref: "ui-navigation.md §2, §3, §7, §10"
depends_on:
  - task-082
progress: complete
coverage_sections:
  - "ui-navigation.md §2 Workspace Home"
  - "ui-navigation.md §3 Repo Mode"
  - "ui-navigation.md §10 Cross-Workspace View"
commits: ["bbb7aa61f860f3f4e73b862a2aa35061e87e1c32", "c69d9fd546809cfac5195d7663526f5c24848ea1", "4cef86db1b2fe6c0dc182068ea8c0ed0a789a15b", "af4dd137adb2697aea0acb1d939b91efcc4b9b36", "6cf4a22459fa8099e36df4296da9caddd17f8551", "084040ecc21bf806644630f653fdbeccdd31749d", "c38c8e00398862f4f49c526105b493cb9f9e922c", "7e5cdaba110cbc9de3f5a9f6ad79303897ce7776", "7c2b975731bfd631f3e6b91da5d40790c196108f", "796c800da477a8cd45aeb60ca9ec7665a11938b8", "45a3f5e4f2c9d844dd417401e4946ab0da85c857", "13d856c82e77a1cd2e414ff5383e843774ff028d", "4772240df3f290bd094a163705a06cabc125a6a4", "d4e72e32cd9fad766a2c54973b04d631ab179a3f"]
---

## Authority and Scope

`ui-navigation.md` explicitly supersedes `human-system-interface.md` §1,
including the six-item sidebar and its 6×3 content matrix. The original
task-083 plan targeted that superseded matrix. Implement scope-aware content
in the current workspace-home / repo-mode navigation instead. The deeper HSI
§2–§12 behaviors remain valid; this task changes their navigation and scoping,
not their underlying requirements.

Task-082 is historical shell work, not an instruction to restore its obsolete
sidebar. Preserve the canonical shell restored by the main-baseline repair.

## Required Behavior

1. **Cross-workspace view (`/all`, §10):** Show the Decisions, Workspaces,
   Specs, Briefing, and Agent Rules sections for workspaces the caller can
   access. Preserve workspace/repo attribution on aggregated items; opening
   an item must enter its actual owning scope. Briefing aggregates the real
   per-workspace API results. Tenant settings and tenant-rule editing retain
   their authorization gates.
2. **Workspace home (`/workspaces/:slug`, §2):** Decisions, Specs, Architecture,
   Repos, Briefing, and Agent Rules use the selected workspace's real data.
   A repo or spec click carries the owning repo into repo mode. Rules merge
   the actual Global/Workspace sources. Settings and rule management use the
   canonical workspace routes, not a sidebar or fabricated scope identity.
3. **Repo mode (§3, §7):** Specs is the default landing tab; Architecture,
   Decisions, Code, and Settings are independently addressable tabs for the
   selected repo. Repo Decisions filter by the actual repo; workspace-only
   notifications are not silently assigned to a repo. Agent/entity drill-downs
   preserve the selected workspace/repo and the back path.
4. **Scope transitions:** Use client-side navigation with URL/history support.
   After switching scopes, a delayed response from the old scope must not
   overwrite the new scope's content. Lookup/request errors must surface as
   errors, rather than successful empty data or fallback scope identities.

## Acceptance and Verification

- [ ] Cross-workspace attribution links enter the item's owning workspace/repo.
- [ ] Workspace sections and repo tabs issue queries for their selected scope.
- [ ] Repo-mode entry defaults to Specs; deep links and back navigation retain scope.
- [ ] Delayed old-scope responses cannot replace current-scope content.
- [ ] Scoped request failures are visible and recoverable.
- [ ] The canonical shell and mobile workspace-section navigation remain intact.
- [ ] Existing meaningful navigation tests pass; add a regression only for a
  demonstrated missing behavior, with real API arguments and rendered results.

Read `docs/ui.md`, the cited navigation sections, and the actual routing/data
loaders before editing. Reuse working behavior. Do not introduce the permanent
sidebar, legacy scope breadcrumb control, or Architecture-as-default routing
from the superseded plan. Do not mark unrelated partial navigation features
verified as a side effect of this task.

## Shipped

Scope-aware content routing on the canonical workspace-home / repo-mode
navigation (no sidebar; the superseded 6×3 matrix is not restored). The
acceptance checkboxes above are left unchecked intentionally: the assigned
contract prose is immutable from the implementation side, and satisfaction
is claimed here, per section, with the verifying evidence below.

- **Cross-workspace (§10):** `CrossWorkspaceHome` renders all five sections
  from real per-workspace API results — Decisions via `api.myNotifications()`
  (response shape `{notifications:[...]}`), Workspaces list with health,
  Specs via per-workspace `specsForWorkspace(ws.id)` aggregation
  (`Promise.allSettled`, the same pattern as Briefing) with
  `workspace_id`/`workspace_name` attribution backfilled on every row,
  Briefing via per-workspace `getWorkspaceBriefing`, Agent Rules via
  `getMetaSpecs(scope=Global)`. Item clicks call `goToEntityDetail`, which
  resolves the entity's repo, derives the owning workspace from
  `workspace_id`/repo lookup, switches `currentWorkspace` to it (error toast
  when the workspace is not in the caller's membership — no fabricated scope
  identity), persists the selection, and reloads scoped data — including
  when the app was loaded directly at `/all` (bookmark, shared deep link,
  refresh): `goToEntityDetail` resolves the owning workspace from the
  membership list when no workspace is selected, and surfaces an error
  toast when no owner can be determined (repair `084040ecc`, below). The
  Specs
  stat-card counts derive from the same scoped list. Tenant settings
  (`/all/settings`) and tenant agent-rules stay admin-gated
  (`onSettings`/`onManageAgentRules` undefined for non-admins).
- **Workspace home (§2):** every section loader in `WorkspaceHome.svelte`
  queries with the selected workspace's id behind one generation guard
  (`wsLoadGen`) — the workspace-change effect bumps it once; delayed
  old-scope responses (success or failure) are discarded. The Specs query is
  enforced server-side: GET `/api/v1/specs?workspace_id=` filters ledger
  entries to those the workspace owns — directly via `workspace_id` or via
  the owning repo — so no workspace home renders another workspace's specs.
  Agent Rules merges `getMetaSpecs(scope=Workspace&scope_id=:id)` with ALL
  tenant `scope=Global` rules, grouped by kind with Tenant/Workspace scope
  badges and 🔒 on required; a failed lookup surfaces as error + Retry,
  never an empty successful set. Settings/rule management use the canonical
  routes `/workspaces/:slug/settings` and `/workspaces/:slug/agent-rules`.
- **Repo mode (§3/§7):** `goToRepo` defaults to the Specs tab; all tabs are
  URL-addressable via `/workspaces/:slug/r/:repo/:tab` (`parseUrl`/`urlFor`
  implement the §7 table incl. multi-segment spec paths and entity detail
  routes); popstate restores workspace, repo (with `repoIdCache` + API
  resolution fallback), and tab. The Architecture tab has the
  Graph | Briefing control-bar sub-tab (`?subTab=briefing` deep link);
  repo Briefing is narrowed server-side (`?repo_id=` on GET briefing and in
  the `briefing/ask` body; `assemble_briefing` filters MRs, tasks,
  cross-workspace links, spec-assertion notifications, and completed agents
  by `agent.repo_id`; the REST handler 404s unknown repos and 403s
  cross-workspace repos; MCP resource delegates unchanged at workspace
  scope). Repo Decisions (`Inbox.svelte` scope=repo) filters client-side on
  exact `repo_id` so workspace-only notifications (`repo_id: NULL`) never
  appear under a repo. Drawer/section navigation at repo scope stays in the
  repo (Briefing → Architecture sub-tab, not workspace home).
- **Scope transitions (§4 requirements):** client-side navigation with
  pushState/popstate throughout; `WorkspaceHome`, `Inbox`, and `Briefing`
  each carry generation guards so a delayed old-scope response cannot
  overwrite the new scope's content; workspace lookup failures surface as
  error toasts/states (cross-workspace unknown-workspace toast, Agent Rules
  error + Retry, per-section load errors) rather than empty success. The
  guard is pinned by `web/src/__tests__/StaleScopeResponse.test.js`
  (`af4dd137a`): each of the three loaders is rendered for real, switched
  to a new scope, and the old scope's deferred response is resolved after
  the switch — the new scope's rendered content is preserved and the old
  scope's never appears (mutation probe in Verification).

### Verification

- CI repair (`c69d9fd5`, closing the run-42441309727 E2E finding): the
  12 explorer visual regressions failed because the committed baselines
  predate this task's spec-required Graph | Briefing control-bar sub-tab
  on the repo-scope Architecture tab — the bar consumes 34px of header
  height, so every `.treemap-canvas-area` shot rendered 34px shorter
  (919x397 → 919x363, 579x335 → 579x301) and the full-page tabpanel
  re-flowed. Baselines replaced with the failed run's own actual
  screenshots (the authoritative rendering of the specced UI in the gate
  environment; dist verified byte-identical to the candidate's verified
  asset build by content hash, same chromium 1208). Local verification:
  full e2e suite 64/64 passed against the new baselines; local rendering
  is byte-deterministic run-to-run; mutation probe — serving the
  pre-sub-tab dist fails `zoom_level_0` on the 919x397 vs 919x363 size
  mismatch, so the baselines genuinely pin the new UI. Assertion strength
  unchanged (same tests, locators, maxDiffPixelRatio 0.02); toolbar and
  blast-radius-tiered baselines untouched and still passing. Evidence
  under `/tmp/stage/review-evidence/`.
- CI-gate repair (`6cf4a224`, closing the `9bb8899472` verification
  finding): the two repo-wide gate failures in `api/specs.rs` were
  pre-existing legacy-debt sites that `c38c8e003`'s 173 inserted lines had
  shifted off their `path:line` exemption entries. Both sites are now
  fixed for real and their exemption entries deleted (byte-slice list
  shrinks by one; fabricated-scope list shrinks by one with
  `FROZEN_EXEMPTION_COUNT` lowered 5 → 4, per each check's documented
  procedure — no exemption was added or re-pinned):
  `spec_index` truncates the sha via `chars().take(8)`
  (check-byte-slice-truncation OK), and `reject_spec`'s notification
  fanout resolves the tenant from the workspace record, skipping +
  logging when unresolvable — the `emit_reconciliation_completed`
  pattern (check-fabricated-scope-defaults OK). New Rust regressions:
  `reject_spec_notification_uses_workspace_tenant` (notification carries
  the workspace's real tenant, not the repo id or a fabricated
  "default"; mutation probe — restoring the repo_id fallback fails
  exactly this test) and
  `reject_spec_skips_notifications_when_workspace_unresolvable`
  (unresolvable workspace skips the fanout). Focused probe:
  `cargo test -p gyre-server --lib api::specs::tests` → 54 passed /
  0 failed (52 pre-existing + 2 new).
- Earlier-round evidence (retained): `/all` page-load entry repair
  (`084040ecc`) with regression
  `web/src/__tests__/CrossWorkspaceEntry.test.js` (mutation probe —
  restoring the original early return fails both tests); specs-scope
  repair (`c38c8e003`) with Rust test `list_specs_honors_workspace_id_scope`
  and vitest regression `fetches specs per accessible workspace, not the
  unscoped list` (both with passing mutation probes). Focused vitest runs
  at that head: CrossWorkspaceHome → 38 passed; WorkspaceHome,
  WorkspaceHomeSections, WorkspaceHomeArchitecture, WorkspaceHomeScopeDrill,
  WorkspaceDrawerSectionNav, ScopeTransitions, SpecDashboard → 94 passed;
  AppShell, RepoMode, RepoModeHeader → 116 passed; all 0 failed. Earlier
  evidence (myNotifications envelope, `?subTab` deep link, task-084 base
  rebase with the 9-tab `WorkspaceSettings` union → 71 passed) is retained
  under `/tmp/stage/review-evidence/README.md`.
- Review repair (`af4dd137a`, closing the 3c1e3f8d finding that the §4
  stale-response guard had no regression): test-only addition — the
  production guards were already real. `StaleScopeResponse.test.js`
  renders the real `WorkspaceHome`, `Inbox`, and `Briefing` with real API
  arguments (`myNotifications()` for the Decisions loader,
  `myNotifications({workspace_id})` for Inbox, `getWorkspaceBriefing(id,
  null)` for Briefing), switches scope via the workspace prop change
  App.svelte drives, and resolves the old scope's still-pending deferred
  promise after the switch. Mutation probe: with `stale()` forced to
  `false` in WorkspaceHome.svelte and every `gen !== loadGen` check
  removed in Inbox.svelte (2 sites) and Briefing.svelte (5 sites), all
  three tests fail (the late old-scope response wipes the new workspace's
  decisions / renders the old workspace's rows); guards intact → 3/3
  pass. Focused navigation suite including the new file and the
  Agent-Rules guard regression (`WorkspaceHomeRulesFailure.test.js`):
  7 files, 119 tests, 0 failures (evidence retained under
  `/tmp/stage/review-evidence/`).
- Baseline rebase resolution (`c42b8f85a`, merging `124a852be` task-193
  spec-approval-attestation): the new baseline carries duplicate fixes for
  the same two `api/specs.rs` legacy-debt sites this branch fixed in
  `6cf4a224`, which collided textually. Resolved by taking the baseline's
  variants — `spec_index` now truncates via
  `e.current_sha.get(..8).unwrap_or(&e.current_sha)` (equivalent
  char-boundary-safe truncation to the `chars().take(8)` this branch
  shipped) and `reject_spec`'s skip-and-log tenant resolution is
  unchanged in behavior; the exemptions file keeps the baseline's
  provenance comment and stays at 4 entries with `FROZEN_EXEMPTION_COUNT`
  = 4. Post-merge focused probe: `cargo test -p gyre-server --lib
  api::specs::tests` → 58 passed / 0 failed (54 from the `6cf4a224` round
  plus task-193's 4 new tests; the three task-083 regressions
  `list_specs_honors_workspace_id_scope`,
  `reject_spec_notification_uses_workspace_tenant`,
  `reject_spec_skips_notifications_when_workspace_unresolvable` all pass);
  `check-byte-slice-truncation.sh` → OK;
  `check-fabricated-scope-defaults.sh` → OK. `web/dist` restored to HEAD
  after the cargo run's build.rs side-effect; not committed (executor
  owns generated assets).
- Commit attribution (`d4e72e32`, recorded in this file's `commits:`
  frontmatter by the task-088 integration round): a downstream task's
  integration commit whose subject references this task ("fix(task-088):
  integrate with task-083 canonical navigation") merges this task's base
  into its candidate and regenerates the 11 explorer visual baselines the
  merged toolbar (arch sub-tabs + view selector) re-drifted, plus scopes
  one test's tab queries to the `explorer-view-selector` tablist. The
  `check-task-commit-attribution.sh` gate requires every task referenced
  in a product-surface commit subject to list that commit, so it is
  recorded here for review-scoping visibility; the change itself is
  documented in task-088's Shipped notes.
- `web/dist` was not rebuilt or committed here (the executor restores
  generated assets from the assignment base and final integration
  rebuilds them remotely); full suites, all-target Clippy, and exact-head
  GitHub CI remain owned by verification and publication.

Contract note for the reviewer: an earlier attempt ticked the acceptance
checkboxes in this file, which the pipeline's `requirement_parts` treats as
contract prose and flags as a contract change. The assigned text is restored
byte-for-byte here; only `progress:`, `commits:`, and this `## Shipped`
section (an operational section) differ from the assignment.
