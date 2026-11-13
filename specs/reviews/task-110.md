# Review — task-110: Implement tenant & workspace invitation flow

Candidate: 9427e681cf628b1f2efe945ec8838cbba91a954b (base 6f5a631c9fc09664a3e766cd10536f8b56d9ce2f)
Reviewer stage: independent review (round after repair commit 5df8f2b1, credential
delivery at local-mode invitation acceptance — prior finding d6b8a97).

## Verdict: APPROVED

## Scope inspected

Full diff base→candidate (~7,700 added lines): domain entities
(gyre-domain/src/invitation.rs), ports (gyre-ports/src/invitation.rs, user.rs),
SQLite/Postgres/mem adapters + 3 migrations (000063/64/65), API handlers
(gyre-server/src/api/invitations.rs, 4,571 lines incl. tests), route registration
(api/mod.rs, lib.rs magic-link routes outside the auth chain), ABAC mappings
(abac_middleware.rs), auth.rs SSO email-claim link, mem.rs test-state stores,
expiry job spawn (main.rs), docs, coverage rows.

## Findings from this round's repair (d6b8a97): resolved

`POST /api/v1/invite/{token}/accept` now mints the account's first API key
(CSPRNG `gyre_{uuid}`, only SHA-256 hash persisted via `state.api_keys` — the
store the auth extractor consults) in the accept response. Mint is correctly
gated: only for accounts that exist because of invitation acceptance (fresh
local create, or the `tenant:{tenant}:email:{email}`-namespaced local account
found on re-invite/retry); a linked pre-existing account (SSO subject /
SCIM / bootstrap) returns `api_key: null` so token possession cannot mint a
credential bypassing the account's existing auth factor. Mint happens only
after all side effects succeed; on failure the invitation rolls back to
Pending and a retry re-mints (account dedups via the namespace lookup).
Tests exercise the real routing chain: the minted key authenticates as the
new user through `GET /api/v1/users/me`, a re-invite delivers a fresh key
for the same account, and the SSO-link arm asserts `api_key` is null with no
parallel account. These are hard tests — they fail if the mint is removed.

## Independent probes run (evidence: /tmp/stage/review-evidence/)

- Mechanical invariant scripts — all exit 0: check-arch, check-abac-route-registry,
  check-migration-versions, check-mem-port-contracts, check-inert-enforcement,
  check-scope-literal-defaults, check-fabricated-scope-defaults,
  check-in-memory-state-stores, check-migration-sql-portability,
  check-abac-exempt-handlers, check-dead-message-kinds, check-byte-slice-truncation,
  check-lossy-secret-conversion, check-relative-path-defaults,
  check-forged-scope-fields, check-forwarded-header-trust,
  check-unbounded-external-http, check-fail-open-ref-resolution.
  check-task-commit-attribution fails ONLY against the review-stage working-tree
  reset of specs/tasks/task-110.md (progress/commits reverted to the assignment
  baseline by the executor); against the committed candidate's task file it
  passes (exit 0) — verified and restored.
- `cargo test -p gyre-domain invitation` — 4/4 pass.
- `cargo test -p gyre-adapters invitation` — 5/5 pass (incl.
  pending-scope unique index enforced by the database, migration 000064
  raced-duplicate dedupe, transition_status rejects wrong source status).
- `cargo test -p gyre-adapters sqlite::user` — 18/18 pass (incl.
  claim_external_id sanctioned repoint, find_by_email case-insensitive,
  username immutability).
- `cargo check -p gyre-server` — clean (5m46s).

## Structural verification against spec (user-management.md §73–§192, §498–§532)

- All 5 tenant invitation endpoints + policy endpoints implemented, registered,
  ABAC-mapped, with per-handler TenantAdmin/containment guards (403 tests
  through the routing chain).
- Workspace invitation lifecycle (pending → accept/decline/expire/revoke)
  implemented on both surfaces: token routes (lib.rs, outside auth middleware —
  token is the auth factor, single-use conditional transition) and the
  spec-named authenticated invitee routes (`/workspaces/{id}/invite/accept`,
  `/invite/decline`, `GET /users/me/invitations`) with invitee-identity and
  tenant-containment guards and cross-tenant 403 tests.
- Single-use acceptance: `transition_status(from, to)` is a conditional
  UPDATE in SQLite/Postgres and a check-and-set under the store lock in mem;
  concurrent-claim test proves exactly one winner.
- Tokens: 32 bytes from ring SystemRandom, only SHA-256 hash persisted;
  unique index on token_hash.
- Expiry: background job spawned in main.rs (interval env, zero-guard),
  marks Pending past expires_at as Expired via the same conditional
  transition; accept/decline paths also terminalize immediately.
- InvitationPolicy per-tenant (kv store), fails closed on corrupt rows;
  max_pending_invitations cap enforced (429) with capacity freed by revoke;
  allow_re_invite=false blocks re-invite after terminal states on both flows.
- Bulk invite accepts an array, per-entry errors without aborting the batch.
- SSO link: accept links a same-email same-tenant SSO-provisioned account
  (no parallel account); auth.rs `find_or_create_user_tenant` claims a
  local-mode invitation account on first SSO login only when
  `email_verified=true` and the tenant matches (unverified-email and
  cross-tenant claim attempts tested and rejected).

## Deferred to the independent verifier (server test compilation exceeds
the review sandbox's 4 GiB budget; wrapper rejects `-p gyre-server` test builds)

- `cargo test --all` (acceptance criterion)
- `cargo test -p gyre-server api::invitations` (30+ HTTP-level tests)
- `cargo test -p gyre-server auth::` (JWT email-link claim tests)
- `cargo clippy --all-targets` and `cd web && npm ci && npm test` (frontend
  untouched by this diff; web/dist regeneration is verifier-owned)

## Notes (non-blocking)

- Email delivery of the magic link is not implemented: the platform has no
  email transport anywhere (no SMTP/lettre in any crate); the invite URL is
  returned in the create response and the invitee gets a typed in-app
  InvitationReceived notification carrying the accept route. Spec §Email
  Notifications remains a separate open coverage row; not a defect of this task.
- `Option<AuthenticatedAgent>` on the magic-link routes means an invalid
  Bearer token degrades to the unauthenticated path; the invitation token
  remains the auth factor and the invitee-match guard only adds restrictions,
  so no privilege escalation (documented trust model, tested).
