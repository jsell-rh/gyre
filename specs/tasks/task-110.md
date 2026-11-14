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
commits: ["d1103854d71b5b8adec5f2c7e03d691f7e8dc059", "5df8f2b136b1ad5792b58269cb4dc778ec301b05", "e52fea4f3bfc0ce1a57f99b0db6c98f55fe14209", "6bef61b7f7c9d5ccf391a9514a9064b8888d10f5", "ea366b150438e6b185edcf812cab5c5b1384b77a", "797cf52f94910a8c8bb9a5ae3f4f59868a99d820", "c1de316bd3c1f9b2fd09689891f58f36abf95c66", "8c469c538bd84cf880cb72522855c66a7a8f7d4d", "0f4fea9aa717616b17ad1c76bc868d21906e22cc", "8f66d5a436ac2926fa8b12714abc34540a81d1fb"]
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

**This round: repair review finding F1 (attempt 6fd7a0e0) — the
invitee-facing workspace-invitation routes were unreachable in
production — plus the merge onto assignment base 9bfab365 (task-149
analytics).** No invitation behavior otherwise changed; the
implementation approved across prior rounds (domain entities, ports,
SQLite/Postgres/mem adapters, migrations 2026-11-13-000063/64/65, the
five tenant endpoints plus policy endpoints, workspace invitation
lifecycle, CSPRNG + SHA-256 tokens, per-tenant `InvitationPolicy`,
bulk invite, background expiry job, docs) carries over.

### Repairs this round

- **F1 (BLOCKING, code):** `POST /api/v1/workspaces/{id}/invite/accept`
  and `/invite/decline` were mapped as non-exempt workspace-resource
  routes in `abac_middleware.rs`, so the middleware derived
  `subject.member_of_resource_workspace=false` for the invited user
  (an invitation exists only for non-members) and the seeded builtin
  `workspace-membership-required` (Deny, priority 940) returned 403
  before the per-handler invitee check ran — the spec-named invitee
  endpoints and the `accept_url` embedded in the invitee's
  `InvitationReceived` notification were dead in real deployments.
  Fix: both routes are now `RouteResourceMapping::exempt`; per-handler
  authorization (workspace load, tenant containment 403,
  caller-must-be-the-invitee 404) was restructured into
  `authorize_my_pending_invitation` so the decision is visible in the
  exempt handlers' bodies — `check-abac-exempt-handlers.sh` passes (81
  handlers checked), `check-abac-route-registry.sh` passes.
- **Production-chain regression test**
  `invitee_accepts_and_declines_through_abac_middleware`: the real
  `api_router` layered with `abac_middleware` (as `build_router` does),
  seeded builtin policies, API-key invitee. A control assertion first
  proves the non-member invitee is still 403-denied on a
  workspace-MAPPED route (so the builtin is live in the fixture), then
  accept → 204 + membership activated, and decline → 204 +
  `Declined` + no membership. Without the exemption this test fails
  with 403 on both invitee routes.
- Merged assignment base 9bfab365 (task-149 analytics UI + retention)
  and resolved `specs/coverage/SUMMARY.md` by mechanical recount via
  `scripts/update-coverage-summary.sh` — this also fixes the round's
  non-blocking process finding (stale abac-policy-engine / agent-gates
  / analytics rows; the merged tree recounts to 191 assigned / 228
  implemented / 67%).
- Restored this task's `commits:` frontmatter (assignment reset had
  emptied it): the nine previously recorded SHAs (all re-verified
  in-branch) plus this round's fix `d1103854`.

### Verification evidence

- `cargo check -p gyre-server --tests` — OK (all edits type-checked,
  including the new regression test).
- `bash scripts/check-abac-route-registry.sh` — OK.
- `bash scripts/check-abac-exempt-handlers.sh` — OK (81 handlers).
- `bash scripts/update-coverage-summary.sh` — regenerated SUMMARY.md.

### Deferred to the independent verifier

(gyre-server test compilation exceeds the model-resident 4 GiB budget)

- `cargo test -p gyre-server invitee_accepts_and_declines_through_abac_middleware`
  (the new production-chain regression test).
- `cargo test -p gyre-server api::invitations` (full invitation
  module — the shared invitee lookup was restructured).
- `cargo test -p gyre-server builtin_workspace_membership_required_denies_non_member_live`
  (adjacent builtin-policy live test, unchanged).
- `cargo test --all` (task acceptance criterion; verifier-owned).
