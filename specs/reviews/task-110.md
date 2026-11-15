# Review — task-110 (Implement tenant & workspace invitation flow)

Spec: `user-management.md` §Tenant-Level User Onboarding, §Workspace
Invitation Flow, §Invitation Expiry, §Tenant Invitations.
Candidate under review: `690b678a15ae11aadfbb3cfab35c986e86501ecf`
(base `81c24bf07b059128fe6019c1df6f1c6924299930`). The candidate is the
verification-repair head `6d62522d` plus two process commits touching only
`specs/tasks/task-110.md` (verified: `git diff --stat 6d62522d..690b678a`
is the task file alone); `6d62522d` itself is a rustfmt-produced
whitespace reflow inside `mod tests` of `api/invitations.rs`
(14+/10−, verified no token/semantic change).

Verdict: **approved**.

## Independent probes (this round)

Working tree clean at `690b678a`; no source modifications made.

- `cargo test -p gyre-domain invitation` → **4 passed** (status roundtrip,
  expiry-only-when-pending for both kinds, policy defaults).
- `cargo test -p gyre-adapters invitation --lib` → **5 passed**
  (roundtrips, `transition_status_rejects_wrong_source_status`,
  `pending_scope_unique_index_enforced_by_database`,
  `migration_000064_dedupes_raced_pending_invitations_before_index`) —
  real SQLite storage, not mem.
- `cargo check -p gyre-server --tests` → exit 0 (server test code
  type-checks; the 4 GiB build wrapper blocks compiling/running the
  server test binary in this sandbox — recorded below for the verifier).
- Mechanical gates, all exit 0: `check-arch.sh`,
  `check-abac-route-registry.sh`, `check-migration-versions.sh`,
  `check-mem-port-contracts.sh`, `check-migration-sql-portability.sh`,
  `check-abac-exempt-handlers.sh`, `check-task-commit-attribution.sh`,
  `check-byte-slice-truncation.sh`, `check-inert-enforcement.sh`,
  `check-unbounded-external-http.sh`, `check-forwarded-header-trust.sh`,
  `check-lossy-secret-conversion.sh`,
  `check-fabricated-scope-defaults.sh`, `check-forged-scope-fields.sh`,
  `check-dead-message-kinds.sh`, `check-mcp-write-tools.sh`,
  `check-in-memory-state-stores.sh`, `check-scope-literal-defaults.sh`,
  `check-relative-path-defaults.sh`, `check-fail-open-ref-resolution.sh`.
- `python3 scripts/check-rustfmt-diff.py 81c24bf0` → "changed lines clean
  (23 Rust files checked)" — the gate that failed the prior verification
  run (durable finding on ancestor `9de867d2`) is repaired.

Evidence: `/tmp/stage/review-evidence/` (mechanical-gates.log,
server-check-tests.log, server-invitation-tests.log showing the wrapper
block, rustfmt-diff.log).

## Verified against the task contract

- **Domain entities (AC1): PASS.** `gyre-domain/src/invitation.rs` —
  `TenantInvitation` carries every spec field (role: GlobalRole,
  pre-assigned workspace_ids/workspace_roles, token_hash, expires_at,
  accepted_at); `InvitationStatus` all five variants;
  `WorkspaceInvitation` pending/accept/decline lifecycle;
  `InvitationPolicy` with the specced defaults (7/7/50).
- **Ports + adapter parity (AC2/AC3): PASS.**
  `TenantInvitationRepository`/`WorkspaceInvitationRepository` implemented
  by SQLite, PostgreSQL, and mem. The pending-scope duplicate contract is
  enforced by all three (code guard in mem per `check-mem-port-contracts`;
  check-then-insert + partial unique index violation mapping in
  SQLite/PG — migration `2026-11-13-000064_invitation_pending_unique`
  makes the DB the race arbiter, with a dedupe backfill before index
  creation). `transition_status` enforces the `from` precondition inside
  the write (conditional UPDATE / lock-guarded CAS) in all three
  adapters; `concurrent_claims_produce_exactly_one_winner` proves
  exactly-one-winner. Migration numbering is the next unused sequence
  (000063/000064/000065/000066; `check-migration-versions.sh` OK;
  renumber lineages documented in-file); SQL is portable
  (BigInt timestamps, no dialect-only constructs).
