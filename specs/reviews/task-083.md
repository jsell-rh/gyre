# Review — task-083 (Canonical Navigation — Scope-Aware Content Routing)

Spec: `specs/system/ui-navigation.md` §2 (Workspace Home), §3 (Repo Mode),
§7 (URL Structure), §10 (Cross-Workspace View), plus the §4-basis scope-transition
requirements quoted in the task (stale-response guard, error surfacing).
Candidate: `804ce394b9796cacc952092c7d909ffd52f8a1d5` (base `124a852be`, includes
the baseline merge `c42b8f85a`). Verdict: **approved**.

All evidence (commands, mutations, outputs) retained under
`/tmp/stage/review-evidence/README.md`. Summary of independent verification:

## Probes run (at the exact candidate HEAD)

- `cargo test -p gyre-server --lib api::specs::tests` → **58 passed / 0 failed**,
  including `list_specs_honors_workspace_id_scope`,
  `reject_spec_notification_uses_workspace_tenant`,
  `reject_spec_skips_notifications_when_workspace_unresolvable`.
- **Mutation probe (server scoping):** neutralizing the `?workspace_id=` filter
  in `list_specs` (in_workspace forced true) fails exactly
  `list_specs_honors_workspace_id_scope`; restored, it passes. The server-side
  scoping is real and pinned.
- **Mutation probe (§4 stale-response guards):** forcing `stale()` → false in
  WorkspaceHome.svelte and removing the `gen !== loadGen` checks in Inbox.svelte
  (2 sites) and Briefing.svelte (5 sites) fails **all 3** StaleScopeResponse
  tests (late old-scope response replaces the new workspace's content);
  guards intact → 3/3 pass. The generation guards are real, not audit-only.
- Focused vitest (after `npm ci`): new regressions (StaleScopeResponse,
  CrossWorkspaceEntry, WorkspaceHomeRulesFailure) → 7/7; the navigation suite
  (16 files: WorkspaceHome* family, CrossWorkspaceHome, Inbox, Briefing,
  ExplorerViewScope, ScopeTransitions, SpecDashboard, AppShell, RepoMode,
  RepoModeHeader, TenantSettings, WorkspaceSettings) → **437 passed / 0
  failed**; shell/mobile (NoSidebar, BidirectionalNav, WorkspaceCards) → 60/60.
- Gate scripts at candidate: check-byte-slice-truncation OK,
  check-fabricated-scope-defaults OK (4 entries = FROZEN_EXEMPTION_COUNT 4),
  check-arch OK, check-mem-port-contracts OK, check-task-commit-attribution OK.

## Contract assessment

1. **Cross-workspace (§10):** all five sections render from real per-workspace
   API results; Specs aggregates per accessible workspace (`Promise.allSettled`
   over `specsForWorkspace(ws.id)`, never the unscoped list — regression pins
   `not.toHaveBeenCalledWith(null)`) with attribution backfill; the
   `{notifications:[...]}` server envelope is pinned by regressions. Item clicks
   (`goToEntityDetail`) resolve the entity's repo, derive the owning workspace
   from the caller's membership, switch scope, persist, and reload — including
   the page-load-at-/all path, with an error toast and no fabricated identity
   when the owner is undeterminable (verified by CrossWorkspaceEntry tests
   rendering the REAL App and asserting URL, localStorage, and history.state).
   Tenant settings/rules stay admin-gated.
2. **Workspace home (§2):** every section loader queries with the selected
   workspace's id behind the shared generation guard; Specs is additionally
   scoped server-side (`GET /api/v1/specs?workspace_id=` — filter + regression +
   passing mutation probe; docs/api-reference.md updated). Agent Rules merges
   the real Global + Workspace sources with scope badges, 🔒 on required only,
   and surfaces failures as error + Retry rather than empty success.
3. **Repo mode (§3/§7):** `goToRepo` defaults to Specs; parseUrl/urlFor
   implement the §7 table (multi-segment spec paths, entity routes, canonical
   workspace routes for settings/agent-rules); the Graph|Briefing Architecture
   sub-tab is deep-linkable (`?subTab=briefing`) and drawer Briefing stays at
   repo scope; repo Decisions filter on exact `repo_id` so `repo_id: NULL`
   never appears under a repo (regression); repo Briefing narrows via
   `?repo_id=` on GET briefing and the ask body, reusing the baseline's
   server-side narrowing (task-196) as instructed.
4. **Scope transitions:** pushState/popstate throughout; stale-response guards
   verified by mutation probe; lookup failures surface as errors (unknown
   workspace toast, Agent Rules error + Retry, per-section load errors).

The visual-baseline re-baseline (c69d9fd5) is justified: dimension analysis of
all 12 changed shots shows exactly the claimed uniform 34px canvas-area shrink
(919x397→919x363, 579x335→579x301, etc.) from the spec-required sub-tab bar,
with toolbar/tiered baselines untouched and the spec file (locators,
maxDiffPixelRatio) unchanged. A live e2e run was blocked by the sandbox's
loopback client-connection restriction (SSRF filter on 127.0.0.0/8 blocks
Playwright's webServer health check; no chromium process spawns) — an
infrastructure restriction, not a code defect; the verifier should run
`cd web && npx playwright test tests/e2e/explorer-visual.spec.js` where
loopback works.

## Non-blocking observations

- `goToEntityDetail`'s `if (!currentRepo) return;` (repo unresolvable after
  entering the owning workspace) is a pre-existing base behavior retained;
  the candidate fixed the reported silent-no-op path (unresolvable owning
  workspace → error toast) but this narrower edge (deleted/inaccessible repo)
  still aborts silently and can leave the workspace selection switched.
  Recorded for a future hardening pass; not a regression and not required by
  this task's acceptance criteria.
- The candidate did not touch `specs/coverage/system/ui-navigation.md` — no
  side-effect verification claims; conservative and correct (auditor-owned).

Independent evidence supports the task contract; no fake implementations,
missing enforcement, unsafe scope handling, or weak tests found.
