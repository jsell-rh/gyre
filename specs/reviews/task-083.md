# Review: task-083 — Canonical Navigation — Scope-Aware Content Routing

Candidate reviewed: `0c09a6fa833b3718793b28e22d78f30b77ad6f40`
(assigned base `11b7460496b7b0916d00476d7e9b4d9851153f68`).
Evidence: `/tmp/stage/review-evidence/README.md` (probe commands, outputs,
mutation results). Working tree verified clean after all mutations.

## Verified with independent evidence

- **Cross-workspace (§10).** All five sections render from real per-workspace
  API results; Specs aggregates per-workspace via `specsForWorkspace(ws.id)`
  with attribution backfill instead of the unscoped list (test "fetches specs
  per accessible workspace, not the unscoped list"). The server-side scope
  filter is real: mutation M3 (filter body no-op'd) fails
  `list_specs_honors_workspace_id_scope`. `/all` page-load entry is real:
  mutation M1 (restored base's `if (!currentWorkspace) return;`) fails both
  CrossWorkspaceEntry tests, which render the real App + real
  CrossWorkspaceHome and assert URL, persisted selection, history state, and
  the error toast on undeterminable owner. Tenant settings / tenant rules stay
  admin-gated (`onSettings`/`onManageAgentRules` undefined for non-admins,
  App.svelte:1747-1748; gated in tests).
- **Workspace home (§2).** Every section loader queries with the selected
  workspace's id behind the shared `wsLoadGen` guard (see finding F1 for the
  guard's test gap). Agent Rules merges `scope=Workspace&scope_id=:id` with ALL
  `scope=Global` rules, grouped by kind, scope-badged, 🔒 on required;
  failed lookup surfaces as error + Retry (tested). Settings/rule management
  use the canonical routes (`/workspaces/:slug/settings`,
  `/workspaces/:slug/agent-rules`).
- **Repo mode (§3/§7).** `goToRepo` defaults to the Specs tab
  (App.svelte:356); `parseUrl`/`urlFor` implement the §7 URL table including
  multi-segment spec paths and entity detail routes; repo Decisions filters on
  exact `repo_id` so `repo_id: NULL` workspace notifications never appear
  under a repo (Inbox test "repo scope shows only notifications with matching
  repo_id"); repo Briefing is narrowed server-side — mutation M4 (GET briefing
  passes `None` instead of `repo_filter`) fails
  `briefing_get_repo_filter_scopes_and_validates_at_http_level`; the handler
  404s unknown repos and 403s cross-workspace repos; `briefing/ask` carries a
  per-repo ABAC caller-scope guard (tested with a JWT-admin denial case).
- **CI-gate repair (6cf4a224).** Both previously-failing gates now pass at the
  candidate (`check-byte-slice-truncation` OK via `chars().take(8)`;
  `check-fabricated-scope-defaults` OK via workspace-record tenant resolution
  with skip+log). The fixes are real, not exemption shuffles: the byte-slice
  exemption list shrank by one entry and the fabricated-scope list shrank by
  one with `FROZEN_EXEMPTION_COUNT` lowered 5→4 per each check's documented
  procedure. Mutation M2 (restored the `"default"` tenant fallback) fails
  `reject_spec_notification_uses_workspace_tenant`.
- **Contract integrity.** `specs/tasks/task-083.md` differs from the assignment
  only in `progress:`, `commits:`, and the appended `## Shipped` operational
  section — the acceptance checkboxes are restored byte-for-byte. Commit
  attribution check passes; all 8 listed product commits are on the branch.
  The canonical shell has no permanent sidebar (App.svelte:1263); mobile
  workspace-section navigation intact.
- **Focused suites.** `cargo test -p gyre-server --lib api::specs::tests` →
  54/0; `api::graph::tests` → 28/0. After `npm ci`: vitest 13 files →
  438 passed / 0 failed across the navigation suites.

## R1 Findings

- [open] **F1: The §4 acceptance criterion "Delayed old-scope responses cannot
  replace current-scope content" has no regression test.** Mutation probe: with
  every stale-response generation guard disabled (`stale()` forced to
  `return false` in WorkspaceHome.svelte; the `if (gen !== loadGen) return;`
  checks removed at Inbox.svelte (2 sites) and Briefing.svelte (5 sites)), the
  full navigation suite still passes — 242 tests across 9 files, 0 failures.
  No shipped test detects removal of the guards. Context: at the assigned base,
  Inbox.svelte and Briefing.svelte had no stale-response guard at all and
  WorkspaceHome.svelte guarded only the Agent-Rules loader, so this was a
  demonstrated missing behavior that this task fixed (guards added in
  checkpoint 13d856c82); the task's verification rule requires a regression
  "with real API arguments and rendered results" for demonstrated missing
  behaviors, and every other repair in this task's Shipped notes carries a
  mutation-probe-verified regression — this one does not. The production
  behavior itself is NOT fake: a reviewer-written throwaway test (real
  WorkspaceHome render; deferred old-workspace `myNotifications` promise
  resolved after a rerender to a new workspace; assert the new workspace's
  decision item stays rendered and the stale item never appears) passes at the
  candidate HEAD and fails with the guard disabled. The throwaway was deleted
  after the probe (tree clean); the repair is a test-only addition of exactly
  that shape.

## Verdict

Not approved on F1 alone. Everything else in the task contract — including the
production code for F1's behavior — is verified real by independent evidence
(probe + mutation results under /tmp/stage/review-evidence). Repair: add the
generation-guard regression (real API arguments, rendered results, kills on
guard removal) and re-submit.
