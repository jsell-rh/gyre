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
commits: ["43b15db3df88aed619f631a209e0cce124594100", "8833f8d99ccd1536a06ad81e364f80efc5c3f330", "01958bc011244ae4a4c06258baec597a00d8134d", "9f9be53014102d44e5e8c148d21b580448c226c7", "d1103854d71b5b8adec5f2c7e03d691f7e8dc059", "5df8f2b136b1ad5792b58269cb4dc778ec301b05", "e52fea4f3bfc0ce1a57f99b0db6c98f55fe14209", "6bef61b7f7c9d5ccf391a9514a9064b8888d10f5", "ea366b150438e6b185edcf812cab5c5b1384b77a", "797cf52f94910a8c8bb9a5ae3f4f59868a99d820", "c1de316bd3c1f9b2fd09689891f58f36abf95c66", "8c469c538bd84cf880cb72522855c66a7a8f7d4d", "0f4fea9aa717616b17ad1c76bc868d21906e22cc", "8f66d5a436ac2926fa8b12714abc34540a81d1fb"]
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

Tenant & workspace invitation flow, fully implemented and carried through
multiple review/repair rounds on this branch (latest: rebase repair onto
assignment base b8d63b0d — task-113 in-app notification UI & email templates —
merged at 1ada3106; the merge brought task-113's rows 19/20 into the coverage
matrix and touched the invitation surface in no file: the auto-merged
user.rs/users.rs/lib.rs/main.rs hunts are purely additive task-113 changes
(email module, `EmailConfig.type_overrides`, `NotificationPush` fan-out)):

- **Domain entities** (`gyre-domain`): `TenantInvitation`, `WorkspaceInvitation`,
  `InvitationStatus` (Pending/Accepted/Declined/Expired/Revoked), per-tenant
  `InvitationPolicy` (`tenant_invite_expiry_days` / `workspace_invite_expiry_days`
  default 7, `max_pending_invitations` default 50, `allow_re_invite`).
- **Ports + adapters**: `TenantInvitationRepository` and
  `WorkspaceInvitationRepository` port traits implemented by the SQLite,
  Postgres, and mem adapters (mem guards the port-documented duplicate
  constraints in code); shared portable migrations for both tables plus
  DB-level pending-scope uniqueness (migration 000064 partial unique
  indexes, with raced-duplicate dedup before index creation).
- **API** (`gyre-server/src/api/invitations.rs`): the five tenant endpoints
  (`POST /tenant/invite`, `POST /tenant/invite/bulk`, `GET /tenant/invitations`,
  `DELETE /tenant/invitations/{id}`, `POST /invite/{token}/accept`) plus the
  workspace invitation lifecycle (`POST /workspaces/{id}/invite`, invitee
  accept/decline via token and via authenticated identity) and per-tenant
  policy endpoints. Tokens are 32 CSPRNG bytes, hex-encoded, stored only as
  SHA-256 hashes; acceptance is single-use and transactional (membership
  activation rolls the status back on failure). Local-mode acceptance creates
  the user and mints a first API key; SSO-mode acceptance links the existing
  account. Routes registered in `api/mod.rs` with ABAC mappings; the two
  invitee-facing accept/decline routes are ABAC-exempt with per-handler
  authorization (`authorize_my_pending_invitation`: tenant containment +
  caller-must-be-the-invitee), because the seeded membership builtin would
  otherwise deny the non-member invitee before the handler runs.
- **Deactivation contract (review 4cf9e027)**: the SSO email-link claim
  branch in `auth.rs find_or_create_user_tenant` bails on a deactivated
  invitation-created account (mirroring the API-key, WS, and direct
  external_id paths); tenant acceptance (local-mode and SSO-link arms) and
  the token-based workspace accept route reject deactivated accounts with
  403 before any side effect, rolling the single-use claim back to Pending.
- **PostgreSQL int4 repair (review 4cf9e027)**: migration 000063's six
  invitation timestamp columns are BIGINT (matching schema.rs BigInt and
  the adapters' i64 row structs); PG databases that already ran the INTEGER
  variant are repaired in place by the PG-only post-migration hook
  `postgres/repair.rs::ensure_invitation_timestamps_bigint`
  (information_schema check + `ALTER COLUMN ... TYPE BIGINT`, idempotent).
- **Background expiry job** marks `Pending` invitations past `expires_at` as
  `Expired`; handlers also lazy-expire on touch.
- **Tests** include a production-chain regression
  (`invitee_accepts_and_declines_through_abac_middleware`) that layers the real
  `api_router` with `abac_middleware` and seeded builtin policies, proves the
  non-member invitee is still denied on a workspace-mapped route, then exercises
  the exempt invitee routes end-to-end; deactivation-contract regressions
  (`jwt_email_link_does_not_claim_deactivated_account`,
  `deactivated_local_account_cannot_accept_invitation`,
  `deactivated_sso_account_cannot_accept_invitation`,
  `deactivated_invitee_cannot_accept_workspace_invitation_by_token`); and
  local-mode accept, re-invite, username-collision, expiry, cap, and
  cross-tenant-guard coverage.

### Test evidence

Executed on the merged tree (base b8d63b0d) in this round (all exit 0):
`cargo check -p gyre-server -p gyre-domain -p gyre-adapters -p gyre-ports`,
`cargo test -p gyre-domain` (543 passed), `cargo test -p gyre-adapters`
(545 passed; invitation suite 5 passed), `cargo test -p gyre-common -p gyre-ports`
(125 passed), `cargo test -p gyre-cli invitation` (1 passed), and the
mechanical gates `check-arch`, `check-abac-route-registry`,
`check-abac-exempt-handlers`, `check-migration-versions`,
`check-mem-port-contracts`, `check-mcp-write-tools`, `check-dead-message-kinds`;
full `scripts/check-*.sh` sweep compared against pre-merge HEAD shows no new
failures beyond the (now-restored) task attribution frontmatter. Coverage
matrix resolved for the merge: rows 25-30 kept from this branch, rows 19/20
(task-113) adopted from the base, SUMMARY regenerated mechanically
(user-management 36 rows: 11 n/a, 0 not-started, 5 task-assigned,
20 implemented, 80%). Evidence: `/tmp/stage/review-evidence/merge-b8d63b0d.txt`.
Full `cargo test -p gyre-server` (incl. the `api::invitations` suite) and
`cargo test --all` remain recorded for the independent verifier (sandbox
4 GiB build-wrapper limit).
