---
title: "Repo lifecycle — archive push rejection, admin repos tab, gate config UI"
spec_ref: "repo-lifecycle.md §API Summary"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "repo-lifecycle.md §Admin → Workspace Scope → Repos Tab"
  - "repo-lifecycle.md §Gates (Admin → Repo Scope → Gates)"
  - "repo-lifecycle.md §API Summary"
commits: ["171afb8d00354203f8c5b498c91a9099cc3c6358", "38a0dae19505ad7fb4c079d2f8403e7a322f8deb", "a695457ced16e8a2f68568d1de6372e169082695", "37437a9971ae8219e9b8b9dc19ebda33d3b7243c", "b14e7698ca1e57d72eba1c3ab7e418aac42700b2", "894d587bde149adf6680cc3b9658972beabe867f", "47cbda81ee8a868dcc9ed3abd05401b46a97cd43", "5b38269c71429a6dc38321e081abcd257e81cc46", "ea5c61aa5fe05e5ce0748ecf0d7077ff27c1ec16"]
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
