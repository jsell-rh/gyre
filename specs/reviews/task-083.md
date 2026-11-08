# Review — task-083 (Canonical Navigation — Scope-Aware Content Routing)

Spec: `specs/system/ui-navigation.md` §2 (Workspace Home), §3 (Repo Mode),
§7 (URL Structure), §10 (Cross-Workspace View); scope-transition requirements
from the task's Required Behavior 4.
Candidate: `01f5e3bc74fa7041eb75a940559b9e51ebcb62f9` (base
`39c7759f6b8fbc55adcdf00072cf644201fb790e`). Verdict: **approved**.

Evidence: focused probes with commands, sources, exit codes, and outputs under
`/tmp/stage/review-evidence/` (see its README). All probes ran against the
exact candidate with a clean tree; the two mutation probes below were restored
and `git status --porcelain` verified empty afterwards.

## What was verified (independently, not from the implementer's labels)

- **§10 cross-workspace:** `CrossWorkspaceHome` renders all five sections from
  real per-workspace API calls. The Decisions envelope fix (`{notifications:[...]}`,
  commit 7e5cdaba) is pinned by two regressions that fail under the pre-fix
  `data?.items` read. Specs aggregate per accessible workspace
  (`Promise.allSettled` over `specsForWorkspace(ws.id)`) with
  workspace/repo attribution backfilled on every row; the stat cards derive
  from the same scoped list. `goToEntityDetail` resolves the owning workspace
  from the caller's membership — including the page-load-at-`/all` case
  (repair 084040ecc, regression `CrossWorkspaceEntry.test.js` renders the real
  App) — and surfaces an error toast when no owner can be determined. Tenant
  settings / tenant agent-rules stay admin-gated (`onSettings`/
  `onManageAgentRules` undefined for non-admins; `userIsAdmin` derived from
  `api.me()`).
- **§2 workspace home:** every section loader queries with the selected
  workspace's id; server-side scoping of `GET /api/v1/specs?workspace_id=` is
  real (filter by direct `workspace_id` or via the owning repo; excluded
  otherwise) and pinned by `list_specs_honors_workspace_id_scope` (ws-a vs
  ws-b vs empty vs unscoped-all assertions). Agent Rules merges
  `getMetaSpecs(scope=Workspace&scope_id=:id)` with all `scope=Global` rules,
  shows the full cascade with Tenant/Workspace badges and 🔒 on required, and
  a failed lookup surfaces as error + Retry (`WorkspaceHomeRulesFailure`).
- **§3/§7 repo mode:** Specs is the default tab (`goToRepo` default,
  `parseUrl` fallback); all tabs are URL-addressable per the §7 table,
  including multi-segment spec paths and entity detail routes; popstate
  restores workspace/repo/tab with `repoIdCache` + API fallback. The
  Architecture Graph|Briefing sub-tab is deep-linkable (`?subTab=briefing`)
  and the param is retained after the initial read (regression in
  `ExplorerViewScope.test.js`). Repo Briefing is narrowed server-side:
  `?repo_id=` on GET briefing and in the `briefing/ask` body, with 404 for
  unknown repos, 403 for cross-workspace repos, a per-repo ABAC
  caller-scope guard on the ask path (JWT admin cannot bypass repo policy by
  naming a repo in the body), and `assemble_briefing` filtering MRs, tasks,
  cross-workspace links, spec-assertion notifications, and completed agents
  by their repo binding — 20 focused Rust briefing tests pass. Repo Decisions
  (Inbox scope=repo) filters client-side on exact `repo_id`, so workspace-only
  notifications never appear under a repo (regression in `Inbox.test.js`).
  Drawer Briefing at repo scope stays in the repo (AppShell regression:
  `/r/:repo/architecture?subTab=briefing`, not workspace home).
- **§4 scope transitions:** client-side navigation with pushState/popstate
  throughout. Generation guards in `WorkspaceHome` (shared `wsLoadGen`),
  `Inbox`, and `Briefing` are pinned by `StaleScopeResponse.test.js`, which
  renders the real components, switches scope via the prop change App.svelte
  drives, and resolves the old scope's deferred response after the switch.
- **CI-gate repair (6cf4a224):** both exemption removals are real fixes, not
  re-pinning — `spec_index` truncates via `chars().take(8)` (byte-slice gate
  OK with the entry deleted) and `reject_spec` resolves the tenant from the
  workspace record, skipping + logging when unresolvable (fabricated-scope
  gate OK with the entry deleted and FROZEN_EXEMPTION_COUNT lowered 5 → 4).
- **Gates:** arch, abac-route-registry, mcp-write-tools, forged-scope-fields,
  inert-enforcement, forwarded-header-trust, relative-path-defaults,
  byte-slice-truncation, fabricated-scope-defaults — all exit 0 at the
  candidate.
- **No superseded-plan regression:** no permanent sidebar (`NoSidebar` passes),
  no legacy breadcrumb scope control, no Architecture-as-default routing.

## Mutation probes (performed by this review)

1. Restoring the legacy `entry.repo_id.unwrap_or("default")` tenant fallback in
   `reject_spec` fails `reject_spec_notification_uses_workspace_tenant`
   (exit 101, tenant assertion) — the regression genuinely enforces the
   workspace's real tenant.
2. Removing the post-await `gen !== loadGen` guard in `Inbox.svelte` fails the
   Inbox stale-scope regression (the late old-workspace response replaces the
   new workspace's rows); guards intact → 3/3 pass.

## Test-quality spot checks

Focused vitest batches: 3 files/7 tests, 4 files/146 passed (41 skipped — all
three skips are pre-existing `describe.skip` "old layout" blocks present at the
base), 8 files/216 tests, 4 files/52 tests — 0 failures. New/modified tests
assert real API arguments (`myNotifications()` / `myNotifications({workspace_id})`,
`getWorkspaceBriefing(id, null, repoId)`, `specsForWorkspace(ws.id)` per
workspace, `briefingAsk` body with `repo_id`) and rendered results, not mock
echoes.

## Non-blocking observations (not findings)

- `goToEntityDetail` resolves the owning workspace from `data.workspace_id`
  before the repo's own workspace. For a ledger entry whose `workspace_id` and
  `repo_id` disagree (or a workspace_id-only entry with no repo), the click
  switches workspace but then returns at `if (!currentRepo) return;` without a
  toast. The production sync path (`spec_registry.rs::sync_spec_ledger`) always
  sets `repo_id` and `workspace_id` together from the same source repo, so this
  state is not reachable through normal ingestion; the reachable defect class
  (no workspace selected at `/all`) is repaired and tested.
- `navigateToSpec` on workspace home no-ops if the spec's repo is absent from
  the workspace's repo list — same practically-unreachable data-divergence
  class as above.

Full suites, all-target Clippy, and exact-head CI remain owned by verification.
