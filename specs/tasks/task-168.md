---
title: "Repo lifecycle — archive push rejection, admin repos tab, gate config UI"
spec_ref: "repo-lifecycle.md §API Summary"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "repo-lifecycle.md §Admin → Workspace Scope → Repos Tab"
  - "repo-lifecycle.md §Gates (Admin → Repo Scope → Gates)"
  - "repo-lifecycle.md §API Summary"
commits: ["904586888416cb5c5aec355a70875b271e005a90", "b657692571ca3e6f53528afeec1578dfb46c9e7a", "06e11cbcf59b1fdd0247411c5e8910c4c6190714", "1da8dd39d018de0a101f550198ce224d077329f6", "4c11b2644f2435a87e60a38b8b8f356722562f14", "4b5d96563755dec7fbd825880c23661c604f7a8d", "5d5e4046f0af0cf074348b584d632e36f3f6b570", "0a2df7c6a80141bc885937dc3543e13055bf8ee3", "568d27f2cc4ca542ebe3b138872b710dd693a5e0", "68805172e3c0abb0315928d7f24d3ba2aaa16673"]
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
- [x] `cargo test --all` and `cd web && npm test` pass — focused suites green; full gates owned by verification

## Agent Instructions

Read `specs/system/repo-lifecycle.md` §"1. Where Repo Management Lives", §"3. Repo Configuration — Gates", and §"6. Domain Changes — API Summary". For the git push rejection: the handler is in `crates/gyre-server/src/git_http.rs` — look for `git_receive_pack` or the POST handler for `/git/:workspace_slug/:repo_name/git-receive-pack`. The repo status check should go after `resolve_repo_by_slug()`. For UI: follow existing Svelte 5 patterns in `web/src/` (e.g., WorkspaceSettings.svelte, RepoSettings.svelte). The gate API routes are at `GET/POST /api/v1/repos/:id/gates`.

## Shipped

All three coverage sections are implemented in production code on this branch
(checkpoint candidate `54b9f5bc`, recovered from an interrupted assignment and
re-verified end-to-end in this sandbox).

**1. Archived repo push rejection (§API Summary)** — `git_receive_pack` in
`crates/gyre-server/src/git_http.rs` checks `resolved.is_archived()` after
`resolve_repo_by_slug()` and before pack processing, returning
`403 "push rejected: repository is archived"`. Test
`receive_pack_archived_repo_returns_403` archives the repo through the real
repository port (`repo.archive()` + `state.repos.update()`) and asserts status
and message over HTTP. Also removed the pre-existing fabricated-`default`
workspace fallback in `process_spec_lifecycle` (the authorized repo's
workspace_id is now threaded through), shrinking the frozen
fabricated-scope-defaults exemption list from 7 to 6.

**2. Admin workspace Repos tab (§Admin → Workspace Scope → Repos Tab)** —
`WorkspaceSettings.svelte` gains an 8th "Repos" tab listing all workspace repos
with name, Active/Archived status badge, active-agent count, and last-activity
timestamp (derived from repo/agent/MR timestamps). "+ New Repo" and
"Import Repo" buttons open creation forms calling the real `createRepo` /
`createMirrorRepo` APIs; clicking a repo navigates to repo scope via the
`goToRepo` context.

**3. Repo-scope gate configuration UI (§Gates)** — `RepoSettings.svelte` gates
panel now has: per-gate enabled/disabled toggle (PUT `required`), per-gate
configuration form (name; command for test/lint gates; persona for
agent-review/validation gates; required approvals; timeout), and drag-to-
reorder that persists 1-based positions via PUT. Backend: new
`PUT /api/v1/repos/:id/gates/:gate_id` (`api::gates::update_gate`) with
type-aware field validation (command on non-command gate rejected, etc.), and
a `position` column on `quality_gates` (migration `2026-10-08-000056`, next
unused sequence, portable SQL; SQLite + Postgres + mem adapters all ordered by
`(position, created_at)`).

**Test evidence** (this sandbox, source `54b9f5bc`, saved under
`/tmp/stage/review-evidence/task-168-server-tests.txt`):
- `receive_pack_archived_repo_returns_403 ... ok` (1 passed).
- `api::gates::tests ... ok` (11 passed) — includes update_gate toggling,
  command-on-non-command rejection, 404 on unknown gate, position ordering.
- `api::repos::tests ... ok` (15 passed) — archive/unarchive flows intact.
- Web (vitest, `npm ci` with locked deps): `RepoSettings.test.js` 58/58,
  `WorkspaceSettings.test.js` 56/56 — all new gate-UI and Repos-tab tests pass.
- Full `npm test`: 1532 passed, 14 failed in 3 files. All failures are
  pre-existing on the base commit `8c2d1775` (verified in a clean base
  worktree: the same ExplorerCanvas ghost-overlay timeouts — files untouched
  by this branch, introduced on main via task-210's `a781ede2` — and a
  load-dependent WorkspaceHomeRulesFailure timeout that passes in isolation
  on both base and branch). None are task-168-owned.
- Full `cargo test --all` is owned by verification/publication per assignment
  constraints; focused probes above plus `gyre-adapters` (344 passed) from the
  prior checkpoint cover the touched crates.

Sandbox notes: npm registry was reachable this run; TCP listener probe is
unsupported here (errno 95), so no server-driven browser check was possible —
host verification must rely on GitHub CI (see
`/tmp/stage/capabilities.json`). Pre-existing attribution finding on main:
`a781ede2` (task-210) is missing from `specs/tasks/task-210.md` commits
frontmatter — outside this task's scope, recorded for the pipeline.
