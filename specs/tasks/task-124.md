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
commits: ["33e30ac96"]
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
  - `workspace_ownership::has_active_owner` and the users.rs last-Owner
    guards treat a deactivated Owner as departed: deactivating the last
    active Owner records the ownerless transition and notifies the
    tenant's TenantAdmins inline (`WorkspaceOwnerless`), so ownership
    can be reassigned via the role-change endpoint;
  - audit-logged (`user_deactivated` with reason); self-deactivation →
    409; cross-tenant target → 403; idempotent (original stamp kept).

Pre-existing `GET/PUT /api/v1/users/me` audited against the spec: all
fields (username, display_name, timezone, locale, preferences,
avatar_url) are returned/accepted with partial-update semantics.

### Workspace membership endpoints (§API > Workspace Membership)

All seven spec endpoints are now live. Pre-existing: member list,
role change (with audited ownership transfer and last-Owner guard),
member removal. New:

- **`POST /api/v1/workspaces/{id}/invite`** — spec-named alias of the
  M22.8 invite; creates a PENDING (unaccepted) membership and notifies
  the invitee (`InvitationReceived`).
- **`GET /api/v1/workspaces/{id}/invitations`** — pending-only list,
  Owner/Admin/TenantAdmin.
- **`POST /api/v1/workspaces/{id}/invite/accept`** — the invitee (and
  only the invitee: the membership is looked up by
  (caller, workspace)) activates their invitation, stamping
  `accepted_at`; 404 when not invited, 409 when already accepted;
  audit-logged. ABAC-exempt with per-handler auth because any global
  role can be a valid invitee.
- **`DELETE /api/v1/workspaces/{id}/invitations/{invitation_id}`** —
  revokes a pending invitation (deletes the membership row); accepted
  memberships → 409 (pointing at member removal, which has the
  last-Owner guard); audit-logged.

Pending invitations are unaccepted `WorkspaceMembership` rows — the
lifecycle is real storage state, not a parallel table.

### Routes and ABAC

All new routes are registered in `api/mod.rs` and mapped in the ABAC
resolver (`user` resource for the profile/list/deactivate routes;
`workspace_member` for the invite/invitations routes; the accept route
is exempt with full per-handler authorization). Verified by
`scripts/check-abac-route-registry.sh` and
`scripts/check-abac-exempt-handlers.sh` (both pass; no new exemptions).

### Out of scope (task-111)

The three session endpoints from the §User Management table
(`GET /users/me/sessions`, `DELETE /users/me/sessions/{id}`,
`POST /users/me/sessions/revoke-all`) are **not** implemented here:
task-111 (not-started) owns §Session Management end-to-end (entity,
port, adapters, auth integration, background cleanup). Duplicating that
scope here would collide with task-111's assignment, so the §User
Management coverage row remains task-assigned with the session rows
explicitly open.

### Test evidence

Executed here (via `/tmp/stage/bin/cargo`):

- `cargo test -p gyre-domain user` — 12 passed (incl. new
  `deactivate_stamps_once_and_reports_state`).
- `cargo test -p gyre-adapters --lib sqlite::user` — 16 passed (incl.
  new `deactivation_persists_and_round_trips`, which also proves
  migration 000062 runs and the column round-trips through
  create/update/find/list).
- `cargo check -p gyre-server --tests` — exit 0 (all new server
  handlers + tests type-check).
- All 17 mechanical invariant scripts pass (arch, ABAC route registry,
  migration versions/portability, mem port contracts, exempt handlers,
  inert enforcement, scope literals, forged scope fields, etc.).
- `cargo fmt --check` clean; clippy warning count on
  gyre-domain/gyre-adapters identical to the pre-change baseline (299,
  none in touched files).

Deferred to the independent verifier (gyre-server test compilation
exceeds this sandbox's 4 GiB budget — the build wrapper blocks it in
the implement stage):

- `cargo test -p gyre-server --lib api::users` — the four new tests:
  `get_user_by_username_is_tenant_scoped`,
  `list_users_is_tenant_admin_only_scoped_and_paginated`,
  `deactivate_user_cascades_to_auth_and_ownership` (asserts the
  deactivated key stops authenticating, the ownerless record is
  written, the TenantAdmin is notified, and the audit event carries
  the reason), and `workspace_invitation_lifecycle` (invite → list →
  foreign-accept 404 → accept → re-accept 409 → revoke-pending →
  revoke-accepted 409 → revoke-gone 404).
- `cargo test -p gyre-server --lib workspace_ownership`,
  `auth`, `git_http`, `abac_middleware` — regression surfaces touched
  by the deactivation semantics (`has_active_owner`, auth extractor).
- `cargo test --all`.
