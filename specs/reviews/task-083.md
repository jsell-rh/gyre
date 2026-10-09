# Review — task-083 (Canonical Navigation — Scope-Aware Content Routing)

Spec: `specs/system/ui-navigation.md` §2 (Workspace Home), §3 (Repo Mode), §7 (URL Structure), §10 (Cross-Workspace View), plus the scope-transition requirements (§4 stale-response rule cited by the task contract).
Candidate: `0dff8bda78170e5ba6b47b1ac7e17d6ceef97ae9` (base `8c2d1775`, branch `pipeline/task-083/78e68d1e4dd4455f925874c577bfc88c-1`). Task contract prose verified byte-identical to base except `progress:`, `commits:`, and the appended `## Shipped` operational section.
Verdict: **approved**. Evidence under `/tmp/stage/review-evidence/` (review-log.md).

## Probes (independently reproduced this round)

- `cd web && npm ci` → 169 packages (locked).
- `npx vitest run` (full) → **58 files, 1545 passed / 0 failed, 41 pre-existing skips** — matches the Shipped claim.
- `npx vitest run Briefing + Inbox + AppShell` → **128 passed**; `WorkspaceHomeRulesFailure + CrossWorkspaceHome + WorkspaceDrawerSectionNav` → **41 passed**.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib api::graph::tests` → **23 passed / 0 failed**; `--lib briefing` → **19 passed / 0 failed**.
- `scripts/check-forged-scope-fields.sh`, `check-abac-route-registry.sh`, `check-arch.sh` → all OK.
- Sandbox cannot run TCP listeners (`accept` errno 95, see `/tmp/stage/capabilities.json`), so Playwright e2e could not run here. Static check: `git diff --name-only web/e2e` → 0 files; exact-head GitHub CI (`e2e.yml`) remains the mandatory transport check. Required host checks: repo-mode entry defaults to Specs tab; deep links `/workspaces/:slug/r/:repo/:tab` and back/forward retain scope; visual snapshots.

## Verified behavior (no findings)

**§10 Cross-workspace.** All five sections render from real per-workspace API results. The candidate fixed a real base bug: `/all` Decisions parsed `data?.items` but `users.rs get_my_notifications` returns `{notifications:[...]}` — at base the section always rendered empty; now fixed with a comment naming the server shape. Specs rows carry `workspace_id`/`repo_id` (SpecLedgerResponse serializes both; ledger entries populate them), and item clicks pass `repo_id` into `goToEntityDetail`. Briefing aggregates per-workspace `getWorkspaceBriefing` client-side. Attribution badges resolve via the workspace-name map; the badge click enters the owning workspace. `onSettings`/`onManageAgentRules` are passed only when `userIsAdmin` (App.svelte:1684-1685) — non-admins get no surface; `create_workspace` has real non-admin tenant-override rejection (workspaces.rs test `create_workspace_non_admin_tenant_override_ignored`).

**Owning-scope entry.** `goToEntityDetail` resolves the entity's repo (from `data.repo_id`/`repository_id` or the entity's own API payload), derives `ownerWsId` from `data.workspace_id` or the repo's `workspace_id`, and when it differs from the current workspace, looks the owner up in the caller's membership `workspaces` list — a miss shows an error toast and aborts (no fabricated scope identity, no fallback "default"), a hit switches `currentWorkspace`, persists to localStorage, and reloads scoped data.

**§2 Workspace home.** Every section loader (decisions, repos, specs, rules, tasks, MRs, agents, budget, dep-health, merge queue, activity, arch graph) captures `wsLoadGen` and discards stale success, stale failure, and stale loading-clear. The workspace-change `$effect` bumps the generation exactly once — the base's per-loader `rulesRequestSeq` was subsumed by the shared counter, and the pre-existing `WorkspaceHomeRulesFailure.test.js` (blob-identical at base and candidate) proves both the failure→alert→Retry path with real API args (`{scope:'Workspace', scope_id:'ws-1'}`) and the stale-response guard (workspace A's delayed response resolved after navigation to B cannot overwrite B's rules). Agent Rules merges Workspace-scoped with ALL Global meta-specs (spec §2: optional tenant rules visible for binding selection), grouped by kind with Tenant/Workspace badges and 🔒 on required. Settings/rule management use canonical routes (`/workspaces/:slug/settings`, `/workspaces/:slug/agent-rules`).

**§3/§7 Repo mode.** `goToRepo` defaults to Specs (canonical bare repo URL per §7); all tabs are addressable; `parseUrl` handles multi-segment spec paths and entity detail routes. popstate restores workspace (wsId, slug fallback), repo (`repoIdCache` + API resolution fallback), and tab. The Architecture tab has the Graph | Briefing control-bar sub-tab (`?subTab=briefing` deep link; AppShell test asserts the drawer's Briefing click at repo scope lands on `/r/:repo/architecture?subTab=briefing`, staying in scope rather than escaping to workspace home). Drawer/section navigation at tenant scope preserves tenant scope (Agent Rules → `/all/agent-rules`, settings admin-only no-op otherwise).

**Repo Briefing narrowing (server-side, real enforcement).** `?repo_id=` on GET briefing and `repo_id` in the `briefing/ask` body are containment-validated before use: unknown repo → 404 (no existence leak), repo in another workspace → 403. `assemble_briefing` filters MRs, tasks, cross-workspace links (by `source_repo_id`), spec-assertion notifications (by `n.repo_id`), gate failures (via filtered MRs), and completed agents (via each agent's `repo_id` binding; workspace orchestrators excluded under a filter) — all covered by the two new Rust tests asserting both per-repo directions and the unfiltered baseline. The MCP resource delegation passes `None` (workspace scope) — correct; MCP briefing is workspace-scoped. `briefing_ask` grounds the LLM context with the repo name.

**Repo Decisions.** Inbox at repo scope client-filters on exact `repo_id`, so workspace-only notifications (`repo_id: null` — trust suggestions, meta-spec drift) never appear under a repo; the workspace filter is server-side via `?workspace_id=` (real Diesel equality filter in the SQLite adapter). Inbox carries its own `loadGen` stale guard.

## Notes (non-blocking, not defects)

- `check-task-commit-attribution.sh` fails on `a781ede2`/task-210 — pre-existing at base (`a781ede2` is an ancestor of `8c2d1775`, present on origin/main and absent from main's task-210.md); outside this task's scope. The two task-083-labeled merge commits in range touch only pipeline scripts/docs on first-parent and are not product surface; the script does not flag them.
- `AppShell.test.js` `parseUrl`/`urlFor` unit tests exercise a documented local replica, not the production functions (pre-existing at base). The behavioral tests do cover the production routing.
- Cosmetic: `rules-scope-badge`/`rules-count` markup classes have no CSS rules, and the rule-item version span uses class `rule-version` while the new CSS block defines `.rules-version` (base defined `.rule-version`; the candidate kept the markup class but renamed the rule). Unstyled badge/version only; the full suite and rendering tests pass. Not a contract failure.

## Contract-prose integrity

An earlier attempt ticked the acceptance checkboxes, which the pipeline flagged as a contract change; the candidate restores the assigned text byte-for-byte and claims satisfaction in the `## Shipped` operational section instead — verified by diff against the base task file.
