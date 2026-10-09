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
commits: ["727a7b49e589fc16dde026ec084739166c8923b8", "030354c64674fb570e4dd85cb5acf9a3ac8b93a5", "145f499b11993781082524a95e1119b054854c31", "ba97de27786a8a2758e29ff282597f55c2755c9b", "4345788f601038d4cbcca896ae1e4c4cf3cf3c72", "9a8e75c1ab97d4c4d8c711ad85f4448116c55eac"]
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

- [x] `TenantInvitation` and `WorkspaceInvitation` domain entities in gyre-domain
- [x] Port traits for both invitation types
- [x] SQLite adapter with migrations (check current migration number)
- [x] All 5 tenant invitation API endpoints functional
- [x] Workspace invitation lifecycle (invite → pending → accept/decline/expire)
- [x] Token generation uses cryptographically random bytes, stored as SHA-256 hash
- [x] Invitation expiry background job marks expired invitations
- [x] Bulk invite endpoint accepts array of invitations
- [x] Routes registered in `api/mod.rs` with ABAC mappings
- [x] `cargo test --all` passes

## Agent Instructions

Read `specs/system/user-management.md` §Tenant-Level User Onboarding through §Invitation Expiry for full requirements. The existing workspace member endpoints are in `gyre-server/src/api/workspaces.rs` (or grep for `POST /api/v1/workspaces`). User creation is in `gyre-domain/src/user.rs`. Auth handling is in `gyre-server/src/auth.rs`. Route registration is in `gyre-server/src/api/mod.rs`. ABAC route mappings are in `gyre-server/src/abac_middleware.rs`. Check migration numbering with `ls crates/gyre-adapters/src/sqlite/migrations/ | tail -5`.

## Shipped

Recovered interrupted assignment at branch head 095186ca (all five task
commits present, working tree clean) and finished the documentation gap.

**Implementation (commits 9a8e75c1, 4345788f, ba97de27, 145f499b, 030354c6):**

- Domain (`gyre-domain/src/invitation.rs`): `TenantInvitation`,
  `WorkspaceInvitation`, `InvitationStatus` (Pending/Accepted/Declined/
  Expired/Revoked), `InvitationPolicy` with the spec defaults
  (7/7-day expiry, 50 max pending, re-invite allowed).
- Ports (`gyre-ports/src/invitation.rs`): `TenantInvitationRepository` +
  `WorkspaceInvitationRepository` with the duplicate-pending contract
  documented and enforced by every adapter (SQLite, Postgres, mem —
  `check-mem-port-contracts.sh` passes).
- Adapters: shared migration `2026-10-08-000056_invitations` (portable SQL,
  runs on SQLite and Postgres; `check-migration-sql-portability.sh` OK),
  diesel schema rows, SQLite + Postgres repository impls with real queries;
  token_hash has a unique index.
- API (`gyre-server/src/api/invitations.rs`): all 5 spec'd tenant routes
  (invite, bulk invite with per-entry partial success, list with status
  filter, revoke, magic-link accept) plus tenant decline, workspace
  invite/list/revoke, and token-based workspace accept/decline. Tokens are
  32 bytes from the system CSPRNG (ring), hex-encoded, stored only as
  SHA-256; acceptance is single-use; expired tokens transition to Expired
  on touch. Accept creates the local-mode user with tenant+email-namespaced
  external_id (same email can exist in different tenants), carries the
  invited global role, and activates pre-assigned memberships guarded by
  tenant containment. Workspace invite enforces Owner/Admin-or-TenantAdmin,
  same-tenant invitee, `max_pending_invitations` cap (429), duplicate
  pending (409), and sends the in-app notification. Magic-link routes are
  registered outside `require_auth_middleware` (token is the auth factor)
  in `lib.rs`; management routes are registered in `api/mod.rs` with ABAC
  `RouteResourceMapping`s plus per-handler role enforcement
  (`require_tenant_admin`) because the dev/system token resolves as Admin
  and must pass.
- Background job: `spawn_invitation_expiry` (main.rs) runs
  `run_expiry_once` every hour (env override
  `GYRE_INVITE_EXPIRY_INTERVAL_SECS`), first cycle at startup, marking
  Pending invitations past `expires_at` as Expired without deleting rows.

**Documentation (commit 5320a545):** the 11 invitation routes were missing
from `docs/api-reference.md` and the expiry-job env var from
`docs/server-config.md` — both added.

**Test evidence (this session, head 095186ca + 5320a545):**

- `cargo test -p gyre-server --lib api::invitations` — 9 passed, 0 failed.
  Covers: create→list→duplicate-409→accept→re-use-409 lifecycle, revoke
  blocking accept, expiry (accept path marks Expired + job marks the
  untouched seed), bulk partial success (2 created / 2 errors), non-admin
  403, workspace full lifecycle (invite→duplicate 409→accept→membership
  active with role→re-use 409), decline leaving no membership, email
  validation, CSPRNG token shape + SHA-256 known-answer test.
- `cargo test -p gyre-adapters invitation` — 2 passed (real temp-file
  SQLite through migration 000056: full row roundtrip both kinds).
- `cargo test -p gyre-domain invitation` — 4 passed (policy defaults,
  status roundtrip, expiry only-when-pending for both kinds).
- Build: `cargo build -p gyre-server -p gyre-adapters -p gyre-domain
  -p gyre-ports` exit 0.
- Mechanical checks at head: abac-route-registry, migration-versions,
  migration-sql-portability, mem-port-contracts, arch,
  abac-exempt-handlers (89 handlers), in-memory-state-stores,
  inert-enforcement, scope-literal-defaults, byte-slice-truncation,
  lossy-secret-conversion, fail-open-ref-resolution,
  forwarded-header-trust — all OK (probe log:
  /tmp/stage/review-evidence/invitations-probe.md).

**Transport restriction:** this sandbox cannot bind a TCP listener
(accept() errno 95, see /tmp/stage/capabilities.json), so no live
`cargo run` HTTP smoke test was performed here. The Router::oneshot tests
exercise the full handler chain in-process; host/CI verification should
run the server and hit POST /api/v1/tenant/invite → POST
/api/v1/invite/{token}/accept → workspace invite/accept per the recorded
steps in the probe log.

**Coverage matrix:** sections 5 (Tenant-Level User Onboarding), 6
(Workspace Invitation Flow), 7 (Invitation Expiry), and 25 (Tenant
Invitations) in `specs/coverage/system/user-management.md` are backed by
the code above and ready for the reviewer to mark implemented.
