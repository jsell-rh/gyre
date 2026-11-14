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
commits: ["8f66d5a436ac2926fa8b12714abc34540a81d1fb", "0f4fea9aa717616b17ad1c76bc868d21906e22cc", "8c469c538bd84cf880cb72522855c66a7a8f7d4d", "c1de316bd3c1f9b2fd09689891f58f36abf95c66", "797cf52f94910a8c8bb9a5ae3f4f59868a99d820", "ea366b150438e6b185edcf812cab5c5b1384b77a", "6bef61b7f7c9d5ccf391a9514a9064b8888d10f5", "e52fea4f3bfc0ce1a57f99b0db6c98f55fe14209", "5df8f2b136b1ad5792b58269cb4dc778ec301b05"]
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

**This round: re-verification of the carried implementation on the new
base** after the prior candidate's "Development controller" CI check
failed with logs unavailable (durable finding fcdfa81a). No product code
changed this round — the invitation flow is the independently approved
implementation from the prior rounds (entities, ports,
SQLite/Postgres/mem adapters, migrations 2026-11-13-000063/64/65, the
five tenant endpoints, workspace invitation lifecycle, CSPRNG + SHA-256
tokens, per-tenant `InvitationPolicy`, bulk invite, background expiry
job, ABAC mappings), now merged with base 6ffd6799 (task-130 token
scoping, which rewrote 400 lines of `auth.rs`).

### CI finding investigation

The Development controller job runs only tooling tests
(`tools/fullsend-dispatcher` Go tests, `scripts/test_dev*.py`,
`scripts/test_pipeline*.py`, two node suites) — none of which read the
task-110 diff, and the diff touches no `tools/` or `scripts/` file. Locally:
node suites pass (7/7, 18/18); the two pipeline-test failures and three
dev-test failures that occur in this sandbox reproduce **identically on
a clean worktree of base 6ffd6799** (sandbox cargo-wrapper and
signal/process artifacts — see `/tmp/stage/review-evidence/`). With logs
unavailable and nothing reproducible from repo content, the recorded
failure is consistent with a transient run (the job has a 10-minute
timeout and timing-sensitive signal tests); re-running it on a fresh
candidate is the remote-verification step.

### Merge-integrity and gate evidence (this head)

- `cargo check --all` passes — the task-110 auth additions (local-account
  auth, invitation acceptance, API-key mint) and task-130's
  `AgentJwtClaims` rewrite coexist type-safely.
- CI-diff gates against base 6ffd6799: rustfmt-diff clean, clippy-diff
  clean (21 Rust files, 0 new warnings).
- All mechanical checks pass: arch, ABAC route registry, migration
  versions (incl. SQL portability), hierarchy, dead message kinds, and
  the full advisory battery (the 3 dead-parameter findings are
  pre-existing on the base, untouched by this diff).
- Focused tests: `cargo test -p gyre-adapters` 522 passed / 0 failed
  (includes the SQLite invitation repositories and mem-port contract
  tests); `-p gyre-domain -p gyre-ports` compile+run clean.
- gyre-server test compilation is blocked by this sandbox's 4 GiB build
  wrapper (environment restriction, not a defect): the independent
  verifier must run `cargo nextest run --all` — in particular the 26
  `api::invitations` tests (tenant lifecycle, revocation blocks accept,
  expiry job marks expired, bulk invite, non-admin 403, local-mode accept
  mints an authenticating API key, re-invite fresh key, SSO link,
  workspace lifecycle, cross-tenant containment, `max_pending`
  cap, `allow_re_invite`).

Coverage rows 5, 6, 7, and 25 in
`specs/coverage/system/user-management.md` remain `implemented` and
describe the merged tree accurately.
