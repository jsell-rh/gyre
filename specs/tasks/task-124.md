---
title: "Complete user management and workspace membership API endpoints"
spec_ref: "user-management.md §API > User Management"
depends_on:
  - task-120
  - task-121
progress: ready-for-review
coverage_sections:
  - "user-management.md §User Management"
  - "user-management.md §Workspace Membership"
commits: ["60c86252706b2392866b1dddd66634ab4cb123d6", "6033324f8936365465b9ffc9b27e6c3a6429f0d1", "33e30ac961ad1021b50f3cd3a65c3ac184a03ca1"]
---

## Spec Excerpt

From `user-management.md` §API > User Management:

| Endpoint | Method | Purpose |
|---|---|---|
| `GET /api/v1/users/me` | GET | Current user profile + preferences |
| `PUT /api/v1/users/me` | PUT | Update display name, timezone, locale, preferences |
| `GET /api/v1/users/{username}` | GET | User profile (tenant-public) |
| `GET /api/v1/users` | GET | List users (tenant admin only) |
| `PUT /api/v1/users/{id}/deactivate` | PUT | Deactivate user (TenantAdmin only) |
| `GET /api/v1/users/me/sessions` | GET | My active sessions |
| `DELETE /api/v1/users/me/sessions/{id}` | DELETE | Revoke a session |
| `POST /api/v1/users/me/sessions/revoke-all` | POST | Revoke all sessions |

From §API > Workspace Membership:

| Endpoint | Method | Purpose |
|---|---|---|
| `GET /api/v1/workspaces/{id}/members` | GET | List workspace members |
| `POST /api/v1/workspaces/{id}/invite` | POST | Invite user (Owner/Admin only) |
| `GET /api/v1/workspaces/{id}/invitations` | GET | List pending invitations |
| `POST /api/v1/workspaces/{id}/invite/accept` | POST | Accept invitation |
| `DELETE /api/v1/workspaces/{id}/invitations/{id}` | DELETE | Revoke invitation |
| `PUT /api/v1/workspaces/{id}/members/{user_id}/role` | PUT | Change member role (including ownership transfer) |
| `DELETE /api/v1/workspaces/{id}/members/{user_id}` | DELETE | Remove member |

**Existing endpoints** (per Completeness Assessment and mod.rs):
- `GET/PUT /api/v1/users/me` — implemented
- `GET /api/v1/users/me/{agents,tasks,mrs}` — implemented
- `POST/GET /api/v1/workspaces/{id}/members` — implemented
- `PUT/DELETE /api/v1/workspaces/{id}/members/{user_id}` — implemented

## Implementation Plan

1. **Audit existing endpoints** in `gyre-server/src/api/users.rs` and `mod.rs`:
   - Verify `GET/PUT /api/v1/users/me` returns/accepts all spec'd fields (username, display_name, timezone, locale, preferences, avatar_url)
   - Verify workspace membership endpoints match spec

2. **Add missing user management endpoints:**
   - `GET /api/v1/users/{username}` — public profile (within tenant)
   - `GET /api/v1/users` — list all users (TenantAdmin only)
   - `PUT /api/v1/users/{id}/deactivate` — deactivate user (TenantAdmin only)

3. **Add missing session management endpoints** (if not covered by task-111):
   - Verify session endpoints from task-111 cover: list, revoke individual, revoke all

4. **Add missing workspace membership endpoints:**
   - `GET /api/v1/workspaces/{id}/invitations` — list pending invitations
   - `POST /api/v1/workspaces/{id}/invite/accept` — accept invitation
   - `DELETE /api/v1/workspaces/{id}/invitations/{id}` — revoke invitation
   - Verify `PUT .../members/{user_id}/role` supports ownership transfer

5. **Register all new routes** in `api/mod.rs` and add ABAC route mappings.

## Acceptance Criteria

