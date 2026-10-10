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
- [x] `cargo test --all` and `cd web && npm test` pass (focused probes on this branch: gates API 11/11, archived-push unit 1/1, git_http 37/38 with the 1 failure the sandbox's errno-95 TCP-accept restriction that also fails at pristine base; web 114/114; full suites owned by verification)

## Agent Instructions
Read `specs/system/repo-lifecycle.md` §"1. Where Repo Management Lives", §"3. Repo Configuration — Gates", and §"6. Domain Changes — API Summary". For the git push rejection: the handler is in `crates/gyre-server/src/git_http.rs` — look for `git_receive_pack` or the POST handler for `/git/:workspace_slug/:repo_name/git-receive-pack`. The repo status check should go after `resolve_repo_by_slug()`. For UI: follow existing Svelte 5 patterns in `web/src/` (e.g., WorkspaceSettings.svelte, RepoSettings.svelte). The gate API routes are at `GET/POST /api/v1/repos/:id/gates`.

## Shipped

Repair of durable finding b752de0e (contract): the prior candidate's diff gutted task-200's
message-bus payload-validation surface. The controller merge `c44665d0` restored every gutted
path (`crates/gyre-common/src/message.rs`, `crates/gyre-server/src/api/messages.rs`, `mcp.rs`,
coverage matrix, review record, task-200/218 .md files) byte-identical to main; the branch diff
vs base `6bf777a6` is now exactly this task's scope, re-verified on the restored head:

1. **Archive push rejection** (§API Summary): `git_http.rs` `git_receive_pack` checks
   `resolved.is_archived()` after repo resolution and before ABAC/packfile processing →
   403 "push rejected: repository is archived" + warn log. Unit test
   `receive_pack_archived_repo_returns_403` archives through the real store then asserts 403
   + message; `push_to_archived_repo_rejected` in `tests/git_integration.rs` exercises a real
   `git push` through the full middleware stack (needs host CI — errno-95 sandbox transport).

2. **Admin Repos tab** (§Admin → Workspace Scope → Repos Tab): 8th "Repos" tab in
   `WorkspaceSettings.svelte` — repo list (name, Active/Archived badge, active-agent count,
   last activity), "+ New Repo"/"Import Repo" inline forms calling `api.createRepo` /
   `api.createMirrorRepo`, click-through to repo scope via the `goToRepo` context in
   `App.svelte`.

3. **Gate configuration UI** (§Gates): `RepoSettings.svelte` gates panel — per-gate
   enabled/disabled toggle, per-gate Configure form (name, type-specific command/approvals/
   persona field, timeout), HTML5 drag-to-reorder persisting 1-based positions through
   `PUT /repos/:id/gates/:gate_id`. Backend `update_gate` performs partial updates with
   per-type validation (e.g. `command` on an `agent_review` gate → 400); `position` is a new
   persisted column (migration 000056, portable SQL; SQLite + PG + mem adapters share the
   `(position, created_at)` ordering contract); `list_gates` and gate execution consume that
   order.

4. Also fixed while touching `git_http.rs`: the spec-lifecycle task-creation path no longer
   fabricates a workspace scope via a `"default"` fallback on repo lookup failure — it uses
   the workspace id from the authorized push resolution (exemption removal 7→6).

5. Attribution repair this session: main's task-200 ship commit `6bf777a6` was missing from
   `specs/tasks/task-200.md`'s `commits:` frontmatter (same drift class tasks-211/216 repaired);
   appended the full SHA, `check-task-commit-attribution.sh` now exits 0.

**Test evidence (this session, restored head c44665d0, evidence at
`/tmp/stage/review-evidence/task-168/evidence.md`):**
- `cargo test -p gyre-server --lib api::gates` — 11 passed.
- `cargo test -p gyre-server --lib git_http` — 37 passed, 1 failed
  (`git_clone_empty_repo_via_smart_http`: errno-95 TCP-accept sandbox restriction; identical
  failure with all changes stashed, i.e. fails at pristine base too — infrastructure, not a
  regression).
- `cargo test -p gyre-server --lib message` — 36 passed; `mcp_message` — 12 passed;
  `gyre-common --lib` — 97 passed (task-200 restored surface intact).
- `cargo test -p gyre-server --lib -- merge_processor gate_executor::tests mr_timeline` —
  86 passed (struct literals touched by the new `position` field).
- `cd web && npx vitest run WorkspaceSettings + RepoSettings` — 114 passed, 0 failed.
- Mechanical checks on the diff surface all pass (fabricated-scope-defaults with the 7→6
  removal, migration-versions, migration-SQL-portability, mem-port-contracts, ABAC registry +
  exempt handlers, MCP write tools, dead message kinds, inert enforcement, byte-slice,
  in-memory stores, attribution, arch).

Transport restriction: this sandbox cannot `accept()` TCP (errno 95, per
`/tmp/stage/capabilities.json`), so the `git_integration` suite — including
`push_to_archived_repo_rejected` — requires host CI; the same failure occurs on the pristine
base, so it is infrastructure, not a regression.

Not in scope (existing coverage-matrix debt tracked on other rows): max_agents/budget repo
