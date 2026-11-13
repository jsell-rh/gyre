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
commits: ["6bef61b7f7c9d5ccf391a9514a9064b8888d10f5", "ea366b150438e6b185edcf812cab5c5b1384b77a", "797cf52f94910a8c8bb9a5ae3f4f59868a99d820", "c1de316bd3c1f9b2fd09689891f58f36abf95c66", "8c469c538bd84cf880cb72522855c66a7a8f7d4d", "0f4fea9aa717616b17ad1c76bc868d21906e22cc", "8f66d5a436ac2926fa8b12714abc34540a81d1fb"]
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

**Rebase repair (review F1): task-110 merged onto the assigned main head
f764bc00** (merge commit `46f49834`, recorded in `commits:` above). The
candidate lineage sat on merge-base 33cd786c while main advanced 1433+
commits; the merge resolves the six content conflicts as unions —
task-110's invitation modules, exports, routes, and doc rows alongside
main's budget-enforcement, repo-access, team, and ownership-transfer
additions — and repairs the structural blocker:

- **Migration renumbering:** the task's Diesel migrations collided with
  main's taken versions (only one migration runs per version,
  alphabetically first). `invitations` 20261010000058 →
  `2026-11-13-000063` (main's `budget_enforcement_action` owns
  20261010000058), `invitation_pending_unique` 20261108000061 →
  `2026-11-13-000064` (main's `spec_links` owns 20261108000061),
  `user_tenant_scope` 20261108000060 → `2026-11-13-000065` (kept
  contiguous with the task's sequence; ordering preserved — the unique
  index migration still sorts after the table migration it indexes).
  Each up/down.sql carries a renumber note; fresh databases now get the
  `tenant_invitations`/`workspace_invitations` tables and both
  pending-scope partial unique indexes.
- **Test updates for the renumbered versions:** the migration-partition
  tests now target the new version strings —
  `migration_000064_dedupes_raced_pending_invitations_before_index`
  (renamed from 000061) and
  `migration_000058_backfills_unique_url_safe_usernames` (excludes
  20261113000065 instead of 20261108000060).
- **web/dist** resolved to the assignment base's assets (regeneration is
  owned by the executor/remote integration, not this assignment).
- **Coverage matrix** (`specs/coverage/system/user-management.md`)
  recomputed against the merged table: 9/36 (11 n/a, 0 not-started,
  16 task-assigned, 9 implemented) — task-110's rows 5/6/7/25 stay
  implemented on top of main's task-122/task-123 rows.

### Verification (this round, at merge commit 46f49834)

- `cargo check --all --all-targets` — exit 0 (covers gyre-server lib +
  tests type checking; the full server test build exceeds this
  sandbox's 4 GiB budget and is left to the independent verifier).
- `cargo test -p gyre-domain invitation` — 4 passed, 0 failed.
- `cargo test -p gyre-adapters invitation` — 5 passed, 0 failed,
  including `migration_000064_dedupes_raced_pending_invitations_before_index`
  against real SQLite at the renumbered version;
  `migration_000058_backfills_unique_url_safe_usernames` and
  `no_duplicate_migration_version_prefixes` — passed.
- All AGENTS.md invariant scripts exit 0 on the merged tree
  (check-arch, check-abac-route-registry, check-abac-exempt-handlers,
  check-mcp-write-tools, check-migration-versions,
  check-migration-sql-portability, check-dead-message-kinds,
  check-byte-slice-truncation, check-relative-path-defaults,
  check-fail-open-ref-resolution, check-task-commit-attribution,
  check-mem-port-contracts, check-fabricated-scope-defaults,
  check-lossy-secret-conversion, check-scope-literal-defaults,
  check-inert-enforcement, check-forged-scope-fields,
  check-forwarded-header-trust, check-in-memory-state-stores,
  check-unbounded-external-http).
- The wider `scripts/check-*.sh` set was run on both the merged tree
  and a pristine `f764bc00` worktree and the logs diffed: identical
  pass/fail sets (55/35; the 35 fail on the pristine base —
  environment awk dialect, usage error, and pre-existing advisory
  findings). The one new file named by
  check-silent-result-discard (`api/invitations.rs:1291`) is the same
  fire-and-forget notification-create pattern the base itself uses in
  `api/specs.rs`, `container_audit.rs`, and `ralph_loop.rs`; no check
  flips from pass to fail due to this merge. Evidence retained under
  `/tmp/stage/review-evidence/`.

### Tests for the independent verifier

- `cargo test -p gyre-server --lib api::invitations` (26 tests, incl.
  `invite_notification_carries_accept_path`).
- `cargo test --all` (full workspace).

### Shipped behavior (carried from prior rounds, unchanged by the merge)

- **Domain & storage:** `TenantInvitation`/`WorkspaceInvitation`/
  `InvitationStatus`/`InvitationPolicy` entities
  (`gyre-domain/src/invitation.rs`), port traits
  (`gyre-ports/src/invitation.rs`), SQLite + Postgres migrations and
  adapters, mem adapters. Tokens are CSPRNG-generated and stored only
  as SHA-256 hashes; the raw token appears solely in the invite
  response.
- **Tenant invitations:** the five specced endpoints — single invite,
  bulk invite, list with status filter, revoke, magic-link
  accept/decline — with TenantAdmin-only authorization, per-tenant
  `InvitationPolicy` expiry/max-pending caps, and SSO-account linking
  on accept when the email already has an account.
- **Workspace invitations:** invite → pending → accept/decline/expire
  lifecycle with membership creation on accept, inviter-side listing,
  invitee-facing `GET /api/v1/users/me/invitations`, and per-workspace
  `max_pending_invitations` enforcement; the invitee notification is
  typed `InvitationReceived` (round-14 repair).
- **Expiry:** background sweeper marks `Pending` invitations past
  `expires_at` as `Expired` for both invitation kinds.
- **Routes & ABAC:** all routes registered in `api/mod.rs` with ABAC
  resolver mappings in `abac_middleware.rs`; no exemption-file entries.
