# Review — task-110 (Implement tenant & workspace invitation flow)

Spec: `specs/system/user-management.md` §Tenant-Level User Onboarding, §Workspace
Invitation Flow, §Invitation Expiry, §Tenant Invitations.
Candidate under review: `dc35e207` (assignment base `706bc7bd`).
Verdict: **needs-revision** — one major security finding (F1, invitation tokens
leak to unauthenticated `/metrics` and info-level logs) and one low finding
(F2, reserved system ABAC identity accepted on the accept-path display_name
write). The core invitation implementation itself is real and sound (details
below); both findings are narrow, mechanical repairs.

## Round 1 (candidate dc35e207)

Probes run (checkout at `dc35e207`; evidence in `/tmp/stage/review-evidence/`
of this review stage):

- `bash scripts/check-arch.sh` → OK.
- `bash scripts/check-abac-route-registry.sh` → OK.
- `bash scripts/check-abac-exempt-handlers.sh` → OK (87 handlers).
- `bash scripts/check-migration-versions.sh` → OK (no duplicate Diesel versions).
- `bash scripts/check-migration-sql-portability.sh` → OK.
- `bash scripts/check-mem-port-contracts.sh` → OK.
- `bash scripts/check-in-memory-state-stores.sh` → OK.
- `bash scripts/check-forged-scope-fields.sh` → OK.
- `bash scripts/check-task-commit-attribution.sh` → the 12 task-110
  product-surface commits it flags against the working tree are an artifact of
  the review stage resetting `specs/tasks/task-110.md` to `commits: []`;
  against the candidate's committed frontmatter every flagged SHA
  (758462f4, 8833f8d9, 01958bc0, 9f9be530, d1103854, 5df8f2b1, e52fea4f,
  ea366b15, 797cf52f, c1de316b, 8c469c53, 0f4fea9a) is present — attribution
  is complete for the candidate.
- `/tmp/stage/bin/cargo check -p gyre-server` → exit 0 (7m56s).
- `/tmp/stage/bin/cargo test -p gyre-domain invitation` → 4/4 passed.
- `/tmp/stage/bin/cargo test -p gyre-adapters --lib invitation` → 4/4 passed
  (roundtrip ×2, wrong-source transition, DB-enforced pending-scope unique).
- `/tmp/stage/bin/cargo test -p gyre-adapters --lib migration_000064` → 1/1
  passed (raced-duplicate dedupe before partial-index creation, real Diesel
  migration harness on SQLite).
- `/tmp/stage/bin/cargo clippy -p gyre-adapters -p gyre-domain -p gyre-ports`
  and `-p gyre-server` → exit 0; all remaining warnings map to pre-existing
  files outside the task-110 diff hunks (verified hunk-by-hunk). The retained
  verification finding 4b40530b (clippy `redundant_field_names` in
  postgres/repair.rs) is repaired by 758462f4 (module-level allow with the
  sibling diesel-derives false-positive rationale).
- Static review of the full base..candidate diff (~8.5k lines) across
  gyre-domain/ports/adapters/server, including two parallel sub-agent passes
  (adapters; auth/users/background job) with independent verification of the
  reported defects in this round.

Server test compilation (`cargo test -p gyre-server --lib api::invitations`)
was rejected by the 4 GiB build wrapper — infrastructure constraint, not a
code defect. **Verifier must run:** `cargo test -p gyre-server --lib
api::invitations`, `cargo test --all`, and the retained `checks.sh` (full
clippy/build/test matrix).

### Verified working (no findings)

- **Domain/ports/adapters are real implementations.** `TenantInvitation` /
  `WorkspaceInvitation` / `InvitationStatus` / `InvitationPolicy` carry every
  spec field; both repository ports implemented by SQLite, PostgreSQL, and
  mem. `transition_status` is a genuine conditional `UPDATE ... WHERE status =
  'from'` (rows>0) in both SQL adapters and a check-and-set under one lock in
  mem — the single-use acceptance guard is atomic in all three. The
  pending-scope duplicate contract is enforced check-then-insert (friendly
  409) **plus** DB partial unique indexes (migration 000064, portable SQL,
  with raced-duplicate dedupe tested against the real migration harness).
  Migration numbering 000063–000066 is collision-free (the same-date
  2026-11-13-000061/62 pairs predate this candidate and have distinct Diesel
  version strings).
- **Token handling**: 32 bytes from ring `SystemRandom` (CSPRNG), hex-encoded;
  only the SHA-256 hash persisted (known-answer test in the suite); accept
  claims Pending→Accepted *before* side effects with rollback on failure.
- **Authorization**: tenant invitation management is TenantAdmin-only
  per-handler with `tenant_id` derived from the authenticated caller
  (API-key path derives it from the user's stored binding, fail-closed when
  absent — auth.rs:1534+); workspace invite/list/revoke enforce
  workspace-tenant containment + Owner/Admin/TenantAdmin; invitee-facing
  accept/decline enforce caller-must-be-invitee (404, no existence leak);
  cross-tenant pre-assignments are skipped at accept; deactivated accounts
  cannot accept (both modes). All routes registered with ABAC resolver
  entries; the exempt invitee routes carry per-handler guards (mechanical
  checks pass).
- **Background expiry job** runs at startup + interval (env-overridable,
  zero-guarded), marks Pending past `expires_at` as Expired via the same
  conditional transition, never deletes; accept also expires overdue tokens
  inline. Policy (expiry days, cap, allow_re_invite) is per-tenant via kv
  store, fails closed on corrupt rows.
