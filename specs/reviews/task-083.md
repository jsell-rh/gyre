# Review: task-083 — Canonical Navigation — Scope-Aware Content Routing

Candidate: `049a7870aaa1c6515b2dfec4273256c17e1d40cf` (merge-continuation
round: task-side checkpoint `937a0409` merged onto assigned base
`1d9547defeb08d9cd2a40f3c656e20644b625602` at merge commit `9c41bce0`, plus
process commit `049a7870`). Evidence:
`/tmp/stage/review-evidence/task-083-review-049a7870.md`.

## Scope of this round

The task-side source (`api/graph.rs`, `mcp.rs`, `docs/ui.md`, `web/src`
navigation components) is byte-identical to the previously reviewed
checkpoint `937a0409`; the only new content is the base merge and the
process commit recording the verification. This review therefore covers
(1) merge integrity and (2) independent re-verification of the task
contract against the merged tree.

## Merge integrity — verified

- Merge-base of task side and base = `88bd81bf`. Base-side source changes
  since the fork: `explorer_ws.rs`, `llm_helpers.rs` (task-070) only;
  task-side: `api/graph.rs`, `mcp.rs`, `docs/ui.md`, `web/src`. **No source
  overlap** — the Shipped section's claim is accurate, and the 'ort'
  auto-merge could not semantically conflict on source.
- Base-side source changes are retained in the candidate; task-side source
  is unchanged by the merge and the process commit (`git diff` empty both
  ways). Conflict-marker scan: clean.
- `web/dist`: the net task-side dist change since the fork is empty (the
  task's earlier dist rebuilds did not survive the checkpoint chain); only
  the base side rebuilt dist (task-070). The merge correctly took the
  base's bundle, so the committed dist predates the task-083 UI changes.
  This is disclosed in the Shipped notes ("final integration rebuilds it
  remotely") and is the assignment-sanctioned state for this round. To
  substitute evidence, the current web source was rebuilt:
  `npm run build` → exit 0 in ~46s, and the fresh bundle contains the
  task-083 UI markers (`arch-subtab-briefing`, `goToEntityDetail`,
  `wsLoadGen`). **Verification must rebuild `web/dist` at integration** —
  the committed bundle cannot serve the task-083 UI.

## Independent verification of the task contract

- **Focused frontend navigation suite** (20 files, incl. all WorkspaceHome
  variants, Inbox, Briefing, RepoMode ×2, ExplorerView scope/drill,
  CrossWorkspaceHome, AppShell, ScopeTransitions, NoSidebar,
  WorkspaceDrawerSectionNav, WorkspaceSettings, TenantSettings):
  **430 passed / 0 failed** (41 pre-existing skips).
- **Rust briefing repo-scope suite** (`cargo test -p gyre-server
  api::graph::tests`): **28 passed / 0 failed**, including the new
  HTTP-level guards (`briefing_get_repo_filter_scopes_and_validates_at_http_level`
  asserts 200+narrowed / 403 cross-workspace repo / 404 unknown repo;
  `briefing_ask_repo_scope_abac_denied_returns_403` seeds a per-repo ABAC
  policy the JWT caller lacks and asserts 403 — a real G6 caller-scope
  guard on the request-body repo scope).
- **Mutation probe (stale-response guard, §4)**: temporarily removed the
  `stale(gen)` guard in `WorkspaceHome.svelte` `loadRules` →
  `WorkspaceHomeRulesFailure.test.js` "keeps workspace B rules when
  workspace A finishes loading after navigation" **fails**; restored. The
  delayed old-scope test is a genuine regression test, not self-confirming.
- **Independent drill probe (§10 acceptance 1)**: temporary App-render
  test (deleted after the run) — from workspace home via the "All
  Workspaces" dropdown, clicking a spec attributed to another workspace
  navigates to `/workspaces/logistics/r/ship-svc/specs/specs/ship/refactor.md`
  (the owning workspace's repo mode) and reloads scoped data for the
  owning workspace (`api.workspaceBudget('ws-2')`). Attribution links enter
  the item's actual owning scope; no fabricated scope identity (unknown
  owner surfaces an error toast).
- **Mechanical invariants**: `check-abac-route-registry.sh` OK,
  `check-mcp-write-tools.sh` OK (8 write tools gated),
  `check-task-commit-attribution.sh` OK.
- **Contract integrity**: "Authority and Scope", "Required Behavior", and
  "Acceptance and Verification" sections of `specs/tasks/task-083.md` are
  byte-for-byte identical to the assigned task text; acceptance boxes
  unticked; only `progress:`, `commits:`, and the operational `## Shipped`
  section differ, as disclosed.

## Observations (non-blocking)

- **Pre-existing at base, outside the diff:** landing directly on `/all`
  (bookmark / refresh / shared link — §7 promises URL-addressable deep
  links) leaves `currentWorkspace` null (the mount branch for
  `cross_workspace` sets only `mode`/`crossWorkspaceTab`), and
  `goToEntityDetail`'s first statement is `if (!currentWorkspace) return;`
  — so item clicks in the cross-workspace home are silent no-ops until the
  user enters a workspace once. Verified identical at base `1d9547de`
  (same guard, same mount branch, same `nav()` wiring in
  `CrossWorkspaceHome`), so the candidate did not introduce it, and the
  candidate's diff fixes the drill for the normal navigation path. This
  belongs to the canonical-shell entrypoint flow (task-082 / main-baseline
  scope, which this task was told to preserve), not to this diff. Recommend
  a follow-up task: on mount at `/all`, seed `currentWorkspace` from the
  persisted `gyre_workspace_id` (or first membership) so drill-downs work
  from deep links.
- `CrossWorkspaceHome.svelte` passes `tenant_id: 'default'` in its
  create-workspace call — pre-existing (the task's diff to this file is
  only the notifications-shape fix), and the server derives/validates
  tenant scope; noted only for completeness.

## Verdict

The merge is sound (no source overlap, both sides intact), the task-side
implementation satisfies the assigned required behaviors with real,
independently verified production code and genuine regression tests, and
the contract prose is intact. The committed `web/dist` staleness is the
assignment-sanctioned deferral and is covered by a clean source-rebuild
probe; integration must rebuild it. **Approved.**
