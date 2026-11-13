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
commits: ["797cf52f94910a8c8bb9a5ae3f4f59868a99d820", "c1de316bd3c1f9b2fd09689891f58f36abf95c66", "8c469c538bd84cf880cb72522855c66a7a8f7d4d", "0f4fea9aa717616b17ad1c76bc868d21906e22cc", "8f66d5a436ac2926fa8b12714abc34540a81d1fb", "ea366b15c9e482b9b8ad2d54ec3a43252a2ac1e6"]
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

**Repair round 14 (review F1): the workspace-invite notification is now
typed `InvitationReceived`.** `invite_to_workspace` had constructed the
invitee's in-app notification as `NotificationType::TrustSuggestion`, so
the invitee's Inbox (web `Inbox.svelte`) classified every workspace
invitation as a trust-policy suggestion. Commit `ea366b15` constructs it
with `NotificationType::InvitationReceived` (user-management.md
§Notification Entity: "A user was invited to a workspace", matching the
sibling `invite_member` handler) and extends
`invite_notification_carries_accept_path` to assert `notification_type`
is `InvitationReceived` and not `TrustSuggestion` — the old code fails
that assertion, so the mis-typing cannot silently return. Type-checked
with `cargo check -p gyre-server --lib --tests` (exit 0); the gyre-server
test build exceeds this sandbox's 4 GiB budget and is recorded for the
independent verifier:
`cargo test -p gyre-server --lib api::invitations::tests::invite_notification_carries_accept_path`.

### Shipped behavior (carried from prior rounds, unchanged this round)

- **Domain & storage:** `TenantInvitation`/`WorkspaceInvitation`/
  `InvitationStatus`/`InvitationPolicy` entities
  (`gyre-domain/src/invitation.rs`), port traits
  (`gyre-ports/src/invitation.rs`), SQLite + Postgres migrations and
  adapters, mem adapters. Tokens are CSPRNG-generated and stored only as
  SHA-256 hashes; the raw token appears solely in the invite response.
- **Tenant invitations:** the five specced endpoints — single invite,
  bulk invite, list with status filter, revoke, magic-link
  accept/decline — with TenantAdmin-only authorization, per-tenant
  `InvitationPolicy` expiry/max-pending caps, and SSO-account linking on
  accept when the email already has an account.
- **Workspace invitations:** invite → pending → accept/decline/expire
  lifecycle with membership creation on accept, inviter-side listing,
  invitee-facing `GET /api/v1/users/me/invitations`, and per-workspace
  `max_pending_invitations` enforcement.
- **Expiry:** background sweeper marks `Pending` invitations past
  `expires_at` as `Expired` for both invitation kinds.
- **Routes & ABAC:** all routes registered in `api/mod.rs` with ABAC
  resolver mappings in `abac_middleware.rs`; no exemption-file entries.
