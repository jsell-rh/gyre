# Review — task-110 (Implement tenant & workspace invitation flow)

Spec: `specs/system/user-management.md` §Tenant-Level User Onboarding, §Workspace
Invitation Flow, §Invitation Expiry, §Tenant Invitations.
Candidate under review: `15a0b0707d94328bd52bbf562baea637b230058b` (assignment
base `8d07a41a`). Server-side crates byte-identical to the prior approved head
`aa92dbe9`; this round's diff is the rebase reconciliation onto the
task-126-carrying base plus task-file/coverage bookkeeping.
Verdict: **needs-revision** — one blocking finding (F1: the invitee-facing
authenticated accept/decline routes are unreachable through the production
ABAC chain). The invitation implementation itself (entities, ports, adapters,
migrations, tenant endpoints, magic-link acceptance, policy, expiry job,
mem adapters, SSO link, tests) verified clean.

Evidence: `/tmp/stage/review-evidence/` (probe logs, check outputs, F1
analysis with file/line references).

## Probes run

- `cargo check -p gyre-server --tests` — clean (16m09s; wrapper-allowed;
  `cargo-check-gyre-server.log`).
- `cargo test -p gyre-domain -p gyre-ports -p gyre-adapters` — **all pass**
  (533 + 539 + 8 passed, 0 failed), including the real-SQLite invitation
  adapter suite: `tenant_invitation_roundtrip`,
  `workspace_invitation_roundtrip`,
  `transition_status_rejects_wrong_source_status`,
  `pending_scope_unique_index_enforced_by_database`,
  `migration_000064_dedupes_raced_pending_invitations_before_index`, plus
  `sqlite::user` `tenant_scope_and_global_role_round_trip`,
  `claim_external_id_is_the_sanctioned_repoint`,
  `find_by_email_is_case_insensitive_and_returns_all_matches`
  (`cargo-test-adapters.log`).
- All 21 repo mechanical invariant scripts PASS
  (`check-arch`, `check-abac-route-registry`, `check-abac-exempt-handlers`,
  `check-mem-port-contracts`, `check-migration-versions`,
  `check-migration-sql-portability`, `check-in-memory-state-stores`,
  `check-inert-enforcement`, `check-task-commit-attribution` (against HEAD
  state; the executor's assignment reset reverts task-110.md's `commits:`
  frontmatter, which fails the check on the reset working tree and passes
  once restored — restored afterwards, no source edits), and the rest).
- matchit duplicate-`{param}`-name concern on
  `/api/v1/workspaces/:id/invitations/:id` resolved as a non-issue: matchit
  0.7.3 normalizes param names per route; axum `Path<(String, String)>` is
  positional.

## Verified working (no findings)

- **Domain/ports/adapters real.** `TenantInvitation`/`WorkspaceInvitation`/
  `InvitationPolicy`/`InvitationStatus` in gyre-domain; both repository ports
  with a `transition_status` conditional-claim contract; SQLite, Postgres,
  and mem adapters all enforce the pending-scope uniqueness (code guard +
  partial unique index migration `2026-11-13-000064` with raced-duplicate
  dedupe, violation mapped to the port-contract error in all adapters) and
  the conditional `UPDATE ... WHERE status = from` single-use claim.
  Migrations numbered 000063-000066 continue the used sequence; portable
  SQL (SQLite + PG); `check-migration-versions` passes.
