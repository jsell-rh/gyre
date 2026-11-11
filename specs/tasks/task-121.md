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
commits: ["467e577f5cd6f7507652a2d84e0bd534a17bd4c5", "337447e39cc439505dcebf4f522f65b0c77ebe18", "a8c3af6f3b12a1524964baaef0e48897ad317755", "93c4017c45159c73c59160eb83f45de08ba2c342", "8fc4245da30d5c96d1c0cb2568ef70e573ed84d3"]
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
end-to-end, plus the review-repair round and this verification-repair round clearing the
clippy-diff gate findings (user-management.md §Workspace Membership, §Ownership Transfer &
Reclamation, §Repo Access).

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

### Behavior (review repair)

- `subject.workspace_role` is resolved for repo-addressed routes (`/api/v1/repos/:id/...`)
  by loading the repo's workspace before policy evaluation, so the builtin
  `repo-access-override-management` policy is live on the override routes (previously
  default-denied 403 before the handler ran — dead policy).
- `load_repo_scoped` is fail-closed on an unresolvable workspace scope: a dangling
  repo→workspace reference or a failed lookup is NotFound, never a pass-through of the tenant
  gate.
- `create_workspace` seeds the creator's active Owner membership (same-tenant user
  principals), and ownerless tracking fires only on owner departure (evidence: an Owner
  membership row, which survives user deletion). Never-owned workspaces (system/agent-operated)
  are not flagged orphaned and generate no TenantAdmin notification noise.

### Behavior (verification repair)

- gyre-ports `RepoAccessOverrideRepository` carries the codebase-standard
  `#[allow(clippy::double_must_use)]` for async_trait's generated must_use futures: the new
  file made that pre-existing pattern lint fall on changed lines in the clippy-diff gate.
- git_http `authorize_repo_access`: doc list continuations indented
  (`clippy::doc_lazy_continuation`) and the error type boxed to `Box<Response>`
  (`clippy::result_large_err`); denial responses are dereferenced unchanged at all three
  call sites (fetch/push/rpc), so 403/500 behavior is identical.
- workspace_ownership: test-only `NotificationPriority` import moved into the `#[cfg(test)]`
  module (`unused_imports` in the lib target).

### Test evidence

- Feature/review rounds (focused runs via `/tmp/stage/bin/cargo`, summaries retained under
  /tmp/stage/review-evidence): `api::repo_access` 7/7 through the production router
  (api_router + abac_middleware + seeded builtin policies) with workspace-member API keys,
  including the regression test that fails (403) when repo-addressed workspace-role
  resolution is disabled; `workspace_ownership` 8/8; `api::workspaces::tests` 20/20;
  `api::users::tests` 23/23; `git_http` 61/61 (incl.
  `repo_access_enforced_on_git_transport_for_user_principals`: Developer fetch/push 200,
  Viewer fetch 200 / push 403, non-member 403, Developer-restricted-to-Viewer push 403);
  `abac_middleware` 14/14; full gyre-server lib suite 1727/1727.
- This verification-repair round: `cargo clippy -p gyre-server --all-targets
  --all-features -- -W clippy::all` reports 0 findings on changed lines vs base 65f71e65
  (previously double_must_use, unused_imports, doc_lazy_continuation ×4, result_large_err);
  same for `-p gyre-common -p gyre-domain -p gyre-ports -p gyre-adapters`;
  `scripts/check-rustfmt-diff.py` clean; `cargo test -p gyre-domain repo_access` 8/8.
- Deferred to the independent verifier (server test compilation exceeds this sandbox's
  budget): rerun the gyre-server suites above — the `Box<Response>` change is
  behavior-transparent (deref at call sites) and type-checked with `--all-targets`.
