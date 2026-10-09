---
title: "Repo lifecycle — archive push rejection, admin repos tab, gate config UI"
spec_ref: "repo-lifecycle.md §API Summary"
depends_on: []
progress: complete
coverage_sections:
  - "repo-lifecycle.md §Admin → Workspace Scope → Repos Tab"
  - "repo-lifecycle.md §Gates (Admin → Repo Scope → Gates)"
  - "repo-lifecycle.md §API Summary"
commits: ["b657692571ca3e6f53528afeec1578dfb46c9e7a", "06e11cbcf59b1fdd0247411c5e8910c4c6190714", "1da8dd39d018de0a101f550198ce224d077329f6", "4c11b2644f2435a87e60a38b8b8f356722562f14", "4b5d96563755dec7fbd825880c23661c604f7a8d", "5d5e4046f0af0cf074348b584d632e36f3f6b570", "0a2df7c6a80141bc885937dc3543e13055bf8ee3", "568d27f2cc4ca542ebe3b138872b710dd693a5e0", "68805172e3c0abb0315928d7f24d3ba2aaa16673"]
---

## Spec Excerpt

### §API Summary (repo-lifecycle.md)

Git push rejection: archived repos should reject pushes with an appropriate error.

### §Admin → Workspace Scope → Repos Tab (repo-lifecycle.md)

Repo management is a tab in the Admin view at workspace scope. The **Repos tab** shows:
- List of all repos (name, status, agent count, last activity)
- "+ New Repo" button
- "Import Repo" button
- Click a repo → navigates to repo scope

### §Gates (Admin → Repo Scope → Gates) (repo-lifecycle.md)

- List of configured gates (name, type, enabled/disabled toggle)
- "+ Add Gate" button with gate type selector
- Per-gate configuration (test command, lint command, reviewer persona)
- Drag to reorder

## Implementation Plan

1. **Git push rejection for archived repos**: In `crates/gyre-server/src/git_http.rs`, the `git_receive_pack` handler (POST endpoint) should check the repo's `status` field. If `RepoStatus::Archived`, return an error response before processing the pack. The error message should explain that the repo is archived. Check `resolve_repo_by_slug()` — it already loads the repo entity; add a status check after resolution.

2. **Admin Repos tab**: Create a `RepoList.svelte` component (or extend the existing workspace settings) that shows all repos in the workspace with: name, status badge (Active/Archived), active agent count, last activity timestamp. Include "+ New Repo" and "Import Repo" buttons that open the existing creation forms. Wire into the Admin workspace-scope tab bar.

3. **Gate configuration UI**: Create a `GateConfig.svelte` component for repo-scope admin that:
   - Lists configured gates via `GET /api/v1/repos/:id/gates`
   - Shows each gate's name, type, and enabled status with a toggle
   - Provides "+ Add Gate" button with a type selector dropdown
   - Shows per-gate configuration fields (test command, lint command, etc.)
   - Saves changes via `PUT /api/v1/repos/:id/gates/:gate_id`
   - Wire into the repo-scope admin view

## Acceptance Criteria

- [x] `git push` to an archived repo returns an error (not silently accepted) — 403 + "push rejected: repository is archived" before packfile processing (git_http.rs, `resolved.is_archived()` check after `resolve_repo_by_slug()`); covered by unit test `receive_pack_archived_repo_returns_403` (passing) and full-server integration test `push_to_archived_repo_rejected` (real `git push`, runs on host — sandbox cannot bind loopback listeners)
- [x] Admin workspace scope has a "Repos" tab listing all repos — WorkspaceSettings.svelte Repos tab (name, status badge, agent count, last activity)
- [x] Repos tab shows status badges and "+ New Repo" / "Import Repo" buttons
- [x] Repo scope admin has a "Gates" panel showing gate configuration — RepoSettings.svelte Gates panel
- [x] Gates can be added, toggled, and configured through the UI — add/type-selector, enabled toggle, per-gate command config, position (drag-order) via gates API (`position` column + migration 000056)
- [x] `cargo test --all` and `cd web && npm test` pass — focused suites pass (git_http 37/38; the 1 failure is `git_clone_empty_repo_via_smart_http`, a pre-existing OpenShell loopback-listener limitation, `getpeername() errno 95`, not a code defect; gates API 11/11; web RepoSettings+WorkspaceSettings 114/114); full suites left to the controller per round protocol

