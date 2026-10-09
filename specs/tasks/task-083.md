---
title: "Canonical Navigation — Scope-Aware Content Routing"
spec_ref: "ui-navigation.md §2, §3, §7, §10"
depends_on:
  - task-082
progress: ready-for-review
coverage_sections:
  - "ui-navigation.md §2 Workspace Home"
  - "ui-navigation.md §3 Repo Mode"
  - "ui-navigation.md §10 Cross-Workspace View"
commits: ["5d514baf349cdcc75b8e16a1cc4c7c0f58dcd0a1", "41f04f5d67f2a7f7185b8d13a48feee00e8237ec", "454af032edcebb0b71098688ed35f69a0f6a84fd", "987c8067be9ae1f231adf1358bdb006e9fd33c9b", "07eb5915a082ae5a9334f41bb349379280f0fa3e", "7a12fa3665be2123f2793ec3c12015c04cf261cc", "ba656318cd5da9ee833bbb625ecd6b421e30e53c", "ad35a4a8a961c36bf876b5299020198af7f00e2d", "d6073fb363d8be7335f9509c78c26224d2abc9f3", "4a781f24150f595948019832e908f74fc4dda8ae", "70c4a880250b835a455728b7a246d0c54b881a99", "c793573beecc212e18ee4fae664e5adedb1941ee"]
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

- [x] Cross-workspace attribution links enter the item's owning workspace/repo.
- [x] Workspace sections and repo tabs issue queries for their selected scope.
- [x] Repo-mode entry defaults to Specs; deep links and back navigation retain scope.
- [x] Delayed old-scope responses cannot replace current-scope content.
- [x] Scoped request failures are visible and recoverable.
- [x] The canonical shell and mobile workspace-section navigation remain intact.
- [x] Existing meaningful navigation tests pass; add a regression only for a
  demonstrated missing behavior, with real API arguments and rendered results.

Read `docs/ui.md`, the cited navigation sections, and the actual routing/data
loaders before editing. Reuse working behavior. Do not introduce the permanent
sidebar, legacy scope breadcrumb control, or Architecture-as-default routing
from the superseded plan. Do not mark unrelated partial navigation features
verified as a side effect of this task.

## Shipped

Scope-aware content routing on the canonical workspace-home / repo-mode
navigation (no sidebar; the superseded 6×3 matrix is not restored):

- **Cross-workspace (§10):** `goToEntityDetail` resolves the entity's repo,
  derives the owning workspace from `workspace_id`/repo lookup, switches
  `currentWorkspace` to it (error toast on unknown workspace — no fallback
  identity), persists the selection, and reloads scoped data. Decisions/spec
  rows and workspace badges in `CrossWorkspaceHome` carry repo/workspace ids
  into that path. Briefing remains real per-workspace API aggregation;
  `myNotifications` response shape fixed to `{notifications:[...]}`. Tenant
  settings/agent-rules stay admin-gated (`onSettings`/`onManageAgentRules`
  undefined for non-admins).
- **Workspace home (§2):** every loader queries with the selected
  workspace's id and is generation-guarded (one bump per workspace change —
  a delayed old-scope success *or* failure is discarded). Agent Rules shows
  the full effective cascade — `getMetaSpecs(scope=Workspace&scope_id=:id)`
  merged with ALL tenant `scope=Global` rules — grouped by kind with
  Tenant/Workspace scope badges, 🔒 on required, recency-based reconcile
  banner, error + Retry instead of an empty successful set.
- **Repo mode (§3/§7):** Specs stays the default landing tab; all tabs
  URL-addressable via `/workspaces/:slug/r/:repo/:tab`; Architecture gains
  the Graph | Briefing control-bar sub-tab (`?subTab=briefing` deep link);
  repo Briefing is narrowed server-side (`?repo_id=` on GET briefing and in
  the `briefing/ask` body — `assemble_briefing` filters every section;
  handler 404s unknown repos and 403s cross-workspace repos). Repo Decisions
  filters client-side on `repo_id` so workspace-only notifications
  (`repo_id: NULL`) never appear under a repo; the count badge uses the same
  filter. Drawer/section navigation at repo scope stays in the repo
  (Briefing → architecture sub-tab, not workspace home).
- **Server (HSI §1.5 repo-scope Briefing):** `assemble_briefing` takes an
  optional repo filter applied to MRs, tasks, cross-workspace links, and
  completed agents (via `agent.repo_id` binding); REST handler validates the
  repo belongs to the path workspace; MCP resource delegates unchanged
  (workspace scope). Covered by `briefing_repo_filter_*` tests (2) plus the
  full graph/briefing module suites (23 + 19).
- **Workspace/Tenant settings:** canonical routes `/workspaces/:slug/settings`
  and `/all/settings`; WorkspaceSettings gains a Repos tab (list/create/
  import via real `createRepo`/`createMirrorRepo`); TenantSettings gains a
  Workspaces tab (list + `createWorkspace` + navigate on create) — both
  load-once-when-opened and error-surfaced.
- **Docs:** `docs/ui.md` Architecture Tab section now documents the
  Graph | Briefing sub-tab (commit 5d514baf).

Verification (focused probes, evidence in
`/tmp/stage/review-evidence/task-083-verification.md`):

- Full frontend unit suite after `npm ci`: **58 files, 1545 passed,
  0 failed** (41 pre-existing skips). Includes the new/updated behavioral
  regressions: repo-scope Briefing fetch args + rendered sections, Inbox
  scope filtering (server `?workspace_id=`; repo `repo_id` null exclusion),
  Agent-Rules failure/retry + cross-workspace stale-response guard, drawer
  section navigation to real DOM anchors, repo-mode Briefing drawer click
  staying in repo scope, no-sidebar shell, settings tabs.
- `cargo test -p gyre-server --lib api::graph::tests::briefing_repo_filter`
  → 2 passed; `... api::graph::tests` → 23 passed; `... briefing` → 19
  passed (covers every `assemble_briefing` call site).
- Playwright e2e (default-tab, back/forward, visual snapshots) cannot run
  here: the config's webServer needs a TCP listener on :2222 and this
  sandbox's `accept()` fails with errno 95 (capabilities.json). Static
  review of the specs shows this branch changes none of their selectors;
  exact-head GitHub CI remains mandatory.

