# Review: TASK-124 — User Management & Workspace Membership API Endpoints

**Reviewer:** Independent review (round 3)
**Candidate:** `eae42bc6113f0fc9c2999809d3c96123d16e1e56` (base `199c1bb78f53f27088c54a0929b5605e7302e874`)
**Date:** 2026-11-13
**Verdict:** PASS

---

## Prior-round findings — both verified repaired

- **R1 (impersonation deactivation gap, fixed in `6033324f`):** `resolve_impersonation_auth`
  (crates/gyre-server/src/impersonation.rs) now rejects a deactivated target
  (`target.is_deactivated()` → None) AND loads + checks the impersonator's account
  (deactivated or missing → None), shared by all three call sites (HTTP extractor step-1.5,
  WS `authenticate_token`, global auth gate). `deactivate_user` additionally ends active
  impersonation sessions inline (`end_active_sessions_for_user`). Three regression tests in
  `crates/gyre-server/src/api/impersonation.rs` (`deactivating_target_revokes_impersonation_token`,
  `deactivating_impersonator_revokes_impersonation_token`,
  `resolver_rejects_deactivated_accounts_even_with_unended_session` — the last bypasses the
  cascade so only the resolver's account-state check can reject) are hard tests: each
  pre-conditions that the token authenticates (200) before deactivating, then asserts 401.
- **R2 (false "verified covered by task-111" claim, repaired in `2d45596d`):** the task's
  Session-endpoints section now truthfully states the endpoints are NOT implemented. All
  three factual claims re-verified independently: no `me/sessions` routes anywhere in
  `crates/` (grep), task-111 is `not-started` with `commits: []`, and coverage row 26
  records the three session endpoints as open. The task's acceptance criteria do not
  include the session endpoints.

## Acceptance criteria — verified

1. **`GET /api/v1/users/{username}`** — real handler on `users.find_by_username` +
   `workspace_memberships.list_by_user`; tenant containment in-handler (cross-tenant → 404,
   no existence leak). Test: `get_user_by_username_is_tenant_scoped` (full router, positive +
   cross-tenant + nonexistent paths).
2. **`GET /api/v1/users`** — TenantAdmin-only per-handler check (stored global_role
   authoritative, JWT-claim lag handled), tenant-scoped, `limit/offset` (max 200) with
   `total`. Test: `list_users_is_tenant_admin_only_scoped_and_paginated` (Developer → 403,
   cross-tenant exclusion, pagination walk).
3. **`PUT /api/v1/users/{id}/deactivate`** — TenantAdmin-only, same-tenant, self → 409,
   idempotent (original stamp kept), audit-logged with reason. Real cascade: new
   `users.deactivated_at` column (migration `2026-11-13-000062`, next unused sequence,
   portable plain `ALTER TABLE`), rejected on the API-key extractor path, the WS
   `authenticate_token` API-key path, SSO `find_or_create_user` (and thus `validate_jwt`),
   and the impersonation resolver; ownerless flow treats a deactivated Owner as departed
   with inline TenantAdmin notification. Test:
   `deactivate_user_cascades_to_auth_and_ownership` asserts the pre-deactivation 200 →
   post-deactivation 403 on the same API key, ownerless transition record,
   `WorkspaceOwnerless` notification, exactly one audit event with reason, and idempotent
   re-stamp. Adapter round-trip test `deactivation_persists_and_round_trips`
   (gyre-adapters, SQLite) and domain test `deactivate_stamps_once_and_reports_state` pass.
4. **Invitation endpoints** — `GET .../invitations` (pending only, Owner/Admin/TenantAdmin),
   `POST .../invite` (spec-named alias of POST /members), `POST .../invite/accept`
   (per-handler: membership looked up by (caller, workspace), so nobody else's invitation
   can be accepted; ABAC-exempt with in-handler auth — consistent with the exempt-handler
   invariant script), `DELETE .../invitations/{id}` (pending only, cross-workspace id → 404).
   Test: `workspace_invitation_lifecycle` covers the full loop including foreign-accept →
   404, re-accept → 409, revoke-accepted → 409, revoke-pending → 204 with row deletion,
   re-revoke → 404.
5. **Ownership transfer** — `update_member_role` has last-active-Owner protection
   (`count_active_owners_excluding` skips deactivated users), transfer audit event
   (`workspace_ownership_transferred` with previous/new role + reason), and clears
   ownerless tracking on Owner assignment. Test:
   `ownership_transfer_unblocks_old_owner_removal_and_is_audited`.
6. **Routes + ABAC** — all new routes registered in `api/mod.rs` with
   `RouteResourceMapping` entries in `abac_middleware.rs`;
   `check-abac-route-registry.sh` passes.
7. **`cargo test --all`** — deferred to the verifier per the 4 GiB budget rule (see below).

## Probes run (evidence in `/tmp/stage/review-evidence/`)

- All 18 mechanical invariant scripts: exit 0 (arch, ABAC registry, migration
  versions/portability, mem port contracts, exempt handlers, byte-slice, relative paths,
  fail-open refs, scope defaults ×2, lossy secrets, inert enforcement, forwarded headers,
  in-memory state, unbounded HTTP, MCP write tools, dead message kinds).
- `check-task-commit-attribution.sh`: OK at the committed candidate (run in a temp
  worktree at `eae42bc6`; the failure against the working tree is solely the controller's
  task-file reset to `commits: []`, not a candidate defect).
- `cargo check -p gyre-server` — exit 0. `cargo check -p gyre-server --tests` — exit 0.
- `cargo test -p gyre-domain user` — 12 passed.
- `cargo test -p gyre-adapters --lib sqlite::user` — 16 passed (incl. deactivation
  round-trip).
- `update-coverage-summary.sh` re-run at the candidate worktree: only the date line
  differs — the SUMMARY flips for unrelated specs (meta-spec-reconciliation,
  platform-model, spec-lifecycle) are a faithful mechanical recount of per-spec coverage
  files already in the base, not fabricated claims.

## Non-blocking observations

- The candidate's own commit `eae42bc6` ("process: record task-124 branch commits")
  dropped the docs-only repair commit `2d45596d` from the `commits:` frontmatter. This is
  permitted by the mechanical attribution rule (docs/specs-only commits are out of its
  scope; both product-surface commits `33e30ac9` and `6033324f` are listed) and the
  repair's effect is present in the tree, so nothing is invisible to review scoping that
  the rule requires to be visible.
- `PUT /users/{id}/deactivate` requires a JSON body (documented in api-reference.md as
  `{reason?}`, may be `{}`); an empty-body PUT returns a 4xx. The spec does not define the
  body, so this is an API-shape choice, not a defect.

## Verifier must run (deferred from review per the 4 GiB rule)

    cargo test -p gyre-server --lib api::users
    cargo test -p gyre-server --lib api::impersonation
    cargo test --all
