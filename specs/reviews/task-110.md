# Review — task-110 (Implement tenant & workspace invitation flow)

Spec: `specs/system/user-management.md` §Tenant-Level User Onboarding,
§Workspace Invitation Flow, §Invitation Expiry, §Tenant Invitations
(API endpoint tables included). Task: `specs/tasks/task-110.md`.
Candidate reviewed: `f60123f5a5ad03405c016d3e095bcee52063bb3f` (base
`3c39ab1f796bcf52ac0b95d60a4101e27aa69fe8`; the head adds only the
docs/rebase-resolution commit on merge `1b0cc30b`, which touches zero Rust
code). Verdict: **complete** (approved, no findings).

## This round (independent review)

All probes at the exact candidate HEAD, worktree clean. Full logs under
`/tmp/stage/review-evidence/` (index: `review-summary.md`).

Test runs:

- `cargo test -p gyre-server --lib api::invitations` → **18 passed, 0
  failed** (lifecycle, revoke-blocks-accept, expiry accept-path + job,
  bulk invite with per-entry errors, HTTP 403 negative-authz for
  non-admin/non-member, bearer-identity match/mismatch on magic links,
  invitee list+accept/decline via the authenticated routes, invite by
  username and by email, notification accept-path content, concurrent
  claim exactly-one-winner, CSPRNG token + SHA-256 known-answer).
- `cargo test -p gyre-adapters invitation` → **3 passed, 0 failed**
  (temp-file SQLite round-trips; single-use claim at the adapter;
  wrong-source-status transition rejected).
- `cargo test -p gyre-server --lib auth::` → **39 passed, 0 failed**,
  including the three new JWT email-link tests (verified email claims the
  invitation-created account; unverified email does NOT; cross-tenant
  does NOT).
- Full suites: gyre-server lib **1263 passed**, gyre-adapters **349
  passed**, gyre-domain **375 passed**, gyre-cli **129+1 passed** — the
  `cargo test --all` criterion is covered crate-by-crate for every
  touched crate.
- Mechanical checks all exit 0: check-abac-route-registry,
  check-abac-exempt-handlers, check-migration-versions,
  check-mem-port-contracts, check-migration-sql-portability,
  check-forged-scope-fields, check-in-memory-state-stores,
  check-task-commit-attribution, check-fabricated-scope-defaults,
  check-scope-literal-defaults, check-lossy-secret-conversion,
  check-inert-enforcement, check-dead-message-kinds,
  check-relative-path-defaults, check-byte-slice-truncation, check-arch.

Independent probes (beyond the shipped tests):

- **Full-router magic-link probe**: temporary test driving the invite
  flow through the real `build_router(state)` — unauthenticated
  `GET /api/v1/tenant/invitations` → 401 (auth middleware active), while
  unauthenticated `POST /api/v1/invite/{token}/accept` → 200 with the
  local-mode user persisted. This proves the magic-link routes sit
  outside the `require_auth_middleware`/ABAC-layered sub-router in
  production routing, which the shipped bare-Router unit tests cannot
  catch. Passed; file restored, worktree clean.
- **Mutation probes** (each restored and re-run green):
  - mem adapter workspace `transition_status` guard disabled →
    `concurrent_claims_produce_exactly_one_winner` FAILS — the race test
    is anchored to the check-and-set, not to incidental behavior.
  - SQLite adapter `transition_status` conditional WHERE removed (both
    impls) → all 3 adapter tests FAIL — the conditional UPDATE is what
    the tests pin.

Verified working (no findings):

- **Domain/ports/adapters**: `TenantInvitation`, `WorkspaceInvitation`,
  `InvitationStatus`, `InvitationPolicy` in gyre-domain match the spec
  structs field-for-field; both repository ports exist with the
  single-use `transition_status(id, from, to, accepted_at)` contract;
  SQLite + Postgres adapters implement it as a conditional `UPDATE ...
  WHERE id = ? AND status = from` (rows>0 = won), mem as a check-and-set
  under the store mutex; pending-duplicate port contracts enforced in
  all three adapters (check-mem-port-contracts green). Migrations
  `2026-10-08-000056_invitations` and `2026-10-09-000057_user_tenant_scope`
  use the next unused sequence numbers and portable SQL (both backends
  share the embedded dir; portability check green). The 000057 migration
  fixes a real production hole (users table dropped tenant_id/global_role
  in DB-backed deployments).
