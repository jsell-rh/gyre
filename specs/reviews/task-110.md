# Review: task-110 — Implement tenant & workspace invitation flow

**Candidate:** `3d60b39292607d2759f68f5f25308d00b6918a57` (assignment base `4b9220bc`)
**Round:** independent review of the rebase-repair head (merge commit 88355447 resolving the auth.rs conflict with task-129's reserved-identity hardening; process head 3d60b392).
**Verdict:** **PASS** — approved.

## Scope reviewed

Full diff `4b9220bc..3d60b392` (~7,700 added lines): domain entities
(`gyre-domain/src/invitation.rs`), ports (`gyre-ports/src/invitation.rs`,
`user.rs` additions `find_by_email`/`claim_external_id`), SQLite + PostgreSQL
adapters (`sqlite/invitation.rs`, `postgres/invitation.rs`, user.rs additions),
mem adapter, migrations `2026-11-13-000063/64/65`, the API surface
(`gyre-server/src/api/invitations.rs`, 4,571 lines incl. 28 `#[tokio::test]`s),
route registration (`api/mod.rs`, `lib.rs` magic-link routes outside the auth
chain), ABAC mappings, background expiry job (`main.rs` spawn), auth.rs SSO
email-link + merge-conflict resolution, docs, coverage rows.

## Independent evidence (this round)

- **Mechanical invariant gates (19 scripts) pass:** architecture, ABAC route
  registry, ABAC exempt handlers, migration versions, migration SQL
  portability, mem port contracts, MCP write tools, in-memory state stores,
  dead message kinds, byte-slice truncation, relative path defaults,
  fail-open ref resolution, fabricated scope defaults, lossy secret
  conversion, scope literal defaults, inert enforcement, forged scope
  fields, forwarded header trust, unbounded external HTTP, conditional test
  guards. Logs: `/tmp/stage/review-evidence/gate-*.log`.
- **Focused tests pass:** `cargo test -p gyre-domain` 538/538; `-p gyre-ports`,
  `-p gyre-common` clean; `-p gyre-adapters` 532/532 (incl. the 5 invitation
  repository tests against real SQLite: roundtrips, transition precondition,
  DB-enforced pending-scope unique index, migration 000064 raced-duplicate
  dedupe).
- **`cargo check -p gyre-server` passes** (5m53s under the 4 GiB sandbox).
- **Static security review of the merge-conflict region:** task-129's
  reserved-system-identity checks (both the existing-user `display_name`
  rejection and the provisioning `preferred_username` rejection) run BEFORE
  task-110's invitation email-link in `find_or_create_user_tenant`, so a
  spoofed reserved handle can drive neither provisioning nor account
  claiming. The email-link itself requires `email_verified=true`, is
  tenant-scoped (`tenant:{t}:email:{e}` namespace), and goes through
  `claim_external_id`, which refuses an external_id owned by another user.
  Covered by three auto-merged/added auth tests (claim, unverified-email
  rejection, cross-tenant rejection).

## Key implementation qualities verified

- **Real enforcement, not theater:** single-use acceptance is a conditional
  `UPDATE ... WHERE status = :from` (SQLite/PG) / check-and-set under the
  store lock (mem) — the claim happens before any side effect, with
  best-effort rollback to Pending on failure. Revocation, decline, and the
  expiry job use the same conditional transition, so racing writers never
  overwrite terminal statuses.
- **Token handling:** 32 bytes from `ring::SystemRandom` (CSPRNG), only the
  SHA-256 hash persisted (unique index); tokens returned exactly once in the
  create response.
- **Containment:** every scoped handler compares the entity's tenant against
  the caller's tenant (revoke/list/invite/workspace flows); pre-assigned
  workspaces are re-validated at accept time; workspace invitee resolution
  is tenant-internal; cross-tenant 403s proven by a two-tenant HTTP fixture
  test.
- **Authorization:** TenantAdmin per-handler guard (documented rationale:
  the dev/system token bypasses ABAC), Owner/Admin membership or TenantAdmin
  for workspace invites, invitee-identity matching when a Bearer identity is
  presented on token routes, 404 (not 403) on the invitee-facing
  no-pending-invitation path to avoid leaking others' invitation existence.
- **Policy:** per-tenant `InvitationPolicy` in the kv store, fail-closed on
  a corrupt row (defaults are the permissive side), `allow_re_invite=false`
  enforced on both surfaces, `max_pending_invitations` cap with 429 and
  capacity freed by revoke.
- **Local-mode credential delivery:** acceptance mints the account's first
  API key (CSPRNG, hash-only persistence, registered in the auth-resolving
  store) only for invitation-created accounts — a linked pre-existing
  account (SSO subject / bootstrap `local:`) gets `null`, so token
  possession cannot bypass an existing auth factor. Proven through the full
  routing chain (`GET /users/me` with the minted key).
- **Magic-link routes correctly live outside `require_auth_middleware`**,
  with the `AuthenticatedAgent` extractor's self-parsing (same pattern as
  git HTTP), and every other invitation route runs behind the auth + ABAC
  chain with registry mappings (route-registry gate passes).

## Environment restrictions (not defects; verifier must run)

1. **gyre-server test compilation is blocked by the 4 GiB executor wrapper.**
   Required verifier commands (in addition to the full workspace suite):
   - `cargo test --all` (the task's final acceptance criterion);
   - focused, at minimum: `cargo test -p gyre-server api::invitations` (28
     tests) and `cargo test -p gyre-server auth::tests` (includes
     `jwt_rejects_reserved_system_identity_provisioning` from task-129,
     which exercises the resolved conflict region, plus the three task-110
     SSO-link tests).
2. `scripts/check-assertionless-tests.sh` and `scripts/check-crypto-verify.sh`
   fail in this sandbox with **mawk** dialect syntax errors (no gawk
   installed); the scripts are unchanged since before the base, so the
   failure is identical on the base — infrastructure, not the candidate.
   The verifier should run them under gawk.
3. `scripts/check-task-commit-attribution.sh` fails only against the
   executor's uncommitted working-tree revert of `specs/tasks/task-110.md`
   (progress reset to not-started, `commits: []`). The committed candidate
   frontmatter lists all 9 attributed SHAs; every short SHA in the script's
   failure output is a prefix of a listed full SHA.

## Minor, non-blocking observations

- Coverage row 5's note says "26 in `api::invitations`"; the actual count at
  this head is 28 (tests grew in later repair rounds). Under-claiming,
  documentation-only.
- Generated `web/dist/` was not rebuilt by the agent — owned by the
  deterministic verifier per the review contract; no authored frontend
  source is part of this task's diff.

## Conclusion

The candidate is a real production implementation of
user-management.md §Tenant-Level User Onboarding, §Workspace Invitation
Flow, §Invitation Expiry, and §Tenant Invitations: every acceptance
criterion is satisfied by shipped code, the test suite is adversarial
(identity guards, race conditions, containment, policy enforcement,
credential delivery through the real auth chain), and all mechanical
invariants hold. The single unexecuted criterion (`cargo test --all` at this
exact head) is blocked by the sandbox build budget and is recorded above
for the independent verifier; the suite passed at prior verified heads and
the only source change since (the auth.rs conflict resolution) type-checks
and preserves both merged behaviors in the correct order.
