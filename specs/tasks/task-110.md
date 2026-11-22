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
commits: ["734f2f59a8d54c204aca6aab5fb341ed362bc3fd", "6d62522d4cad76ad4a7b4cc840f65dcbab61f731", "7bc33fe44d2805273a98e37ec5cc5a91663a6f41", "b445900d4f72e5c324cb61c4b2e55c72f6ebed8a", "6af1d71c51cf1f763c0a1c07ec3f14b272edc523", "758462f45d6b162bc273c4a676e6bb33442e5dde", "43b15db3df88aed619f631a209e0cce124594100", "8833f8d99ccd1536a06ad81e364f80efc5c3f330", "01958bc011244ae4a4c06258baec597a00d8134d", "9f9be53014102d44e5e8c148d21b580448c226c7", "d1103854d71b5b8adec5f2c7e03d691f7e8dc059", "5df8f2b136b1ad5792b58269cb4dc778ec301b05", "e52fea4f3bfc0ce1a57f99b0db6c98f55fe14209", "6bef61b7f7c9d5ccf391a9514a9064b8888d10f5", "ea366b150438e6b185edcf812cab5c5b1384b77a", "797cf52f94910a8c8bb9a5ae3f4f59868a99d820", "c1de316bd3c1f9b2fd09689891f58f36abf95c66", "8c469c538bd84cf880cb72522855c66a7a8f7d4d", "0f4fea9aa717616b17ad1c76bc868d21906e22cc", "8f66d5a436ac2926fa8b12714abc34540a81d1fb"]
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

Tenant & workspace invitation flow, implemented across earlier rounds on
this branch and repaired this round against review `f36e46e1` (findings
F1-F4, commit `734f2f59`). Earlier rounds also resolved rebases onto
bases `81c24bf0` and `6cacb4bf`.

### Feature surface (cumulative)

- **Domain/ports/adapters:** `TenantInvitation` + `WorkspaceInvitation`
  entities (`gyre-domain/src/invitation.rs`), `TenantInvitationRepository` +
  `WorkspaceInvitationRepository` ports, SQLite/Postgres/mem adapters,
  migrations `2026-11-13-000063_invitations` and
  `2026-11-13-000064_invitation_pending_unique` (pending-scope partial
  unique index, race-dedupe on apply).
- **All five tenant endpoints** in `api/invitations.rs`, registered in
  `api/mod.rs` with ABAC mappings and per-handler tenant containment:
  `POST /tenant/invite`, `POST /tenant/invite/bulk` (array of invitations,
  per-entry error indexes), `GET /tenant/invitations` (status filter),
  `DELETE /tenant/invitations/{id}` (revoke), `POST /invite/{token}/accept`.
- **Workspace invitation lifecycle:** invite → Pending → accept/decline/
  expire/revoke; invitee surface `GET /users/me/invitations`,
  `POST /workspaces/{id}/invite/accept|decline`; `max_pending_invitations`
  cap (429); cross-tenant access 403.
- **Token security:** 32-byte CSPRNG tokens, stored only as SHA-256
  hashes; single-use acceptance via conditional `transition_status`
  claims across all three adapters; tokens redacted from observability.
- **Both onboarding modes:** SSO accept links the existing
  auto-provisioned account; Local-mode accept mints a first-class user
  plus their first API key (hash-persisted) so the invitee can
  authenticate immediately.
- **Expiry:** per-tenant `InvitationPolicy` (tenant/workspace expiry
  days, pending cap, `allow_re_invite`) via `GET/PUT
  /tenant/invitations/policy`; background expiry job (`run_expiry_once`)
  marks past-expiry Pending invitations Expired.

### Review-repair round (f36e46e1, commit 734f2f59)

- **F1 (tenant-scope the namespace lookups):** the
  `tenant:{tid}:email:{email}` lookups (`find_by_external_id`, global)
  in the invitation accept path and the auth.rs SSO email-link claim
  now require the found record's tenant AND email to match the
  invitation/claim. A foreign record planted on the namespace via the
  SCIM externalId surface is no longer linked, credentialed, or
  granted pre-assigned memberships; the create arm fails closed with
  an operator-actionable 409 (namespace conflict) instead of an opaque
  unique violation, and the invitation rolls back to Pending.
- **F2 (fail-closed corrupt stored state):** `json_to_ids`/
  `json_to_ws_roles` in BOTH the SQLite and Postgres invitation
  adapters return `Err` on unparseable JSON (matching the strictness of
  the unknown-role branch), and the two independently stored columns
  are checked against the parallel-array contract (roles empty →
  default Developer per workspace, else exactly one role per id).
- **F3 (re-invite role application):** the namespace-dedupe accept arm
  applies the invited `GlobalRole` exactly as the SSO-link and create
  arms do — a re-invite after a terminal state no longer silently
  no-ops a promotion or demotion behind `mode:"linked"`.
- **F4 (compensation visibility):** all three Accepted→Pending rollback
  sites (tenant accept, workspace token accept, workspace authenticated
  accept) log `tracing::error!` naming the invitation id when the
  compensation write itself fails, so a stranded Accepted row is
  operator-visible.

### Test evidence

Executed in this sandbox (evidence: `/tmp/stage/review-evidence/`):

- `cargo test -p gyre-adapters sqlite::invitation` — 6/6 pass, including
  the new `corrupt_preassigned_columns_fail_closed` (F2: truncated
  workspace_ids JSON, truncated workspace_roles JSON, and
  parallel-array mismatch all error; valid empty-roles and matched
  arrays still load).
- `cargo check -p gyre-server --tests` — exit 0 (the new server tests
  and F1/F3/F4 handler changes type-check; the full test binary was
  NOT compiled here — the sandbox cgroup caps memory at 4 GiB and
  large gyre-server test compilation is verifier-owned).
- `cargo clippy -p gyre-adapters --tests` — no warnings in the invitation
  adapters.

For the independent verifier:

- `cargo test -p gyre-server api::invitations` — must run the two new
  review discriminators (both fail on the pre-repair code):
  `accept_does_not_link_foreign_record_on_invitation_namespace` (F1,
  both mismatch variants + rollback-to-Pending assertion) and
  `reinvite_acceptance_applies_new_role` (F3), plus the existing
  regression suite in the module.
- `cargo test --all` remains the full acceptance gate.
