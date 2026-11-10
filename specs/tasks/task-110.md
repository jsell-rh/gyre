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
commits: ["797cf52f94910a8c8bb9a5ae3f4f59868a99d820", "c1de316bd3c1f9b2fd09689891f58f36abf95c66", "8c469c538bd84cf880cb72522855c66a7a8f7d4d", "0f4fea9aa717616b17ad1c76bc868d21906e22cc", "8f66d5a436ac2926fa8b12714abc34540a81d1fb"]
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

**Repair round 12 (requirements changed): merge the new base
`3d87a1507` into the retained task-110 checkpoint, merge commit
`bc4bc1090`.** The base carries the task-108/109/174 surface
(admin impersonation, spec-lifecycle configuration, explorer view
updates). The merge auto-resolved everywhere except one conflict in
`crates/gyre-server/src/api/mod.rs`: both sides had inserted a new
module declaration at the same alphabetical position (`invitations`
from task-110, `impersonation` from the base). Resolution keeps both
modules; the full route registrations, ABAC registry entries, and
lib.rs pre-auth magic-link router for both surfaces were verified
present at the merged head. No product behavior of the invitation
flow changed.

### Shipped behavior (carried from prior rounds, re-verified at this head)

- **Tenant invitations:** domain entities
  (`gyre-domain/src/invitation.rs`), ports
  (`gyre-ports/src/invitation.rs`), SQLite/Postgres migrations +
  adapters, and the five specced endpoints: single invite, bulk
  invite, list with status filter, revoke, magic-link
  accept/decline. Tokens are CSPRNG-generated and stored only as
  SHA-256 hashes. Per-tenant `InvitationPolicy` endpoints configure
  expiry; an hourly background job
  (`GYRE_INVITE_EXPIRY_INTERVAL_SECS`) marks past-due `Pending`
  invitations `Expired`.
- **Workspace invitations:** invite (by `user_id`, `username`, or
  `email`, per spec), list, revoke, token accept/decline, plus the
  authenticated invitee surface: `GET /api/v1/users/me/invitations`,
  `POST /api/v1/workspaces/{id}/invite/accept` and `/invite/decline`
  (caller must be the invited user; no pending invitation → 404).
  In-app notification to the invitee carries `kind:
  workspace_invitation` with `accept_url`/`decline_url`.
- **Expiry:** `InvitationPolicy` (tenant/workspace expiry days,
  `max_pending_invitations`, `allow_re_invite`) persisted per tenant
  via `GET/PUT /api/v1/tenant/invitations/policy`; corrupt policy row
  fails closed. Background job uses the same conditional
  `transition_status` claim as accept, so a racing accept is never
  overwritten; expired tokens are rejected on accept (409 → Expired).

### Test evidence (this head, merge commit `bc4bc1090`)

- `cargo test -p gyre-server --lib api::invitations` — 26 passed / 0
  failed (lifecycle, TenantAdmin guard, cross-tenant 403, token
  CSPRNG+hash, cap, decline, expiry, SSO-link, username collisions).
- `cargo test -p gyre-server --lib impersonation` — 15 passed / 0
  failed (merged-in base surface, conflict-adjacent).
- `cargo test -p gyre-server --lib spec_lifecycle` — 9 passed / 0
  failed (merged-in base surface).
- `cargo test -p gyre-adapters invitation` — 5 passed / 0 failed
  (roundtrips, pending-scope unique index, raced-duplicate dedupe,
  transition_status source-status guard).
- `cargo test -p gyre-server --lib` — 1657 passed / 0 failed
  (merge-validation: task-110 invitations coexist with the base's
  impersonation/spec-lifecycle route additions).
- `cargo test -p gyre-adapters` — 420 passed / 0 failed;
  `gyre-domain` 452, `gyre-common` 120, `gyre-cli` 150 — all green.
- `cargo check --workspace --all-targets` — exit 0.
- Mechanical guards all green: migration versions, ABAC route
  registry, ABAC-exempt handlers, arch, mem port contracts, MCP write
  tools, dead message kinds, scope literal defaults, fabricated scope
  defaults, inert enforcement, lossy secret conversion, forwarded
  header trust, in-memory state stores, unbounded external HTTP,
  migration SQL portability, forged scope fields, task commit
  attribution.
- Evidence retained at
  `/tmp/stage/review-evidence/task-110-rebase-round12.md`.
- web/dist untouched (executor restores from assignment base; remote
  rebuild owns publication).
