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
commits: ["e52fea4f3bfc0ce1a57f99b0db6c98f55fe14209", "6bef61b7f7c9d5ccf391a9514a9064b8888d10f5", "ea366b150438e6b185edcf812cab5c5b1384b77a", "797cf52f94910a8c8bb9a5ae3f4f59868a99d820", "c1de316bd3c1f9b2fd09689891f58f36abf95c66", "8c469c538bd84cf880cb72522855c66a7a8f7d4d", "0f4fea9aa717616b17ad1c76bc868d21906e22cc", "8f66d5a436ac2926fa8b12714abc34540a81d1fb"]
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

**This round: merge onto the assigned base f7f5c59f (which carries task-125)
plus repair of the retained verification findings** (merge commit `1b961567`,
repair commit `e52fea4f`). The invitation implementation itself is unchanged
production code carried from the prior accepted rounds:

- **Merge resolution:** the only content conflict was the
  `specs/coverage/system/user-management.md` header — resolved as a union:
  task-110's implemented rows 5/6/7/25 alongside task-125's implemented rows
  28/29; coverage counts recomputed against the merged table (11/36: 11 n/a,
  14 task-assigned, 11 implemented, 0 verified — row statuses verified by
  counting the merged table).
- **Post-merge compile repair:** task-125 widened
  `NotificationRepository::list_for_user` with a `repo_id` filter; the
  invitation test call site now passes the new parameter (caught by clippy
  E0061 during repair verification, fixed).
- **Retained verification findings fixed** (`e52fea4f`):
  - `clippy::double_must_use` on the two invitation port traits — an
    async-trait + clippy-1.99 macro-expansion artifact (it fires on every
    `#[async_trait]` trait in the crate, including untouched ones like
    `agent.rs`); suppressed at the two new traits with a comment naming the
    cause, not by touching other tasks' traits.
  - `clippy::useless_format` (revoke conflict message), `clippy::clone_on_copy`
    (Copy `WorkspaceRole` at accept), `clippy::unnecessary_sort_by` (three mem
    invitation listings → `sort_by_key(Reverse(created_at))`).
  - rustfmt over every task-110-touched Rust file (the
    `check-rustfmt-diff.py` gate reports only the first 12 offending lines;
    the new files were formatted in a non-rustfmt style throughout).

### Verification (this round, at repair commit e52fea4f)

- `python3 scripts/check-rustfmt-diff.py f7f5c59f` — exit 0 (21 files clean).
- `cargo clippy --all-targets --all-features -- -W clippy::all` — exit 0,
  and a changed-line-diff replay of `check-clippy-diff.py` against base
  f7f5c59f reports zero gate failures (all targets compile — includes
  gyre-server lib + tests type checking).
- `cargo test -p gyre-domain invitation` — 4 passed, 0 failed.
- `cargo test -p gyre-adapters invitation` — 5 passed, 0 failed, including
  `migration_000064_dedupes_raced_pending_invitations_before_index` against
  real SQLite at the renumbered migration version.
- All AGENTS.md invariant scripts exit 0 on the repaired tree (arch, ABAC
  route registry + exempt handlers, MCP write tools, migration versions +
  SQL portability, dead message kinds, byte-slice truncation, relative path
  defaults, fail-open ref resolution, task commit attribution, mem port
  contracts, fabricated scope defaults, lossy secret conversion, scope
  literal defaults, inert enforcement, forged scope fields, forwarded header
  trust, in-memory state stores, unbounded external HTTP).
- Probe commands, exit codes, and outputs retained under
  `/tmp/stage/review-evidence/`.

### Tests for the independent verifier

- `cargo test -p gyre-server --lib api::invitations` (26 tests, incl.
  `invite_notification_carries_accept_path`).
- `cargo test --all` (full workspace).

### Shipped behavior (carried from prior rounds, unchanged by this repair)

- **Domain & storage:** `TenantInvitation`/`WorkspaceInvitation`/
  `InvitationStatus`/`InvitationPolicy` entities
  (`gyre-domain/src/invitation.rs`), port traits
  (`gyre-ports/src/invitation.rs`), SQLite + Postgres migrations (renumbered
  `2026-11-13-000063..000065` after the earlier rebase) and adapters, mem
  adapters. Tokens are CSPRNG-generated (ring `SystemRandom`, 32 bytes) and
  stored only as SHA-256 hashes; the raw token appears solely in the invite
  response.
- **Tenant invitations:** the five specced endpoints — single invite, bulk
  invite (array of invitations), list with status filter, revoke, magic-link
  accept/decline — with TenantAdmin-only authorization, per-tenant
  `InvitationPolicy` expiry/max-pending caps, and SSO-account linking on
  accept when the email already has an account (no shadow account).
- **Workspace invitations:** invite → pending → accept/decline/expire
  lifecycle with membership creation on accept, inviter-side listing,
  invitee-facing `GET /api/v1/users/me/invitations`, and per-workspace
  `max_pending_invitations` enforcement; the invitee notification is typed
  `InvitationReceived`.
- **Expiry:** background sweeper marks `Pending` invitations past
  `expires_at` as `Expired` for both invitation kinds, using the same
  `transition_status` conditional claim so a racing accept is never
  overwritten.
- **Routes & ABAC:** all routes registered in `api/mod.rs` with ABAC
  resolver mappings in `abac_middleware.rs`; no exemption-file entries.
