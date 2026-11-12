---
title: "Implement workspace membership model, ownership transfer, and repo access"
spec_ref: "user-management.md §Workspace Membership"
depends_on:
  - task-120
progress: not-started
coverage_sections:
  - "user-management.md §Workspace Membership"
  - "user-management.md §Ownership Transfer & Reclamation"
  - "user-management.md §Repo Access"
commits: []
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
