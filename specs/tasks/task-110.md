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
commits: ["5df8f2b136b1ad5792b58269cb4dc778ec301b05", "e52fea4f3bfc0ce1a57f99b0db6c98f55fe14209", "6bef61b7f7c9d5ccf391a9514a9064b8888d10f5", "ea366b150438e6b185edcf812cab5c5b1384b77a", "797cf52f94910a8c8bb9a5ae3f4f59868a99d820", "c1de316bd3c1f9b2fd09689891f58f36abf95c66", "8c469c538bd84cf880cb72522855c66a7a8f7d4d", "0f4fea9aa717616b17ad1c76bc868d21906e22cc", "8f66d5a436ac2926fa8b12714abc34540a81d1fb"]
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

**This round: rebase repair onto assignment base 4b9220bc** (durable finding
07881b6d, merge conflict in `crates/gyre-server/src/auth.rs`). Main advanced
past the previously reconciled head 1ee52c43 with task-135 (gate chains),
task-146 (analytics), task-011 (spec save write-through), task-129 (ABAC
evaluation engine), and task-147 (analytics decision API). No product code
changed beyond the one conflict resolution: the invitation flow is the
independently approved implementation from prior rounds (domain entities,
ports, SQLite/Postgres/mem adapters, migrations 2026-11-13-000063/64/65, the
five tenant endpoints, workspace invitation lifecycle, CSPRNG + SHA-256
tokens, per-tenant `InvitationPolicy`, bulk invite, background expiry job,
ABAC mappings), now merged with base 4b9220bc as merge commit 88355447.

### Conflict resolved (source only)

- `gyre-server/src/auth.rs` — `find_or_create_user_tenant` was hit from both
  sides: task-110 renamed `find_or_create_user` and added the invitation
  email-link (verified-email claim of a local-mode invitation account);
  task-129 added the reserved-system-identity provisioning rejection. The
  resolution keeps both: task-129's `preferred_username ==
  SYSTEM_ABAC_IDENTITY` fail-closed check runs *before* the invitation
  email-link, so a spoofed reserved handle can drive neither new-user
  provisioning nor invitation-account claiming. Task-129's
  existing-user `display_name` check and its
  `jwt_rejects_reserved_system_identity_provisioning` test auto-merged
  intact; the reserved-identity ordering is enforced by control flow (the
  bail precedes the link block).

### Merge-integrity and gate evidence (this head)

- `cargo check -p gyre-server` passes (7m19s, 4 GiB sandbox).
- `cargo fmt --all --check` clean.
- Mechanical checks pass: architecture lint, migration versions (base's
  20261008000063 analytics_scope_columns and 20261112000062 gate_chain_config
  are Diesel-distinct from task-110's 20261113000063/64/65), migration SQL
  portability, ABAC route registry, ABAC exempt handlers, MCP write tools,
  mem port contracts, and the remaining invariant lints (full sweep log in
  `/tmp/stage/review-evidence/gate-*.log`).
- Focused tests: `cargo test -p gyre-adapters` 532 passed / 0 failed —
  includes the merged `migration_000058_backfills...` exclusion-set test
  (both task-110's m65 and task-112's m60 exclusions kept), the SQLite
  invitation repositories, and the mem-port contract tests;
  `-p gyre-domain -p gyre-ports -p gyre-common` 671 passed / 0 failed.
- gyre-server test compilation is deferred to the independent verifier
  (4 GiB cgroup build budget, environment restriction, not a defect). The
  verifier must run the full workspace suite — in particular the 26
  `api::invitations` tests (tenant lifecycle, revocation blocks accept,
  expiry job marks expired, bulk invite, non-admin 403, local-mode accept
  mints an authenticating API key, re-invite fresh key, SSO link, workspace
  lifecycle, cross-tenant containment, `max_pending` cap, `allow_re_invite`)
  plus `auth::tests::jwt_rejects_reserved_system_identity_provisioning`
  (task-129, exercises the resolved conflict region) and task-110's
  SSO-link auth tests in the same module.

Coverage rows 5, 6, 7, and 25 in
`specs/coverage/system/user-management.md` remain `implemented` and describe
the merged tree accurately; the audit header records this repair merge.
