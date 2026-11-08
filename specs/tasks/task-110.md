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
commits: ["0f4fea9aa717616b17ad1c76bc868d21906e22cc", "8f66d5a436ac2926fa8b12714abc34540a81d1fb", "dbfc76e139c0a09e8cc27ab08451bc77c40a4dbc", "8c469c538bd84cf880cb72522855c66a7a8f7d4d"]
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

**Repair round 5: merge of upstream `def0add9e` (task-157 meta-spec
rollout policies) onto candidate `c1f347a4f`, committed as `dbfc76e13`,
plus review findings F1a/F1b closed by `8c469c538`.**

The merge (25 files, +2913/−68: rollout-policy ports/adapters/migration,
reconciliation.rs rolling-rollout machinery, task-157 record) resolved
with no textual conflicts; semantic verification confirmed no new
interactions with task-110's surface — migration versions stay unique
(upstream `2026-11-06-000057_rollout_policies` derives version
`20261106000057`, distinct from every existing prefix), and the
fresh-DB migration boot in `cargo test -p gyre-adapters` runs the
combined migration set green.

**F1a/F1b (non-atomic creation lint):**
`accept_tenant_invitation_claimed` (user + pre-assigned memberships)
and `invite_to_workspace` (invitation + invitee notification) create
across two repositories with no transaction, failing
`scripts/check-non-atomic-creation.sh` (2 violations, exit 1). No
cross-repo transaction facility exists in the server, and both sites
have recoverable failure semantics, so each now carries the check's
documented inline `// non-atomic-create:ok - <reason>` marker with its
specific rationale (same pattern as git_http.rs/snapshot.rs/spawn.rs):

- `accept_tenant_invitation_claimed`: the caller compensates by
  rolling the invitation back to Pending on any side-effect error; a
  retry dedups — the user is found via `find_by_external_id` instead
  of re-created, and membership creation skips existing rows.
- `invite_to_workspace`: the notification is an explicitly best-effort
  `let _ =` side channel; the invitee's durable surface is
  `GET /api/v1/users/me/invitations`, so a lost notification cannot
  orphan the invitation.

Script now exits 0. No exemption files were touched.

Evidence log: `/tmp/stage/review-evidence/task-110-repair-round-5.md`.

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

### Test evidence (re-run at merged head + fix `8c469c538`, this round)

- `cargo check --all` (SKIP_WEB_BUILD=1) at the merged tree — clean.
- `cargo test -p gyre-adapters` — 388 passed / 0 failed / 12 ignored:
  includes the fresh-DB migration boot over the combined migration
  set (task-110 invitations + user-tenant-scope index + upstream
  rollout_policies) and the
  `claim_external_id_is_the_sanctioned_repoint` contract test.
- `cargo test -p gyre-server --lib auth::` — 46 passed / 0 failed
  (both sides' suites: upstream provisioning/username derivation and
  task-110 invitation-link tests).
- `cargo test -p gyre-server --lib api::invitations` — 18 passed / 0
  failed; `reconciliation` — 36 passed / 0 failed (upstream task-157
  suite at the merged tree).
- `scripts/check-non-atomic-creation.sh` — exits 0 (was 2 violations
  before `8c469c538`).
- Mechanical guard scripts verified relative to BOTH parents: every
  check that passes on both `def0add9e` and `c1f347a4f` passes at the
  merged head; every check that fails on both parents fails with an
  identical finding set (output-diffed per script; remaining diffs
  are ordering/path-prefix/line-number drift only). One check flips:
  `check-dependent-scope-resolution.sh` fails at the merged head on
  upstream task-157's `maybe_advance_rolling_rollout` — but it fails
  identically at the upstream base `def0add9e` itself (pre-existing
  mainline condition, inherited verbatim by the merge, not introduced
  or modified by task-110; left for an upstream-side fix rather than
  editing approved task-157 logic on this branch).
- Loopback TCP listeners are permitted in this sandbox and the
  in-process HTTP-layer tests above exercise the full routing chain;
  a separate live-server smoke remains for host verification / CI at
  the PR head.
