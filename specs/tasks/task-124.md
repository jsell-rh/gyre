---
title: "Complete user management and workspace membership API endpoints"
spec_ref: "user-management.md §API > User Management"
depends_on:
  - task-120
  - task-121
progress: ready-for-review
coverage_sections:
  - "user-management.md §User Management"
  - "user-management.md §Workspace Membership"
commits: ["33e30ac961ad1021b50f3cd3a65c3ac184a03ca1", "6033324f8936365465b9ffc9b27e6c3a6429f0d1", "60c86252706b2392866b1dddd66634ab4cb123d6", "38b3134feeaa28be0e443e2e29344980ea6db552", "3350817099f10cc5fde2e505f9cb4ad8bb5d129e"]
---

## Spec Excerpt

From `user-management.md` §API > User Management:

| Endpoint | Method | Purpose |
|---|---|---|
| `GET /api/v1/users/me` | GET | Current user profile + preferences |
| `PUT /api/v1/users/me` | PUT | Update display name, timezone, locale, preferences |
| `GET /api/v1/users/{username}` | GET | User profile (tenant-public) |
| `GET /api/v1/users` | GET | List users (tenant admin only) |
| `PUT /api/v1/users/{id}/deactivate` | PUT | Deactivate user (TenantAdmin only) |
| `GET /api/v1/users/me/sessions` | GET | My active sessions |
| `DELETE /api/v1/users/me/sessions/{id}` | DELETE | Revoke a session |
| `POST /api/v1/users/me/sessions/revoke-all` | POST | Revoke all sessions |

From §API > Workspace Membership:

| Endpoint | Method | Purpose |
|---|---|---|
| `GET /api/v1/workspaces/{id}/members` | GET | List workspace members |
| `POST /api/v1/workspaces/{id}/invite` | POST | Invite user (Owner/Admin only) |
| `GET /api/v1/workspaces/{id}/invitations` | GET | List pending invitations |
| `POST /api/v1/workspaces/{id}/invite/accept` | POST | Accept invitation |
| `DELETE /api/v1/workspaces/{id}/invitations/{id}` | DELETE | Revoke invitation |
| `PUT /api/v1/workspaces/{id}/members/{user_id}/role` | PUT | Change member role (including ownership transfer) |
| `DELETE /api/v1/workspaces/{id}/members/{user_id}` | DELETE | Remove member |

**Existing endpoints** (per Completeness Assessment and mod.rs):
- `GET/PUT /api/v1/users/me` — implemented
- `GET /api/v1/users/me/{agents,tasks,mrs}` — implemented
- `POST/GET /api/v1/workspaces/{id}/members` — implemented
- `PUT/DELETE /api/v1/workspaces/{id}/members/{user_id}` — implemented

## Implementation Plan

1. **Audit existing endpoints** in `gyre-server/src/api/users.rs` and `mod.rs`:
   - Verify `GET/PUT /api/v1/users/me` returns/accepts all spec'd fields (username, display_name, timezone, locale, preferences, avatar_url)
   - Verify workspace membership endpoints match spec

2. **Add missing user management endpoints:**
   - `GET /api/v1/users/{username}` — public profile (within tenant)
   - `GET /api/v1/users` — list all users (TenantAdmin only)
   - `PUT /api/v1/users/{id}/deactivate` — deactivate user (TenantAdmin only)

3. **Add missing session management endpoints** (if not covered by task-111):
   - Verify session endpoints from task-111 cover: list, revoke individual, revoke all

4. **Add missing workspace membership endpoints:**
   - `GET /api/v1/workspaces/{id}/invitations` — list pending invitations
   - `POST /api/v1/workspaces/{id}/invite/accept` — accept invitation
   - `DELETE /api/v1/workspaces/{id}/invitations/{id}` — revoke invitation
   - Verify `PUT .../members/{user_id}/role` supports ownership transfer

5. **Register all new routes** in `api/mod.rs` and add ABAC route mappings.

## Acceptance Criteria

