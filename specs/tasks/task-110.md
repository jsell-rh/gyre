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
commits: ["797cf52f94910a8c8bb9a5ae3f4f59868a99d820", "c1de316bd3c1f9b2fd09689891f58f36abf95c66", "8c469c538bd84cf880cb72522855c66a7a8f7d4d", "0f4fea9aa717616b17ad1c76bc868d21906e22cc", "8f66d5a436ac2926fa8b12714abc34540a81d1fb", "58a012bc53ef194627a13b80ad478e0b4a2a15db"]
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

**Repair round 10 — review finding (coverage docs), commit
`58a012bc5`.** Single finding, docs-only: the task's four declared
coverage sections — `user-management.md` rows 5 (Tenant-Level User
Onboarding), 6 (Workspace Invitation Flow), 7 (Invitation Expiry),
25 (Tenant Invitations) — were still `task-assigned` with empty
notes even though all 10 acceptance criteria pass with independent
evidence (26/69/1564/400 test runs; mutation probes killed;
mechanical guards verified across rounds 8–9). Repaired in
`specs/coverage/system/user-management.md`: rows 5/6/7/25 marked
`implemented` with evidence notes pending independent review, header
counts 3/36 → 7/36 (23 → 19 task-assigned, 3 → 7 implemented), per
the task-111/task-120 ready-for-review precedent. No product code
changed this round; re-verified at this head:
`cargo test -p gyre-server --lib api::invitations` — 26 passed /
0 failed.

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
- **Policy enforcement (round 7):** `allow_re_invite: false` rejects
  re-invites after a terminal-state invitation with 409 on both
  surfaces; pending-scope uniqueness is enforced by partial unique
  indexes (migration `2026-11-08-000059_invitation_pending_unique`)
  with raced-duplicate dedupe, so terminal statuses never conflict
  and re-invite after expiry/revocation is unaffected.
- **SSO/local account linking (round 4):**
  `UserRepository::claim_external_id` is the single sanctioned
  external_id transition (SQLite/Postgres/mem); the auth email-link
  path claims through it after external_id match and verified
  same-tenant email, and stamps `last_login_at` per upstream's
  every-login contract.
- **Repair round 9 (findings `d559534738b54ad398ee79fd95770725`,
  commit `797cf52f`):** tenant-invitation accept LINKS an existing
  SSO-provisioned account via the new case-insensitive, tenant-
  scoped `UserRepository::find_by_email` (no parallel shadow
  account) and ACTIVATES pre-assigned memberships; derived usernames
  resolve collisions through `sanitize_username` +
  `resolve_unique_username` (bob, bob-2, ...) while explicit handles
  are validated before the single-use claim (400 invalid / 409
  taken, both retryable); `GYRE_INVITE_EXPIRY_INTERVAL_SECS=0`
  falls back to default instead of panicking the interval;
  `load_policy` fails closed on a corrupt policy row; +6 HTTP-layer
  two-tenant containment tests. Evidence log:
  `/tmp/stage/review-evidence/task-110-repair-round-9.md`.
- **Docs:** `docs/api-reference.md` covers all invitation routes,
  selectors and the policy endpoints; `docs/server-config.md`
  documents `GYRE_INVITE_EXPIRY_INTERVAL_SECS`.

### Test evidence (round 9, commit `797cf52f`)

- `cargo check -p gyre-server -p gyre-adapters -p gyre-ports`
  (SKIP_WEB_BUILD=1) — clean.
- `cargo test -p gyre-server --lib api::invitations` — 26 passed /
  0 failed (was 20; +6 new tests).
- `cargo test -p gyre-server --lib auth` — 69 passed / 0 failed.
- `cargo test -p gyre-server --lib` (full) — 1564 passed / 0 failed.
- `cargo test -p gyre-adapters` (full) — 400 passed / 0 failed /
  12 ignored (was 399; +1 `find_by_email` adapter test).
- Mechanical guards: every guard OK at the assignment-start head
  `39bef9d66` is still OK with this diff; the 33 guards that FAIL at
  that head fail byte-identically with and without this diff
  (verified by stashing), all in task-089/157/upstream code —
  e.g. `check-non-atomic-creation` → `git_http.rs:1555`,
  `check-dependent-scope-resolution` → `reconciliation.rs:925` —
  left for upstream-side fixes per the round-8 note.

### Test evidence (round 8, at merge commit `b53fa1484`)

- `cargo check --all` (SKIP_WEB_BUILD=1) — clean.
- `cargo test -p gyre-server --lib auth` — 69 passed / 0 failed.
- `cargo test -p gyre-server --lib api::invitations` — 20 passed /
  0 failed.
- `cargo test -p gyre-adapters --lib migration` — 8 passed /
  0 failed, including `no_duplicate_migration_version_prefixes`,
  `migration_000058_backfills_unique_url_safe_usernames` (exercising
  the renumbered migration in its target batch),
  `migrations_create_tables`, `migrations_pending_zero_after_startup`
  (full embedded set on a fresh database), and
  `migration_000059_dedupes_raced_pending_invitations_before_index`.
- `cargo test -p gyre-adapters` (full) — 399 passed / 0 failed /
  12 ignored.
- Mechanical guards at the merge head — all exit 0:
  `check-migration-versions` (FAIL at the auto-merged tree before the
  renumber, OK after), `check-migration-sql-portability`,
  `check-fabricated-scope-defaults`, `check-scope-literal-defaults`,
  `check-arch`, `check-abac-route-registry`,
  `check-abac-exempt-handlers`, `check-inert-enforcement`,
  `check-mem-port-contracts`, `check-mcp-write-tools`,
  `check-dead-message-kinds`, `check-byte-slice-truncation`,
  `check-relative-path-defaults`, `check-fail-open-ref-resolution`,
  `check-lossy-secret-conversion`, `check-forwarded-header-trust`,
  `check-in-memory-state-stores`, `check-unbounded-external-http`,
  `check-non-atomic-creation`, `check-forged-scope-fields`, and
  `check-task-commit-attribution`.
- Known pre-existing (verified identical at upstream base `774737638`
  via a scratch worktree this round):
  `check-dependent-scope-resolution.sh` reports 2 findings in
  task-089/157 code (`explorer_views.rs:736`, `reconciliation.rs:925`);
  left for upstream-side fixes.
