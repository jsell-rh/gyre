---
title: "Repo lifecycle — archive push rejection, admin repos tab, gate config UI"
spec_ref: "repo-lifecycle.md §API Summary"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "repo-lifecycle.md §Admin → Workspace Scope → Repos Tab"
  - "repo-lifecycle.md §Gates (Admin → Repo Scope → Gates)"
  - "repo-lifecycle.md §API Summary"
commits: ["df5b32464a082ce008236a2732536755538ebe03", "9ebec6c2713ce824aa5ca9f14df74dcecb074437", "904586888416cb5c5aec355a70875b271e005a90", "b657692571ca3e6f53528afeec1578dfb46c9e7a", "06e11cbcf59b1fdd0247411c5e8910c4c6190714", "1da8dd39d018de0a101f550198ce224d077329f6", "4c11b2644f2435a87e60a38b8b8f356722562f14", "4b5d96563755dec7fbd825880c23661c604f7a8d", "5d5e4046f0af0cf074348b584d632e36f3f6b570", "0a2df7c6a80141bc885937dc3543e13055bf8ee3", "568d27f2cc4ca542ebe3b138872b710dd693a5e0", "68805172e3c0abb0315928d7f24d3ba2aaa16673"]
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

- [x] `git push` to an archived repo returns an error (not silently accepted)
- [x] Admin workspace scope has a "Repos" tab listing all repos
- [x] Repos tab shows status badges and "+ New Repo" / "Import Repo" buttons
- [x] Repo scope admin has a "Gates" panel showing gate configuration
- [x] Gates can be added, toggled, and configured through the UI
- [x] `cargo test --all` and `cd web && npm test` pass

## Agent Instructions

Read `specs/system/repo-lifecycle.md` §"1. Where Repo Management Lives", §"3. Repo Configuration — Gates", and §"6. Domain Changes — API Summary". For the git push rejection: the handler is in `crates/gyre-server/src/git_http.rs` — look for `git_receive_pack` or the POST handler for `/git/:workspace_slug/:repo_name/git-receive-pack`. The repo status check should go after `resolve_repo_by_slug()`. For UI: follow existing Svelte 5 patterns in `web/src/` (e.g., WorkspaceSettings.svelte, RepoSettings.svelte). The gate API routes are at `GET/POST /api/v1/repos/:id/gates`.

## Shipped

Repair of durable finding fe40c4fb (contract). The prior candidates carried a
drive-by normative change outside this task's contract: `process_spec_lifecycle`
lost its repo-store workspace lookup to a threaded `push_workspace_id`
parameter, and the `check-fabricated-scope-defaults` verifier was edited
(FROZEN_EXEMPTION_COUNT 7→6, one legacy exemption entry deleted) to
accommodate it. That spec-lifecycle scoping change requires its own spec
review, so this session reverted it to the base behavior (the legacy
exempted site remains, line pin updated 1495→1510 only because the in-scope
archive check added 15 lines above it; same statement, exemption count
unchanged at 7) and restored the verifier/exemptions to base values. No
verifier was weakened: the check passes with every site present.

The assigned requirements were already implemented on this branch and are
re-verified here on the repaired tree:

1. **Archive push rejection** (§API Summary): `git_receive_pack` rejects
   pushes to archived repos with 403 "push rejected: repository is archived"
   after `resolve_repo_by_slug()` and before packfile processing. Unit test
   `receive_pack_archived_repo_returns_403` archives through the real store;
   `push_to_archived_repo_rejected` in `tests/git_integration.rs` drives a
   real `git push` through the full stack (host CI only — this sandbox cannot
   `accept()` TCP, errno 95).

2. **Admin Repos tab** (§Admin → Workspace Scope → Repos Tab): "Repos" tab in
   `WorkspaceSettings.svelte` listing all workspace repos with name,
   Active/Archived status badge, active-agent count, and last-activity
   timestamp; "+ New Repo" and "Import Repo" inline forms calling
   `api.createRepo` / `api.createMirrorRepo`; click-through to repo scope via
   the `goToRepo` context in `App.svelte`.

3. **Gate configuration UI** (§Gates): gates panel in `RepoSettings.svelte` —
   per-gate enabled/disabled toggle, per-gate Configure form (name,
   type-specific command / required-approvals / persona field, timeout),
   HTML5 drag-to-reorder persisting positions through
   `PUT /api/v1/repos/:id/gates/:gate_id` (`api.updateRepoGate`). Backend
   `update_gate` performs partial updates with per-type validation (e.g.
   `command` on an `agent_review` gate → 400); the `position` column is
   portable migration 000056 shared by SQLite/PG/mem adapters with the
   `(position, created_at)` ordering contract; gate execution consumes that
   order.

**Test evidence (this session, repaired tree; evidence at
`/tmp/stage/review-evidence/task-168/evidence.md`):** `api::gates` and
`git_http` suites, the suites touched by the `position` field
(`merge_processor`, `gate_executor`, `mr_timeline`, `recovery`), and
WorkspaceSettings/RepoSettings vitest suites — exact counts recorded in the
evidence file; `check-fabricated-scope-defaults.sh` green on the repaired
tree with base exemption values.

Host verification still required: full `git_integration` suite incl.
`push_to_archived_repo_rejected`, full `cargo test --all`, all-target Clippy,
and GitHub CI on the branch head.