## Agent Instructions

Read `specs/system/repo-lifecycle.md` §"1. Where Repo Management Lives", §"3. Repo Configuration — Gates", and §"6. Domain Changes — API Summary". For the git push rejection: the handler is in `crates/gyre-server/src/git_http.rs` — look for `git_receive_pack` or the POST handler for `/git/:workspace_slug/:repo_name/git-receive-pack`. The repo status check should go after `resolve_repo_by_slug()`. For UI: follow existing Svelte 5 patterns in `web/src/` (e.g., WorkspaceSettings.svelte, RepoSettings.svelte). The gate API routes are at `GET/POST /api/v1/repos/:id/gates`.


## Review (2026-10-09, round 2)

Comparison base 66422bd4 → HEAD 83ff5d3. Task code byte-identical to the
prior round's evidence tree (368a3b4) — only dev tooling and the frontmatter
commit list differ — so the prior round's Rust test evidence was reused after
confirming source identity; the interrupted web test run was re-executed fresh.

**§API Summary — archived push rejection: enforced.**
`git_http.rs` `git_receive_pack`: `resolved.is_archived()` check after
`resolve_repo_by_slug()` (tenant-scoped resolution: workspace by slug under
caller tenant, repo by name within workspace) and before the packfile body is
read or `git receive-pack` runs. Returns 403 + "push rejected: repository is
archived", mirroring the pre-existing read-only-mirror check pattern
(placement before `check_repo_abac` matches that sibling convention; probe
distinguishes archived vs. other states only within the caller's own tenant
scope). Unit test `receive_pack_archived_repo_returns_403` (asserts 403 +
"archived" in body; passed in prior round at 368a3b4, same source). Full-server
integration test `push_to_archived_repo_rejected` does a real `git push` after
archiving via the API and asserts failure + "archived" in stderr — cannot run
under OpenShell (loopback listener limitation); left to host gates per round
protocol, consistent with the documented `getpeername() errno 95` environment
failure. No alternate server-side push entry point exists (mirror fetch is
upstream sync, not user push; archived-mirror-sync is a pre-existing row-14
gap outside this task's sections).

**§Repos Tab: wired through the real entry point.**
WorkspaceSettings.svelte renders a `repos` tab (8th tab; keyboard nav tests
cover it as the last tab). Table columns: name (button → `goToRepo` context,
set at App.svelte:533, App is an ancestor so `getContext` resolves),
Active/Archived status badge, active agent count (workspace agents filtered
by repo + `status === 'active'`), last activity (max of repo.updated_at,
agent spawn times, MR timestamps). "+ New Repo" (POST /repos) and "Import
Repo" (POST /repos/mirror) forms. Loads via `api.workspaceRepos(wsId)` /
`api.agents({workspaceId})` / `api.mergeRequests({workspace_id})` — all real
endpoints with matching server-side query params. All 16 used
`workspace_settings.repos.*` locale keys resolve in en.json (checked
mechanically). Tests assert real API call arguments, badge contents
('Active'/'Archived'), and agent-count filtering — not self-confirming.

**§Gates: enforced, durable, load-bearing.**
`RepoSettings.svelte` gates tab: gate cards (name, type, required/optional
toggle), per-gate Configure form (command / required_approvals / persona /
timeout per gate type, matching server-side per-type validation), "+ Add Gate"
with 5-type selector, HTML5 drag-to-reorder persisting 1-based positions via
PUT. Backend: `update_gate` (PUT /api/v1/repos/:id/gates/:gate_id) does
repo-match scoping (404 on cross-repo gate id), per-type field validation
(400 on command-for-agent_review etc.), and persists via
`QualityGateRepository::save` — real storage in all three adapters
(SQLite/Postgres diesel rows + mem), with `position` column added by
migration 000056 (next unused sequence number, portable `ALTER TABLE ... ADD
COLUMN ... DEFAULT 0`). All adapters order by `(position, created_at)`
consistently; `gate_executor` consumes that order. The enabled/disabled
toggle is not a UI no-op: `check_gates_for_mr` re-reads `g.required` from
storage at merge-check time (gate_executor.rs:1088-1107) and
`run_post_merge_gate_commands` blocks only on `required` failures. API tests
cover ordering, reorder-via-PUT, toggle+command update with persistence
re-read, per-type rejection, cross-repo 404. Web tests cover toggle →
`updateRepoGate('repo-1','g1',{required:false})`, config save with edited
fields, persona-vs-command field switching, and drag → position PUTs
(g3→2, g2→3, g1 untouched).

**Mechanical gates (all pass at 83ff5d3):** check-fabricated-scope-defaults
(exemption count 7→6 after genuinely deleting the git_http.rs:1495 ws_id
fallback — the `push_workspace_id` thread now derives from authorized repo
resolution, a real fix, not gate weakening), check-migration-versions,
check-abac-route-registry (PUT added to the already-registered
`/api/v1/repos/:id/gates/:gate_id` path with existing RouteResourceMapping
at abac_middleware.rs:127), check-mem-port-contracts,
check-task-commit-attribution.

**Test evidence:** web RepoSettings+WorkspaceSettings 114/114, exit 0
(re-run this round at 83ff5d3; saved under /tmp/stage/review-evidence/).
Rust `receive_pack_archived_repo_returns_403` 1/1 and gates API 11/11
(prior round, identical task source). git_http suite 37/38 — the single
failure is `git_clone_empty_repo_via_smart_http`, the documented OpenShell
loopback limitation (`getpeername() errno 95`), not a code defect.

**Minor observations (non-blocking, not this task's scope):**
- Gate execution spawns gates concurrently (`tokio::spawn` per gate), so
  "gates execute in order" holds for result-creation order (position-ordered
  list) but not wall-clock serialization — pre-existing executor behavior;
  the cited UI section is satisfied.
- 3 pre-existing `state_referenced_locally` svelte warnings at
  WorkspaceSettings.svelte:50-52 exist identically at base 66422bd4; the
  sync `$effect` handles prop changes.
- Coverage matrix rows 3/11/19 still read `task-assigned` — controller owns
  matrix transitions on integration.

Verdict: all three cited sections have real, wired, persisted behavior with
tests that fail if the behavior breaks. **complete.**

## Shipped

- Archived repos reject `git push` with 403 "push rejected: repository is
  archived" at the receive-pack entry (before packfile processing), with unit
  + full-server `git push` integration tests; the push-path workspace scope
  no longer falls back to a fabricated "default" (exemption removed).
- Admin workspace scope gains a "Repos" tab listing every workspace repo
  (name → repo-scope navigation, Active/Archived badge, active agent count,
  last activity) with "+ New Repo" and "Import Repo" forms.
- Repo-scope admin gains a Gates panel: per-gate enabled/disabled toggle,
  per-type configuration forms (command/approvals/persona/timeout), add gate
  with 5-type selector, and drag-to-reorder persisted through a new
  `position` column (migration 000056) ordered consistently across SQLite,
  Postgres, and mem adapters — order and toggle are consumed by the real
  gate executor at merge-check time.
- New `PUT /api/v1/repos/:id/gates/:gate_id` endpoint with per-type field
  validation, cross-repo 404 scoping, and position updates (registered in the
  ABAC route registry via the existing path mapping).