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
commits: ["5df8f2b136b1ad5792b58269cb4dc778ec301b05", "e52fea4f3bfc0ce1a57f99b0db6c98f55fe14209", "6bef61b7f7c9d5ccf391a9514a9064b8888d10f5", "ea366b150438e6b185edcf812cab5c5b1384b77a", "797cf52f94910a8c8bb9a5ae3f4f59868a99d820", "c1de316bd3c1f9b2fd09689891f58f36abf95c66", "8c469c538bd84cf880cb72522855c66a7a8f7d4d", "0f4fea9aa717616b17ad1c76bc868d21906e22cc", "8f66d5a436ac2926fa8b12714abc34540a81d1fb", "647ffd942d15a05976045b169fb70f67f22d1ca9"]
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

**This round: rebase repair onto assignment base 5984177** (durable finding
6f6a6b73 — merge conflicts in `abac_middleware.rs`, `api/mod.rs`, and
`specs/coverage/system/user-management.md`). The base advanced past the
previously reconciled 4b9220bc with task-124 (user management + workspace
membership API endpoints). The invitation flow itself is the independently
approved implementation from prior rounds (domain entities, ports,
SQLite/Postgres/mem adapters, migrations 2026-11-13-000063/64/65, the five
tenant endpoints, workspace invitation lifecycle, CSPRNG + SHA-256 tokens,
per-tenant `InvitationPolicy`, bulk invite, background expiry job, ABAC
mappings) — no invitation behavior changed this round.

### Conflicts resolved

- `api/mod.rs` + `abac_middleware.rs` `/users/me` blocks (textual): both
  sides kept — task-110's `GET /users/me/invitations` route/mapping plus
  task-124's `GET /users/:username` and `PUT /users/:id/deactivate`.
- **Duplicate workspace-invitation routes (semantic, both sides added at
  different spots so git auto-merged them into a tree axum rejects):**
  task-124 and task-110 had each implemented the SAME four spec paths
  (`POST /workspaces/{id}/invite`, `GET .../invitations`,
  `POST .../invite/accept`, `DELETE .../invitations/{id}`) — task-124 as
  unaccepted membership rows (api/users.rs), task-110 as the
  `WorkspaceInvitation` entity (api/invitations.rs). Resolution: task-110's
  model keeps the paths (this task's contract; strict superset covering
  §Workspace Invitation Flow's decline, configurable expiry, per-tenant
  policy, `max_pending_invitations` cap, and the background expiry job).
  task-124's three now-orphaned handlers (`list_invitations`,
  `accept_invitation`, `revoke_invitation`), their route registrations,
  ABAC resolver entries, api-reference rows, and their
  `workspace_invitation_lifecycle` test were removed; task-124's member
  list/role-change/removal endpoints, deactivation cascade, and user
  listing are untouched. `POST /workspaces/{id}/members` (M22.8
  `invite_member`) is unchanged from the merge base.
- **Diesel version collision:** task-124's
  `2026-11-13-000063_user_deactivated_at` and task-110's
  `2026-11-13-000063_invitations` both map to version 20261113000063 (only
  one runs per version — the alphabetically first — so
  `users.deactivated_at` would silently never be created on fresh
  databases). Renumbered to `2026-11-13-000066_user_deactivated_at` (next
  unused sequence), with a renumber note in its up.sql;
  `check-migration-versions.sh` passes.
- `specs/coverage/system/user-management.md`: merged audit header and rows
  25/26/27 recomputed against the merged tree (15/36 implemented, 10
  task-assigned, 11 n/a); row 27's seven-endpoint table remains satisfied
  (invitation paths now served by task-110's model). `SUMMARY.md`
  regenerated with the canonical script.

### Merge-integrity and gate evidence (this head)

- `cargo check -p gyre-server` passes (8m00s, 4 GiB sandbox).
- `cargo fmt --all --check` clean.
- Mechanical checks pass: architecture, migration versions (post-renumber),
  migration SQL portability, ABAC route registry, ABAC exempt handlers,
  MCP write tools, mem port contracts, dead message kinds, byte-slice
  truncation, relative path defaults, fail-open ref resolution, fabricated
  scope defaults, lossy secret conversion, scope literal defaults, inert
  enforcement, forwarded header trust, in-memory state stores, unbounded
  external HTTP (sweep logs in `/tmp/stage/review-evidence/gate-*.log`).
- Focused tests: `cargo test -p gyre-adapters` 533 passed / 0 failed —
  includes the merged migration suite (renumbered 000066 runs; the 000058
  backfill exclusion-set test), the SQLite invitation repositories, and
  the mem-port contract tests; `-p gyre-domain -p gyre-common -p gyre-ports`
  672 passed / 0 failed.
- gyre-server test compilation is deferred to the independent verifier
  (4 GiB cgroup build budget — environment restriction, not a defect). The
  verifier must run the full workspace suite — in particular the 26
  `api::invitations` tests (tenant lifecycle, revocation blocks accept,
  expiry job marks expired, bulk invite, non-admin 403, local-mode accept
  mints an authenticating API key, re-invite fresh key, SSO link, workspace
  lifecycle incl. accept/decline/revoke, cross-tenant containment,
  `max_pending` cap, `allow_re_invite`), `auth::` invitation/SSO-link and
  task-129 reserved-identity tests (the previously conflicted region), and
  task-124's user-management tests in `api::users` (deactivate cascade,
  username profile, users listing — the handlers this merge kept).

Coverage rows 5, 6, 7, 25 (task-110) plus 27 (merged) in
`specs/coverage/system/user-management.md` are `implemented` and describe
the merged tree; the audit header records this repair merge and the model
choice.
