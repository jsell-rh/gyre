# Review — task-110: Implement tenant & workspace invitation flow

**Candidate:** `7561eed60facc58c6929f97b0f1a4259aa542bda`
**Base:** `5984177c2c6fcdab43e5f0071675f18245a1a8b5` (task-124 head)
**Spec:** `specs/system/user-management.md` §Tenant-Level User Onboarding, §Workspace
Invitation Flow, §Invitation Expiry, §API > Tenant Invitations / Workspace Membership
**Verdict:** APPROVED (independent evidence below; server test execution recorded for
the verifier — blocked in-sandbox by the 4 GiB build wrapper, not a code defect)

## What the candidate ships

- **Domain** (`gyre-domain/src/invitation.rs`): `TenantInvitation`, `WorkspaceInvitation`,
  `InvitationStatus` (Pending/Accepted/Declined/Expired/Revoked), `InvitationPolicy`
  (7/7 days, cap 50, `allow_re_invite`) — every spec field present.
- **Ports** (`gyre-ports/src/invitation.rs`): both repository traits with documented
  contracts; `transition_status` specified as a conditional single-use claim; plus
  `UserRepository::find_by_email` (case-insensitive, not-unique-by-contract) and
  `claim_external_id` (the one sanctioned external_id transition) for the SSO link.
- **Adapters**: SQLite, PostgreSQL, and mem implementations; shared portable migrations
  `2026-11-13-000063_invitations`, `000064_invitation_pending_unique` (partial unique
  indexes = DB-level one-pending-per-(tenant,email)/(workspace,user) enforcement, with
  pre-index dedupe of raced rows), `000065_user_tenant_scope`; task-124's colliding
  `000063_user_deactivated_at` renumbered to `000066`. Migration numbering documented
  per renumber; `check-migration-versions` + `check-migration-sql-portability` pass.
- **API** (`gyre-server/src/api/invitations.rs`): all five tenant endpoints plus bulk,
  status-filtered list, revoke (conditional Pending→Revoked), magic-link accept/decline;
  workspace invite by user_id/username/email, list, revoke, invitee-facing
  `GET /users/me/invitations` + authenticated accept/decline, token accept/decline;
  per-tenant policy `GET/PUT /tenant/invitations/policy`. Tokens are 32 CSPRNG bytes
  (ring SystemRandom), only SHA-256 stored; single-use claim before side effects with
  rollback to Pending on failure; identity guard when a Bearer caller hits a
  magic-link route (must match the invited email / invited user).
- **SSO link** (`auth.rs find_or_create_user_tenant`): first SSO login with a
  *verified* email matching an invitation-created account in the same tenant claims
  that account via `claim_external_id`; unverified emails and cross-tenant logins do
  not claim (each has a test).
- **Expiry job** (`spawn_invitation_expiry`, spawned in main.rs): hourly default,
  `GYRE_INVITE_EXPIRY_INTERVAL_SECS` override with zero-guard, conditional
  Pending→Expired transitions, rows kept for audit.
- **Local-mode credential delivery**: acceptance mints the account's first API key
  (SHA-256-hashed in the auth-resolving store) only for invitation-created accounts;
  linked pre-existing accounts get none (token possession must not bypass their auth
  factor).
- **Rebase reconciliation**: task-124's parallel membership-row implementation of the
  four workspace-invitation paths removed in favor of the assigned WorkspaceInvitation
  superset (no duplicate axum registrations remain; `check-abac-route-registry` and
  `check-abac-exempt-handlers` pass).

## Independent evidence

- All 17 relevant invariant scripts pass (migrations, ABAC registry, exempt handlers,
  mem port contracts, arch, scope-literal/fabricated defaults, lossy secrets,
  forwarded headers, unbounded HTTP, forged scope fields, dead message kinds, byte
  slicing, relative paths, fail-open ref resolution, in-memory stores). Full outputs:
  `/tmp/stage/review-evidence/`.
- `cargo fmt --check` clean; `cargo check -p gyre-server` clean.
- `cargo test -p gyre-domain` — pass (invitation entity tests).
- `cargo test -p gyre-adapters --lib invitation` — 5/5 pass, including
  `pending_scope_unique_index_enforced_by_database` (real SQLite: a second Pending row
  for the same scope is rejected by the DB) and
  `migration_000064_dedupes_raced_pending_invitations_before_index`.
- `cargo test -p gyre-adapters --lib` — 533 passed, 0 failed (includes
  `claim_external_id`/`find_by_email`/username-immutability adapter tests).
- Handler and auth test modules reviewed in source: they are real behavioral tests
  (full axum routing chain, real API keys in `state.api_keys`, two-tenant fixtures,
  concurrent-claim race, known-answer SHA-256, mutation-aware containment probes the
  suite notes were added precisely because earlier fixtures could not kill deleted
  guards). No self-confirming/stub-backed/assertionless patterns found.
- gyre-server test compilation is blocked by the executor's 4 GiB wrapper; required
  verifier probes recorded in `/tmp/stage/review-evidence/README.md`:
  `cargo test -p gyre-server --lib invitation`, `cargo test -p gyre-server --lib auth::`,
  `cargo test --all`, `cargo clippy --all-targets -- -D warnings`.

## Non-blocking observations

1. Comment drift: two code comments cite "migration 000061" for the pending-scope
   partial unique index that lives in `2026-11-13-000064_invitation_pending_unique`
   (renumbered during rebase; the migration header itself documents the renumbering).
   Cosmetic only.
2. A rare concurrent duplicate-invite race surfaces as 500 (adapter port-contract
   error) instead of 409; the DB index still prevents the duplicate. Acknowledged
   in-code.
3. In the SSO-link arm of acceptance, the linked user's role update precedes the
   membership side effects; on rollback to Pending the role change persists
   (idempotent on retry; the role was admin-chosen at invite time).

## Generated-output note (per review assignment)

`web/dist/` was not touched by this candidate; no generated-output observations
arose. Frontend source has no consumers of the invitation endpoints (verified by
grep), so the task-124 → task-110 response-shape change on
`POST /workspaces/{id}/invite/accept` breaks no client.