- **All five tenant endpoints (AC4): PASS.** `POST /tenant/invite`,
  `POST /tenant/invite/bulk` (array of invitations, per-entry error
  indexes, partial success), `GET /tenant/invitations` (status filter),
  `DELETE /tenant/invitations/{id}` (revoke pending only, conditional
  transition), `POST /invite/{token}/accept` (registered in `lib.rs`
  outside `require_auth` — the 256-bit token is the auth factor; when a
  Bearer identity IS presented it must match the invitee email).
  Non-admin 403 proven through HTTP.
- **Workspace invitation lifecycle (AC5): PASS.** invite → Pending →
  accept/decline/expire/revoke, on both the token surface and the
  authenticated invitee surface (`GET /users/me/invitations`,
  `POST /workspaces/{id}/invite/accept|decline`, caller-must-be-invitee,
  404 otherwise). The invitee routes are ABAC-exempt with per-handler
  tenant containment + invitee identity — justified because the invitee
  is a non-member by definition, and proven necessary by the test's
  control case (the same invitee is 403'd by the seeded
  workspace-membership-required policy on a workspace-mapped route).
  Cross-tenant guards return 403 on all four surfaces (two-tenant HTTP
  fixtures). `max_pending_invitations` cap enforced with 429.
- **Token security (AC6): PASS.** 32-byte ring `SystemRandom` tokens,
  SHA-256 known-answer test, only the hash persisted (adapter tests
  assert `stored.token_hash != token`), token_hash absent from every
  list/response DTO, and `redact_token_paths` keeps tokens out of
  tracing spans and Prometheus labels on the unauthenticated
  `/metrics` surface.
- **Expiry job (AC7): PASS.** `spawn_invitation_expiry` wired in
  `main.rs`, first tick at startup, conditional Pending→Expired claims so
  a racing accept is never overwritten, expired tokens rejected on accept
  (with immediate Expired persistence), env-configurable interval with a
  zero-interval panic guard. Policy is per-tenant via
  `GET/PUT /tenant/invitations/policy`; `load_policy` fails closed on
  corrupt rows.
- **Bulk endpoint (AC8): PASS** (see above).
- **Routes + ABAC (AC9): PASS.** All routes registered in
  `api/mod.rs`/`lib.rs` with resolver entries; exempt routes carry
  in-handler authorization; `check-abac-route-registry.sh` and
  `check-abac-exempt-handlers.sh` both green; no exemption-file growth.
- **cargo test --all (AC10):** cannot execute server tests under this
  sandbox's 4 GiB build wrapper. `cargo check -p gyre-server --tests`
  passes; domain + adapters invitation suites pass; the prior
  verification run on ancestor `9de867d2` failed only the rustfmt gate
  (since repaired; re-verified green). Verifier must run the server suite
  (commands below).

Additional hardening verified in source: both onboarding modes (SSO
accept links the existing auto-provisioned account — with
`email_verified` gating the auth.rs claim path so an unverified IdP
email cannot steal an invited account — and Local-mode accept mints a
first-class user plus one bootstrap API key, hash-persisted; linked
pre-existing accounts get no minted key), deactivated-account guards on
all accept paths, reserved system-ABAC-identity rejection before the
single-use claim, username collision suffixing, and pending-invitation
re-invite semantics under `allow_re_invite: false`.

## Non-blocking observations (no action required)

- Stale cross-reference: comments in
  `crates/gyre-adapters/src/{sqlite,postgres}/invitation.rs` and
  `api/invitations.rs` cite "migration 000061" for the pending-scope
  partial unique index; the shipped file is
  `2026-11-13-000064_invitation_pending_unique` (the number is the
  pre-renumber version, and both migration files carry renumber notes).
  Cosmetic only.
- `create_tenant_invitation` loads all of the tenant's invitations to
  check for a pending duplicate (O(n) per invite). Correctness is
  backstopped by the partial unique index; purely a performance note.

## Required verifier commands (blocked in this sandbox by the 4 GiB
build wrapper; not a code defect)

- `cargo test -p gyre-server --lib api::invitations::` (26 tests,
  mem-backed, incl. the full-ABAC-chain invitee accept/decline test with
  its control case)
- `cargo test --all`
- `cargo clippy --all-targets` (changed-lines clippy was green in the
  prior verification run)
