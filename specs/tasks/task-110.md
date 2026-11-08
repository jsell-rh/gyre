---
title: "Implement tenant & workspace invitation flow"
spec_ref: "user-management.md §Tenant-Level User Onboarding"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "user-management.md §Tenant-Level User Onboarding"
  - "user-management.md §Workspace Invitation Flow"
  - "user-management.md §Invitation Expiry"
  - "user-management.md §Tenant Invitations"
commits: ["0f4fea9aa717616b17ad1c76bc868d21906e22cc", "8f66d5a436ac2926fa8b12714abc34540a81d1fb"]
---

## Spec Excerpt

From `user-management.md` §Tenant-Level User Onboarding, §Workspace Invitation Flow, §Invitation Expiry:

**Tenant invitations** allow admins to invite users by email. Two modes: SSO mode (user auto-provisions on first login, invitation pre-assigns workspace memberships) and Local mode (magic link or password setup, no Keycloak dependency).

```rust
pub struct TenantInvitation {
    pub id: Id,
    pub tenant_id: Id,
    pub email: String,
    pub invited_by: Id,
    pub role: GlobalRole,
    pub workspace_ids: Vec<Id>,
    pub workspace_roles: Vec<WorkspaceRole>,
    pub status: InvitationStatus,
    pub token_hash: String,       // SHA-256 of invitation token
    pub expires_at: u64,
    pub created_at: u64,
    pub accepted_at: Option<u64>,
}

pub enum InvitationStatus { Pending, Accepted, Declined, Expired, Revoked }
```

**API endpoints:**

| Endpoint | Method | Purpose |
|---|---|---|
| `POST /api/v1/tenant/invite` | POST | Invite user to tenant (TenantAdmin only) |
| `POST /api/v1/tenant/invite/bulk` | POST | Bulk invite (TenantAdmin only) |
| `GET /api/v1/tenant/invitations` | GET | List pending/expired/accepted invitations |
| `DELETE /api/v1/tenant/invitations/{id}` | DELETE | Revoke pending invitation |
| `POST /api/v1/invite/{token}/accept` | POST | Accept invitation via magic link token |

**Workspace invitations** are separate: once a user exists in the tenant, workspace access is granted through `POST /api/v1/workspaces/{id}/invite`. The existing workspace member endpoints handle membership, but the invitation flow (pending state, expiry, accept/decline) is not implemented.

**Invitation expiry:** Background job marks expired invitations as `Expired`. Configurable per-tenant: `tenant_invite_expiry_days` (default: 7), `workspace_invite_expiry_days` (default: 7), `max_pending_invitations` per workspace (default: 50).

## Implementation Plan

1. **Domain entities in `gyre-domain`:**
   - Add `TenantInvitation` struct with all spec fields
   - Add `InvitationStatus` enum
   - Add `InvitationPolicy` struct for configurable expiry
   - Add `WorkspaceInvitation` struct (workspace-level invitation with pending/accept/decline lifecycle)

2. **Port traits in `gyre-ports`:**
   - `TenantInvitationRepository` — CRUD + list by tenant + find by token_hash
   - `WorkspaceInvitationRepository` — CRUD + list by workspace + find by token

3. **SQLite adapter in `gyre-adapters`:**
   - Migration for `tenant_invitations` table
   - Migration for `workspace_invitations` table
   - Implement both repository traits

4. **API endpoints in `gyre-server`:**
   - `POST /api/v1/tenant/invite` — create invitation, generate token, hash and store
   - `POST /api/v1/tenant/invite/bulk` — batch create
   - `GET /api/v1/tenant/invitations` — list with status filter
   - `DELETE /api/v1/tenant/invitations/{id}` — revoke (set status to Revoked)
   - `POST /api/v1/invite/{token}/accept` — validate token, create user (local mode) or link account (SSO), activate workspace memberships
   - Wire workspace invitation endpoints into existing workspace member routes

5. **Background job:**
   - Expiry checker runs on server startup interval (e.g., every hour)
   - Marks `Pending` invitations past `expires_at` as `Expired`

6. **Register routes in `api/mod.rs`** and add ABAC route mappings in `abac_middleware.rs`.

## Acceptance Criteria

