# Review — task-110 (Implement tenant & workspace invitation flow)

Candidate: `0996b57e524bad9598fa6e59def2547fb3971895` (base `c9cc1536`)
Spec: `specs/system/user-management.md` §Tenant-Level User Onboarding,
§Workspace Invitation Flow, §Invitation Expiry, §Tenant Invitations
Verdict: **needs-revision** — production implementation verified clean
across three independent deep reviews and all mechanical invariants; the
findings are test-coverage gaps below the repo's own rule-2 bar ("a test
that doesn't fail when the specced behavior breaks does not close a
section"), concentrated on two acceptance criteria.

## Probes run (evidence: /tmp/stage/review-evidence/)

- All 19 mechanical invariant scripts (arch, ABAC registry + exempt
  handlers, migration versions + SQL portability, mem port contracts,
  in-memory state stores, scope-literal/fabricated defaults, inert
  enforcement, forged scope fields, lossy secret conversion, forwarded
  header trust, unbounded external HTTP, dead message kinds, byte-slice
  truncation, relative path defaults, fail-open ref resolution): **all
  exit 0**.
- `/tmp/stage/bin/cargo check -p gyre-server` → **exit 0**.
- `/tmp/stage/bin/cargo test -p gyre-domain invitation` → **4/4 passed**.
- `/tmp/stage/bin/cargo test -p gyre-adapters invitation` → **5/5
  passed** (incl. DB-level pending-scope unique-index proof and the
  000064 dedupe migration test on real tempdir SQLite).
- Manual verification of each finding below at the candidate tree
  (grep + read of the test module).

Server tests the verifier must execute (not run here — 4 GiB budget):
`cargo test --all`, at minimum `cargo test -p gyre-server invitation`
plus the `users.rs`/`auth.rs`/`middleware.rs` invitation-related tests
(`jwt_email_link_*`, `local_mode_accept_mints_authenticating_api_key`,
`accept_links_existing_sso_user_instead_of_parallel_account`,
`deactivated_*_cannot_accept_*`, metrics redaction tests).

## Verified clean (no findings)

- **Domain/ports**: `TenantInvitation`, `WorkspaceInvitation`,
  `InvitationStatus`, `InvitationPolicy` match the spec structs exactly;
  port contracts document and adapters enforce the pending-duplicate and
  atomic-transition invariants.
- **Adapters**: `transition_status` enforces the from-precondition in
  the UPDATE's WHERE clause (SQLite + PG); `create()` maps
  UniqueViolation on the 000064 partial unique indexes to the
  port-contract error (race backstop beyond check-then-insert); mem
  adapter enforces both contracts under the store lock; migrations
  000063/000064 are the next unused sequence numbers and run on both
  SQLite and PostgreSQL (no UPDATE...FROM, BIGINT timestamps, PG-only
  int4→int8 repair hook correctly ordered after migrations).
- **API**: all 5 tenant endpoints + workspace invite/list/revoke/token
  accept/decline + invitee-facing routes implemented with real storage.
  Token is 32-byte CSPRNG (ring SystemRandom), stored only as SHA-256
  hex. Single-use claim via conditional Pending→Accepted transition
  before side effects, with rollback compensation on failure. Cross-tenant
  containment (403), invitee-matching guard for authenticated callers,
  deactivated-account blocking, reserved system ABAC identity rejection,
  max_pending_invitations cap, allow_re_invite=false enforcement,
  policy fail-closed on corrupt rows, SSO link-vs-create semantics with
  tenant-namespaced external_id and no credential mint for pre-existing
  SSO accounts.
- **Routes/ABAC**: every invitation route registered with a resolver
  entry or an intentional exemption carrying per-handler tenant
  containment (invitee routes route through
  `authorize_my_pending_invitation`; token routes check invitee identity
  when a Bearer is presented). Exemption files at frozen baseline. Expiry
  job wired at `main.rs:165`, first tick expires backlog, env-override
  zero-guard.
- **middleware.rs diff** is invitation-token redaction from tracing
  spans and Prometheus label values (fixes a real token leak via
  unauthenticated /metrics) — a security improvement, not a regression.
- **auth.rs diff**: SSO email-link claim requires tenant scope +
  email_verified + tenant-namespaced external_id; no cross-tenant claim;
  deactivation fail-closed on all extractors.
- Test quality overall is high: real mem repos, real auth extractors
  with real API keys, side-effect assertions, a production
  router+ABAC-middleware test with a negative control, and SQLite tests
  that bypass the repository to prove DB-level constraints.

## Findings (all test-coverage; production code clean)

- **F1 (primary)**: `run_expiry_once`'s workspace arm
  (`invitations.rs:1985-2002`) is never exercised by the job's only test
  caller (line 2386, tenant-only seeds). Deleting the workspace loop
  leaves the suite green — the "expiry background job marks expired
  invitations" acceptance criterion is unproven for workspace
  invitations. Fix: seed an expired Pending `WorkspaceInvitation`, call
  `run_expiry_once`, assert the transition to Expired.
- **F2**: `bulk_invite_creates_all_and_reports_errors` (line 2398)
  trusts the response body (`created_count`/`error_count`) with no
  storage assertion that the two valid invitations were persisted as
  Pending. Fix: assert via `list_by_tenant`.
- **F3**: tenant decline happy path untested — POST
  `/api/v1/invite/:token/decline` is exercised only in its 403 arm
  (line 2514). Fix: token-only decline test asserting 204, status
  `Declined`, no account created.
- **F4 (minor)**: the token-hash comment in
  `tenant_invite_lifecycle_create_list_revoke` (~line 2158) describes a
  check the test never performs; no direct
  `stored.token_hash == hash_token(&token)` assertion anywhere.
- **F5 (minor)**: `require_tenant_admin` Member-403 tested through HTTP
  only on POST /tenant/invite; list/bulk/revoke/policy endpoints share
  the helper but have no non-admin test.
- **F6 (minor)**: `invite_to_workspace`'s membership-role gate
  (Viewer/Developer member → 403) untested; only the non-member case is
  covered.

All six are surgical test additions; no production changes required.
