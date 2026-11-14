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

**This round: CI repair only — no code changed.** The durable finding
(7ec83a8d, category `ci`) recorded run 43610196702 at PR head a8996690
failing the `Development controller` job with "logs not yet available".
Diagnosis: the job's only failed step was `python3 -m unittest discover -s
scripts -p 'test_pipeline*.py'`; the candidate tree's `scripts/` is
byte-identical to the assignment base 44cf25bf (which passed the same job
in run 43602842046) except for one comment line in
`warn-continue-creation-exemptions.txt`, and the suite is hermetic
(temp-dir stores/fixtures). The two failures reproducible in this sandbox
(`test_real_timeout_kills_check...`, `test_crash_cleanup_stops_durable_agent...`)
are this sandbox's signal-policy restriction (`os.killpg` → EPERM) and
reproduce identically at the assignment base — environment evidence, not
defects. A re-run of the failed job at the **exact same head**
(`gh run rerun 43610196702 --failed`, attempt 2) passed every job including
`Development controller` and `Test` — the original failure was a runner
flake. Evidence retained in `/tmp/stage/review-evidence/`.

### Shipped behavior (carried, independently approved in prior rounds)

- Domain entities (`gyre-domain/src/invitation.rs`): `TenantInvitation`,
  `WorkspaceInvitation`, `InvitationStatus`, per-tenant `InvitationPolicy`.
- Ports (`gyre-ports/src/invitation.rs`): `TenantInvitationRepository`,
  `WorkspaceInvitationRepository`; SQLite/Postgres/mem adapters with
  migrations 2026-11-13-000063/64/65 (shared dir, both dialects).
- All five tenant endpoints (`POST /tenant/invite`, `/tenant/invite/bulk`,
  `GET /tenant/invitations`, `DELETE /tenant/invitations/{id}`,
  `POST /invite/{token}/accept`) plus the workspace invitation lifecycle
  (`POST /workspaces/{id}/invite`, list, accept, decline, revoke), CSPRNG
  tokens stored as SHA-256 hashes, local-mode accept minting a first API
  key, SSO account linking, background expiry job
  (`spawn_invitation_expiry`), ABAC route mappings.

### Verification evidence (this head)

- Exact-head GitHub CI: run 43610196702 attempt 2 — all jobs success
  (Development controller, Test, Clippy, Format, Architecture lint, Web
  build, agent runner tests).
- `cargo check -p gyre-server` and `cargo check -p gyre-server --all-targets`
  pass in this sandbox (4 GiB budget).
- `cargo test -p gyre-adapters` 533 passed / 0 failed; `-p gyre-domain
  -p gyre-ports` 547+8 passed / 0 failed (includes invitation repositories,
  migrations, mem-port contracts).
- Mechanical checks pass: architecture, migration versions, migration SQL
  portability, ABAC route registry, ABAC exempt handlers, MCP write tools,
  mem port contracts, dead message kinds, byte-slice truncation, relative
  path defaults, fail-open ref resolution, fabricated scope defaults, lossy
  secret conversion, scope literal defaults, inert enforcement, forwarded
  header trust, in-memory state stores, unbounded external HTTP.
- Node suites (`scripts/dashboard/test`, `scripts/test`) pass locally.
- gyre-server test binaries exceed the sandbox build budget; the
  independent verifier owns the full workspace suite — notably the 26
  `api::invitations` tests and task-124's `api::users` tests in the merged
  region.
