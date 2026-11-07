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
commits: ["7e5cdaba110cbc9de3f5a9f6ad79303897ce7776", "7c2b975731bfd631f3e6b91da5d40790c196108f", "796c800da477a8cd45aeb60ca9ec7665a11938b8", "45a3f5e4f2c9d844dd417401e4946ab0da85c857", "13d856c82e77a1cd2e414ff5383e843774ff028d"]
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
  (response shape `{notifications:[...]}` fixed), Workspaces list with health,
  Specs via `specsForWorkspace(null)` with `workspace_id`/`repo_id` on every
  row, Briefing via per-workspace `getWorkspaceBriefing` client-side
  aggregation, Agent Rules via `getMetaSpecs(scope=Global)`. Item clicks call
  `goToEntityDetail`, which resolves the entity's repo, derives the owning
  workspace from `workspace_id`/repo lookup, switches `currentWorkspace` to
  it (error toast when the workspace is not in the caller's membership — no
  fabricated scope identity), persists the selection, and reloads scoped
  data. Tenant settings (`/all/settings`) and tenant agent-rules stay
  admin-gated (`onSettings`/`onManageAgentRules` undefined for non-admins).
- **Workspace home (§2):** every section loader in `WorkspaceHome.svelte`
  queries with the selected workspace's id behind one generation guard
  (`wsLoadGen`) — the workspace-change effect bumps it once; delayed
  old-scope responses (success or failure) are discarded. Agent Rules merges
  `getMetaSpecs(scope=Workspace&scope_id=:id)` with ALL tenant
  `scope=Global` rules, grouped by kind with Tenant/Workspace scope badges
  and 🔒 on required; a failed lookup surfaces as error + Retry, never an
  empty successful set. Settings/rule management use the canonical routes
  `/workspaces/:slug/settings` and `/workspaces/:slug/agent-rules`.
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
  error + Retry, per-section load errors) rather than empty success.

Verification (current round: rebase repair onto task-084 base `4d6759a0`,
repair id `dbc943c2`):

- Prior review rounds closed two findings on candidate `9af70150d` — the
  myNotifications `{notifications:[...]}` envelope regression tests
  (`7e5cdaba`, mutation probe: reverting to `data?.items` fails exactly
  those 2 tests) and the `?subTab` deep-link param retention fix
  (`7c2b9757`, mutation probe: restoring the deletion fails exactly that
  test). Evidence retained under `/tmp/stage/review-evidence/README.md`
  from that round.
- This round rebased the candidate onto task-084's base
  (`4d6759a0`, HSI Policies ↔ Trust Level Integration UI), which added a
  `policies` tab to `WorkspaceSettings` while this branch had added a
  `repos` tab. Conflict resolution (merge commit `77e9299b3`):
  `TAB_IDS` takes the union (general, trust, policies, teams, budget,
  repos, compute, llm, audit); the test file keeps this branch's Repos
  tab tests, adopts base's `Trust Level tab` describe title, and the
  auto-merged tab-count test was corrected from 8 to 9 tabs (the union
  renders nine). Keyboard-navigation expectations (ArrowRight
  general→trust, ArrowLeft/End wrap to audit) remain valid under the
  merged order.
- Fixed-state probes (`npx vitest run`, locked deps via `npm ci`):
  `WorkspaceSettings.test.js` → 71 passed / 0 failed (both sides'
  tests pass against the union); focused navigation suites (AppShell,
  CrossWorkspaceHome, ExplorerViewScope, ExplorerViewDrillUrl,
  ExplorerView-responsive, RepoMode, RepoModeHeader) → 174 passed /
  0 failed. `web/dist` was not rebuilt here (the executor restores
  generated assets from the assignment base and final integration
  rebuilds them remotely); full suites, all-target Clippy, and
  exact-head GitHub CI remain owned by verification and publication.

Contract note for the reviewer: an earlier attempt ticked the acceptance
checkboxes in this file, which the pipeline's `requirement_parts` treats as
contract prose and flags as a contract change. The assigned text is restored
byte-for-byte here; only `progress:`, `commits:`, and this `## Shipped`
section (an operational section) differ from the assignment.
