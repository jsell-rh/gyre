---
title: "Implement workspace membership model, ownership transfer, and repo access"
spec_ref: "user-management.md §Workspace Membership"
depends_on:
  - task-120
progress: complete
coverage_sections:
  - "user-management.md §Workspace Membership"
  - "user-management.md §Ownership Transfer & Reclamation"
  - "user-management.md §Repo Access"
commits: ["7939a4a6663022b071ec9175001487d6bf44c54b", "64fa14fd4fbb1feecbd69c04d3cb8020fc4a237f", "8fc4245da30d5c96d1c0cb2568ef70e573ed84d3", "93c4017c45159c73c59160eb83f45de08ba2c342", "a8c3af6f3b12a1524964baaef0e48897ad317755", "337447e39cc439505dcebf4f522f65b0c77ebe18", "467e577f5cd6f7507652a2d84e0bd534a17bd4c5"]
---

## Spec Excerpt

From `user-management.md` §Workspace Membership:

```rust
pub struct WorkspaceMembership {
    pub id: Id,
    pub user_id: Id,
    pub workspace_id: Id,
    pub role: WorkspaceRole,
    pub invited_by: Id,
    pub accepted_at: Option<u64>,
    pub created_at: u64,
}

pub enum WorkspaceRole {
    Owner,      // Full workspace control, budget management, can delete workspace
    Admin,      // Manage members, configure repos, manage personas
    Developer,  // Create/modify tasks, MRs, spawn agents, approve specs
    Viewer,     // Read-only access to all workspace data
}
```

From §Ownership Transfer & Reclamation:
- When a workspace Owner leaves, Tenant Admin receives notification
- Tenant Admin can reassign ownership via `PUT /api/v1/workspaces/{id}/members/{new_owner_id}/role`
- Orphaned workspace flagged in admin dashboard after 30 days
- Owner can voluntarily transfer; must designate new Owner before downgrading
- Cannot remove last Owner

From §Repo Access:

| Workspace Role | Repo Access |
|---|---|
| Owner | Full (read, write, admin, manage gates, manage budget) |
| Admin | Full (read, write, admin, manage gates) |
| Developer | Read, write, create MRs, create tasks, spawn agents |
| Viewer | Read-only |

Per-repo overrides can restrict (but not expand) access beyond workspace role.

## Implementation Plan

1. **Audit existing membership model:**
   - Check `WorkspaceMembership` struct and `WorkspaceRole` enum in domain
   - Check existing `POST/GET /api/v1/workspaces/:id/members` and `PUT/DELETE .../members/:user_id` endpoints
   - Identify gaps between existing implementation and spec

2. **Ownership transfer logic:**
   - Add validation: cannot remove/downgrade last Owner
   - Add `transfer_ownership` action in role change endpoint
   - When Owner deactivated: create notification for Tenant Admin
   - Orphaned workspace detection: background job checks workspaces with no active Owner

3. **Repo access inheritance model:**
   - Define permission set per WorkspaceRole (per spec table)
   - Add `RepoAccessOverride` entity for per-repo restrictions
   - Overrides can only restrict, never expand beyond workspace role
   - Migration for `repo_access_overrides` table if needed

4. **ABAC integration:**
   - Ensure workspace role is available as `subject.workspace_role` attribute
   - Repo access checks use membership + override logic

## Acceptance Criteria

