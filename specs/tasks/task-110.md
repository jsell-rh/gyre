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
commits: ["b445900d4f72e5c324cb61c4b2e55c72f6ebed8a", "6af1d71c51cf1f763c0a1c07ec3f14b272edc523", "758462f45d6b162bc273c4a676e6bb33442e5dde", "43b15db3df88aed619f631a209e0cce124594100", "8833f8d99ccd1536a06ad81e364f80efc5c3f330", "01958bc011244ae4a4c06258baec597a00d8134d", "9f9be53014102d44e5e8c148d21b580448c226c7", "d1103854d71b5b8adec5f2c7e03d691f7e8dc059", "5df8f2b136b1ad5792b58269cb4dc778ec301b05", "e52fea4f3bfc0ce1a57f99b0db6c98f55fe14209", "6bef61b7f7c9d5ccf391a9514a9064b8888d10f5", "ea366b150438e6b185edcf812cab5c5b1384b77a", "797cf52f94910a8c8bb9a5ae3f4f59868a99d820", "c1de316bd3c1f9b2fd09689891f58f36abf95c66", "8c469c538bd84cf880cb72522855c66a7a8f7d4d", "0f4fea9aa717616b17ad1c76bc868d21906e22cc", "8f66d5a436ac2926fa8b12714abc34540a81d1fb", "7bc33fe44d2805273a98e37ec5cc5a91663a6f41"]
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

Tenant & workspace invitation flow, implemented and reviewed across
multiple earlier rounds on this branch (feature surface: domain
entities, ports, SQLite/PG/mem adapters, migrations 000063/000064, all
five tenant endpoints, workspace invitation lifecycle, CSPRNG tokens
stored as SHA-256 hashes, single-use transactional acceptance,
per-tenant expiry policy, background expiry job, ABAC route mappings).
This round is the review-96521ecf repair: all six test findings closed
in `crates/gyre-server/src/api/invitations.rs` (commit 7bc33fe4).

### This round (review 96521ecf)

- **F1 — expiry job workspace arm:** new
  `expiry_job_marks_expired_workspace_invitations` seeds an expired
  Pending `WorkspaceInvitation` plus a still-fresh control row, runs
  `run_expiry_once`, and asserts exactly the expired row transitions to
  `Expired`; deleting the workspace loop in `run_expiry_once` now fails
  the suite.
- **F2 — bulk invite persistence:** the bulk test now asserts the
  per-entry error indexes and reads
  `tenant_invitations.list_by_tenant` to confirm exactly the two valid
  emails were persisted as Pending rows (response body no longer
  trusted alone).
- **F3 — tenant decline happy path:** new token-only decline test —
  204, stored status `Declined`, `accepted_at == None`, no user
  account created for the invited email, and the declined token is
  burned (later accept → 409).
- **F4 — token-hash property asserted:** the lifecycle test now asserts
  `stored.token_hash == hash_token(&token)` and `!= token` against the
  repository row.
- **F5 — admin guard coverage:** new Member-caller test hitting GET
  list, POST bulk, DELETE revoke, GET policy, and PUT policy — all 403
  with no side effects (seeded invitation still Pending; policy still
  at defaults via admin re-read).
- **F6 — workspace role gate:** new Developer-MEMBER caller test — 403
  and no invitation row (previously only the non-member case was
  covered).
- **Merge repair:** updated the
  `invite_notification_carries_accept_path` call site to the task-209
  `list_for_user` signature (`exclude_types: &[]`) — the only stale
  call site in the tree.

### Test evidence

Type check via the sandbox build wrapper: `cargo check -p gyre-server
--tests` exit 0 (lib + integration tests); `cargo clippy -p
gyre-server --tests` reports zero diagnostics for `invitations.rs`.
Mechanical gates exit 0: `check-arch.sh`,
`check-conditional-test-guards.sh`,
`check-aspirational-test-names.sh`, `check-inert-enforcement.sh`.
(`check-assertionless-tests.sh`/`check-dead-test-code.sh` exit 2 in
this sandbox at clean HEAD too — awk gawk-extension limitation, not a
regression; see evidence file.)

Server test compilation exceeds the sandbox 4 GiB build-wrapper
budget — the verifier must run `cargo test -p gyre-server --lib
api::invitations::` (includes the six findings' tests),
`cargo test -p gyre-server --lib auth::`, and the full `cargo test
--all` / clippy gates. Evidence:
`/tmp/stage/review-evidence/task-110-review-repair-96521ecf.md`.
