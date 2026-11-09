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
commits: ["8c469c538bd84cf880cb72522855c66a7a8f7d4d", "0f4fea9aa717616b17ad1c76bc868d21906e22cc", "8f66d5a436ac2926fa8b12714abc34540a81d1fb"]
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

**Repair round 6: rebase of the task-110 branch onto upstream `fefde2946`
(upstream task-089 HSI Explorer Generated Views + task-103 secrets
surface / JWT tenant-claim schema default), committed as `0b8d68602`.**

The merge left two conflicts, both resolved by combining the two
sides' intents:

- **`crates/gyre-server/src/auth.rs` (JwtClaims):** upstream task-103
  replaced the use-site `tenant_id` fallback with the claim's serde
  schema default (`tenant_id: String` + `default_tenant_claim()`);
  task-110 added the `email_verified` claim used by
  `find_or_create_user_tenant` to claim invitation-created local-mode
  accounts. The merged struct keeps both — schema-default `String`
  tenant_id plus `email_verified: Option<bool>` — which is exactly the
  shape the auto-merged usage sites already assume
  (`claims.tenant_id` moved as `String`; `claims.email_verified
  .unwrap_or(false)`).
- **`scripts/fabricated-scope-defaults-exemptions.txt`:** took
  upstream's removal of the auth.rs JWT-fallback entry (count 4→3,
  matching the auto-merged `FROZEN_EXEMPTION_COUNT=3` in the check
  script). The candidate's re-anchored entry (773→777) is dead at the
  merged tree because the use-site fallback no longer exists; no new
  entries were added. `check-fabricated-scope-defaults.sh` exits 0.

web/dist assets ride the upstream merge unchanged; the executor
restores generated assets from the assignment base.

Evidence log: `/tmp/stage/review-evidence/task-110-repair-round-6.md`.

### Shipped behavior (task-110, prior rounds)

- **Tenant invitations:** domain entities
  (`gyre-domain/src/invitation.rs`), ports
  (`gyre-ports/src/invitation.rs`), SQLite/Postgres migration
  `2026-10-08-000056_invitations` + adapters, and the five specced
  endpoints: single invite, bulk invite, list with status filter,
  revoke, magic-link accept/decline. Tokens are CSPRNG-generated and
  stored only as SHA-256 hashes. Per-tenant `InvitationPolicy`
  endpoints configure expiry; an hourly background job
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
- **Docs:** `docs/api-reference.md` covers all invitation routes and
  selectors; `docs/server-config.md` documents
  `GYRE_INVITE_EXPIRY_INTERVAL_SECS`.

### Test evidence (re-run at rebased head `0b8d68602`, this round)

- `cargo check --all` (SKIP_WEB_BUILD=1) at the merged tree — clean.
- `cargo test -p gyre-server --lib auth::` — 47 passed / 0 failed
  (upstream task-103 tenant-claim schema-default tests plus task-110
  invitation-link tests, including the `email_verified`
  claim-link cases, through the full routing chain).
- `cargo test -p gyre-server --lib api::invitations` — 18 passed / 0
  failed.
- `cargo test -p gyre-adapters secret` — 17 passed / 0 failed
  (upstream task-103 secret surface at the merged tree).
- Mechanical guards at the rebased head: `check-fabricated-scope-
defaults`, `check-non-atomic-creation`, `check-arch`,
`check-abac-route-registry`, `check-mcp-write-tools`,
`check-migration-versions`, `check-task-commit-attribution`,
`check-inert-enforcement`, `check-lossy-secret-conversion`,
`check-mem-port-contracts`, `check-forwarded-header-trust`,
`check-unbounded-external-http`, `check-fail-open-ref-resolution`,
`check-byte-slice-truncation`, `check-relative-path-defaults`,
`check-dead-message-kinds`, `check-in-memory-state-stores`, and
`check-abac-exempt-handlers` — all exit 0.
- `check-dependent-scope-resolution.sh` fails at the rebased head with
  2 findings (`maybe_advance_rolling_rollout`, task-157;
  `recent_mrs_with_traces`, task-089) — but both fail identically at
  the upstream base `fefde2946` itself (verified by checking out the
  base and re-running; finding sets identical, only the scanned-
  function count differs from task-110's added functions). Pre-existing
  mainline conditions inherited verbatim by the rebase, not introduced
  or modified by task-110; left for upstream-side fixes rather than
  editing approved task-089/157 logic on this branch.
- Loopback TCP listeners are permitted in this sandbox and the
  in-process HTTP-layer tests above exercise the full routing chain;
  a separate live-server smoke remains for host verification / CI at
  the PR head.
