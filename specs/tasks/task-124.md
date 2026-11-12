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
commits: ["6033324f8936365465b9ffc9b27e6c3a6429f0d1", "33e30ac961ad1021b50f3cd3a65c3ac184a03ca1"]
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

- **`GET /api/v1/users/{username}`** — tenant-public profile
  (`{id, username, display_name, avatar_url, timezone, created_at,
  memberships:[{workspace_id, role, ...}]}`). Cross-tenant usernames are
  indistinguishable from nonexistent ones (404) — tenants are hard
  isolation boundaries, so the endpoint cannot probe foreign handles.
- **`GET /api/v1/users`** — TenantAdmin-only (per-handler role check,
  same split as `POST /users`), scoped to the caller's tenant, paginated
  via `?limit=&offset=` (max 200); response carries `total` and each
  user's `deactivated_at`.
- **`PUT /api/v1/users/{id}/deactivate`** — TenantAdmin-only soft
  deactivation with a real cascade, not a flag flip:
  - new `users.deactivated_at` column (migration
    `2026-11-13-000062_user_deactivated_at`, portable SQL, next unused
    sequence number; round-trip covered by an adapter test);
  - the auth extractor's API-key path, the WebSocket auth path, and the
    SSO `find_or_create_user` login path all reject deactivated accounts
    (403 / login error) — a deactivated user's existing key stops
    working immediately;
  - active impersonation sessions the user participates in (as
    impersonator OR target) are ended immediately
    (`impersonation::end_active_sessions_for_user`), revoking unexpired
    impersonation JWTs, and `resolve_impersonation_auth` independently
    re-checks both accounts' deactivation state on every call —
    belt-and-suspenders, so a rogue deactivated TenantAdmin's
    unexpired impersonation token cannot keep API access as the target
    user (review repair, commit 6033324f8);
  - `workspace_ownership::has_active_owner` and the users.rs last-Owner
    guards treat a deactivated Owner as departed: deactivating the last
    active Owner records the ownerless transition and notifies the
    tenant's TenantAdmins inline (`WorkspaceOwnerless`), so ownership
    can be reassigned via the role-change endpoint;
  - audit-logged as `user_deactivated` with reason; idempotent
    (second call keeps the original stamp); self-deactivation → 409;
    cross-tenant target → 403.

### Workspace membership endpoints (§API > Workspace Membership)

- **`GET /api/v1/workspaces/{id}/invitations`** — list pending
  invitations (member-visible);
- **`POST /api/v1/workspaces/{id}/invite`** — invite user
  (Owner/Admin only, per-handler check);
- **`POST /api/v1/workspaces/{id}/invite/accept`** — accept invitation
  (invitee only, idempotent);
- **`DELETE /api/v1/workspaces/{id}/invitations/{id}`** — revoke
  invitation (Owner/Admin only);
- **`PUT /api/v1/workspaces/{id}/members/{user_id}/role`** — role
  change incl. ownership transfer validation (last-active-Owner
  protection, demotion guards);
- existing list/add/remove member endpoints audited against spec.

All routes registered in `api/mod.rs` with ABAC `RouteResourceMapping`
entries in `abac_middleware.rs`.

### Session endpoints

Verified covered by task-111 (list / revoke individual / revoke all
under `/api/v1/users/me/sessions`); no duplication added.

### Test evidence

- Endpoint behavior (list/deactivate cascade, invitations, ownership
  transfer): integration tests in `crates/gyre-server/src/api/users.rs`
  and the workspace handlers (commit 33e30ac96), previously reviewed.
- Impersonation deactivation repair (commit 6033324f8): three
  regression tests in `crates/gyre-server/src/api/impersonation.rs` —
  `deactivating_target_revokes_impersonation_token`,
  `deactivating_impersonator_revokes_impersonation_token`, and
  `resolver_rejects_deactivated_accounts_even_with_unended_session`
  (deactivates the user record directly so only the resolver's
  account-state check can reject the token).
- This sandbox: `cargo check -p gyre-server` and `cargo check -p
  gyre-server --tests` both pass (exit 0, evidence in
  `/tmp/stage/review-evidence/`). gyre-server test compilation is
  deferred to the verifier per the 4 GiB budget rule; the verifier must
  run:

      cargo test -p gyre-server --lib impersonation