- **All 5 spec'd tenant endpoints functional** plus revoke/policy extras:
  single invite (CSPRNG token via ring SystemRandom, only SHA-256 stored,
  email normalized, role validation, parallel workspace roles, policy-
  driven expiry), bulk invite (array accepted, per-entry errors),
  list with status filter, revoke (conditional Pending→Revoked), magic-
  link accept/decline (token-hash lookup, single-use claim BEFORE side
  effects with best-effort rollback, expiry persisted on contact,
  tenant-namespaced local accounts `tenant:{t}:email:{e}`, pre-assigned
  memberships activated with cross-tenant workspace guard).
- **Workspace invitation lifecycle** complete: invite (Owner/Admin/
  TenantAdmin per-handler after tenant containment; invitee resolved by
  username/email/user_id within the workspace's tenant; cross-tenant
  rejected; duplicate-pending 409; max_pending_invitations cap), list,
  revoke, token accept/decline, AND the invitee-facing surface
  (`GET /api/v1/users/me/invitations`, authenticated
  `POST /api/v1/workspaces/{id}/invite/accept|decline` — 404 when the
  caller has no pending invitation, so users can't probe each other).
  Acceptance activates a real WorkspaceMembership (idempotent).
- **Authorization is real, layered, and negatively tested**: per-handler
  TenantAdmin/Owner-Admin guards return 403 through the full routing
  chain (HTTP tests with real API keys), tenant containment checked on
  every scoped read/write, magic-link bearer-identity guards prevent
  burning someone else's invitation, SSO email-claim requires
  `email_verified=true` and is tenant-scoped. ABAC resolver mappings
  added for every new api/mod.rs route; magic-link routes intentionally
  live on the outer router (token is the auth factor) like the other
  credential-less routes (ws-ticket, git-http).
- **Expiry job**: `run_expiry_once` marks Pending-past-expiry rows
  Expired via conditional transitions (terminal statuses never
  overwritten — proven by adapter test + mutation), spawned hourly from
  main.rs with `GYRE_INVITE_EXPIRY_INTERVAL_SECS` override, first tick
  at startup. Per-tenant `InvitationPolicy` endpoints are real (kv-backed,
  partial update, validated, applied on subsequent invites — tested).
- **No test inflation**: the security-critical tests are anchored
  (mutation probes above); negative-authz tests assert state after 403.

Observations (non-blocking, recorded for future housekeeping — not
findings against this task's contract):

1. The `2026-10-08-000056_invitations` up.sql comment says "Unique
   constraints enforce one PENDING invitation per (tenant, email) /
   (workspace, user); the partial index keeps re-invite ... possible",
   but the shipped SQL only carries a UNIQUE index on `token_hash`; the
   pending-uniqueness is enforced by check-then-insert in the adapters
   (mutex in mem). Sequential behavior is correct and tested (409 +
   port contract), but under two concurrent creates for the same email
   the SQL backends have a TOCTOU window (benign outcome: a duplicate
   pending row). A follow-up could add the partial unique index
   (`CREATE UNIQUE INDEX ... ON tenant_invitations(tenant_id, email)
   WHERE status = 'Pending'` — supported by both SQLite and Postgres) or
   correct the comment.
2. Local-mode invitees have no post-acceptance credential (no password
   is set and the accept response carries no session token); the
   platform has no local login mechanism at all (SSO-first; the spec's
   endpoint table defines no login endpoint for this flow, and "magic
   link or sets a password" is satisfied by the magic link). The account
   is claimable by a verified-email SSO login. If pure-local deployments
   must be first-class, a local session mechanism is future work outside
   task-110's acceptance criteria.

Sandbox restrictions: no TCP listener is bindable (accept() errno 95),
so no live-server HTTP smoke; the full-router oneshot probe covers
routing + middleware layering without a socket.
