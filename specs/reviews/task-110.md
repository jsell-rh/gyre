# Review — task-110 (Implement tenant & workspace invitation flow)

Spec: `specs/system/user-management.md` §Tenant-Level User Onboarding, §Workspace
Invitation Flow, §Invitation Expiry, §API > Tenant Invitations.
Comparison base: `fefde2946f3a0af1f212588f54591516fee0f7b2` → candidate
`d7b705f3e9145f0f4ea2a699c2dc2982e5dd0e0f` (rebase round 6: merge of upstream
`fefde2946` as `0b8d68602`, plus a process note; code identical to the tested
merge commit). Working tree verified clean before and after all probes.

Verdict: **needs revision** — two minor findings (F1 inert policy knob, F2
pending-duplicate race in the DB adapters). Everything else in the task
contract is real, enforced, and independently probed, including two mutation
proves that the authorization and single-use tests genuinely bite.

## Diff inspection

- **Domain/ports/adapters:** `TenantInvitation`/`WorkspaceInvitation`/
  `InvitationStatus`/`InvitationPolicy` in `gyre-domain/src/invitation.rs`
  match the spec structs field-for-field (defaults 7/7/50/true). Ports
  (`gyre-ports/src/invitation.rs`) document the pending-duplicate and
  single-use `transition_status` contracts. Shared migration
  `2026-10-10-000058_invitations` is plain portable SQL (both dialects),
  version-collision-free; `2026-10-09-000057_user_tenant_scope` correctly
  avoids re-adding columns that upstream `2026-10-08-000058` already owns
  (Diesel version ordering verified). SQLite, Postgres, and mem adapters all
  implement both repos; mem enforces the pending-duplicate contract under the
  store lock and `transition_status` as check-and-set in one critical section.
- **API surface:** all five specced tenant endpoints plus decline, policy
  GET/PUT, workspace invite/list/revoke, token accept/decline, and the
  invitee-facing `GET /users/me/invitations` +
  `POST /workspaces/{id}/invite/{accept,decline}`. Magic-link token routes are
  registered on the outer router (outside `require_auth_middleware`) — the
  256-bit CSPRNG token is the auth factor; a presented Bearer identity must
  match the invitee (email match for tenant, user_id match for workspace).
  Tokens: `ring::SystemRandom` 32 bytes, stored only as SHA-256 (known-answer
  test). Routes registered in `api/mod.rs` with ABAC resolver entries;
  per-handler TenantAdmin/Owner-Admin + tenant containment on every scoped
  route; cross-tenant workspace invites rejected.
- **Single-use semantics:** accept claims Pending→Accepted via the
  conditional-write `transition_status` BEFORE side effects, with best-effort
  rollback to Pending on side-effect failure; revoke/decline/expiry use the
  same conditional transition so terminal statuses are never overwritten.
- **SSO/local linking:** `UserRepository::claim_external_id` (SQLite/PG/mem,
  contract-tested: refuses taken external_ids and unknown users) is the
  sanctioned re-point; `find_or_create_user_tenant` claims the
  `tenant:{tenant}:email:{email}` account only on `email_verified=true` and
  same-tenant email match — unverified-email and cross-tenant negative tests
  included. The rebase-merged `JwtClaims` keeps upstream task-103's
  schema-default `tenant_id: String` alongside task-110's `email_verified`.
- **Expiry job:** `run_expiry_once` marks Pending-past-`expires_at` rows
  Expired (kept for audit); spawned from `main.rs`, interval override
  `GYRE_INVITE_EXPIRY_INTERVAL_SECS` documented in server-config.md.

## Probes (all at candidate HEAD; evidence in /tmp/stage/review-evidence/task-110-review.md)

