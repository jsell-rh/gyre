# Review — task-110 (Implement tenant & workspace invitation flow)

Spec: `specs/system/user-management.md` §Tenant-Level User Onboarding,
§Workspace Invitation Flow, §Invitation Expiry, §Tenant Invitations.
Candidate under review: `17b3307a4ce98a0b5fabc6d88b57d8772fe029c0` against
assignment base `2824d9b186d5da08ae043772762246ee5b620e89` (merged at
`7d932e41`). This candidate is the rustfmt-repair round: relative to the
previously verified `a309844`, the task-110-owned delta is commit `9f9be530`
(whitespace-only join of two `let invitation =` bindings in the invitee
accept/decline handlers) plus the process commit `17b3307a`; all other
content in the base..candidate diff is either task-110 work verified in
prior rounds or upstream base content.

## Round N (2026-11-14) — independent review of candidate `17b3307a`

Verdict: **approved** (no findings). Probes and evidence under
`/tmp/stage/review-evidence/`.

### Probes run (exit-coded, logs retained)

- `python3 scripts/check-rustfmt-diff.py 2824d9b1…` → exit 0, changed lines
  clean (22 Rust files) — the prior round's only failure is fixed.
- All 19 mechanical invariant scripts → PASS: check-arch,
  check-abac-route-registry, check-abac-exempt-handlers (81 handlers),
  check-mcp-write-tools, check-migration-versions,
  check-migration-sql-portability, check-mem-port-contracts,
  check-dead-message-kinds, check-byte-slice-truncation,
  check-relative-path-defaults, check-fail-open-ref-resolution,
  check-fabricated-scope-defaults, check-lossy-secret-conversion,
  check-scope-literal-defaults, check-inert-enforcement,
  check-forged-scope-fields, check-forwarded-header-trust,
  check-in-memory-state-stores, check-unbounded-external-http.
- `bash scripts/check-task-commit-attribution.sh` → exit 0 (all task-labeled
  product-surface commits recorded, including the rustfmt repair).
- `cargo test -p gyre-domain invitation` → 4/4 pass.
- `cargo test -p gyre-adapters invitation` → 5/5 pass, including
  `pending_scope_unique_index_enforced_by_database` (real SQLite rejects a
  second Pending row per scope at the DB level) and
  `migration_000064_dedupes_raced_pending_invitations_before_index`.
- `cargo check -p gyre-server --tests` → clean (7m37s). This is the
  sanctioned server type-check; the build wrapper rejects gyre-server test
  binaries in this sandbox (4 GiB rule) — see "For the verifier" below.

### Source review (full diff base..candidate, 37 files)

- **Domain** (`gyre-domain/src/invitation.rs`): `TenantInvitation` with every
  spec field, `WorkspaceInvitation`, `InvitationStatus`
  (Pending/Accepted/Declined/Expired/Revoked), `InvitationPolicy` with the
  spec defaults (7/7/50) and `allow_re_invite`.
- **Ports** (`gyre-ports/src/invitation.rs`): both repository traits with
  port-documented duplicate constraints (one Pending per (tenant,email) /
  (workspace,user)) and an atomic `transition_status(id, from, to)` whose
  `from` precondition must live in the write itself — the single-use
  acceptance guard. `UserRepository` gains `find_by_email` (case-insensitive,
  returns all matches, callers must scope by tenant — email is not unique
  across tenants) and `claim_external_id` (the single sanctioned
  external_id re-point for the SSO link).
- **Adapters**: SQLite, Postgres, and mem all implement both invitation
  repositories. SQLite/Postgres enforce the duplicate constraint as
  check-then-insert mapped to the port error PLUS a DB-level partial unique
  index (`2026-11-13-000064`, with pre-index raced-duplicate dedupe) as the
  race backstop; `transition_status` is a conditional UPDATE with the `from`
  status in the WHERE clause. The mem adapter guards both under one lock
  critical section. Migrations 000063–000066 are plain portable SQL
  (portability check passes), correctly sequenced, and the renumbering notes
  document the upstream version collisions they avoid.
