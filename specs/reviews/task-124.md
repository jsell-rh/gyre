# Review — task-124: Complete user management and workspace membership API endpoints

**Candidate:** `b5b14adcc7ce45e739795d96b14321ec0dada5b5`
**Base:** `6f5a631c9fc09664a3e766cd10536f8b56d9ce2f`
**Round:** 2026-11-13 (re-review after merge-conflict repair + base reconciliation)
**Verdict:** Approved (see /tmp/stage/verdict.json; evidence in /tmp/stage/review-evidence/)

## What was reviewed

Full diff base→candidate: `api/users.rs` (+~1100), `api/mod.rs`, `abac_middleware.rs`,
`auth.rs`, `impersonation.rs`, `api/impersonation.rs`, `workspace_ownership.rs`,
`domain/user.rs`, SQLite/PostgreSQL user adapters, Diesel schema, migration
`2026-11-13-000063_user_deactivated_at`, docs, coverage, task notes.

## Findings from prior rounds — verified fixed

- **E0061 stale `list_for_user` call (users.rs test, 7 args vs 8 after task-125's
  `repo_id: Option<&Id>` parameter):** fixed by 33508170. Reproduced the probe:
  `cargo check -p gyre-server --tests` now exits 0 (8m37s, log retained). No
  conflict markers anywhere in the candidate.

## Independent evidence

1. `cargo check -p gyre-server --tests` — exit 0.
2. `cargo test -p gyre-domain user::` — 12 passed (deactivation stamping + idempotency).
3. `cargo test -p gyre-adapters --lib deactivation_persists_and_round_trips` —
   passed against real SQLite storage (create/update/find_by_*/list all see
   `deactivated_at`).
4. All repo invariant scripts pass (ABAC route registry, exempt handlers,
   migration versions + SQL portability, arch, mem port contracts, inert
   enforcement, dead message kinds, byte-slice, relative paths, fail-open refs,
   fabricated scope defaults, lossy secrets, forged scope fields, forwarded
   headers, unbounded HTTP).
5. Manual policy-path analysis: builtin policies allow `read` on `user` for
   Developer/ReadOnly/Admin global roles, so the tenant-public profile is
   reachable in production (the middleware layer sits outside `api_router`,
   which the unit tests bypass); the TenantAdmin-only routes enforce the
   restrictive gate per-handler (tested Developer → 403), consistent with the
   POST /users bootstrap split.

## Assessment against acceptance criteria

- `GET /users/{username}` — tenant-public profile with memberships; cross-tenant
  → 404 (no existence leak). Implemented + tested.
- `GET /users` — TenantAdmin-only (stored global_role authoritative, not JWT
  claim), tenant-scoped, paginated. Implemented + tested.
- `PUT /users/{id}/deactivate` — TenantAdmin-only, same-tenant, self-deactivation
  → 409, idempotent; real cascade: migration-backed `deactivated_at`, auth
  rejection on API-key/WS/SSO/user-JWT paths, impersonation sessions ended +
  resolver fail-closed on either side's deactivation, ownerless-workspace flow
  with inline TenantAdmin notification, audit event with reason. Implemented +
  tested behaviorally (same key 200→403, ownerless record, notification, audit).
- Invitation list/accept/revoke — implemented as real membership-row lifecycle
  (pending = unaccepted); invitee-only accept, revoke pending-only with
  workspace binding check; audited. Implemented + tested.
- Role change / ownership transfer — pre-existing (task-121) with last-active-Owner
  guard now correctly excluding deactivated Owners; transfer test present.
- Routes + ABAC mappings — all registered; `invite/accept` is resolver-exempt
  with per-handler invitee-only auth (any global role can be an invitee);
  check-abac-route-registry.sh and check-abac-exempt-handlers.sh pass.
- `cargo test --all` — test targets compile (checked); execution deferred to the
  independent verifier per the 4 GiB sandbox budget. Commands recorded below.

## Coverage honesty

Row 27 (§Workspace Membership) marked implemented — all seven endpoints live.
Row 26 (§User Management) honestly remains task-assigned: the three session
endpoints are owned by not-started task-111. No fake completion.

## Verifier must run (server tests not linked in this sandbox)

```
cargo test -p gyre-server --lib api::users::tests::get_user_by_username_is_tenant_scoped
cargo test -p gyre-server --lib api::users::tests::list_users_is_tenant_admin_only_scoped_and_paginated
cargo test -p gyre-server --lib api::users::tests::deactivate_user_cascades_to_auth_and_ownership
cargo test -p gyre-server --lib api::users::tests::workspace_invitation_lifecycle
cargo test -p gyre-server --lib api::users::tests::ownership_transfer_unblocks_old_owner_removal_and_is_audited
cargo test -p gyre-server --lib api::impersonation::tests::deactivating_target_revokes_impersonation_token
cargo test -p gyre-server --lib api::impersonation::tests::deactivating_impersonator_revokes_impersonation_token
cargo test -p gyre-server --lib api::impersonation::tests::resolver_rejects_deactivated_accounts_even_with_unended_session
cargo test --all   # acceptance criterion
```
