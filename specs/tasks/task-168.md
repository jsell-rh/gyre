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

Recovered from checkpoint 89bff3c7 (prior agent exit 130 mid-finalization;
the implementation was already complete on this branch -- this session
re-verified it on the current merged head 2261f959 and finalized the task
record; the merge added only spec files, no code drift).

1. **Archive push rejection** (§API Summary): `git_receive_pack` rejects
   pushes to archived repos with 403 "push rejected: repository is archived",
   checked after `resolve_repo_by_slug()` and before packfile processing
   (mirrors the read-only-mirror rejection shape). Verified on this head:
   unit test `receive_pack_archived_repo_returns_403` passes (archives
   through the real store, hits the real endpoint); full-stack
   `push_to_archived_repo_rejected` (real `git push`, real TCP listener)
   requires a listener and is owned by host CI -- this sandbox cannot
   `accept()` (errno 95, capabilities.json).

2. **Admin Repos tab** (§Admin → Workspace Scope → Repos Tab): "Repos" tab in
   `WorkspaceSettings.svelte` listing workspace repos with name, Active/
   Archived status badge, active-agent count, and last-activity timestamp;
   "+ New Repo" and "Import Repo" inline forms calling `api.createRepo` /
   `api.createMirrorRepo`; click-through to repo scope via the `goToRepo`
   context. Verified on this head: WorkspaceSettings vitest 56/56.

3. **Gate configuration UI** (§Gates): gates panel in `RepoSettings.svelte` --
   per-gate required/optional toggle, per-gate Configure form (name,
   type-specific command / required-approvals / persona field, timeout),
   HTML5 drag-to-reorder persisting positions through
   `PUT /api/v1/repos/:id/gates/:gate_id` (`api.updateRepoGate`). Backend
   `update_gate` performs partial updates with per-type validation (command
   only on Test/Lint gates, persona only on Agent gates, required_approvals
   only on RequiredApprovals gates); `list_gates` orders by
   `(position, created_at)` with the same contract verified in the SQLite,
   Postgres, and mem adapters (migration `2026-10-08-000056_gate_position`,
   next unused sequence number, portable SQL both dialects). Verified on
   this head: api::gates 11/11, RepoSettings vitest 58/58.

Focused probes on this head (sandbox): git_http 37/38 -- the single failure
`git_clone_empty_repo_via_smart_http` is the documented sandbox TCP
restriction (`getpeername() errno 95`, test unmodified from base); adapter
ordering contract read-verified identical across sqlite/postgres/mem;
mechanical gates green on this head: abac-route-registry,
migration-versions, migration-sql-portability, fabricated-scope-defaults,
task-commit-attribution. Full workspace suites, architecture checks,
all-target Clippy, and GitHub CI are owned by verification and publication.
Evidence: /tmp/stage/review-evidence/task-168/evidence.md
