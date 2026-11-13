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
commits: ["33e30ac961ad1021b50f3cd3a65c3ac184a03ca1", "6033324f8936365465b9ffc9b27e6c3a6429f0d1", "60c86252706b2392866b1dddd66634ab4cb123d6", "38b3134feeaa28be0e443e2e29344980ea6db552", "3350817099f10cc5fde2e505f9cb4ad8bb5d129e"]
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

From `user-management.md` §API > Workspace Membership:

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

- [x] `GET /api/v1/users/{username}` returns public profile within tenant
- [x] `GET /api/v1/users` lists users (TenantAdmin only, paginated)
- [x] `PUT /api/v1/users/{id}/deactivate` deactivates user with cascade (TenantAdmin only)
- [x] Workspace invitation list, accept, and revoke endpoints functional
- [x] Role change endpoint supports ownership transfer validation
- [x] All routes registered in `api/mod.rs` with ABAC route mappings
- [ ] `cargo test --all` passes — deferred to the independent verifier per the sandbox memory budget (see below)

## Agent Instructions

Read `specs/system/user-management.md` §API > User Management and §API > Workspace Membership. Existing user API: `gyre-server/src/api/users.rs`. Route registration: `gyre-server/src/api/mod.rs` — grep for `/users/` to see existing routes. ABAC route mappings: `gyre-server/src/abac_middleware.rs`. Session management: check if task-111 already covers session endpoints. Workspace member endpoints: grep for `invite_member\|list_members\|update_member_role\|remove_member` in the api/ directory.

## Shipped

### User management endpoints (§API > User Management)

Five of the eight spec endpoints are implemented in production code;
the three session endpoints are owned by task-111, so the
§User Management coverage row stays task-assigned over them:

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
  last-Owner guard plus audited ownership transfer
  (`ownership_transfer_unblocks_old_owner_removal_and_is_audited`).

All routes are registered in `api/mod.rs` with ABAC resolver entries
(`invite/accept` is resolver-exempt with per-handler auth since any
global role can be an invitee). Pending invitations are unaccepted
membership rows — the lifecycle is real storage state, not a parallel
table.

### This round (contract restoration + re-verification)

The executor restored the assigned task document (contract repair
0a1ec38f); this round preserves the existing production fix and
re-verifies it against the current checkout:

- Review finding 3cd87005 (stale 7-argument
  `NotificationRepository::list_for_user` call in the test
  `deactivate_user_cascades_to_auth_and_ownership` after the rebase
  brought in task-125's 8-argument signature) remains fixed by commit
  33508170: the call at `crates/gyre-server/src/api/users.rs:4102`
  passes `None` for the new `repo_id: Option<&Id>` parameter. `None` is
  semantically correct, not just type-correct — the assertion checks
  for a `WorkspaceOwnerless` notification, which has no repo scope, so
  an unfiltered (all-repos) listing is what it needs.
- Fresh probe on HEAD `f1778e09`:
  `/tmp/stage/bin/cargo check -p gyre-server --tests` exits 0
  (~8m55s; log at
  `/tmp/stage/review-evidence/cargo-check-gyre-server-tests-round2.log`).
  A tree-wide grep over `crates/gyre-server/src`,
  `crates/gyre-adapters/src`, and `crates/gyre-domain/src` confirms no
  other stale `list_for_user` call sites.
- Running the test binary itself is deferred to the verifier per the
  sandbox memory budget (commands below).

### Prior rounds (context)

- Rebase repair (merge of `6f5a631c`): the only code conflict was the
  `use users::{...}` import list in `api/mod.rs` (task-124 handlers vs
  task-125 notification handlers), resolved as the union; coverage
  narratives merged and mechanically recounted.
- 38b3134f: fixed a test-fixture bug — `seed_workspace_with_owner`
  built the Owner membership as pending instead of the production
  `create_workspace` semantics (accepted immediately), which had broken
  `workspace_invitation_lifecycle`.

### Server tests the verifier must execute

Large gyre-server test compilation is deferred to the independent
verifier per the sandbox budget:

- `cargo test --all` — the assignment's definition-of-done gate; the
  compile error repaired in the previous round (E0061 in
  `deactivate_user_cascades_to_auth_and_ownership`) was the blocker.
- `cargo test -p gyre-server api::users::tests` — includes
  `deactivate_user_cascades_to_auth_and_ownership` (the repaired call
  site), `workspace_invitation_lifecycle`,
  `get_user_by_username_is_tenant_scoped`,
  `list_users_is_tenant_admin_only_scoped_and_paginated`,
  `ownership_transfer_unblocks_old_owner_removal_and_is_audited`,
  `last_owner_cannot_be_removed_or_downgraded`,
  `member_management_requires_owner_admin_or_tenant_admin`,
  `invite_member_rejects_duplicate_membership`, plus the task-125
  notification handlers that share `users.rs` after the rebase
  (`list_my_notifications`, `get_unread_count`,
  `mark_all_read_notifications`, `notification_stream`).
- `cargo test -p gyre-server api::impersonation::tests` — includes
  `deactivating_target_revokes_impersonation_token`,
  `deactivating_impersonator_revokes_impersonation_token`,
  `resolver_rejects_deactivated_accounts_even_with_unended_session`.