- **API** (`api/invitations.rs`): all five spec tenant endpoints plus bulk
  (partial success with per-entry errors), per-tenant policy GET/PUT, the
  workspace invitation lifecycle (`POST /workspaces/{id}/invite` by
  user_id/username/email with Owner/Admin/TenantAdmin guard + tenant
  containment, list, revoke, invitee accept/decline both via token and via
  authenticated identity), and the invitee surface
  (`GET /users/me/invitations`). Tokens are 32 ring-SystemRandom bytes,
  hex-encoded, persisted only as SHA-256. Acceptance claims the invitation
  atomically BEFORE side effects and rolls back to Pending on failure;
  user creation dedups on retry via the tenant-namespaced external_id.
  Local-mode acceptance mints a first API key (only for accounts the
  invitation itself created — a pre-existing SSO/bootstrap binding gets
  `api_key: null`, so token possession cannot bypass an IdP). SSO-mode
  acceptance links the existing same-tenant account by email and applies
  the invited GlobalRole. Pre-assigned workspaces are validated against the
  invitation's tenant before membership activation.
- **Unauthenticated token routes** (`/api/v1/invite/:token/{accept,decline}`,
  `/api/v1/workspaces/invitations/:token/{accept,decline}`) are correctly
  registered in `lib.rs` OUTSIDE `require_auth_middleware` (the magic-link
  token is the auth factor); the `AuthenticatedAgent` extractor parses the
  Bearer header itself, so the optional-identity invitee-match guard
  (`require_matching_invitee`) is live code, and it rejects identity-less
  callers (agent/dev tokens) rather than letting them burn someone else's
  invitation.
- **ABAC**: routes registered in `api/mod.rs` with resolver mappings; the
  two invitee-facing accept/decline routes are resolver-exempt with genuine
  per-handler authorization (`authorize_my_pending_invitation`: tenant
  containment 403, caller-must-be-the-invitee 404) because the invitee is a
  non-member by definition — the exemption is justified and the
  production-chain regression test proves both that the seeded membership
  builtin still denies the non-member on workspace-mapped routes AND that
  the exempt routes work through the real `abac_middleware` layer.
- **Expiry**: `run_expiry_once` marks past-due Pending invitations Expired
  via the same conditional transition; `spawn_invitation_expiry` is wired in
  `main.rs` (hourly default, `GYRE_INVITE_EXPIRY_INTERVAL_SECS` override
  with zero-guard against tokio's panic). Handlers lazy-expire on touch.
  Policy loading fails CLOSED on a corrupt stored row.
- **SSO link** (`auth.rs`): `find_or_create_user_tenant` claims a
  local-mode invitation account on first SSO login only when the email
  matches within the same tenant AND `email_verified=true` — tests cover
  the claim, the unverified-email rejection, and the cross-tenant
  non-link. `claim_external_id` refuses to steal an external_id owned by
  another user.
- **Test quality**: no inflation found. The suite drives real axum routers
  (one test layers the production `api_router` + `abac_middleware` with
  seeded builtins), asserts on real store state (memberships, users,
  notifications, KV policy), includes the SHA-256 known-answer test, a
  concurrent-claim exactly-one-winner test, and negative controls
  (non-admin 403, cross-tenant 403, mismatched-bearer 403, revoked/
  expired-token 409). The task-124 membership-row stand-in for the four
  workspace-invitation paths was removed at the merge in favor of this
  model (duplicate route registrations would otherwise panic axum).

### For the verifier (blocked here only by the 4 GiB build-budget rule)

- `cargo test -p gyre-server invitations` — the full invitation suite
  (includes the ABAC production-chain regression).
- `cargo test -p gyre-server jwt_` — the SSO email-link tests in auth.rs.
- Workspace-wide `cargo test --all` (task acceptance criterion).

Generated-output observation (non-blocking, per checkpoint policy): no
`web/dist/` regeneration is part of this candidate; the authored frontend
surface for this task is none (no web/ changes in scope) and the verifier
rebuilds the bundle remotely regardless.
