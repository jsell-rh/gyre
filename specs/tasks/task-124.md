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
commits: ["3350817099f10cc5fde2e505f9cb4ad8bb5d129e", "38b3134feeaa28be0e443e2e29344980ea6db552", "60c86252706b2392866b1dddd66634ab4cb123d6", "6033324f8936365465b9ffc9b27e6c3a6429f0d1", "33e30ac961ad1021b50f3cd3a65c3ac184a03ca1"]
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
the three session endpoints are owned by task-111, so the
§User Management coverage row stays task-assigned over them:

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
  last-Owner guard plus audited ownership transfer
  (`ownership_transfer_unblocks_old_owner_removal_and_is_audited`).

All routes are registered in `api/mod.rs` with ABAC resolver entries
(`invite/accept` is resolver-exempt with per-handler auth since any
global role can be an invitee). Pending invitations are unaccepted
membership rows — the lifecycle is real storage state, not a parallel
table.

### This round (merge of task-112's branch `1ee52c43`, checkpoint 4fd846f2)

The executor staged the assigned repair: merge the task-112 pipeline
branch (base `1ee52c43` — notification delivery channels, who-gets-
notified routing, escalation routing) into this task-124 head. The
merge auto-resolved all code files; the only textual conflict was
`specs/coverage/system/user-management.md`, where the two branches had
implemented disjoint rows of the same coverage matrix from a common
ancestor:

- Resolved as the union: rows 16/17/18 (task-112 implemented) come
  from the base side, rows 26 (partial, session endpoints owned by
  task-111) / 27 (task-124 implemented) from this branch. The
  `Coverage:` header line was recomputed mechanically from the merged
  table (`grep -c` per status, the same semantics as
  `scripts/update-coverage-summary.sh`): 36 rows, 11 n/a, 0
  not-started, 14 task-assigned, 11 implemented, 0 verified —
  11/36. `specs/coverage/SUMMARY.md` regenerated via the script
  (also picks up base-side platform-model.md row 7 / task-130 that
  the stale SUMMARY had missed).
- Migration-version invariant holds under the merge: the base's
  `2026-11-09-000060/000061` and this branch's
  `2026-11-12/13-000060/000061/000063` map to distinct full Diesel
  versions (date+sequence, dashes removed), so every migration runs;
  `scripts/check-migration-versions.sh` exits 0.

No task-124 source semantics changed: every code file auto-merged,
`cargo check -p gyre-server` and `cargo check -p gyre-server --tests`
both pass on the merged tree, and all mechanical invariant scripts
exit 0 (arch, ABAC route registry, ABAC exempt handlers, MCP write
gates, migration versions/portability, byte-slice truncation, dead
message kinds, mem port contracts, inert enforcement, relative path
defaults, fail-open ref resolution, fabricated scope defaults, lossy
secret conversion, scope literal defaults, task commit attribution,
forwarded-header trust, in-memory state stores, unbounded external
HTTP, forged scope fields). No duplicate axum route paths were
introduced by the merged `api/mod.rs`.

Fresh probes on the merged tree (logs under `review-evidence/`):

- `cargo test -p gyre-common` — 125 passed.
- `cargo test -p gyre-domain` — 510 + 8 passed.
- `cargo test -p gyre-adapters` — 517 passed (includes both the
  task-112 `user_channel_preferences` adapter tests and the task-124
  migration backfill/deactivation storage paths on real SQLite).
- `cargo check -p gyre-server --tests` — exit 0 (3m23s;
  `review-evidence/cargo-check-gyre-server-tests-merge-112.log`).
- Running the server test binary itself is deferred to the verifier
  per the sandbox memory budget (commands below).

### Prior rounds (context)

- Contract restoration round 0a1ec38f: same preservation pattern; the
  shipped notes were rewritten rather than appended.
- Rebase repair (merge of `6f5a631c`): the only code conflict was the
  `use users::{...}` import list in `api/mod.rs` (task-124 handlers vs
  task-125 notification handlers), resolved as the union; coverage
  narratives merged and mechanically recounted.
- Merge of `1ee52c43` (this round's base, recorded above): only
  conflict was the coverage matrix header; union + mechanical
  recount. Earlier contract-restoration rounds (cebe3a499 and prior)
  preserved the shipped production fixes unchanged and re-verified
  them; review finding 3cd87005 (stale 7-argument
  `list_for_user` call in `deactivate_user_cascades_to_auth_and_
  ownership`) remains fixed by commit 33508170.
- 38b3134f: fixed a test-fixture bug — `seed_workspace_with_owner`
  built the Owner membership as pending instead of the production
  `create_workspace` semantics (accepted immediately), which had broken
  `workspace_invitation_lifecycle`.

### Server tests the verifier must execute

Large gyre-server test compilation is deferred to the independent
verifier per the sandbox budget:

- `cargo test --all` — the assignment's definition-of-done gate; the
  compile error repaired earlier (E0061 in
  `deactivate_user_cascades_to_auth_and_ownership`) was the blocker
  and `cargo check -p gyre-server --tests` now passes.
- `cargo test -p gyre-server api::users::tests` — includes
  `deactivate_user_cascades_to_auth_and_ownership` (the repaired call
  site), `workspace_invitation_lifecycle`,
  `get_user_by_username_is_tenant_scoped`,
  `list_users_is_tenant_admin_only_scoped_and_paginated`,
  `ownership_transfer_unblocks_old_owner_removal_and_is_audited`,
  `last_owner_cannot_be_removed_or_downgraded`,
  `member_management_requires_owner_admin_or_tenant_admin`,
  `invite_member_rejects_duplicate_membership`, plus the task-125
  notification handlers that share `users.rs` after the rebase
  (`list_my_notifications`, `get_unread_count`,
  `mark_all_read_notifications`, `notification_stream`).
- `cargo test -p gyre-server api::impersonation::tests` — includes
  `deactivating_target_revokes_impersonation_token`,
  `deactivating_impersonator_revokes_impersonation_token`,
  `resolver_rejects_deactivated_accounts_even_with_unended_session`.
