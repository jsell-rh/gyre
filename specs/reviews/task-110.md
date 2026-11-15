# Review: task-110 — Implement tenant & workspace invitation flow

**Candidate:** `a7ab219ffb6916c0053b31491b3f3a7ebb70e896`
**Base:** `c9cc1536b4a3d8c0c51744c0af645ae9f765c581`
**Round:** independent review (post review-96521ecf repair, commit 7bc33fe4)
**Verdict:** **APPROVED** (non-blocking observations recorded below)

## Scope inspected

Full diff base→candidate (~9.2k insertions): `gyre-domain/src/invitation.rs`,
`gyre-ports/src/invitation.rs`, `gyre-adapters` (SQLite + PostgreSQL invitation
adapters, user adapter additions `find_by_email`/`claim_external_id`, migration
renumber notes, PG repair module), `gyre-server` (`api/invitations.rs` handlers +
~3.6k lines of tests, `api/mod.rs` route registration, `abac_middleware.rs`
mappings, `auth.rs` SSO email-link, `mem.rs` adapters, `middleware.rs` token
redaction, `lib.rs`/`main.rs` wiring), migrations 000063–000066, docs, coverage
matrix updates.

## Independent evidence (see /tmp/stage/review-evidence/)

- `cargo test -p gyre-domain invitation` — 4/4 pass (entity semantics,
  expiry-only-when-Pending, policy defaults).
- `cargo test -p gyre-adapters invitation` — 5/5 pass against **real SQLite**
  with the real migration chain, including
  `pending_scope_unique_index_enforced_by_database` (proves the DB-level
  one-Pending-per-scope index, not just adapter check-then-insert) and
  `migration_000064_dedupes_raced_pending_invitations_before_index`.
- `cargo test -p gyre-adapters sqlite::user` — 36/36 pass, including
  `tenant_scope_and_global_role_round_trip` (DB-backed tenant scope repair),
  `find_by_email_is_case_insensitive_and_returns_all_matches`,
  `claim_external_id_is_the_sanctioned_repoint`.
- `cargo check -p gyre-server` and `cargo check -p gyre-server --tests` —
  clean (lib + test code type-checks).
- All mechanical gates exit 0 on the committed tree: check-arch,
  check-migration-versions, check-abac-route-registry,
  check-abac-exempt-handlers (87 handlers), check-mem-port-contracts,
  check-migration-sql-portability, check-in-memory-state-stores,
  check-forged-scope-fields, check-scope-literal-defaults,
  check-inert-enforcement, check-forwarded-header-trust,
  check-relative-path-defaults, check-byte-slice-truncation,
  check-lossy-secret-conversion, check-fail-open-ref-resolution,
  check-fabricated-scope-defaults, check-unbounded-external-http,
  check-dead-message-kinds, check-mcp-write-tools.
- Commit attribution: all 14 product-surface task-110 commits in the range are
  listed in the candidate's `specs/tasks/task-110.md` commits frontmatter
  (verified from the committed HEAD; the stage's working-tree reset of that
  file to `commits: []` makes the script report FAIL in the sandbox — an
  artifact of the checkout, not a candidate defect).

## Task-contract assessment (all acceptance criteria)

- **Domain entities** — `TenantInvitation` carries every spec field
  (role: GlobalRole, parallel workspace_ids/workspace_roles, token_hash,
  expires_at, accepted_at); `InvitationStatus` has all five variants;
  `InvitationPolicy` defaults 7/7/50/allow_re_invite; `WorkspaceInvitation`
  pending/accept/decline lifecycle. ✓
- **Ports** — both repositories with an atomic `transition_status(id, from,
  to)` single-use claim contract; enforced as a conditional UPDATE in
  SQLite/PG and a check-and-set under the store lock in mem; pending-scope
  duplicate contract enforced in all three adapters (partial unique index
  backstop + mem code guard — check-mem-port-contracts passes). ✓
- **Migrations** — 000063 (tables, BIGINT timestamps, portable SQL),
  000064 (pending-scope partial unique indexes + raced-duplicate dedupe),
  000065 (tenant-scope index), 000066 (task-124 renumber). No duplicate Diesel
  versions; portability check passes. ✓
- **All 5 tenant endpoints functional** — invite (CSPRNG token, SHA-256 only,
  email normalization, parallel-role validation, policy expiry, 409 on
  duplicate pending, allow_re_invite=false enforcement), bulk (per-entry
  errors + persistence asserted in-test), list (status filter), revoke
  (conditional transition, tenant containment, terminal rows kept),
  accept (magic link; validates before claim; single-use claim before side
  effects; rollback to Pending on failure; local-mode create with
  tenant-namespaced external_id, username contract, reserved-identity guard,
  first API key minted only for invitation-owned accounts; SSO link;
  deactivated-account rejection; cross-tenant workspace pre-assignment
  skipped). ✓