- [ ] WorkspaceMembership entity matches spec (id, user_id, workspace_id, role, invited_by, accepted_at, created_at)
- [ ] WorkspaceRole enum: Owner, Admin, Developer, Viewer
- [ ] Cannot remove or downgrade last Owner of a workspace
- [ ] Ownership transfer: PUT changes role, audit-logged
- [ ] Tenant Admin notified when workspace has no active Owner
- [ ] Orphaned workspace detection (30-day configurable period)
- [ ] Repo access permissions derived from workspace role per spec table
- [ ] Per-repo overrides can restrict (but not expand) access
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/user-management.md` §Workspace Membership through §Repo Access. Existing membership endpoints: grep for `invite_member\|list_members` in `gyre-server/src/api/`. Workspace domain: `gyre-domain/src/workspace.rs`. Member role change: grep for `members/:user_id` in `gyre-server/src/api/mod.rs`. Notification creation: grep for `NotificationType` in `gyre-common/`. Background jobs: grep for `tokio::spawn` in `gyre-server/src/main.rs` or `lib.rs`.

## Shipped

Workspace membership, ownership transfer/reclamation, and repo access are implemented
end-to-end (user-management.md §Workspace Membership, §Ownership Transfer & Reclamation,
§Repo Access), across the feature round, three review/verification repair rounds, a
rebase repair round, and this migration-collision repair round.

### Behavior (feature)

- `WorkspaceMembership` (gyre-domain) carries id, user_id, workspace_id, role, invited_by,
  accepted_at, created_at (plus an `accepted` invite flag); `WorkspaceRole` is
  Owner/Admin/Developer/Viewer with a privilege rank used for restrict-only override
  composition.
- Membership management (`POST/GET/PUT/DELETE /api/v1/workspaces/:id/members...`) is restricted
  to Owner, workspace Admin, or Tenant Admin; inviting an existing member is a 409. The last
  Owner cannot be removed or downgraded; ownership transfer via the role-change PUT is
  audit-logged and clears ownerless tracking.
- Ownerless tracking: when the last active Owner leaves, every TenantAdmin of the tenant is
  notified (`WorkspaceOwnerless`, priority rank 2) inline and via the daily
  `orphaned_workspace_check` job (deduplicated); after `GYRE_ORPHANED_WORKSPACE_DAYS`
  (default 30) the workspace is flagged in `GET /api/v1/admin/orphaned-workspaces`.
- Repo access: `RepoPermission` sets per role follow the spec table; `RepoAccessOverride`
  (port + SQLite/Postgres adapters + migration) stores per-repo restrictions; the effective
  role is the more restrictive of workspace role and override (restrict-only). git_http
  read/write paths enforce the effective permission set via `authorize_repo_access`.

### Behavior (prior repair rounds, retained)

- `subject.workspace_role` is resolved for repo-addressed routes before policy evaluation,
  so the builtin `repo-access-override-management` policy is live on the override routes;
  `load_repo_scoped` is fail-closed on an unresolvable workspace scope (dangling
  repo→workspace reference is NotFound, never a pass-through of the tenant gate).
- `create_workspace` seeds the creator's active Owner membership; ownerless tracking fires
  only on owner departure (an Owner membership row survives user deletion), so
  never-owned workspaces are not flagged orphaned.
- Duplicate repo-access override PUTs return 409 on all adapters via a
  guard-before-insert (`find_by_repo_and_user`) plus `is_duplicate_override_error`, which
  walks the whole error chain to recognize mem/SQLite/PostgreSQL duplicate shapes.

### Behavior (this round: 000060 migration collision repair)

- Verification (checks.sh, exit 1) caught a duplicate Diesel migration version: task-121's
  `repo_access_overrides` and base task-118's `repo_agent_limits` both claimed
  `20261112000060`. Diesel runs only one migration per version and silently drops the
  other, so fresh installs were missing one side of the collision (empirically the
  `repo_access_overrides` table — the task-121 surface).
- Fix: renumbered the newer task-121 migration to the next unused sequence
  `2026-11-12-000061_repo_access_overrides` (SQL unchanged; `embed_migrations!` embeds by
  directory, so the rename is transparent to both backends).
- Regression guard: `migrations_create_000060_repo_columns_and_overrides`
  (gyre-adapters sqlite tests) asserts both sides of the collision exist after startup —
  `repositories.max_agents` / `max_agent_iterations` (task-118) and the
  `repo_access_overrides` table (task-121) — and `repo_access_overrides` was added to the
  `migrations_create_tables` list. Mutation probe: restoring the duplicate 000060 version
  makes the new test fail (`table 'repo_access_overrides' missing`), so the test kills the
  silent-drop regression on either side of the collision.

### Test evidence

- This round: `cargo test -p gyre-adapters --lib sqlite::tests` 9/9 pass (incl. the new
  guard and the mutation probe above); `cargo fmt -p gyre-adapters -- --check` exit 0;
  `cargo clippy -p gyre-adapters --lib --tests` — no findings on changed lines (all 13
  crate warnings are pre-existing in other files); `scripts/check-migration-versions.sh`
  and `scripts/check-migration-sql-portability.sh` both pass (evidence:
  /tmp/stage/review-evidence/task-121-migration-renumber-7939a4a66.md).
- Prior rounds (focused runs via `/tmp/stage/bin/cargo`): `api::repo_access` through the
  production router (api_router + abac_middleware + seeded builtin policies), including
  the regression test that fails (403) when repo-addressed workspace-role resolution is
  disabled; `workspace_ownership` 8/8; `api::workspaces::tests` 20/20; `api::users::tests`
  23/23; `git_http` 61/61 (incl. repo-access enforcement on the git transport);
  `abac_middleware` 14/14; full gyre-server lib suite 1727/1727;
  `cargo test -p gyre-domain repo_access` 8/8; clippy 0 findings on changed lines vs base.
- Deferred to the independent verifier (server test-profile compilation exceeds this
  sandbox's 4 GiB cgroup budget): `cargo test -p gyre-server --lib` — notably
  `api::repo_access` (override CRUD, restrict-only composition, duplicate-409 through the
  production ABAC pipeline), `workspace_ownership`, `api::workspaces::tests`,
  `api::users::tests`, `git_http`, and `abac_middleware`.