- **Tenant invitation endpoints.** All five spec endpoints live
  (invite/bulk/list/revoke/magic-link accept), TenantAdmin-only per-handler
  (`require_tenant_admin`), tenant containment inherent (invitation created
  in the caller's stored tenant), 409 on duplicate pending, `allow_re_invite`
  enforcement on both surfaces, per-tenant policy via kv store with
  fail-closed corrupt-row handling, `expires_in_days` validation.
- **Token handling.** 32-byte ring `SystemRandom` CSPRNG, hex; only SHA-256
  hash stored (known-answer test); single-use claim before side effects with
  rollback to Pending on failure; expired tokens transitioned to Expired and
  rejected.
- **Local-mode credential delivery.** Acceptance mints the account's first
  API key (hash persisted in the auth-resolving store; proven to authenticate
  through the real extractor in `local_mode_accept_mints_authenticating_api_key`),
  with `api_key: null` for pre-existing SSO-bound accounts — token possession
  cannot bypass the IdP. Username contract enforced pre-claim (400/409
  retryable); deterministic suffix dedupe across tenants.
- **SSO link.** `find_or_create_user_tenant` claims a
  `tenant:{t}:email:{e}` account on first SSO login only with
  `email_verified=true` and within the same tenant, via the sanctioned
  `claim_external_id` port transition; unverified-email and cross-tenant
  non-claims are regression-tested (`auth::tests`).
- **Expiry job.** `run_expiry_once` + `spawn_invitation_expiry` (spawned in
  main.rs; interval env override with zero-guard), conditional
  Pending→Expired transitions, rows kept for audit.
- **Magic-link routes** (`/api/v1/invite/:token/accept|decline`,
  `/api/v1/workspaces/invitations/:token/accept|decline`) correctly live
  outside the auth middleware chain (token is the auth factor) with an
  optional-Bearer invitee-match guard.
- **Coverage matrix.** user-management rows 5/6/7/25 → `implemented` with
  accurate notes; header counts match the mechanical recount; row 27 records
  the merge disposition of task-124's parallel handlers (removed with their
  tests; task-110's suite covers the same paths).
- **Rebase reconciliation** onto base `8d07a41a` verified: server crates
  byte-identical to `aa92dbe9`; task-126 CLI invitation paths resolve against
  the registered routes with matching body shapes; commit attribution
  restored and passing.

## Findings

- [**F1 — blocking (code): invitee-facing workspace invitation accept/decline
  are denied by the ABAC middleware in the production chain.**]
  `POST /api/v1/workspaces/{id}/invite/accept` and `.../decline`
  (`accept_my_workspace_invitation`/`decline_my_workspace_invitation`,
  `crates/gyre-server/src/api/invitations.rs`) are mapped in
  `abac_middleware.rs` (lines ~609-616) as non-exempt `workspace`-resource
  routes. For any pattern under `/api/v1/workspaces/`, the middleware sets
  `resource.workspace_id` and then `subject.member_of_resource_workspace`
  from the real membership store — **false for the invited user, who by
  construction is not yet a member** (`invite_to_workspace` 409s on an
  existing membership; the membership is created only at acceptance). The
  seeded builtin `workspace-membership-required` (Deny, priority 940, scope
  Tenant/None, conditions `subject.type == "user"` ∧
  `subject.member_of_resource_workspace == false`, actions/resource `*`) then
  wins evaluation — nothing above 940 matches a normal invitee
  (system-full-access 1000 is system-principal-keyed; tenant-isolation 950
  requires differing tenants) — and the middleware returns 403 before the
  per-handler invitee check runs. The mechanism is live-proven by the existing
  test `builtin_workspace_membership_required_denies_non_member_live`
  (`abac_middleware.rs` ~7185). Every test of these two handlers
  (`invitee_lists_and_accepts_their_invitation`,
  `other_user_cannot_accept_my_invitation`,
  `invitee_declines_via_authenticated_route`) builds a bare router **without**
  the ABAC middleware, so the production pipeline is never exercised and the
  suite is green while the spec-named invitee endpoint
  (user-management.md §API > Workspace Membership) and the `accept_url`
  embedded in the invitee's `InvitationReceived` notification return 403 in a
  real deployment. The token-based magic-link routes still work, so the flow
  degrades to token-only acceptance.
  **Repair:** register the two patterns as `RouteResourceMapping::exempt`
  (the model the routes' own per-handler authorization already implements —
  and the registration task-124 used before the merge), then add a regression
  test through the production pipeline (`api_router` + layered
  `abac_middleware` + `seed_builtin_policies`) asserting the invitee gets
  200/204. Full analysis with the file/line evidence chain and the exact
  verifier probe: `/tmp/stage/review-evidence/F1-invitee-accept-blocked-by-abac.md`.

- [-] **Non-blocking (process): `specs/coverage/SUMMARY.md` rows for
  abac-policy-engine (committed 3 assigned/14 implemented), agent-gates
  (14/2), and analytics (4/8) do not match a mechanical recount of the same
  tree's coverage tables (1/16, 13/3, 2/10; TOTAL 193/226 vs committed
  198/221).** Direction is conservative (undercounts implemented); the rows
  predate this branch (the base was staler still) and concern other tasks'
  specs; the user-management row this task touched is correct. The
  verifier/auditor should regenerate SUMMARY.md
  (`bash scripts/update-coverage-summary.sh`) after integration.

## Deferred to the verifier (blocked in-sandbox by the 4 GiB build wrapper)

- `cargo test -p gyre-server --lib api::invitations` (28 tests).
- `cargo test -p gyre-server --lib auth::tests` (invitation email-link tests).
- The F1 regression probe described above (predicted: 403 on this candidate).
- Full `cargo test --all` and the exact-head GitHub checks.
