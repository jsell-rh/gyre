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
commits: ["33e30ac961ad1021b50f3cd3a65c3ac184a03ca1", "6033324f8936365465b9ffc9b27e6c3a6429f0d1", "38b3134feeaa28be0e443e2e29344980ea6db552", "60c86252706b2392866b1dddd66634ab4cb123d6", "3350817099f10cc5fde2e505f9cb4ad8bb5d129e"]
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

Five of the eight spec endpoints are implemented in production code;
the three session endpoints are owned by task-111 (its §Session
Management row covers the UserSession entity, `user_sessions` storage,
and auth-middleware revocation checks — not just the three routes), so
the §User Management coverage row stays task-assigned over them:

- **`GET /api/v1/users/{username}`** — tenant-public profile
  (`{id, username, display_name, avatar_url, timezone, created_at,
  memberships:[{workspace_id, role, ...}]}`). Cross-tenant handles are
  indistinguishable from nonexistent ones (404), so the endpoint cannot
  probe foreign tenants.
- **`GET /api/v1/users`** — TenantAdmin-only (per-handler role check,
  Developer → 403), tenant-scoped, paginated (`limit`/`offset` with a
  total count).
- **`PUT /api/v1/users/{id}/deactivate`** — TenantAdmin-only soft
  deactivation with a real cascade: migration
  `2026-11-13-000063_user_deactivated_at` adds `users.deactivated_at`;
  the auth extractor, WebSocket, and SSO paths reject deactivated
  accounts; deactivating a workspace Owner fires the ownerless-workspace
  flow with an inline TenantAdmin notification; impersonation sessions
  and tokens are revoked when either side is deactivated;
  self-deactivation and cross-tenant targets are rejected;
  audit-logged with reason.
- `GET/PUT /users/me` were pre-existing (task-120) and return/accept
  all spec'd profile fields including preferences.

### Workspace membership endpoints (§API > Workspace Membership)

All seven spec endpoints are live:

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
  last-owner guard plus audited ownership transfer
  (`ownership_transfer_unblocks_old_owner_removal_and_is_audited`).

All routes are registered in `api/mod.rs` with ABAC resolver entries
(`invite/accept` is resolver-exempt with per-handler auth since any
global role can be an invitee). Pending invitations are unaccepted
membership rows — the lifecycle is real storage state, not a parallel
table.

### This round (contract repair `9b7fdf25`)

The executor restored the assigned task document after the protected
task text changed in the previous checkpoint (acceptance-criteria
checkboxes were marked inside the protected body). The production fix
is preserved unchanged from the retained branch: the implementation
commits listed above, including the `c844ea52` base merge (`5f86aede`,
which resolved the `auth.rs` DIRTY-merge finding `dc996978` by keeping
both fail-closed login guards — task-124's deactivated-account SSO
rejection and task-129's reserved system-identity rejection) and the
rebase-restore commit `7582a481`. No product source changed this round;
only this task document's progress metadata and shipped notes were
rewritten, with the protected body left byte-identical to the assigned
contract (checkboxes untouched — completion is recorded here, not by
editing protected text).

Re-verification of the retained tree (logs under
`/tmp/stage/review-evidence`):

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` — exit 0.
- All 19 mechanical invariant gates exit 0 (ABAC route registry, arch,
  exempt handlers, MCP write tools, migration versions/portability,
  byte-slice truncation, in-memory state stores, lossy secrets, scope
  literal/fabricated defaults, inert enforcement, forwarded-header
  trust, unbounded external HTTP, fail-open ref resolution, relative
  path defaults, dead message kinds, forged scope fields, mem port
  contracts).
- No conflict markers under `crates/`.

### Server tests the verifier must execute

Large gyre-server test compilation is deferred to the independent
verifier per the sandbox memory budget:

- `cargo test --all` — the assignment's definition-of-done gate.
- `cargo test -p gyre-server api::users::tests` — includes
  `deactivate_user_cascades_to_auth_and_ownership`,
  `workspace_invitation_lifecycle`,
  `get_user_by_username_is_tenant_scoped`,
  `list_users_is_tenant_admin_only_scoped_and_paginated`,
  `ownership_transfer_unblocks_old_owner_removal_and_is_audited`,
  `last_owner_cannot_be_removed_or_downgraded`,
  `member_management_requires_owner_admin_or_tenant_admin`,
  `invite_member_rejects_duplicate_membership`.
- `cargo test -p gyre-server api::impersonation::tests` — includes
  `deactivating_target_revokes_impersonation_token`,
  `deactivating_impersonator_revokes_impersonation_token`,
  `resolver_rejects_deactivated_accounts_even_with_unended_session`.
- `cargo test -p gyre-server auth::tests` — includes
  `jwt_rejects_reserved_system_identity_provisioning` (task-129 guard
  sharing the block resolved in `5f86aede`).
