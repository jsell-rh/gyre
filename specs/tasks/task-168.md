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

All three coverage sections are closed with production code on head (verified this session on the merged base `a11ba8d3` via merge `317718f8`; no production changes were needed in this session — the repair audit confirmed the tree satisfies the contract end-to-end).

1. **Archive push rejection (§API Summary, §4 step 5)** — `git_receive_pack` in `crates/gyre-server/src/git_http.rs` checks `resolved.is_archived()` (real `Repository` domain method) immediately after `resolve_repo_by_slug()` and returns `403` with "push rejected: repository is archived" before the packfile is processed; a `warn!` records agent/workspace/repo context. Unit test `receive_pack_archived_repo_returns_403` archives the repo through the store and asserts the 403 + message; integration test `push_to_archived_repo_rejected` (git_integration.rs Test 13) drives a real `git push` over HTTP.
2. **Admin Repos tab (§Admin → Workspace Scope → Repos Tab)** — `WorkspaceSettings.svelte` gains a `repos` tab listing every workspace repo (name, Active/Archived status badge, active-agent count, last-activity timestamp), with "+ New Repo" and "Import Repo" buttons opening real forms backed by `api.createRepo` / `api.createMirrorRepo`; clicking a repo navigates to repo scope via the `goToRepo` context wired in App.svelte.
3. **Gate configuration UI (§Gates)** — `RepoSettings.svelte` gates panel now has: enabled/disabled toggle (PUT `{required}`), per-gate configuration form with type-specific fields (command for test/lint gates, required-approvals, reviewer persona, timeout), and drag-to-reorder persisting 1-based `position` values via PUT with a server reload on failure. Backing API: real `update_gate` PUT handler (`api/gates.rs`) with per-type field validation and repo-ownership enforcement; `QualityGate.position` + migration `2026-10-08-000056_gate_position`; mem adapter ordering parity (position, created_at). Add Gate with type selector and delete existed at base.

**Test evidence (this session, on the final tree):** `git_http` unit suite 37/38 — the single failure `git_clone_empty_repo_via_smart_http` is the documented sandbox TCP-accept restriction (errno 95 `getpeername()`, capabilities.json; identical failure reproduced at pristine base in the prior session) and is unrelated to the archive guard, which passes (`receive_pack_archived_repo_returns_403` ok). `api::gates` 11/11, `merge_processor` 49/49, `gate_executor` 24/24, `mr_timeline` 13/13, `recovery` 3/3, `gyre-domain` 371/371. Web: RepoSettings + WorkspaceSettings vitest 114/114 (after `npm ci`). Mechanical checks all OK: attribution (after recording `df5b3246` here and base `a11ba8d3` in task-068's list — the merge makes that task-068-labeled commit reachable, so its attribution is required), abac-route-registry, fabricated-scope-defaults, migration-versions, migration-sql-portability, mem-port-contracts, arch, inert-enforcement, lossy-secret-conversion, scope-literal-defaults, in-memory-state-stores, dead-message-kinds, byte-slice-truncation, relative-path-defaults, mcp-write-tools. `web/dist` rebuilt with all new surfaces present. Full evidence: `/tmp/stage/review-evidence/task-168/evidence.md`.

**Left to host CI (listener-dependent / full-suite, owned by verification):** `git_integration` including `push_to_archived_repo_rejected`, full `cargo test --all`, Clippy, full web suite, GitHub checks on the pushed head.