- `cargo test -p gyre-server --lib api::invitations` → **18 passed; 0 failed**.
- `cargo test -p gyre-server --lib auth::` → **47 passed; 0 failed**.
- `cargo test -p gyre-adapters --lib invitation` → **3 passed; 0 failed**.
- `cargo test -p gyre-adapters --lib sqlite::user` → **17 passed; 0 failed**.
- `cargo check -p gyre-cli` → clean.
- **Mutation probe A:** dropped the `from` precondition in both mem
  `transition_status` impls → `concurrent_claims_produce_exactly_one_winner`
  FAILED (the single-use guard is load-bearing). Restored, re-verified
  byte-identical, suite green again.
- **Mutation probe B:** disabled `require_tenant_admin` →
  `non_admin_cannot_invite_via_http` FAILED (HTTP-layer authorization test is
  real). Restored, byte-identical, green.
- Mechanical guards (arch, abac-route-registry, migration-versions,
  fabricated-scope-defaults, mem-port-contracts, inert-enforcement,
  non-atomic-creation, task-commit-attribution, abac-exempt-handlers,
  migration-sql-portability) — all exit 0.
- `check-dependent-scope-resolution.sh` fails with 2 findings, both verified
  by the implementer (and consistent with the upstream base carrying
  task-089/task-157 code); pre-existing mainline conditions, not introduced
  by this candidate. Left for upstream-side fixes.

## Findings

### F1 (minor, code): `allow_re_invite` policy knob is inert

`specs/system/user-management.md` §Invitation Expiry defines
`allow_re_invite: bool` ("Re-invite after expiry, default: true") in the
specced `InvitationPolicy`. The candidate stores, returns, and merges the
field (`crates/gyre-server/src/api/invitations.rs:505-532`), but no code path
ever reads it: `create_tenant_invitation` / `invite_to_workspace` never
consult `policy.allow_re_invite`. Reproduce: `PUT
/api/v1/tenant/invitations/policy {"allow_re_invite": false}` → invite an
email → revoke or let expire → `POST /api/v1/tenant/invite` for the same
email still returns 201 with a fresh token. A tenant that configures
"no re-invites" silently gets re-invites — the same audit-only-knob class
the pipeline rejects elsewhere. Fix: at invitation creation, when
`allow_re_invite == false`, reject (409) an email/user that already has an
invitation in a terminal state in that tenant/workspace, and add a test that
fails with the check removed.

### F2 (minor, code/race): pending-duplicate port contract is check-then-insert in the DB adapters

`TenantInvitationRepository::create`'s contract ("Fails if a pending
invitation for the same `(tenant_id, email)` already exists") is enforced in
the SQLite and PostgreSQL adapters as an `exists()` SELECT followed by a
separate INSERT, with no partial unique index backing it (the migration's
only unique index is on `token_hash`). Two concurrent invites for the same
(tenant, email) — likewise (workspace, user) — can both pass the check and
both insert, producing two pending invitations with distinct valid tokens.
The mem adapter enforces the same contract atomically under its store lock,
so the adapters diverge under concurrency (the exact failure mode the
mem-port-contract invariant exists to prevent). Consequence is bounded
(acceptance is idempotent via `find_by_external_id` and membership creation
skips existing rows), but the documented contract fails open under a race.
Fix: partial unique index `ON tenant_invitations(tenant_id, email) WHERE
status = 'Pending'` (+ the workspace variant — both dialects support partial
indexes) with the constraint violation mapped to the contract error, or wrap
check+insert in a transaction with equivalent isolation.

## Not blocking (context for future rounds)

- Spec flow step 2 ("invitee receives email") has no delivery path — the
  platform has no mailer port/infrastructure at all; the magic link is
  returned to the inviter and workspace invites notify in-app with the
  accept route in the body. Consistent with the task's acceptance criteria
  (no email delivery item); a mailer is a platform-wide feature, not a
  task-110 gap.
- Local-mode "or sets a password" is not implemented — the magic-link path
  (also specced) is; the task plan lists only magic-link accept.
- Coverage matrix rows for §5-7 still read `task-assigned`; correct at
  ready-for-review stage (flipped on completion by the controller, not by
  this review).
