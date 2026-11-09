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
commits: ["8c469c538bd84cf880cb72522855c66a7a8f7d4d", "0f4fea9aa717616b17ad1c76bc868d21906e22cc", "8f66d5a436ac2926fa8b12714abc34540a81d1fb", "c1de316bd3c1f9b2fd09689891f58f36abf95c66"]
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

**Repair round 7 (review findings F1/F2 from checkpoint `04455398`),
committed as `c1de316bd`.**

- **F1 — `allow_re_invite` is now enforced, not just stored.**
  `create_tenant_invitation` and `invite_to_workspace` reject with 409
  when the per-tenant policy sets `allow_re_invite: false` and the
  (tenant, email) / (workspace, user) already has an invitation in a
  terminal state (Accepted/Declined/Expired/Revoked); the default
  (`true`) keeps the spec's re-invite-with-fresh-token flow. Pending
  duplicates keep their pre-existing 409. Two HTTP-layer tests cover
  both surfaces; both were verified to fail when their check is
  removed. `docs/api-reference.md` now documents the policy endpoints
  (`GET/PUT /api/v1/tenant/invitations/policy`) and the re-invite 409
  semantics.
- **F2 — pending-scope uniqueness is enforced by the database.**
  Migration `2026-11-08-000059_invitation_pending_unique` adds partial
  unique indexes `ON tenant_invitations (tenant_id, email) WHERE
  status = 'Pending'` and the `(workspace_id, user_id)` workspace
  variant (plain portable SQL — correlated-EXISTS dedupe + partial
  index, runs identically on SQLite and PostgreSQL; the dedupe marks
  superseded raced duplicates `Expired` so `CREATE UNIQUE INDEX`
  cannot fail on a deployment that already raced). The SQLite and
  PostgreSQL adapters keep the friendly pre-check and map the unique
  violation to the same port-contract error as the race backstop; the
  mem adapter already enforced the contract atomically under its
  store lock. Terminal statuses never conflict with the partial
  index, so re-invite after expiry/revocation is unaffected.

Evidence log: `/tmp/stage/review-evidence/task-110-repair-round-7.md`.

### Shipped behavior (task-110, prior rounds)

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
- **Single-use race safety:** `transition_status(id, from, to,
  accepted_at)` enforces conditional status claims inside the write
  (SQLite/Postgres conditional `UPDATE`, mem check-and-set); accept
  claims Pending→Accepted before creating user/memberships, decline,
  revoke, and expiry use the same transition, so terminal statuses
  are never overwritten by racing writers.
- **Authorization:** all routes registered in `api/mod.rs` with ABAC
  resolver mappings and per-handler enforcement; HTTP-layer negative
  tests prove non-admins get 403 (tenant invite) and non-members get
  403 (workspace invite) through the full routing chain.
- **SSO/local account linking (round 4):**
  `UserRepository::claim_external_id` is the single sanctioned
  external_id transition (SQLite/Postgres/mem); the auth email-link
  path claims through it after external_id match and verified
  same-tenant email, and stamps `last_login_at` per upstream's
  every-login contract.
- **Rebase (round 6):** rebased onto upstream `fefde2946`
  (JwtClaims merged with both task-103's schema-default tenant claim
  and task-110's `email_verified`; exemption file took upstream's
  count).
- **Docs:** `docs/api-reference.md` covers all invitation routes,
  selectors and the policy endpoints; `docs/server-config.md`
  documents `GYRE_INVITE_EXPIRY_INTERVAL_SECS`.

### Test evidence (round 7, at head `c1de316bd`)

- `cargo test -p gyre-adapters invitation` — 5 passed / 0 failed,
  including the two new adapter tests:
  `pending_scope_unique_index_enforced_by_database` (raw-SQL
  duplicate inserts against the real index; asserts the
  pending-scope violation is recognized and the token_hash
  violation is NOT misclassified) and
  `migration_000059_dedupes_raced_pending_invitations_before_index`
  (drives the real embedded up.sql against a seeded raced database;
  asserts superseded duplicates → `Expired`, newest per scope survives
  `Pending`, and the index rejects new duplicates).
- `cargo test -p gyre-adapters` (full) — 390 passed / 0 failed /
  12 ignored (includes the duplicate-migration-version and
  migrations-create-tables guards).
- `cargo test -p gyre-server --lib api::invitations` — 20 passed /
  0 failed, including the two new F1 tests
  (`allow_re_invite_false_blocks_tenant_reinvite_after_revoke`,
  `allow_re_invite_false_blocks_workspace_reinvite_after_revoke`).
  Both were separately verified to fail when their policy check is
  disabled (check removed → test fails), then restored.
- `cargo check --all` (SKIP_WEB_BUILD=1) — clean; `cargo clippy -p
  gyre-adapters --all-targets` and `-p gyre-server --lib` show no
  warnings in the changed invitation files.
- Mechanical guards at the round-7 head: `check-migration-versions`,
  `check-migration-sql-portability`, `check-mem-port-contracts`,
  `check-arch`, `check-non-atomic-creation`,
  `check-abac-route-registry`, `check-inert-enforcement`, and
  `check-fabricated-scope-defaults` — all exit 0.
- Known pre-existing (inherited from upstream base `fefde2946`,
  verified identical there in round 6):
  `check-dependent-scope-resolution.sh` reports 2 findings in
  task-089/157 code; left for upstream-side fixes.
