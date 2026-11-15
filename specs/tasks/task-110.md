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
commits: ["6af1d71c51cf1f763c0a1c07ec3f14b272edc523", "758462f45d6b162bc273c4a676e6bb33442e5dde", "43b15db3df88aed619f631a209e0cce124594100", "8833f8d99ccd1536a06ad81e364f80efc5c3f330", "01958bc011244ae4a4c06258baec597a00d8134d", "9f9be53014102d44e5e8c148d21b580448c226c7", "d1103854d71b5b8adec5f2c7e03d691f7e8dc059", "5df8f2b136b1ad5792b58269cb4dc778ec301b05", "e52fea4f3bfc0ce1a57f99b0db6c98f55fe14209", "6bef61b7f7c9d5ccf391a9514a9064b8888d10f5", "ea366b150438e6b185edcf812cab5c5b1384b77a", "797cf52f94910a8c8bb9a5ae3f4f59868a99d820", "c1de316bd3c1f9b2fd09689891f58f36abf95c66", "8c469c538bd84cf880cb72522855c66a7a8f7d4d", "0f4fea9aa717616b17ad1c76bc868d21906e22cc", "8f66d5a436ac2926fa8b12714abc34540a81d1fb"]
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

Tenant & workspace invitation flow, implemented across multiple
review/repair rounds on this branch. The task-110 feature surface
(domains, ports, SQLite/PG/mem adapters, migrations, all five tenant
endpoints, workspace invitation lifecycle, CSPRNG tokens stored as
SHA-256 hashes, single-use transactional acceptance, per-tenant expiry
policy, background expiry job, ABAC route mappings) shipped in the
earlier rounds recorded in `commits:` above and summarized in prior
review rounds; this round repairs the two findings of review 2cff1e20
on top of the merged tree (base 706bc7bd):

- **Token leak into observability (finding 1, `middleware.rs`):**
  `request_tracing` exported the raw URI path both as the `http_request`
  info-span field (default `RUST_LOG=info`) and as Prometheus label
  values, and `GET /metrics` is unauthenticated in the default
  deployment — so the invitation magic-link token (the invitee routes'
  sole auth factor, valid while Pending for the default 7 days) was
  readable by anyone who could reach `/metrics`. `redact_token_paths`
  now rewrites the token segment of `/api/v1/invite/*` and
  `/api/v1/workspaces/invitations/*` paths to a fixed `{token}` template
  before the path is used anywhere, for both the span field and the
  metrics labels; this also bounds the label cardinality to one label
  set per route. Covered by pure-function unit tests plus a full-router
  round trip (`invite_token_redacted_from_metrics_labels`) asserting the
  secret appears nowhere in the rendered metrics and the template is the
  `path` label.
- **Reserved system ABAC identity on the accept path (finding 2,
  `api/invitations.rs`):** the unauthenticated invitation-accept arm
  accepted `display_name == gyre-system-token` (explicitly, or as the
  default derived from the username / email local-part), violating the
  task-129 invariant every sibling display_name write path (profile
  update, create_user, SSO provisioning, SCIM) enforces — the resulting
  account would be bricked behind the ABAC middleware's fail-closed
  identity check with no self-service recovery. Explicit `display_name`
  and `username` values are now rejected 400 before the single-use claim
  (reserved input does not consume the invitation); the derived case is
  rejected inside the create arm, whose failure compensation rolls the
  invitation back to Pending. Covered by
  `accept_rejects_reserved_system_abac_identity` (three input paths,
  non-consumption, and no user created under the reserved handle).

### Test evidence

This round (tree at 6af1d71c), all via the sandbox build wrapper
(`/tmp/stage/bin/cargo`): `cargo check -p gyre-server` exit 0;
`cargo check -p gyre-server --tests` exit 0 (new test code compiles);
`cargo clippy -p gyre-server --lib` clean on every changed line
(remaining warnings are pre-existing crate baseline);
`cargo fmt --check` clean; mechanical gates `check-arch.sh`,
`check-byte-slice-truncation.sh`, `check-inert-enforcement.sh`,
`check-forwarded-header-trust.sh`, `check-task-commit-attribution.sh`
all OK. The extracted `redact_token_paths` logic was additionally
probed standalone (11 path cases behave as asserted). Full server test
compilation exceeds the sandbox's 4 GiB build-wrapper budget — the
verifier must run `cargo test -p gyre-server --lib middleware::` and
`cargo test -p gyre-server --lib api::invitations::` (especially the
five new tests named above) plus the pre-existing invitation suite as
regression; details in `/tmp/stage/review-evidence/repair-2cff1e20.md`.
Earlier-round evidence (domain/adapters/ports/CLI suites, mechanical
gates) is unchanged by this repair, which touches only
`gyre-server/src/middleware.rs` and `gyre-server/src/api/invitations.rs`.