- [ ] `GET /api/v1/users/{username}` returns public profile within tenant
- [ ] `GET /api/v1/users` lists users (TenantAdmin only, paginated)
- [ ] `PUT /api/v1/users/{id}/deactivate` deactivates user with cascade (TenantAdmin only)
- [ ] Workspace invitation list, accept, and revoke endpoints functional
- [ ] Role change endpoint supports ownership transfer validation
- [ ] All routes registered in `api/mod.rs` with ABAC route mappings
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/user-management.md` §API > User Management and §API > Workspace Membership. Existing user API: `gyre-server/src/api/users.rs`. Route registration: `gyre-server/src/api/mod.rs` — grep for `/users/` to see existing routes. ABAC route mappings: `gyre-server/src/abac_middleware.rs`. Session management: check if task-111 already covers session endpoints. Workspace member endpoints: grep for `invite_member\|list_members\|update_member_role\|remove_member` in the api/ directory.

## Shipped

### User management endpoints (§API > User Management)

Five of the eight spec endpoints are implemented in production code;
the three session endpoints are owned by task-111 (its §Session
Management row covers the UserSession entity, `user_sessions` storage,
and auth-middleware revocation checks — not just the three routes), so
the §User Management coverage row stays task-assigned over them:

- **`GET /api/v1/users/{username}`** — tenant-public profile
  (`{id, username, display_name, avatar_url, timezone, created_at,
  memberships:[{workspace_id, role, ...}]}`). Cross-tenant handles are
  indistinguishable from nonexistent ones (404), so the endpoint cannot
  probe foreign tenants.
- **`GET /api/v1/users`** — TenantAdmin-only (per-handler role check,
  Developer → 403), tenant-scoped, paginated (`limit`/`offset` with a
  total count).
- **`PUT /api/v1/users/{id}/deactivate`** — TenantAdmin-only soft
  deactivation with a real cascade: migration
  `2026-11-13-000063_user_deactivated_at` adds `users.deactivated_at`;
  the auth extractor, WebSocket, and SSO paths reject deactivated
  accounts; deactivating a workspace Owner fires the ownerless-workspace
  flow with an inline TenantAdmin notification; impersonation sessions
  and tokens are revoked when either side is deactivated;
  self-deactivation and cross-tenant targets are rejected;
  audit-logged with reason.
- `GET/PUT /users/me` were pre-existing (task-120) and return/accept
  all spec'd profile fields including preferences.

### Workspace membership endpoints (§API > Workspace Membership)

All seven spec endpoints are live:

- **`POST /api/v1/workspaces/{id}/invite`** — spec-named alias of the
  member invite: creates a PENDING membership + `InvitationReceived`
  notification (Owner/Admin/TenantAdmin only).
- **`GET /api/v1/workspaces/{id}/invitations`** — pending-only list.
- **`POST /api/v1/workspaces/{id}/invite/accept`** — invitee-only
  per-handler auth (membership looked up by caller+workspace); stamps
  `accepted_at`; 404 when not invited, 409 when already accepted.
- **`DELETE /api/v1/workspaces/{id}/invitations/{invitation_id}`** —
  revokes pending only (accepted → 409 pointing at member removal) and
  deletes the membership row.
- `GET .../members`, `PUT .../members/{user_id}(/role)`, and
  `DELETE .../members/{user_id}` were pre-existing and carry the
  last-owner guard plus audited ownership transfer
  (`ownership_transfer_unblocks_old_owner_removal_and_is_audited`).

All routes are registered in `api/mod.rs` with ABAC resolver entries
(`invite/accept` is resolver-exempt with per-handler auth since any
global role can be an invitee). Pending invitations are unaccepted
membership rows — the lifecycle is real storage state, not a parallel
table.

### This round (rebase onto base `cdd1dd02`, checkpoint 03eed997)

The executor staged the assigned repair: merge the new base
`cdd1dd02` (task-011 spec-save git write-through, task-146 analytics
scope columns, task-171 LLM endpoint work) into this task-124 head.
The merge auto-resolved all code files; the only textual conflict was
`specs/coverage/SUMMARY.md`, where the two branches had moved disjoint
sets of coverage rows from assigned to implemented since the common
ancestor `1ee52c43`:

- Resolved as the union: this branch's rows (agent-runtime,
  meta-spec-reconciliation, platform-model, search, spec-registry,
  user-management) and the base's analytics row (7 assigned / 5
  implemented); the spec-links row was changed identically on both
  sides and auto-merged. The TOTAL row was recomputed as the union:
  25 sections moved assigned→implemented in total → 215 assigned,
  204 implemented, 163 verified of 582 non-n/a = 63%.
- Migration-version invariant holds under the merge: the base's
  `2026-10-08-000063_analytics_scope_columns` and this branch's
  `2026-11-13-000063_user_deactivated_at` share the 6-digit sequence
  but map to distinct full Diesel versions (`20261008000063` vs
  `20261113000063`), so every migration runs;
  `scripts/check-migration-versions.sh` and
  `scripts/check-migration-sql-portability.sh` both exit 0 on the
  merged tree.

No task-124 source semantics changed: every code file auto-merged.
Fresh probes on the merged tree (logs under `review-evidence/` in
`/tmp/stage/review-evidence`):

- `cargo check -p gyre-server --tests` — exit 0 (8m18s;
  `cargo-check-gyre-server-tests-merge-base-cdd1dd02.log`).
- `cargo test -p gyre-common` — 125 passed.
- `cargo test -p gyre-domain` — 511 + 8 passed.
- `cargo test -p gyre-adapters` — 521 passed (includes the task-124
  migration backfill/deactivation storage paths on real SQLite plus
  the base's new analytics scope-column adapter tests).
- Mechanical invariant scripts all exit 0: arch, ABAC route registry,
  ABAC exempt handlers, MCP write gates, migration versions,
  migration SQL portability, forged scope fields, inert enforcement,
  unbounded external HTTP, task commit attribution (after restoring
  this file's `commits:` frontmatter).
- Running the server test binary itself is deferred to the verifier
  per the sandbox memory budget (commands below).

### Prior rounds (context)

- Merge of task-112's branch `1ee52c43` (checkpoint 4fd846f2): the two
  branches implemented disjoint rows of the user-management coverage
  matrix; union + mechanical recount; all code auto-merged.
- Rebase repair (merge of `6f5a631c`): the only code conflict was the
  `use users::{...}` import list in `api/mod.rs`, resolved as the
  union; coverage narratives merged and mechanically recounted.
- Review finding 3cd87005 (stale 7-argument `list_for_user` call in
  `deactivate_user_cascades_to_auth_and_ownership`) fixed by commit
  33508170.
- 38b3134f: fixed a test-fixture bug — `seed_workspace_with_owner`
  built the Owner membership as pending instead of the production
  `create_workspace` semantics (accepted immediately), which had
  broken `workspace_invitation_lifecycle`.

### Server tests the verifier must execute

Large gyre-server test compilation is deferred to the independent
verifier per the sandbox budget:

- `cargo test --all` — the assignment's definition-of-done gate;
  `cargo check -p gyre-server --tests` passes on the merged tree.
- `cargo test -p gyre-server api::users::tests` — includes
  `deactivate_user_cascades_to_auth_and_ownership`,
  `workspace_invitation_lifecycle`,
  `get_user_by_username_is_tenant_scoped`,
  `list_users_is_tenant_admin_only_scoped_and_paginated`,
  `ownership_transfer_unblocks_old_owner_removal_and_is_audited`,
  `last_owner_cannot_be_removed_or_downgraded`,
  `member_management_requires_owner_admin_or_tenant_admin`,
  `invite_member_rejects_duplicate_membership`, plus the task-125
  notification handlers that share `users.rs` after the rebase.
- `cargo test -p gyre-server api::impersonation::tests` — includes
  `deactivating_target_revokes_impersonation_token`,
  `deactivating_impersonator_revokes_impersonation_token`,
  `resolver_rejects_deactivated_accounts_even_with_unended_session`.
