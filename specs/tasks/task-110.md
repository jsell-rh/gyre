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
commits: ["030354c64674fb570e4dd85cb5acf9a3ac8b93a5", "145f499b11993781082524a95e1119b054854c31", "ba97de27786a8a2758e29ff282597f55c2755c9b", "4345788f601038d4cbcca896ae1e4c4cf3cf3c72", "9a8e75c1ab97d4c4d8c711ad85f4448116c55eac", "5320a545aaa72b039d63aee37369e2bfa9c61c09"]
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

**Contract-repair round (repair finding 0ab8eb07).** The prior candidate
(31df7d24) had edited the assigned contract region — the 10 acceptance
criteria checkboxes were ticked `[ ]` → `[x]` and a non-assigned commit
(727a7b49) was prepended to `commits:`. Per the assigned contract those
edits are not sanctioned (the criteria belong to the reviewer's verdict,
and 727a7b49 is a pipeline checkpoint, not task work), so this round
restored the contract region byte-for-byte to the assigned body and set
only the lifecycle fields an implementer may set: `progress` and the
`commits:` attribution (the five assigned implementation commits plus the
task-labeled docs commit 5320a545). No code changes were needed and none
were made — the lineage already carries the full implementation below.

Shipped behavior (branch lineage, verified this round by reading every
file listed; test evidence at the end):

- **Domain** (`gyre-domain/src/invitation.rs`): `TenantInvitation`,
  `WorkspaceInvitation`, `InvitationStatus`
  (Pending/Accepted/Declined/Expired/Revoked), `InvitationPolicy` with
  the spec defaults (7/7-day expiry, 50 max pending, re-invite allowed).
  Field-for-field match with the user-management.md §Tenant-Level User
  Onboarding struct.
- **Ports** (`gyre-ports/src/invitation.rs`):
  `TenantInvitationRepository` (create/find_by_id/find_by_token_hash/
  list_by_tenant/list_by_status/update_status/delete) and
  `WorkspaceInvitationRepository` (same surface keyed by workspace/user).
  The duplicate-pending contract is documented on the trait and enforced
  by every adapter.
- **Adapters**: shared migration `2026-10-08-000056_invitations` creating
  `tenant_invitations` and `workspace_invitations` (portable SQL running
  on both SQLite and PostgreSQL; unique index on token_hash; JSON TEXT
  columns for workspace ids/roles), diesel `schema.rs` rows, and real
  query implementations in `sqlite/invitation.rs` and
  `postgres/invitation.rs`; the server's mem adapter
  (`mem.rs`) implements both traits with the same contract guards.
- **API** (`gyre-server/src/api/invitations.rs`, 11 routes): the five
  spec'd tenant endpoints — POST /api/v1/tenant/invite (single),
  POST /api/v1/tenant/invite/bulk (per-entry partial success with
  created/errors split), GET /api/v1/tenant/invitations?status= (filter),
  DELETE /api/v1/tenant/invitations/{id} (revoke → Revoked, row kept),
  POST /api/v1/invite/{token}/accept (magic-link) — plus
  POST /api/v1/invite/{token}/decline, workspace
  POST/GET/DELETE invite/invitations, and token-based workspace
  accept/decline. Tokens are 32 bytes from the system CSPRNG (ring),
  hex-encoded, persisted only as SHA-256; acceptance is single-use;
  touching an expired pending invitation persists Expired immediately.
  Accept creates the local-mode user with a tenant+email-namespaced
  external_id (same email can exist in different tenants), carries the
  invited GlobalRole, and activates pre-assigned memberships guarded by
  tenant containment (cross-tenant workspace ids are skipped with a
  warning, never silently granted). Workspace invite enforces
  Owner/Admin-or-TenantAdmin, requires a same-tenant invitee, enforces
  `max_pending_invitations` (429) and duplicate-pending (409), and sends
  the in-app notification.
- **Routing/authorization**: management routes registered in
  `api/mod.rs` with `RouteResourceMapping` ABAC entries in
  `abac_middleware.rs` plus per-handler enforcement
  (`require_tenant_admin`, workspace-role checks, tenant containment) —
  the magic-link routes are registered in `lib.rs` outside
  `require_auth_middleware` because the invitee has no credentials yet
  (the 256-bit token is the auth factor), with bearer-identity matching
  when an identity IS presented.
- **Background job**: `spawn_invitation_expiry` (main.rs) runs
  `run_expiry_once` hourly (`GYRE_INVITE_EXPIRY_INTERVAL_SECS` override,
  first cycle at startup) marking Pending invitations past `expires_at`
  as Expired without deleting rows.
- **Documentation** (5320a545): the invitation routes were added to
  `docs/api-reference.md` and the expiry-job env var to
  `docs/server-config.md`.

Test evidence:

- Prior rounds at this lineage (recorded before the contract repair):
  `cargo test -p gyre-server --lib api::invitations` 9 passed / 0 failed
  (full lifecycle, revoke-blocks-accept, expiry on accept + job marks
  untouched seeds, bulk partial success, non-admin 403, workspace
  lifecycle with role + single-use, decline leaves no membership, email
  validation, CSPRNG token shape + SHA-256 known answer);
  `cargo test -p gyre-adapters invitation` 2 passed (real temp-file
  SQLite through migration 000056); mechanical checks (abac-route-registry,
  migration-versions, migration-sql-portability, mem-port-contracts,
  arch, abac-exempt-handlers, in-memory-state-stores, inert-enforcement,
  scope-literal-defaults, byte-slice-truncation, lossy-secret-conversion,
  fail-open-ref-resolution, forwarded-header-trust) all OK.
- This round, at this exact head (evidence files under
  /tmp/stage/review-evidence/, log: task-110-contract-repair.md):
  `cargo test -p gyre-server --lib api::invitations` 9 passed / 0 failed
  (task-110-server-invitations.txt), `cargo test -p gyre-adapters
  invitation` 2 passed / 0 failed (task-110-adapters-invitation.txt),
  `cargo test --offline -p gyre-domain --lib invitation` 4 passed /
  0 failed (task-110-domain-invitation.txt). The initial cargo runs
  failed because index.crates.io was briefly unreachable (transient
  sandbox network); retries succeeded and the recorded results above are
  from the completed runs. No TCP listener is bindable in this sandbox
  (accept() errno 95, /tmp/stage/capabilities.json), so the live-server
  HTTP smoke (invite → accept → re-use 409 → revoke-blocks-accept →
  bulk → list, exact curl steps recorded in the evidence log) remains
  for host verification / CI at the PR head.
