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
commits: ["45a3f5e4f2c9d844dd417401e4946ab0da85c857", "13d856c82e77a1cd2e414ff5383e843774ff028d"]
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

Verification (merge-continuation round at merge commit `9c41bce0`,
checkpoint `937a0409` merged onto assigned base `1d9547de` by the executor
via 'ort' auto-merge, zero conflicts; evidence:
`/tmp/stage/review-evidence/task-083-merge-1d9547de.txt`):

- Why this round exists: the previous attempt aborted at the pipeline's own
  pre-flight (GitHub API connectivity check failed: transport error,
  deadline elapsed) — infrastructure, not a code defect; the agent never
  started and a source checkpoint was taken. This round verified the
  checkpointed source merged onto the new assigned base is sound.
- Source overlap between the task branch and the base merge: none — the task
  branch changes `api/graph.rs`, `mcp.rs`, `docs/ui.md`, and `web/src`
  navigation components; the base brings only `explorer_ws.rs`,
  `llm_helpers.rs` (task-070), and `specs/tasks/task-070.md` on the source
  side. Conflict-marker scan across `web/src`, `crates`, `specs`, `docs`:
  no matches. `web/dist` was not rebuilt this round (assignment
  instruction); final integration rebuilds it remotely.
- Focused probes on the merged tree: the 18 navigation- and Inbox-related
  test files (Inbox, api, BidirectionalNav, ScopeTransitions, NoSidebar,
  CrossWorkspaceHome, WorkspaceHome ×5, WorkspaceDrawerSectionNav,
  RepoMode ×2, ExplorerViewScope, ExplorerViewDrillUrl, Briefing, AppShell)
  → **337 passed / 0 failed** (41 pre-existing skips). Merge-integrity
  probes: `scripts/check-abac-route-registry.sh` → OK, and
  `scripts/check-mcp-write-tools.sh` → OK (8 write-capable tools gated).
  Compile probe: `SKIP_WEB_BUILD=1 cargo check -p gyre-server` → clean.
- Prior rounds' evidence (review fixes F1–F3 at head `2ff2e596`, full
  frontend suite 1598 passed / 0 failed, cargo `api::graph::tests` 28,
  `briefing` 24, `graph_integration` 37, all 0 failed; earlier rebase
  rounds `38134ff8` onto base `5df14f344` and `1077f081` onto base
  `9bef21a4`, each with a 266/0 focused navigation probe and clean dist
  rebuild) covers the task-side source merged here; this round's diff is
  only the base merge. Full suites, architecture checks, all-target
  Clippy, and exact-head GitHub CI remain owned by verification and
  publication. This sandbox now reports TCP listener support and an
  installed chromium headless shell (`/tmp/stage/capabilities.json`), a
  change from prior rounds' errno 95; Playwright e2e was not run here (it
  needs a built, seeded server and stays with verification).

Contract note for the reviewer: an earlier attempt ticked the acceptance
checkboxes in this file, which the pipeline's `requirement_parts` treats as
contract prose and flags as a contract change. The assigned text is restored
byte-for-byte here; only `progress:`, `commits:`, and this `## Shipped`
section (an operational section) differ from the assignment.