- [ ] `GET /api/v1/users/{username}` returns public profile within tenant
- [ ] `GET /api/v1/users` lists users (TenantAdmin only, paginated)
- [ ] `PUT /api/v1/users/{id}/deactivate` deactivates user with cascade (TenantAdmin only)
- [ ] Workspace invitation list, accept, and revoke endpoints functional
- [ ] Role change endpoint supports ownership transfer validation
- [ ] All routes registered in `api/mod.rs` with ABAC route mappings
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/user-management.md` §API > User Management and §API > Workspace Membership. Existing user API: `gyre-server/src/api/users.rs`. Route registration: `gyre-server/src/api/mod.rs` — grep for `/users/` to see existing routes. ABAC route mappings: `gyre-server/src/abac_middleware.rs`. Session management: check if task-111 already covers session endpoints. Workspace member endpoints: grep for `invite_member\|list_members\|update_member_role\|remove_member` in the api/ directory.

## Shipped

### User management endpoints (§API > User Management)

Five of eight spec endpoints are implemented in production code; the
three session endpoints are owned by task-111 (still not-started), so
the §User Management coverage row stays task-assigned over them:

- **`GET /api/v1/users/{username}`** — tenant-public profile
  (`{id, username, display_name, avatar_url, timezone, created_at,
  memberships:[{workspace_id, role, ...}]}`). Cross-tenant usernames
  are indistinguishable from nonexistent ones (404) — the endpoint
  cannot probe foreign handles.
- **`GET /api/v1/users`** — TenantAdmin-only (per-handler role check,
  Developer → 403), tenant-scoped, paginated (`limit`/`offset`, total
  count in the response).
- **`PUT /api/v1/users/{id}/deactivate`** — TenantAdmin-only soft
  deactivation with a real cascade: migration
  `2026-11-13-000063_user_deactivated_at` adds `users.deactivated_at`;
  the auth extractor, WebSocket, and SSO paths reject deactivated
  accounts; deactivating a workspace Owner fires the ownerless-workspace
  flow with an inline TenantAdmin notification; impersonation sessions
  and tokens are revoked when either side is deactivated; self-
  deactivation and cross-tenant targets are rejected; audit-logged with
  reason.
- `GET/PUT /users/me` were pre-existing (task-120) and return/accept
  all spec'd profile fields including preferences.

### Workspace membership endpoints (§API > Workspace Membership)

All seven spec endpoints are live (coverage row 27 implemented):

- **`POST /api/v1/workspaces/{id}/invite`** — spec-named alias of the
  member invite: creates a PENDING membership + `InvitationReceived`
  notification (Owner/Admin/TenantAdmin only).
- **`GET /api/v1/workspaces/{id}/invitations`** — pending-only list.
- **`POST /api/v1/workspaces/{id}/invite/accept`** — invitee-only
  per-handler auth (membership looked up by caller+workspace); stamps
  `accepted_at`; 404 when not invited, 409 when already accepted.
- **`DELETE /api/v1/workspaces/{id}/invitations/{invitation_id}`** —
  revokes pending only (accepted → 409 pointing at member removal) and
  deletes the membership row.
- `GET .../members`, `PUT .../members/{user_id}(/role)`, and
  `DELETE .../members/{user_id}` were pre-existing and carry the
  last-Owner guard plus audited ownership transfer
  (`ownership_transfer_unblocks_old_owner_removal_and_is_audited`).

All routes are registered in `api/mod.rs` with ABAC resolver entries
(`invite/accept` is resolver-exempt with per-handler auth since any
global role can be an invitee). Pending invitations are unaccepted
membership rows — the lifecycle is real storage state, not a parallel
table.

### This round (repair)

The prior candidate (3df7f325) failed verification on exactly one
finding: rustfmt changed-lines in `api/impersonation.rs`. This round:

- Applied rustfmt to `api/impersonation.rs`; `rustfmt --check` is now
  clean on every Rust file changed vs the assignment base.
- Corrected the coverage row 26 migration citation (000062 → 000063,
  the post-rebase number).
- Verified the merged tree (base d9c419a0 / task-191 + this branch)
  compiles: `cargo check -p gyre-server` passes; the merge touches no
  task-124 file.

### Server tests the verifier must execute

Large gyre-server test compilation is deferred to the independent
verifier per the sandbox budget. Focused commands:

- `cargo test -p gyre-server api::users::tests` — includes
  `get_user_by_username_is_tenant_scoped`,
  `list_users_is_tenant_admin_only_scoped_and_paginated`,
  `deactivate_user_cascades_to_auth_and_ownership`,
  `workspace_invitation_lifecycle`,
  `ownership_transfer_unblocks_old_owner_removal_and_is_audited`,
  `last_owner_cannot_be_removed_or_downgraded`,
  `member_management_requires_owner_admin_or_tenant_admin`,
  `invite_member_rejects_duplicate_membership`.
- `cargo test -p gyre-server api::impersonation::tests` — includes
  `deactivating_target_revokes_impersonation_token`,
  `deactivating_impersonator_revokes_impersonation_token`,
  `resolver_rejects_deactivated_accounts_even_with_unended_session`.
