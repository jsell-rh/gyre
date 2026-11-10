# Review — task-110 (Implement tenant & workspace invitation flow)

Spec: `user-management.md` §Tenant-Level User Onboarding, §Workspace
Invitation Flow, §Invitation Expiry, §Tenant Invitations.
Candidate under review: `8efdd1c071f0036420d576ad7d67d06143c2e0b4`
(base `402142b1c8d214b03977d689742baae6a1af7910`, product commits
`8f66d5a`, `0f4fea9`, `8c469c5`, `c1de316`, `797cf52` + round-8/10
repairs; round-10 docs commit `58a012bc5`). This is review round 11;
rounds 1–9 findings were repaired and re-verified in prior rounds.

Verdict: **PASS** (approved).

## Independent evidence (this round, at candidate head, clean tree)

All commands run with `/tmp/stage/bin/cargo`, `SKIP_WEB_BUILD=1`.

- `cargo test -p gyre-server --lib api::invitations` → 26 passed / 0 failed.
- `cargo test -p gyre-server --lib auth::` → 49 passed / 0 failed.
- `cargo test -p gyre-server --lib` (full) → 1602 passed / 0 failed.
- `cargo test -p gyre-adapters` → 416 passed / 0 failed / 6 ignored,
  including `pending_scope_unique_index_enforced_by_database`,
  `migration_000059_dedupes_raced_pending_invitations_before_index`,
  `transition_status_rejects_wrong_source_status`,
  `claim_external_id_is_the_sanctioned_repoint`,
  `find_by_email_is_case_insensitive_and_returns_all_matches`,
  `tenant_scope_and_global_role_round_trip`.
- `cargo test -p gyre-domain -p gyre-ports` → 454 passed / 0 failed.
- Mechanical guards (exit 0): check-migration-versions, check-arch,
  check-abac-route-registry, check-abac-exempt-handlers,
  check-mem-port-contracts, check-forged-scope-fields,
  check-inert-enforcement, check-migration-sql-portability,
  check-scope-literal-defaults, check-fabricated-scope-defaults,
  check-lossy-secret-conversion, check-in-memory-state-stores.

### Mutation probes (each reverted; `git diff` verified empty after)

1. Removed the `from`-status precondition from the mem adapters'
   `transition_status` → `concurrent_claims_produce_exactly_one_winner`
   FAILED. The single-use race guard is real, not mirrored logic.
2. Removed the `require_matching_invitee` bearer-identity guard from
   `accept_tenant_invitation` →
   `tenant_accept_rejects_mismatched_bearer_identity` FAILED.
3. Disabled `require_tenant_admin` → `non_admin_cannot_invite_via_http`
   FAILED. Authorization is enforced through the full HTTP routing
   chain, not just unit-level.

## Verified against the acceptance criteria (10/10)

- **Domain entities** (`gyre-domain/src/invitation.rs`):
  `TenantInvitation` with every spec field (role, workspace_ids/
  workspace_roles parallel arrays, token_hash, expires_at,
  accepted_at), `InvitationStatus` all 5 variants, `InvitationPolicy`
  (7/7/50 defaults + allow_re_invite), `WorkspaceInvitation`. Real
  unit tests incl. expiry-only-when-Pending semantics.
- **Ports** (`gyre-ports/src/invitation.rs`): both repository traits
  with documented duplicate-rejection and atomic conditional
  `transition_status` contracts.
- **SQLite adapter + migrations**: 000058 (tables, token_hash unique),
  000059 (partial unique pending-scope indexes + portable raced-dup
  dedupe), 000060 (users.tenant_id index, renumbered past upstream
  version collisions — documented). Postgres adapter mirrors SQLite
  contract-for-contract; mem guards in code under the store lock
  (check-mem-port-contracts OK).
- **All 5 tenant endpoints functional** at the HTTP layer (lifecycle
  test covers create → list → revoke → accept; expiry test covers the
  expired-token path and the background job's `run_expiry_once`).
- **Workspace lifecycle**: invite (by user_id/username/email, selector
  ambiguity rejected), pending, token accept/decline, authenticated
  invitee accept/decline (`/workspaces/{id}/invite/accept|decline`,
  caller must be the invitee, 404 otherwise), revoke, expiry; accept
  activates membership idempotently; decline leaves none.
- **Token crypto**: 32 bytes from ring `SystemRandom`, stored only as
  SHA-256 hex; `token_is_csprng_and_hashed` asserts hash-only storage
  and non-determinism across generations.
- **Expiry job**: spawned from main.rs, `GYRE_INVITE_EXPIRY_INTERVAL_SECS`
  (0 → default, no panic), immediate first tick, conditional
  Pending→Expired transitions that never overwrite terminal statuses.
- **Bulk invite** accepts an array; per-entry errors reported without
  aborting valid rows.
- **Routes + ABAC**: all management routes registered in `api_router`
  with `RouteResourceMapping` entries; magic-link routes correctly
  live outside the auth middleware chain (token is the auth factor)
  while rate-limit/tracing still apply; per-handler TenantAdmin /
  Owner-Admin / tenant-containment enforcement proven by negative
  HTTP tests and mutation probes.
- **cargo test** green across server/adapters/domain/ports (above).

## Prior-round repairs spot-verified at this head

- SSO/local account linking on accept (`find_by_email` tenant-scoped,
  case-insensitive; `claim_external_id` as the single sanctioned
  external_id transition; unverified-email and cross-tenant link
  negative tests).
- `allow_re_invite: false` enforcement on both surfaces; pending-scope
  uniqueness at the DB level (partial unique index) with raced-dup
  dedupe migration.
- `load_policy` fails closed on corrupt rows; PUT replaces (remediation
  path).
- Coverage rows 5/6/7/25 in `specs/coverage/system/user-management.md`
  accurately reflect the implemented state (marked "Pending
  independent review" — this review is that confirmation).

## Non-findings (checked, acceptable)

- Exemption-file edits are comment-only documentation of previously
  fixed entries; no new exemptions added.
- `2026-10-10-000058` vs `2026-11-08-000058` directory names do not
  collide under Diesel version semantics (version = full date prefix,
  dashes removed); `check-migration-versions` OK.
- Two `000057` dirs likewise distinct versions (different dates).
- SSO-link path applies the JWT roles to the claimed account while
  leaving `global_role` from the invitation — consistent with the
  inviting admin's choice at invite time; documented in-code.
- Magic-link `invite_url` (raw token) returned only in create
  responses to the inviting admin — the delivery mechanism for the
  out-of-band magic link; only the hash is persisted.

Summary: the invitation flow is a real, production implementation at
every layer (domain → ports → three adapters → HTTP + background job),
with enforcement that survives mutation probes and negative tests
through the full routing chain. No findings.

— Reviewer, 2026-11-09 (round 11)
