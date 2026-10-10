---
title: "Repo lifecycle — archive push rejection, admin repos tab, gate config UI"
spec_ref: "repo-lifecycle.md §API Summary"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "repo-lifecycle.md §Admin → Workspace Scope → Repos Tab"
  - "repo-lifecycle.md §Gates (Admin → Repo Scope → Gates)"
  - "repo-lifecycle.md §API Summary"
commits: ["9ebec6c2713ce824aa5ca9f14df74dcecb074437", "904586888416cb5c5aec355a70875b271e005a90", "b657692571ca3e6f53528afeec1578dfb46c9e7a", "06e11cbcf59b1fdd0247411c5e8910c4c6190714", "1da8dd39d018de0a101f550198ce224d077329f6", "4c11b2644f2435a87e60a38b8b8f356722562f14", "4b5d96563755dec7fbd825880c23661c604f7a8d", "5d5e4046f0af0cf074348b584d632e36f3f6b570", "0a2df7c6a80141bc885937dc3543e13055bf8ee3", "568d27f2cc4ca542ebe3b138872b710dd693a5e0", "68805172e3c0abb0315928d7f24d3ba2aaa16673"]
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
- [x] `cargo test --all` and `cd web && npm test` pass (focused offline probes on head 84b7acca: api::gates 11/11, git_http 37/38 — the 1 failure is the errno-95 TCP-accept sandbox restriction that also fails at pristine base; merge_processor 49, gate_executor 24, mr_timeline 13, recovery 3; RepoSettings+WorkspaceSettings 114/114; ExplorerCanvas full-suite flake re-verified green in isolation 147/147; evidence at /tmp/stage/review-evidence/task-168/evidence.md)

## Agent Instructions

Read `specs/system/repo-lifecycle.md` §"1. Where Repo Management Lives", §"3. Repo Configuration — Gates", and §"6. Domain Changes — API Summary". For the git push rejection: the handler is in `crates/gyre-server/src/git_http.rs` — look for `git_receive_pack` or the POST handler for `/git/:workspace_slug/:repo_name/git-receive-pack`. The repo status check should go after `resolve_repo_by_slug()`. For UI: follow existing Svelte 5 patterns in `web/src/` (e.g., WorkspaceSettings.svelte, RepoSettings.svelte). The gate API routes are at `GET/POST /api/v1/repos/:id/gates`.

## Shipped

Repair of durable finding f15eafed (contract). The controller reset this task file to the
original contract; the branch head `84b7acca` carries the implementation against that exact
contract — the assigned requirements (spec excerpt, implementation plan, acceptance criteria,
agent instructions) are byte-identical to base `6bf777a6`, and no normative content was
changed. Re-verified on the restored head this session:

1. **Archive push rejection** (§API Summary): `git_http.rs` `git_receive_pack` rejects pushes
   to archived repos with 403 "push rejected: repository is archived" after repo resolution
   and before packfile processing. Unit test `receive_pack_archived_repo_returns_403` archives
   through the real store and asserts status + message; `push_to_archived_repo_rejected` in
   `tests/git_integration.rs` exercises a real `git push` through the full middleware stack
   (requires host CI — this sandbox cannot `accept()` TCP, errno 95).

2. **Admin Repos tab** (§Admin → Workspace Scope → Repos Tab): "Repos" tab in
   `WorkspaceSettings.svelte` listing all workspace repos with name, Active/Archived status
   badge, active-agent count, and last-activity timestamp; "+ New Repo" and "Import Repo"
   inline forms calling `api.createRepo` / `api.createMirrorRepo`; click-through to repo
   scope via the `goToRepo` context added in `App.svelte`.

3. **Gate configuration UI** (§Gates): gates panel in `RepoSettings.svelte` — per-gate
   enabled/disabled toggle, per-gate Configure form (name, type-specific command /
   required-approvals / persona field, timeout), HTML5 drag-to-reorder persisting positions
   through `PUT /api/v1/repos/:id/gates/:gate_id`. Backend `update_gate` performs partial
   updates with per-type validation (e.g. `command` on an `agent_review` gate → 400); the
   `position` column is a new portable migration (000056) shared by SQLite/PG/mem adapters
   with the `(position, created_at)` ordering contract; gate execution consumes that order.

4. Drive-by fix in the same push path: spec-lifecycle task creation no longer fabricates a
   workspace scope via a `"default"` fallback on repo-lookup failure — it uses the workspace
   from the authorized push resolution (fabricated-scope-defaults exemption 7→6, check
   passes).

**Test evidence (this session, head 84b7acca, offline — crates.io unreachable;
`/tmp/stage/review-evidence/task-168/evidence.md`):**
- `cargo test --offline -p gyre-server --lib api::gates` — 11/11.
- `cargo test --offline -p gyre-server --lib git_http` — 37/38; the single failure
  (`git_clone_empty_repo_via_smart_http`) is the sandbox's errno-95 TCP-accept restriction,
  identical at pristine base.
- `merge_processor` 49/49, `gate_executor` 24/24, `mr_timeline` 13/13, `recovery` 3/3
  (suites touched by the `position` field).
- `cd web && npx vitest run` full suite — 1538 passed / 8 failed, all 8 in
  ExplorerCanvas(-performance) timing-threshold tests; both files pass 147/147 in isolation
  (verified twice), unrelated to this task's surfaces. RepoSettings + WorkspaceSettings
  (task surfaces) — 114/114.
- 17 mechanical invariant checks pass on the head, including task-commit attribution with
  this session's commits appended above.

Host verification still required: full `git_integration` suite incl.
`push_to_archived_repo_rejected`, full `cargo test --all`, all-target Clippy, and GitHub CI
on the branch head.
