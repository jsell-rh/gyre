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

**This round: rebase repair onto current main head 1ee52c43** (durable
finding 08005f87, merge_state DIRTY — the published PR head 6594708e
conflicted with main after task-112 notification delivery channels and
task-125 API rows landed). No product code changed: the invitation flow
is the independently approved implementation from prior rounds (domain
entities, ports, SQLite/Postgres/mem adapters, migrations
2026-11-13-000063/64/65, the five tenant endpoints, workspace
invitation lifecycle, CSPRNG + SHA-256 tokens, per-tenant
`InvitationPolicy`, bulk invite, background expiry job, ABAC mappings),
now merged with main 1ee52c43 as merge commit 104fc5e3.

### Conflicts resolved (source only)

- `gyre-adapters/src/sqlite/mod.rs` — the `migration_000058_backfills_
  unique_url_safe_usernames` test's exclusion set: both task-110's
  `2026-11-13-000065_user_tenant_scope` (indexes 000058's tenant_id
  column, so it cannot run pre-000058; runs in its deployed batch
  position right after 000058) and task-112's
  `2026-11-09-000060_user_preferences_channel_shape` (rewrites 000058's
  flat backfill literal; runs after 000058 followed by the convergence
  assert) are excluded from `pre` and run explicitly.
- `specs/coverage/system/user-management.md` — header recomputed against
  the auto-merged table: rows 5/6/7/25 (task-110) and 16/17/18
  (task-112) are all implemented in the merged tree; counts 14/36
  (11 n/a, 0 not-started, 11 task-assigned, 14 implemented, 0 verified).
- `web/dist` took main's side verbatim (assignment-base state); no
  rebuild — final integration regenerates and publishes those assets.

### Merge-integrity and gate evidence (this head)

- `cargo check --all` passes — the task-110 invitation/auth additions
  and task-112's `NotificationDispatcher`/channel-preferences wiring
  coexist type-safely.
- `cargo fmt --all --check` clean; `cargo clippy -p gyre-adapters`
  reports zero warnings in the hand-resolved file (remaining warnings
  are pre-existing on both parents).
- Mechanical checks pass: architecture lint, migration versions (no new
  duplicate Diesel versions — 20261113000063/64/65 vs main's
  20261109000060/61 are distinct), migration SQL portability, ABAC
  route registry, ABAC exempt handlers.
- Focused tests: `cargo test -p gyre-adapters` 524 passed / 0 failed —
  includes the merged `migration_000058_backfills...` (pre + 000058 +
  m65 + m60 ordering), `migration_000060_rewrites...`, the SQLite
  invitation repositories, and the mem-port contract tests;
  `-p gyre-domain -p gyre-ports -p gyre-common` 646 passed / 0 failed.
- gyre-server test compilation is blocked by this sandbox's 4 GiB build
  wrapper (environment restriction, not a defect): the independent
  verifier must run `cargo nextest run --all` — in particular the 26
  `api::invitations` tests (tenant lifecycle, revocation blocks accept,
  expiry job marks expired, bulk invite, non-admin 403, local-mode accept
  mints an authenticating API key, re-invite fresh key, SSO link,
  workspace lifecycle, cross-tenant containment, `max_pending` cap,
  `allow_re_invite`) and task-112's notification-dispatcher tests.

Coverage rows 5, 6, 7, and 25 in
`specs/coverage/system/user-management.md` remain `implemented` and
describe the merged tree accurately.
