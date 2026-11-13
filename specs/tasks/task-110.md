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
commits: ["5df8f2b136b1ad5792b58269cb4dc778ec301b05"]
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

**This round: repair of review finding d6b8a97** (commit
`5df8f2b1`) — local-mode invitation acceptance issued no credential, so
the created account could never authenticate. The invitation flow itself
(entities, ports, SQLite/mem adapters, migrations, 5 tenant endpoints,
workspace lifecycle, CSPRNG+SHA-256 tokens, expiry job, bulk invite, ABAC
mappings) is unchanged production code carried from the prior accepted
rounds through merge `a061aed9`.

### Repair: credential delivery at local-mode acceptance

`POST /api/v1/invite/{token}/accept` now mints the account's **first API
key in the accept response** (the reviewer's closure option 1, mirroring
the `POST /api/v1/users` bootstrap pattern — the platform's local
credential is the API key; there is no password-login path anywhere, and
the only other unauthenticated mint creates a *new* user, so without this
the accepted invitee was permanently unable to log in in the spec's
air-gapped/no-IdP scenario):

- Raw key `gyre_{uuid v4}` (CSPRNG), returned exactly once; only the
  SHA-256 hash is persisted, in `state.api_keys` — the store the auth
  extractor consults — so the key authenticates through the real routing
  chain immediately.
- Minted only after every side effect succeeded (memberships activated,
  status Accepted); on mint failure the invitation rolls back to Pending
  and a retry mints again (the account dedups via the namespace lookup).
- **Minted only for accounts that exist because of invitation acceptance
  to that email**: a fresh local-mode create, or the
  `tenant:{tenant}:email:{email}`-namespaced local account found on
  re-invite/retry (whose earlier key, if any, never reached the client).
  A linked **pre-existing** account (SSO Keycloak subject, SCIM,
  bootstrap `local:{username}`) returns `api_key: null` — token
  possession must not mint a credential that bypasses the account's
  existing auth factor. An SSO-claimed local account (external_id
  re-pointed at the Keycloak subject) also falls in the no-mint arm.

Docs (`docs/api-reference.md` accept-row) and coverage row 5 updated to
record the credential-delivery behavior.

### Test evidence

New/extended tests in `api::invitations` (all distinguish the repaired
behavior from the real defect — without the mint, the accept response has
no key and the tests fail):

- `local_mode_accept_mints_authenticating_api_key` — unauthenticated
  accept returns `api_key.key` (`gyre_…`, name `invitation`); the key
  authenticates through `GET /api/v1/users/me` as the new user (id,
  username, display name, tenant binding), and resolves in
  `state.api_keys` via its hash only.
- `reinvite_acceptance_mints_fresh_key_for_existing_local_account` — a
  second invitation to the same tenant+email delivers a **fresh** key
  authenticating as the **same** user (no parallel account).
- `accept_links_existing_sso_user_instead_of_parallel_account`
  (extended) — linked SSO acceptance returns `api_key: null`.

Sandbox probes (exit 0, source SHAs and logs in
`/tmp/stage/review-evidence/task-110-repair.md`): `cargo check -p
gyre-server --tests` (type-checks the new tests), `cargo clippy -p
gyre-server` (zero findings in invitations.rs), `rustfmt --check` on the
touched file, `check-arch.sh`, `check-abac-route-registry.sh`,
`check-mem-port-contracts.sh`. **gyre-server test execution is deferred to
the independent verifier** (sandbox 4 GiB build cap): run `cargo test -p
gyre-server --lib api::invitations::` and the full server suite at head
`5df8f2b1`.
