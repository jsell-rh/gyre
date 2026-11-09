# Review: TASK-110 — Implement tenant & workspace invitation flow

**Reviewer:** Review agent (round 8)
**Date:** 2026-11-08
**Comparison base:** fefde2946f3a0af1f212588f54591516fee0f7b2
**HEAD:** e42f9b0c491edc50393f353e24d1842787cb0e38
**Verdict:** approved

---

## Findings

None blocking. Prior-round findings (F1: `allow_re_invite` enforcement, F2:
DB-level pending-scope uniqueness) are repaired with production code and
independently re-verified this round.

### Contract coverage (user-management.md §Tenant-Level User Onboarding, §Workspace Invitation Flow, §Invitation Expiry, §Tenant Invitations)

- [x] **Domain entities.** `gyre-domain/src/invitation.rs`: `TenantInvitation`
  (all spec fields), `WorkspaceInvitation`, `InvitationStatus`
  (Pending/Accepted/Declined/Expired/Revoked), `InvitationPolicy` with the
  spec defaults (7/7/50/true).
- [x] **Ports.** `gyre-ports/src/invitation.rs`: both repository traits with
  the conditional `transition_status(id, from, to, accepted_at)` single-use
  contract (`Ok(false)` on lost race, `from` precondition enforced in the write).
- [x] **Migrations.** `2026-10-09-000057_user_tenant_scope` (user tenant_id /
  global_role columns — previously silently dropped by DB adapters, killing
  workspace invites on DB-backed deployments), `2026-10-10-000058_invitations`
  (both tables), `2026-11-08-000059_invitation_pending_unique` (partial unique
  indexes + raced-duplicate dedupe). Diesel versions (date+sequence) all
  unique; `check-migration-versions.sh` and
  `check-migration-sql-portability.sh` exit 0.
- [x] **All five tenant endpoints** plus policy GET/PUT, workspace
  invite/list/revoke, token accept/decline for both kinds, and the
  invitee-facing surface (`GET /api/v1/users/me/invitations`,
  `POST /api/v1/workspaces/{id}/invite/accept|decline`). Magic-link routes
  registered outside `require_auth` on the root router (token is the auth
  factor — spec's own trust model); all authenticated routes carry ABAC
  resolver mappings and per-handler tenant containment + role checks.
- [x] **Token handling.** 32-byte ring `SystemRandom` CSPRNG, hex-encoded,
  stored only as SHA-256 hash; token unique-indexed; `invite_url` returned
  only in the create response.
- [x] **Single-use race safety.** Accept claims Pending→Accepted via
  conditional UPDATE (SQLite/Postgres) / locked check-and-set (mem) BEFORE
  user/membership side effects, with compensating rollback to Pending on
  side-effect failure; decline/revoke/expiry use the same transition.
  `concurrent_claims_produce_exactly_one_winner` covers the race.
- [x] **Expiry job.** `run_expiry_once` marks past-due Pending invitations
  Expired (never deleted); spawned hourly in main.rs alongside the other
  background jobs, `GYRE_INVITE_EXPIRY_INTERVAL_SECS` override documented.
- [x] **SSO/local linking.** `UserRepository::claim_external_id` (all three
  adapters, refuses to steal an owned external_id) is the single sanctioned
  re-point; the auth email-link path requires `email_verified=true` AND
  same-tenant email match — positive, unverified-email, and cross-tenant
  tests all present.
- [x] **F1 (round 7).** `allow_re_invite: false` now rejects re-invites of
  terminal-state invitations with 409 on both tenant and workspace surfaces;
  default `true` preserves the spec's re-invite-with-fresh-token flow.
- [x] **F2 (round 7).** Partial unique indexes
  `(tenant_id, email) WHERE status='Pending'` and `(workspace_id, user_id)`
  variant make the DB the arbiter; adapters map the unique violation to the
  port-contract error; migration dedupes raced duplicates before index
  creation (driven against real SQLite by
  `migration_000059_dedupes_raced_pending_invitations_before_index`).

### Independent verification (this round)

- **Test suites** (all crates touched by the candidate, full runs):
  gyre-domain 116/0, gyre-ports 440/0, gyre-common 8/0, gyre-adapters 390/0
  (12 ignored — Postgres tests requiring live DB, standing repo pattern),
  gyre-server lib 1507/0, gyre-server integration 170/0 (api, auth,
  conversation, dependency, e2e ralph, explorer ws, git, graph, OIDC,
  search). `cargo test --all` itself is blocked by the sandbox's
  per-package cargo wrapper; per-crate coverage of every touched crate is
  equivalent for the changed code. Verifier should run the workspace-wide
  suite.
- **Mutation probes** (source edited, test run, source restored — tree
  verified clean afterwards):
  1. Removing the F1 `allow_re_invite` check in `invite_to_workspace` →
     `allow_re_invite_false_blocks_workspace_reinvite_after_revoke` FAILS.
  2. Neutering `run_expiry_once` (early `Ok(0)`) →
     `expired_invitation_cannot_be_accepted_and_job_marks_expired` FAILS.
  3. Disabling `require_matching_invitee` on tenant accept →
     `tenant_accept_rejects_mismatched_bearer_identity` FAILS.
  All three tests kill real bugs; none is self-confirming.
- **Mechanical guards** (all exit 0): check-arch, check-abac-route-registry,
  check-abac-exempt-handlers, check-migration-versions,
  check-migration-sql-portability, check-mem-port-contracts,
  check-inert-enforcement, check-fabricated-scope-defaults,
  check-non-atomic-creation, check-mcp-write-tools.

### Observations (non-blocking)

- Local-mode accounts authenticate subsequently via SSO account-linking or
  API keys; there is no password infrastructure anywhere in the repo. The
  spec's "magic link **or** password setup" is disjunctive and the magic
  link is fully implemented, so this satisfies the task contract; a
  password-set flow would be new scope.
- Workspace-invitation in-app notification reuses
  `NotificationType::TrustSuggestion` with `kind: "workspace_invitation"`
  in the body — no dedicated notification type exists; cosmetic only.
- "Email if configured" (spec §Workspace Invitation Flow step 3): no email
  delivery infrastructure exists in the repo; the mandatory "in-app always"
  half is implemented and carries the accept/decline routes.

Evidence: `/tmp/stage/review-evidence/` (guard logs, test logs, mutation
probe logs).