- [ ] `TenantInvitation` and `WorkspaceInvitation` domain entities in gyre-domain
- [ ] Port traits for both invitation types
- [ ] SQLite adapter with migrations (check current migration number)
- [ ] All 5 tenant invitation API endpoints functional
- [ ] Workspace invitation lifecycle (invite → pending → accept/decline/expire)
- [ ] Token generation uses cryptographically random bytes, stored as SHA-256 hash
- [ ] Invitation expiry background job marks expired invitations
- [ ] Bulk invite endpoint accepts array of invitations
- [ ] Routes registered in `api/mod.rs` with ABAC mappings
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/user-management.md` §Tenant-Level User Onboarding through §Invitation Expiry for full requirements. The existing workspace member endpoints are in `gyre-server/src/api/workspaces.rs` (or grep for `POST /api/v1/workspaces`). User creation is in `gyre-domain/src/user.rs`. Auth handling is in `gyre-server/src/auth.rs`. Route registration is in `gyre-server/src/api/mod.rs`. ABAC route mappings are in `gyre-server/src/abac_middleware.rs`. Check migration numbering with `ls crates/gyre-adapters/src/sqlite/migrations/ | tail -5`.
## Shipped

**Rebase-resolution round 4: merge of upstream `c5deca801a` (task-105
log compression / task-120 user-entity lineage) onto candidate
`7e427038a`, committed as `8a9b01ee4`.** Four textual conflicts
(schema.rs users table, postgres/sqlite user adapters, auth.rs JWT
user resolution) resolved by union — both task-110's tenant scope +
global_role and upstream's username/avatar_url/preferences/
last_login_at survive, with the surviving auth function being
`find_or_create_user_tenant(state, sub, tenant_id, preferred_username,
...)` feeding upstream's username-derivation tail.

Two semantic collisions the auto-merge could not see were found and
fixed as part of the resolution:

1. **Duplicate-column migration abort:** upstream's
   `2026-10-08-000058_user_entity_profile_fields` (Diesel version
   20261008000058) creates `users.tenant_id`/`global_role` and always
   runs before task-110's `2026-10-09-000057_user_tenant_scope`
   (20261009000057), which created the same columns — fresh-DB startup
   aborted with "duplicate column name: tenant_id" (reproduced by
   `cargo test -p gyre-adapters` before the fix). The task-110
   migration is now reduced to its unique contribution
   (`idx_users_tenant`), with the rebase rationale recorded in the
   file; its down.sql drops only the index.
2. **external_id immutability vs. the specced invitation link:**
   upstream's port contract makes `update()` reject external_id
   changes, which broke task-110's "SSO mode: … then account linked"
   path. Added `UserRepository::claim_external_id(user_id,
   new_external_id)` — the single sanctioned external_id transition,
   failing on unknown user or an external_id owned by another user —
   implemented in SQLite, Postgres, and mem; the auth email-link path
   claims through it (resolution order preserved: external_id match,
   then verified same-tenant email claim, then provisioning) and
   stamps `last_login_at` per upstream's every-login contract.

Evidence log: `/tmp/stage/review-evidence/task-110-rebase-round-4.md`.

### Shipped behavior (task-110, prior rounds)

- **Tenant invitations:** domain entities
  (`gyre-domain/src/invitation.rs`), ports
  (`gyre-ports/src/invitation.rs`), SQLite/Postgres migration
  `2026-10-08-000056_invitations` + adapters, and the five specced
  endpoints: single invite, bulk invite, list with status filter,
  revoke, magic-link accept/decline. Tokens are CSPRNG-generated and
  stored only as SHA-256 hashes. Per-tenant `InvitationPolicy`
  endpoints configure expiry; an hourly background job
  (`GYRE_INVITE_EXPIRY_INTERVAL_SECS`) marks past-due `Pending`
  invitations `Expired`.
- **Workspace invitations:** invite (by `user_id`, `username`, or
  `email`, per spec), list, revoke, token accept/decline, plus the
  authenticated invitee surface: `GET /api/v1/users/me/invitations`,
  `POST /api/v1/workspaces/{id}/invite/accept` and `/invite/decline`
  (caller must be the invited user; no pending invitation → 404).
  In-app notification to the invitee carries `kind:
  workspace_invitation` with `accept_url`/`decline_url`.
- **Single-use race safety:** `transition_status(id, from, to,
  accepted_at)` enforces conditional status claims inside the write
  (SQLite/Postgres conditional `UPDATE`, mem check-and-set); accept
  claims Pending→Accepted before creating user/memberships, decline,
  revoke, and expiry use the same transition, so terminal statuses
  are never overwritten by racing writers.
- **Authorization:** all routes registered in `api/mod.rs` with ABAC
  resolver mappings and per-handler enforcement; HTTP-layer negative
  tests prove non-admins get 403 (tenant invite) and non-members get
  403 (workspace invite) through the full routing chain.
- **Docs:** `docs/api-reference.md` covers all invitation routes and
  selectors; `docs/server-config.md` documents
  `GYRE_INVITE_EXPIRY_INTERVAL_SECS`.

### Test evidence (re-run at merged head `8a9b01ee4`, this round)

- `cargo check --all` (SKIP_WEB_BUILD=1) — clean, zero warnings.
- `cargo test -p gyre-adapters` — 383 passed / 0 failed: includes the
  fresh-DB migration boot (proves the duplicate-column fix), the
  `migration_000058_backfills_unique_url_safe_usernames` regression
  test (fixture adjusted to carry the now-dependent tenant-scope index
  migration with its target batch), and the new
  `claim_external_id_is_the_sanctioned_repoint` contract test
  (repoint visible only via the new id, taken external_id rejected
  without side effects, unknown user rejected).
- `cargo test -p gyre-server --lib auth::` — 46 passed / 0 failed:
  BOTH sides' suites — upstream's provisioning (username
  derivation/sanitization, `second_login_stamps_last_login_and_
  keeps_username`) and task-110's link tests
  (`jwt_email_link_claims_invitation_account`, unverified-email and
  cross-tenant negatives).
- `cargo test -p gyre-server --lib api::invitations` — 18 passed / 0
  failed; `api::users` — 14 passed / 0 failed.
- `cargo test -p gyre-domain` — 436 passed / 0 failed.
- All 20 mechanical guard scripts OK at the committed tree (arch,
  abac-route-registry, mcp-write-tools, migration-versions,
  migration-sql-portability, mem-port-contracts,
  task-commit-attribution, and the rest of the repo's invariant
  linters).
- `cargo clippy` on the touched crates — no warnings in any file
  changed this round (remaining output is the pre-existing baseline
  shared by both parents).
- Loopback TCP listeners are permitted in this sandbox and the
  in-process HTTP-layer tests above exercise the full routing chain;
  a separate live-server smoke remains for host verification / CI at
  the PR head.
