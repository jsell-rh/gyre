---
title: "Repo lifecycle — archive push rejection, admin repos tab, gate config UI"
spec_ref: "repo-lifecycle.md §API Summary"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "repo-lifecycle.md §Admin → Workspace Scope → Repos Tab"
  - "repo-lifecycle.md §Gates (Admin → Repo Scope → Gates)"
  - "repo-lifecycle.md §API Summary"
commits: ["0106d3a3adfe0e4c7c175d81e0d301583a7b3255", "d8e78bb6c440c7ba24103f18be30dfbcd669f3b5", "df5b32464a082ce008236a2732536755538ebe03", "9ebec6c2713ce824aa5ca9f14df74dcecb074437", "904586888416cb5c5aec355a70875b271e005a90", "b657692571ca3e6f53528afeec1578dfb46c9e7a", "06e11cbcf59b1fdd0247411c5e8910c4c6190714", "1da8dd39d018de0a101f550198ce224d077329f6", "4c11b2644f2435a87e60a38b8b8f356722562f14", "4b5d96563755dec7fbd825880c23661c604f7a8d", "5d5e4046f0af0cf074348b584d632e36f3f6b570", "0a2df7c6a80141bc885937dc3543e13055bf8ee3", "568d27f2cc4ca542ebe3b138872b710dd693a5e0", "68805172e3c0abb0315928d7f24d3ba2aaa16673"]
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

- [ ] `git push` to an archived repo returns an error (not silently accepted)
- [ ] Admin workspace scope has a "Repos" tab listing all repos
- [ ] Repos tab shows status badges and "+ New Repo" / "Import Repo" buttons
- [ ] Repo scope admin has a "Gates" panel showing gate configuration
- [ ] Gates can be added, toggled, and configured through the UI
- [ ] `cargo test --all` and `cd web && npm test` pass

## Agent Instructions

Read `specs/system/repo-lifecycle.md` §"1. Where Repo Management Lives", §"3. Repo Configuration — Gates", and §"6. Domain Changes — API Summary". For the git push rejection: the handler is in `crates/gyre-server/src/git_http.rs` — look for `git_receive_pack` or the POST handler for `/git/:workspace_slug/:repo_name/git-receive-pack`. The repo status check should go after `resolve_repo_by_slug()`. For UI: follow existing Svelte 5 patterns in `web/src/` (e.g., WorkspaceSettings.svelte, RepoSettings.svelte). The gate API routes are at `GET/POST /api/v1/repos/:id/gates`.

## Shipped

All three coverage sections are implemented in production code (earlier checkpoints
`80d6563`/`6194782` plus the `0106d3a` test-framing fix from this assignment).

### Git push rejection for archived repos (§API Summary)

- `git_info_refs` rejects the `git-receive-pack` advertisement for archived repos
  with a 200 response whose pkt-line stream carries
  `ERR push rejected: repository is archived` — the git-smart-HTTP convention so
  `git push` actually prints the reason (a bare 403 body is never shown by git).
- `git_receive_pack` (POST) keeps a defense-in-depth 403 before the packfile is
  processed, for clients that skip the advertisement.
- Clone/fetch (`git-upload-pack`) still works on archived repos — read-only, not
  invisible.
- Evidence: unit tests
  `info_refs_receive_pack_archived_repo_returns_err_pkt_line`,
  `receive_pack_archived_repo_returns_403`,
  `info_refs_upload_pack_archived_repo_still_serves_clone_advertisement`
  (28 passed, `cargo-lib-gates-archived.log`), and integration test
  `push_to_archived_repo_rejected` in `crates/gyre-server/tests/git_integration.rs`
  which runs a **real `git push`** against a live server and asserts the push
  fails with an "archived" reason (1 passed,
  `cargo-integration-archived-push.log`).
- `0106d3a` fixed the first unit test's framing assertion: it asserted the body
  starts with `# service=...` but the correct wire format starts with the
  pkt-line length prefix (`001f# service=git-receive-pack\n0000`); the assertion
  now pins the exact expected bytes.

### Admin workspace-scope Repos tab (§Admin → Workspace Scope → Repos Tab)

- `WorkspaceSettings.svelte` gains a `repos` tab: table of all workspace repos
  with name, Active/Archived status badge, active agent count, and last-activity
  timestamp; "+ New Repo" and "Import Repo" buttons with creation/mirror forms;
  clicking a repo navigates to repo scope via the `goToRepo` context.

### Repo-scope Gates panel (§Gates — Admin → Repo Scope → Gates)

- `RepoSettings.svelte` gains a Gates panel: lists gates via
  `GET /api/v1/repos/:id/gates` ordered by `position`, per-gate enabled toggle,
  configuration fields (test/lint command, required approvals, reviewer
  persona, timeout), "+ Add Gate" with gate-type selector, and drag-to-reorder
  saving positions via `PUT /api/v1/repos/:id/gates/:gate_id`.
- Backend: new `PUT /api/v1/repos/:id/gates/:gate_id` (`gates::update_gate`)
  with per-gate-type field validation; gate ordering via a new `position`
  column (migration `2026-10-08-000056_gate_position`, SQLite + PostgreSQL
  portable) enforced in both adapters and the mem store.
- Evidence: `web/src/__tests__/WorkspaceSettings.test.js` +
  `web/src/__tests__/RepoSettings.test.js` — 114 vitest tests pass
  (`vitest-task168.log`); gates API unit tests in `gates.rs` (position
  ordering, update validation) pass in the cargo lib run above.

### Invariants

`check-arch.sh`, `check-abac-route-registry.sh`, `check-migration-versions.sh`,
`check-migration-sql-portability.sh`, `check-mem-port-contracts.sh`,
`check-inert-enforcement.sh`, `check-dead-message-kinds.sh`, and
`check-fabricated-scope-defaults.sh` all pass on this tree (the
fabricated-scope exemption diff vs base is a line-number retarget of the
pre-existing git_http entry, not a new entry).
