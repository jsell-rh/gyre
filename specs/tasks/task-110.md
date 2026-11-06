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
commits: ["94555819dba4a33b477b598d6cc57d115fd89283"]
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

**Rebase-resolution round: merge of upstream `9bef21a4` onto candidate
`f60123f5`.** The branch arrived with an interrupted merge whose only
unmerged path was `docs/server-config.md` — both sides had appended
env-var rows at the end of the table. Resolved by keeping BOTH sides:
our task-110 `GYRE_INVITE_EXPIRY_INTERVAL_SECS` row plus upstream's
`GO_CALLGRAPH_BIN` (task-072) and `PYRIGHT_LANGSERVER_BIN` (task-074)
rows. The resolved file is byte-identical to
`9bef21a4:docs/server-config.md` except for exactly that one inserted
row (verified by diff before staging). All other paths auto-merged;
this round's manual resolution touches documentation only — zero Rust
code — and the invitation implementation is intact in the merged tree
(`transition_status` present in the ports, the SQLite/Postgres
invitation adapters, and `mem.rs`; invite routes still registered in
`api/mod.rs`). Evidence log:
`/tmp/stage/review-evidence/task-110-rebase-round-2.md`.

**Sandbox restriction this round (not a code defect):** the focused
test re-run (`cargo test -p gyre-server --lib api::invitations`)
timed out twice at the tool level while compiling, after which the
sandbox's shell supervisor (tirith) blocked every shell command for
the remainder of the assignment window — so the test re-run and the
`git commit` of the resolved merge could not be executed here. The
working tree holds the fully resolved, staged merge; the commit and
test re-run remain for the executor checkpoint / host verification.
Per assignment instructions this transport restriction is recorded
rather than treated as a failure.

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
- **Docs:** `docs/api-reference.md` covers all invitation routes and
  selectors; `docs/server-config.md` documents
  `GYRE_INVITE_EXPIRY_INTERVAL_SECS`.

### Test evidence (prior rounds, re-verified at earlier heads of this
same Rust lineage)

- `cargo test -p gyre-server --lib api::invitations` — 18 passed / 0
  failed (lifecycle suite, invitee-surface tests, HTTP 403
  negative-authz, concurrent-claim exactly-one-winner).
- `cargo test -p gyre-adapters invitation` — 3 passed / 0 failed
  (round-trip single-use claim; wrong-source-status transition
  rejected).
- `cargo test -p gyre-server --lib abac_middleware` — 9/0;
  `cargo test -p gyre-server --lib mem::` — 10/0; full
  `cargo test -p gyre-server --lib` — 1263/0;
  `cargo test --offline -p gyre-domain --lib invitation` — 4/0.
- `bash scripts/check-task-commit-attribution.sh` — exit 0 at the
  prior merge head.
- This round's merge resolution changes no Rust code, so the above
  remains the operative evidence for the implementation; the
  re-run-at-head is deferred to host verification per the sandbox
  restriction noted above. No TCP listener is bindable in this
  sandbox (accept() errno 95, /tmp/stage/capabilities.json), so the
  live-server HTTP smoke also remains for host verification / CI at
  the PR head.