- **Bulk invite** accepts the spec's array shape with per-entry partial
  success; `max_pending_invitations` cap enforced (429); username contract
  (validate/409/suffix-dedupe) enforced pre-claim so invalid input cannot burn
  the single-use claim.
- **Tests are hard, not inflated**: HTTP-level lifecycle/revocation/expiry/
  role-guard/cross-tenant/concurrency/deactivation tests drive the real
  router; adapter tests exercise real SQLite storage and real migration
  application; the SHA-256 known-answer test pins the hash. Multiple prior
  review rounds' repairs (F1/F2/F3 markers in code comments) are visibly
  closed.
- **Coverage matrix** rows 5/6/7/25 updated to `implemented` with accurate
  evidence; the task-124 parallel workspace-invitation implementation was
  correctly removed in favor of task-110's superset model, and task-124's
  migration renumbered (000063→000066) with documentation.

### Findings

- [**F1 (major, security): invitation tokens leak into unauthenticated
  `/metrics` and info-level trace/log spans.**] The four magic-link routes
  (`/api/v1/invite/:token/accept|decline`,
  `/api/v1/workspaces/invitations/:token/accept|decline`) are registered on
  the main Router (lib.rs:927-947), which `request_tracing` wraps
  (middleware.rs:44-70). That middleware records the raw URI path both as the
  `http_request` info-span `path` field (default `RUST_LOG=info`, so it is
  emitted on standard deployments) and as Prometheus label values
  (`http_requests_total{method,path,status}`,
  `http_request_duration_seconds{method,path}`) — with no redaction anywhere.
  `GET /metrics` sits outside the auth'd api router and `metrics_handler`
  (src/metrics.rs:144) performs no authentication, so in the default
  deployment anyone can read the registry. Repro: create an invitation; touch
  the accept URL once (even the invitee's browser GET on the magic link,
  which 405s but still traverses the middleware); `curl /metrics` with no
  credentials → the token is in the label values; while the invitation is
  Pending (default 7 days), an attacker accepts it first — creating an
  account with the invited GlobalRole (potentially TenantAdmin) and
  pre-assigned memberships, or (re-invite case, invitations.rs:811-835)
  minting a fresh API key for an existing invitation-onboarded local account.
  Each distinct token also creates permanently retained label sets
  (unbounded cardinality), defeating the middleware's own "normalise path to
  avoid high cardinality" intent. The repo already treats
  credential-in-URL as a known hazard (lib.rs ws-ticket comment: "Prevents
  leaking real auth tokens in WebSocket URL query parameters"), so this is an
  oversight, not an accepted tradeoff. **Repair:** redact the token segment
  for `/api/v1/invite/*` and `/api/v1/workspaces/invitations/*` in
  `request_tracing` (both the span field and the metrics labels — normalize
  to a path template), add a regression test asserting neither the span field
  nor `/metrics` output contains a freshly issued token after hitting the
  accept route.
- [**F2 (low): reserved system ABAC identity accepted on the accept-path
  display_name write.**] `accept_tenant_invitation_claimed`'s fresh-create arm
  sets `display_name` from the client body (or the username default) without
  rejecting `gyre-system-token` (`SYSTEM_ABAC_IDENTITY`), unlike every
  sibling write path (users.rs:205 profile, users.rs:2080 create_user,
  auth.rs:2097/2121 SSO, scim.rs:318/377 SCIM) — the task-129 invariant that
  API-key/JWT auth resolves ABAC subject.id from display_name. An explicit
  username `gyre-system-token` also passes `validate_username` and becomes
  the display_name. Impact is fail-closed (abac_middleware.rs:2152 denies
  every `/api/v1` request for a non-system principal presenting the
  identity): the invitee bricks their own account with no self-service
  recovery. **Repair:** reject the reserved identity for both explicit
  `display_name` and `username` on the accept path (400, pre-claim, like the
  username format check), mirroring users.rs.

### Observations (non-blocking, recorded for a future spec/implementation
decision — not part of this verdict)

- **TenantAdmin re-invite mints a fresh API key for an existing
  invitation-onboarded local account** (external_id
  `tenant:{t}:email:{e}`): since tenant invitations have no email delivery,
  the invite_url (and thus the token) is returned to the inviting admin, who
  can accept it themselves and receive the account's new key. The code
  deliberately denies this for SSO/SCIM/bootstrap accounts ("token possession
  must not bypass the existing auth factor") but accepts it for
  invitation-onboarded local accounts (retry-dedupe rationale), and a test
  cements it (`reinvite_acceptance_mints_fresh_key_for_existing_local_account`).
  A malicious TenantAdmin gains attributed-to-another-user credential access.
  Comparable to admin password-reset in conventional systems, and the spec is
  silent — but an audit event naming the accepting identity (or refusing to
  mint when the account already holds keys) would close the attribution gap.
- `require_matching_invitee`'s doc comment claims identity-less bearers "go
  through the unauthenticated path where the guard is simply not applicable",
  but the code rejects them (fail-closed). Fix the comment.
- Adapter comments reference "migration 000061" for the pending-scope index
  (renumbered to 000064) — stale comment only.
- `transition_status` overwrites `accepted_at` (including with `None`) on
  non-accept transitions — no current caller is affected; consistent across
  adapters.

Test-inflation check: none found. The suite drives real routers and real
SQLite storage; the concurrency, expiry, deactivation, and cross-tenant tests
fail when the corresponding behavior is removed (several were added as
repairs for exactly such probes in earlier rounds).
