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
commits: ["93c2d7d005530a19d164b67a49247fab8823b3bf", "54f7379a8570789e19553890bc59946945410c62", "024b8825778c0c099a61b26e0a06eebd5dcafe7a", "e3cb3304c40627c4137c60840cfe327299bf9c63", "727a7b49e589fc16dde026ec084739166c8923b8", "030354c64674fb570e4dd85cb5acf9a3ac8b93a5", "145f499b11993781082524a95e1119b054854c31", "ba97de27786a8a2758e29ff282597f55c2755c9b", "4345788f601038d4cbcca896ae1e4c4cf3cf3c72", "9a8e75c1ab97d4c4d8c711ad85f4448116c55eac", "1a51b48c99b306e6b79d7799b78d8198a579cb48", "8cf7e5bc625ad7635a46055d00896e892fe4b9eb"]
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

**Repair round for review findings b646524c (F1-F4), on top of the
recovered task-110 lineage.** The branch started with an interrupted
upstream merge (unmerged `mem.rs`); commit `1a51b48c` resolved it by
keeping both sides (the task-110 invitation repositories and the
upstream secret/trace test modules), and commit `8cf7e5bc` implements
the four review fixes. Evidence log with commands, exit codes, and
source SHAs: `/tmp/stage/review-evidence/task-110-repair-round.md`.

### Shipped behavior (this round)

- **F1/F2 — invitee-side reachability + the specced route:** the
  authenticated invitee surface from user-management.md §API >
  Workspace Membership and §Workspace Invitation Flow steps 1-4 is now
  real: `POST /api/v1/workspaces/{id}/invite/accept` and
  `/invite/decline` (authenticated invitee accepts/declines THEIR
  pending invitation — no token; the caller must be the invited user in
  the workspace's tenant; no pending invitation for the caller → 404),
  and `GET /api/v1/users/me/invitations` (the invitee's
  pending-invitations view, wiring
  `WorkspaceInvitationRepository::list_by_user`, which previously had
  zero production callers). Workspace invites now resolve the invitee
  by `username` or `email` per spec step 1 ("by username
  (tenant-internal) or email"); `user_id` is still accepted and
  multiple selectors must resolve to the same user. The in-app
  notification to the invitee now carries a JSON body with
  `kind: workspace_invitation` and the `accept_url`/`decline_url`
  routes, so the invitee can act without the inviter's token. The
  token-based routes remain as an additional surface, not a
  replacement.
- **F3 — single-use race:** the invitation ports' unconditional
  `update_status` was replaced by
  `transition_status(id, from, to, accepted_at) -> Result<bool>`,
  enforced inside the write itself: a conditional `UPDATE ... WHERE
  id = ? AND status = from` (rows>0 = won) in the SQLite and Postgres
  adapters, and a check-and-set under the store mutex in the mem
  adapter. Accept handlers claim Pending→Accepted BEFORE creating the
  user/memberships (with best-effort rollback to Pending if the side
  effects fail), so two concurrent accepts of the same token cannot
  both win; decline, revoke, and the expiry job use the same
  conditional transition so a terminal status is never overwritten by
  a racing writer.
- **F4 — HTTP-layer negative authorization tests:** a non-admin
  (ReadOnly user with a real API key, resolved through the actual
  extractor) gets 403 from `POST /api/v1/tenant/invite`, and a
  non-member tenant user gets 403 from
  `POST /api/v1/workspaces/{id}/invite` — both through the full
  routing + handler chain, asserting nothing was created. The old
  direct-guard unit test was replaced by these.
- **Docs:** `docs/api-reference.md` documents the three new routes and
  the username/email invite selectors.

### Prior rounds (unchanged lineage)

Domain entities (`gyre-domain/src/invitation.rs`), ports
(`gyre-ports/src/invitation.rs`), SQLite/Postgres migration
`2026-10-08-000056_invitations` + adapters, the five tenant invitation
endpoints (single/bulk invite, list with status filter, revoke,
magic-link accept/decline), workspace invite/list/revoke + token
accept/decline, the hourly expiry job
(`GYRE_INVITE_EXPIRY_INTERVAL_SECS`), CSPRNG tokens stored as SHA-256,
per-tenant `InvitationPolicy` endpoints, ABAC route mappings with
per-handler enforcement.

### Test evidence (this round; head 8cf7e5bc, this sandbox)

- `cargo test -p gyre-server --lib api::invitations` — 18 passed /
  0 failed, exit 0: the full prior lifecycle suite plus the new
  invitee-surface tests (list+accept lifecycle, wrong-user 404,
  authenticated decline, invite by username and by email,
  notification accept-path content), HTTP 403 negative-authz tests,
  and the concurrent-claim exactly-one-winner test.
- `cargo test -p gyre-adapters invitation` — 3 passed / 0 failed,
  exit 0 (temp-file SQLite; the round-trip tests now assert the
  single-use claim at the adapter, and
  `transition_status_rejects_wrong_source_status` proves a Revoked row
  is not overwritten by a late expiry transition).
- `cargo test -p gyre-server --lib abac_middleware` — 9 passed /
  0 failed, exit 0 (resolver covers the new routes).
- `cargo test -p gyre-server --lib mem::` — 10 passed / 0 failed,
  exit 0 (merge resolution intact).
- `cargo test -p gyre-server --lib` — 1263 passed / 0 failed, exit 0.
- `cargo test --offline -p gyre-domain --lib invitation` — 4 passed /
  0 failed, exit 0.
- All 88 mechanical check scripts run; every failure was compared
  against a clean worktree of the base commit
  75ada1c4073ee9d56d3a1aebf5c780582d8867cb and fails identically there
  (awk-dialect issues under this sandbox's awk plus pre-existing
  upstream findings in untouched areas). The one check that initially
  failed only in this tree (`check-warn-continue-creation`, whose
  pre-existing seed_builtin_policies exemption anchor was shifted by
  the new route-mapping lines) was resolved by re-anchoring the
  existing exemption entry to the same site — no new entries, no
  verifier weakening, `git diff base..HEAD -- scripts/` shows only
  that one re-anchored line.
- Pre-existing (identical at base, not from this change): clippy
  `never_loop` deny in `gyre-domain/src/rust_extractor.rs:1071`
  (upstream PR #627), and check-task-commit-attribution flagging
  upstream a781ede2 (task-210).
- No TCP listener is bindable in this sandbox (accept() errno 95,
  /tmp/stage/capabilities.json), so the live-server HTTP smoke remains
  for host verification / CI at the PR head.
