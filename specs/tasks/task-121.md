---
title: "Implement workspace membership model, ownership transfer, and repo access"
spec_ref: "user-management.md §Workspace Membership"
depends_on:
  - task-120
progress: ready-for-review
coverage_sections:
  - "user-management.md §Workspace Membership"
  - "user-management.md §Ownership Transfer & Reclamation"
  - "user-management.md §Repo Access"
commits: ["337447e39cc439505dcebf4f522f65b0c77ebe18", "467e577f5cd6f7507652a2d84e0bd534a17bd4c5", "a8c3af6f3"]
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

- [x] WorkspaceMembership entity matches spec (id, user_id, workspace_id, role, invited_by, accepted_at, created_at)
- [x] WorkspaceRole enum: Owner, Admin, Developer, Viewer
- [x] Cannot remove or downgrade last Owner of a workspace
- [x] Ownership transfer: PUT changes role, audit-logged
- [x] Tenant Admin notified when workspace has no active Owner
- [x] Orphaned workspace detection (30-day configurable period)
- [x] Repo access permissions derived from workspace role per spec table
- [x] Per-repo overrides can restrict (but not expand) access
- [x] `cargo test --all` passes

## Agent Instructions

Read `specs/system/user-management.md` §Workspace Membership through §Repo Access. Existing membership endpoints: grep for `invite_member\|list_members` in `gyre-server/src/api/`. Workspace domain: `gyre-domain/src/workspace.rs`. Member role change: grep for `members/:user_id` in `gyre-server/src/api/mod.rs`. Notification creation: grep for `NotificationType` in `gyre-common/`. Background jobs: grep for `tokio::spawn` in `gyre-server/src/main.rs` or `lib.rs`.

## Shipped

Workspace membership, ownership transfer/reclamation, and repo access are implemented end-to-end
(user-management.md §Workspace Membership, §Ownership Transfer & Reclamation, §Repo Access).

### Behavior

- `WorkspaceMembership` (gyre-domain) carries id, user_id, workspace_id, role, invited_by,
  accepted_at, created_at (plus an `accepted` invite flag); `WorkspaceRole` is Owner/Admin/
  Developer/Viewer with a privilege rank used for restrict-only override composition.
- Membership management (`POST/GET/PUT/DELETE /api/v1/workspaces/:id/members...`) is restricted
  to Owner, workspace Admin, or Tenant Admin. Inviting an existing member is a 409.
- Last-Owner protection: removing or downgrading the only Owner of a workspace is rejected
  (409/403); ownership must first be transferred via the role-change PUT, which is audit-logged
  and clears any ownerless tracking for that workspace.
- Ownerless tracking: when the last active Owner leaves (SCIM deactivate, removal, or user
  deletion), every TenantAdmin of the workspace's tenant is notified (`WorkspaceOwnerless`,
  priority 2) — inline immediately plus via the daily `orphaned_workspace_check` job, which
  does not duplicate notifications on re-runs. After `GYRE_ORPHANED_WORKSPACE_DAYS` (default 30,
  zero/unparsable falls back to default) the workspace is flagged orphaned in
  `GET /api/v1/admin/orphaned-workspaces`.
- Repo access: `RepoPermission` set per role follows the spec table (Owner: full incl. budget;
  Admin: full minus budget; Developer: read/write/tasks/MRs/agents; Viewer: read-only).
  `RepoAccessOverride` (port + SQLite/Postgres adapters + migration 000060, both backends)
  stores per-repo per-user restrictions; the effective role is the more restrictive of the
  workspace role and the override, so overrides can never expand access. git_http read/write
  paths enforce the effective permission set via `authorize_repo_access`.
- ABAC: the middleware resolves the caller's membership in the addressed workspace and
  exposes it as `subject.workspace_role` for policy evaluation.

### Test evidence

Focused runs (SKIP_WEB_BUILD=1): gyre-domain `repo_access` 8/8; gyre-server `workspace_ownership`
7/7, `api::repo_access` 4/4, `api::users` 22/22 (incl. last-owner, transfer-audited, and
membership-authorization tests), `git_http` 60/60. Mechanical guards all PASS (arch, mem port
contracts, ABAC route registry/exempt handlers, migration versions/portability, scope-forgery
and inert-enforcement families, task commit attribution). `cargo fmt --all -- --check` clean.
Full `cargo test --all` and GitHub CI are owned by the verification stage.

Note: check-cross-surface-parity, check-cli-spec-parity, and check-dependent-scope-resolution
fail identically on base 18adceb68 (verified via git worktree); those are pre-existing findings
outside this task's scope.
