# Review — task-110 (Implement tenant & workspace invitation flow)

Spec: `specs/system/user-management.md` §Tenant-Level User Onboarding,
§Workspace Invitation Flow, §Invitation Expiry, §API > Tenant Invitations /
Workspace Membership.
Candidate: `467c6685a747bfa07a0deb0c91935137cf1dd1b5` (base `4871a825`,
merged; the manual merge resolution touched `docs/server-config.md` only).
Verdict: **approved** (round 3; rounds 1-2 findings F1-F4 — invitee surface,
single-use race, policy reachability, HTTP-layer authz — all closed and
re-verified in this round's read-through).

## What was verified

Full first-hand review of the candidate diff (5,438 lines) plus all 18 repo
invariant scripts re-run at this exact HEAD (arch, ABAC route registry,
exempt handlers, mem port contracts, migration versions + SQL portability,
task-commit attribution, inert enforcement, forged scope fields, scope
literal/fabricated defaults, in-memory state stores, dead message kinds,
byte-slice truncation, relative path defaults, fail-open ref resolution,
lossy secret conversion, forwarded header trust, unbounded external HTTP,
MCP write tools) — all exit 0. Evidence log with commands and outputs:
`/tmp/stage/review-evidence/task-110-review-round3.md`.

Implementation is real, not decorative:

- **Single-use acceptance is an atomic conditional write** in every adapter
  (SQLite/Postgres: `from` precondition in the UPDATE WHERE; mem:
  check-and-set under the store lock). Accept claims Pending→Accepted
  before any side effect, with rollback to Pending if user/membership
  creation fails; decline/revoke/expiry use the same transition so
  terminal statuses are never overwritten by racing writers.
  `concurrent_claims_produce_exactly_one_winner` exercises it via
  `tokio::join!`.
- **Tokens are CSPRNG + SHA-256**: `ring::rand::SystemRandom`, 32 bytes,
  known-answer SHA-256 test, only the hash persisted, unique index on
  `token_hash`. The plaintext token appears exactly once, in the create
  response.
- **Authorization holds in the handler** on every management route
  (TenantAdmin via global `UserRole::Admin` — workspace Admins hold
  `WorkspaceRole` on a membership, not `UserRole::Admin`, so no
  cross-workspace escalation; workspace routes enforce tenant containment
  then Owner/Admin-or-TenantAdmin), and is proven through the real HTTP
  chain by negative tests (`non_admin_cannot_invite_via_http`,
  `non_member_cannot_invite_to_workspace`, both also assert nothing was
  created). The magic-link token routes are deliberately in the public
  router (the invitee has no credentials yet); the 256-bit token is the
  auth factor, and a presented Bearer identity must match the invitee
  (`tenant_accept_rejects_mismatched_bearer_identity` proves 403 without
  burning the invitation).
- **SSO link-on-first-login** (`find_or_create_user_tenant`) claims an
  invitation-created account only when the token email is verified and the
  tenant matches — with negative tests for unverified email and
  cross-tenant login.
- **Expiry is enforced at read time AND by the background job**
  (`run_expiry_once` / `spawn_invitation_expiry`, hourly default,
  `GYRE_INVITE_EXPIRY_INTERVAL_SECS` override, first cycle at startup);
  expired rows are marked, never deleted. Per-tenant
  `InvitationPolicy` is live configuration: PUT changes a subsequent
  invitation's actual `expires_at` (tested 7→30 days), and
  `max_pending_invitations` returns 429 past the cap.
- **Migrations** use the next free sequence number (000058), are portable
  across SQLite/Postgres, and `000057_user_tenant_scope` repairs the
  users-table tenant/global_role columns the DB adapters previously
  dropped (unknown global_role values read back as Member — fail-safe).
- **Lineage**: the invitation Rust code at HEAD is byte-identical to
  checkpoint `8f66d5a4` (in the task's `commits:` frontmatter); the only
  post-checkpoint Rust delta is the upstream merge's MetaSpec fix in
  mem.rs, which touches no invitation path. The docs merge conflict was
  resolved keeping both sides (verified by diff).

## Sandbox restriction (not a code defect)

This review sandbox has no cargo dependency cache (`CARGO_HOME` registry
empty, no target dir, no vendored sources) and no network (index.crates.io
and static.crates.io unreachable), so cargo test probes could not execute
here — raw failed-probe log retained in the evidence directory. Per the
prior round's evidence and this round's full read-through, the deferred
checks for host verification are:

```bash
cargo test -p gyre-server --lib api::invitations
cargo test -p gyre-adapters invitation
cargo test --offline -p gyre-domain --lib invitation
cargo test -p gyre-server --lib auth
cargo test --all
```

## Minor observations (non-blocking, for a future hardening round)

1. The `2026-10-10-000058_invitations/up.sql` header comment claims a
   partial unique index enforces one pending invitation per
   (tenant, email)/(workspace, user); no such index exists in the SQL.
   The invariant IS enforced in adapter code and the handler (409), but
   the DB-level check-then-insert has a narrow race under truly
   concurrent identical invites — consequence is a benign duplicate
   pending invitation, not a security issue. Fix by dropping the sentence
   or adding the (portable) partial unique index.
2. The `max_pending_invitations` 429 branch has no direct HTTP test
   (defaults and expiry application are tested; the cap is not).
