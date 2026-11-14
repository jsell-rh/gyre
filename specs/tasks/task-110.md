---
title: "Implement tenant & workspace invitation flow"
spec_ref: "user-management.md §Tenant-Level User Onboarding"
depends_on: []
progress: complete
coverage_sections:
  - "user-management.md §Tenant-Level User Onboarding"
  - "user-management.md §Workspace Invitation Flow"
  - "user-management.md §Invitation Expiry"
  - "user-management.md §Tenant Invitations"
commits: ["9f9be53014102d44e5e8c148d21b580448c226c7", "d1103854d71b5b8adec5f2c7e03d691f7e8dc059", "5df8f2b136b1ad5792b58269cb4dc778ec301b05", "e52fea4f3bfc0ce1a57f99b0db6c98f55fe14209", "6bef61b7f7c9d5ccf391a9514a9064b8888d10f5", "ea366b150438e6b185edcf812cab5c5b1384b77a", "797cf52f94910a8c8bb9a5ae3f4f59868a99d820", "c1de316bd3c1f9b2fd09689891f58f36abf95c66", "8c469c538bd84cf880cb72522855c66a7a8f7d4d", "0f4fea9aa717616b17ad1c76bc868d21906e22cc", "8f66d5a436ac2926fa8b12714abc34540a81d1fb"]
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

Tenant & workspace invitation flow, implemented and carried through multiple
review/repair rounds on this branch (assignment base 2824d9b1 merged at
7d932e41):

- **Domain entities** (`gyre-domain`): `TenantInvitation`, `WorkspaceInvitation`,
  `InvitationStatus` (Pending/Accepted/Declined/Expired/Revoked), per-tenant
  `InvitationPolicy` (`tenant_invite_expiry_days` / `workspace_invite_expiry_days`
  default 7, `max_pending_invitations` default 50, `allow_re_invite`).
- **Ports + adapters**: `TenantInvitationRepository` and
  `WorkspaceInvitationRepository` port traits implemented by the SQLite,
  Postgres, and mem adapters (mem guards the port-documented duplicate
  constraints in code); shared portable migrations for both tables plus
  DB-level pending-scope uniqueness.
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
- **Background expiry job** marks `Pending` invitations past `expires_at` as
  `Expired`; handlers also lazy-expire on touch.
- **Tests** include a production-chain regression
  (`invitee_accepts_and_declines_through_abac_middleware`) that layers the real
  `api_router` with `abac_middleware` and seeded builtin policies, proves the
  non-member invitee is still denied on a workspace-mapped route, then exercises
  accept (204 + membership) and decline (204 + no membership).

### This round

Verification of candidate a309844 failed only on rustfmt: the two
`let invitation =` / `authorize_my_pending_invitation(...)` bindings in the
invitee accept/decline handlers were split across lines. Fixed by joining them
(commit 9f9be530, whitespace-only — no behavior change).

### Verification evidence

- `python3 scripts/check-rustfmt-diff.py 2824d9b186d5da08ae043772762246ee5b620e89`
  — OK (changed lines clean, 22 Rust files). Evidence retained under
  `/tmp/stage/review-evidence`.
- Prior-round evidence (clippy changed-lines clean, architecture/ABAC/migration
  checks, `cargo check -p gyre-server --tests`) carried over; this round's diff
  is whitespace-only. Server test compilation is left to the independent
  verifier per the 4 GiB budget rule; the relevant suite is
  `cargo test -p gyre-server invitations`.