- **Workspace invitation lifecycle** — invite by user_id/username/email with
  selector-consistency check, Owner/Admin member or TenantAdmin gate
  (Developer-member 403 tested), cross-tenant rejection, max_pending cap
  (429), pending duplicate 409, typed InvitationReceived notification with
  accept/decline URLs, accept/decline via token AND via authenticated
  invitee routes, revoke, expiry. ✓
- **Token generation** — ring SystemRandom 32 bytes hex; only SHA-256 hash
  stored; token redacted from tracing spans and Prometheus labels
  (`redact_token_paths`, with a metrics-rendering regression test). ✓
- **Expiry job** — `run_expiry_once` both arms with conditional transitions
  (racing accept never overwritten), tested for tenant AND workspace rows
  with fresh-control rows; spawned from main.rs, interval env-configurable,
  zero-interval falls back (no tokio panic). ✓
- **Bulk invite** — array body, per-entry errors with source indexes, partial
  success; persistence verified through the repository in-test. ✓
- **Routes + ABAC** — all management routes registered and mapped; invitee
  accept/decline routes ABAC-exempt with per-handler tenant containment +
  caller-must-be-invitee (verified through the production middleware chain
  with a deny-control case proving the exemption is what makes them
  reachable). Magic-link routes correctly outside require_auth but inside
  rate-limit/tracing. ✓
- **`cargo test --all`** — gyre-server test *execution* deferred to the
  independent verifier per the 4 GiB build-budget rule; type-checked via
  `cargo check -p gyre-server --tests`. Commands recorded below.

## Prior review round (96521ecf) — repair verification

All six findings genuinely closed in commit 7bc33fe4: F1 workspace-arm expiry
job test with fresh control row; F2 bulk persistence asserted via
`list_by_tenant` + per-entry error indexes; F3 token-only tenant decline
(204, Declined stored, no account created, token burned); F4 token-hash
asserted against the repository row (`== hash_token(&token)`, `!= token`);
F5 Member-403 across list/bulk/revoke/policy routes with no side effects;
F6 Developer-member workspace-invite 403 with no invitation row.

## Non-blocking observations (follow-up, not task-110 defects)

1. **Legacy `POST /api/v1/workspaces/{id}/members` still creates a PENDING
   (unaccepted) membership row, and after this candidate no route accepts a
   pending membership** — the invitee-facing accept route now resolves only
   `WorkspaceInvitation` rows. This is the deliberate, documented supersedure
   of task-124's parallel membership-row implementation (axum rejects the
   duplicate route registration; task-110's model is the assigned superset and
   the spec's §API table lists POST /invite, not POST /members, as the invite
   endpoint). The pending row still grants ABAC workspace membership and
   appears in GET /members; it is removable via DELETE /members/{user_id}.
   Recommend a follow-up task to either make POST /members create accepted
   memberships or route it through the invitation model, so the legacy
   endpoint stops producing a dead-end half-state. `docs/api-reference.md`'s
   POST /members row still mentions "accept/pending lifecycle (M22.8)".
2. Cosmetic: two comments in `gyre-adapters/src/sqlite/invitation.rs` cite
   "migration 000061" for the pending-scope indexes that actually live in
   000064.
3. `create_tenant_invitation` loads all of a tenant's invitations and
   `resolve_workspace_invitee` lists all users per call — O(n) scans
   consistent with existing repo patterns, not a defect at current scale.

## Verifier must run (server test execution, deferred per 4 GiB budget)

```
cargo test -p gyre-server api::invitations        # full invitation suite
cargo test -p gyre-server auth                    # SSO email-link tests (task-110 F2)
cargo test -p gyre-server middleware              # token redaction from metrics
cargo test -p gyre-server users                   # task-124 supersedure area
cargo test --all                                  # acceptance criterion
cargo clippy --all-targets                        # verification-standard lint pass
```

## Verdict

Approved. The implementation is real production code end-to-end (CSPRNG
tokens hashed at rest, DB-arbitrated single-use claims, per-tenant policy
enforcement, cross-tenant containment on every route, fail-closed policy
loading), the test suite is kill-capable (DB-level index probes, production
middleware-chain probes with controls, concurrent-claim race test), and all
mechanical invariants pass. No blocking findings.
