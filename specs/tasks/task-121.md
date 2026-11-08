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
commits: ["COMMIT1"]
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

All three coverage sections are implemented in production code (details in
`specs/coverage/system/user-management.md` rows 4/8/9).

### Workspace Membership

- `WorkspaceMembership`/`WorkspaceRole` already matched the spec struct; the
  membership model is now the enforced access path rather than a passive
  record. `create_workspace` seeds the creating user as an accepted Owner;
  `is_active()` (accepted) gates every authorization decision, so a pending
  invitation grants nothing (spec §Workspace Invitation Flow).
- Member management (`invite`/`role change`/`remove`) requires an active
  Owner/Admin membership in-handler via `require_workspace_role` (middleware
  ABAC policies are global-role based and cannot express workspace
  membership). Bootstrap allowance: a workspace with zero active members can
  be joined by any authenticated caller. Duplicate invites 409.
- Spec-canonical endpoint `PUT /api/v1/workspaces/:id/members/:user_id/role`
  added; the M22.8 legacy `PUT .../members/:user_id` alias is kept.
- DB-backed deployments now persist memberships: `build_state` previously
  hardwired `workspace_memberships` to the mem adapter regardless of storage
  backend; it now uses the `store!` wiring.

### Ownership Transfer & Reclamation

- Last-Owner protection: removing or downgrading the final active Owner
  returns 409 and the membership survives (`cannot_remove_last_owner`,
  `cannot_downgrade_last_owner`). A voluntary transfer must designate the
  new Owner first (same guard; `ownership_transfer_...` test shows the
  two-step flow succeeds).
- Every role change/removal is audit-logged
  (`workspace_ownership_transferred`, `workspace_member_role_changed`,
  `workspace_member_removed`) with previous role, new role, previous owner,
  and the caller-supplied `reason` field.
- Tenant Admins receive a `WorkspaceOwnerless` notification (new
  NotificationType, priority 2) when a workspace has no active Owner:
  immediately on detection (member removal leaving no Owner) and from the
  `orphaned_workspace_detector` job (registered in the job registry, hourly,
  spawned in main.rs).
- The job keeps durable KV-backed governance state (`ownerless_since`,
  `last_notified_at`, `orphaned_since`), notifies once per transition (no
  duplicates within the period), flags the workspace orphaned after
  `GYRE_ORPHANED_WORKSPACE_DAYS` (default 30), and clears tracking when an
  Owner re-appears. `GET /api/v1/admin/orphaned-workspaces` exposes the flag
  set (ABAC `admin` resource, admin-only by the built-in deny policy).

### Repo Access

- `WorkspaceRole::repo_permissions()` encodes the spec table exactly
  (Owner adds manage_budget; Admin lacks it; Developer lacks admin/gates;
  Viewer read-only) with a table-exactness test.
- Enforcement: `workspace_governance::check_repo_access_scope` /
  `check_workspace_membership_scope` on the repo management surface —
  create/list/get/update/archive/unarchive/delete repo and
  add-repo-to-workspace. User callers need an active membership whose role
  grants the requested permission; system token, global Admin, and Agent
  subjects bypass (agents are scoped at spawn time plus the per-repo ABAC
  policy machinery from abac-policy-engine.md, which remains in force).
- Per-repo overrides: `RepoAccessOverride` entity,
  `RepoAccessOverrideRepository` port with SQLite (migration 000059,
  UNIQUE(repo_id, user_id)) and PostgreSQL adapters plus a guarded mem
  adapter; `GET/PUT/DELETE /api/v1/repos/:id/access-overrides[/:user_id]`
  (Owner/Admin only, upsert semantics). Restrict-only is enforced twice:
  set-time rejection of overrides above the member's workspace role, and
  evaluation-time clamping (`effective_repo_role` returns the more
  restrictive role), so an override can never expand access even after a
  later role change.
- ABAC integration: the middleware resolves the workspace id from the route
  pattern (only for workspace-parented resources — repo `:id`s are not
  workspace ids) and sets `subject.workspace_role` from the caller's active
  membership. End-to-end test proves a workspace_role-conditioned policy
  allows an Owner member and denies the same user on a workspace where they
  hold no membership.

### Test evidence

- New focused tests, all failing against the unimplemented behavior:
  domain table/clamp tests (gyre-domain), governance job tests (4),
  repo-access override handler tests (4), membership governance handler
  tests (6, incl. last-owner 409s, audit-log contents, non-member 403),
  middleware attribute test (1), SQLite adapter contract tests (2).
- `cargo test --all`: 1433 gyre-server lib + 377 gyre-adapters + all other
  crates pass. Existing repo-endpoint tests now send the auth header the
  production auth middleware always required (handlers previously ignored
  caller identity entirely); no assertion was weakened.
- Mechanical checks: arch, migration-versions, migration-SQL-portability,
  mem-port-contracts, abac-route-registry, abac-exempt-handlers, api-auth,
  unwired-port-methods, in-memory-state-stores, forged-scope-fields all OK;
  clippy warning count and remaining check-script exit codes are identical
  to the assignment base.

### Notes for review

- Workspace visibility (`GET /api/v1/workspaces` tenant-scoped listing) is
  intentionally unchanged — it is owned by platform-model/HSI navigation,
  not by this task's sections.
- The git HTTP transport keeps its existing per-repo ABAC policy checks
  (abac-policy-engine.md); the membership model is enforced on the repo
  management API surface added here.
