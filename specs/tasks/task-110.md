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

**This round: re-verify the independently approved invitation flow on
assignment base 9afd66c7 (which carries task-133's ABAC MCP permission
resources) and resolve the durable CI finding** — no invitation behavior
changed; the implementation is the one approved across the prior repair
rounds (domain entities, ports, SQLite/Postgres/mem adapters, migrations
2026-11-13-000063/64/65, the five tenant endpoints plus policy endpoints,
workspace invitation lifecycle, CSPRNG + SHA-256 tokens, per-tenant
`InvitationPolicy`, bulk invite, background expiry job, ABAC mappings,
docs).

### CI finding 7ec83a8d (candidate_failed) — resolved as a transient flake

PR #825 head a8996690, CI run 43610196702: the "Development controller"
check failed on attempt 1 at the `test_pipeline*.py` step, then the SAME
tree passed every job on attempt 2 — the run's final conclusion is
`success` (GitHub API evidence saved under
`/tmp/stage/review-evidence/`). Attempt-1 job logs were never retrievable
(blob host denied from this sandbox); with an identical tree passing on
re-run there is no deterministic defect to repair. The two
`test_pipeline*.py` failures that DO reproduce in this sandbox are
environment restrictions, not code defects: this sandbox denies
`os.killpg(2)` (EPERM; plain child kill works — probe saved), which is
exactly what both tests exercise, and they fail identically on the
assignment base. The `test_dev*.py` failures here are the 4 GiB cargo
build wrapper, also unrelated to this branch (it changes no scripts or
manifests).

### Repairs this round

- Restored this task's `commits:` frontmatter, which the assignment reset
  had emptied: `check-task-commit-attribution.sh` was FAILING on the tree
  because the seven task-110-labeled product-surface commits in branch
  history were orphaned from review scoping. All nine previously recorded
  SHAs verified in-branch (`git merge-base --is-ancestor`).

### Verification on the merged tree (head f56a47df)

- `cargo check -p gyre-server` — clean (adapters/domain/ports included).
- `cargo test -p gyre-domain` — 8 passed. `cargo test -p gyre-adapters` —
  533 passed, 0 failed (6 ignored are Docker/SSH/credential-gated
  compute/llm tests, untouched by this branch).
- `check-clippy-diff.py HEAD^1` and `check-rustfmt-diff.py HEAD^1` —
  changed lines clean.
- All non-advisory mechanical gates pass: arch, hierarchy, ABAC route
  registry, ABAC exempt handlers, migration versions + SQL portability,
  mem port contracts, in-memory state stores, forged scope fields, inert
  enforcement, dead message kinds, byte-slice truncation, unwired port
  methods, sandbox sweep artifacts, scope/lossy/fabricated-defaults
  lints. Advisory failures (dead-parameters, ignored-tool-tests,
  tenant-filter/crypto-verify awk) are byte-identical on the assignment
  base in files this branch never touched.

### Verifier must run (blocked in-sandbox by the 4 GiB build wrapper)

- `cargo test -p gyre-server --lib api::invitations` — 28 tests
  (lifecycle, revoke, bulk, expiry job, policy, CSPRNG/hash, SSO link,
  local-mode key mint, cross-tenant guards, concurrency).
- `cargo test -p gyre-server --lib auth::tests` — invitation email-link
  tests (`jwt_email_link_claims_invitation_account`,
  `jwt_unverified_email_does_not_claim_invitation_account`).
- Full `cargo test --all` (CI Test job) and the exact-head GitHub checks.
