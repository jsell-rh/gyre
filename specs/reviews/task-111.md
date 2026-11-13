# Review: task-111 — Implement session management

**Candidate:** `b014b0d8eca4a4c6fb6d6c286dd0933268149264`
**Base:** `f7f5c59f22682d40e6f995b137dfa3ea9bfd3ad3`
**Spec:** `user-management.md` §Session Management
**Round:** 2026-11-13 (independent review)
**Verdict:** **APPROVED** (verifier must run the recorded server suites — see below)

## What was reviewed

Full diff base→candidate (57 files, +3917/−284), read hunk-by-hunk:

- **Domain:** `UserSession` in `gyre-domain/src/user_profile.rs` — all nine spec
  fields verbatim; only the SHA-256 of the presented credential is stored
  (`hash_api_key`, hex SHA-256), never plaintext; `is_active(now)` =
  `!revoked && expires_at > now`.
- **Port:** `SessionRepository` in `gyre-ports/src/user_profile.rs` — create
  (duplicate-id fails), list_for_user, find_by_id, find_by_token_hash,
  find_by_credential_and_device, touch (slides TTL), revoke (owner-scoped),
  revoke_all_for_user, delete_expired_before (never deletes revoked rows).
  Covers every method family in the task plan (names differ, semantics match).
- **Adapters:** SQLite + PostgreSQL (shared migration
  `2026-10-08-000063_user_sessions`, next unused sequence, portable SQL —
  `check-migration-versions.sh` and `check-migration-sql-portability.sh` both
  pass) and mem (`MemSessionRepository` in `gyre-server/src/mem.rs`, code
  guards for the duplicate-id / revoked-retention contracts —
  `check-mem-port-contracts.sh` passes).
- **Auth integration (`auth.rs`):** on successful API-key auth the
  (credential-hash, IP, User-Agent) session is found or minted;
  `last_active_at` + sliding `expires_at` written at most once per 60 s;
  a revoked device session rejects auth with 401 (`track_session` → false);
  `credential_revoked` rejects the key when every session for it is revoked
  ("sign out everywhere", including never-seen devices). Enforced on both the
  HTTP `FromRequestParts` extractor and the WS/TTY `authenticate_token` path
  (ws.rs:586, tty.rs:46 both route through it). IP comes from `ConnectInfo`
  socket peer only — no forwarded-header trust (CWE-348 invariant).
- **API:** `GET/DELETE /api/v1/users/me/sessions[/:id]`,
  `POST /users/me/sessions/revoke-all`, plus TenantAdmin variants
  (`/users/:user_id/sessions...`) with Admin-role **and** tenant-containment
  per-handler checks on ABAC-exempt routes; foreign session id → 404;
  listings active-only; token hash never serialized. Registered in
  `api/mod.rs`, ABAC-exempt entries in `abac_middleware.rs`,
  `check-abac-route-registry.sh` and `check-abac-exempt-handlers.sh` pass.
  Documented in `docs/api-reference.md`.
- **Background cleanup:** `spawn_session_cleanup` (daily, wired in `main.rs`)
  deletes only unrevoked sessions expired > 30 days.
- **Cross-cutting repairs on the branch** (prior rounds, merged forward):
  7-arg `MergeRequest::new` (workspace scope required at construction),
  repo-create scoped to the handle's tenant (SQLite+PG, with killing
  tenant-isolation tests), MCP/REST `create_mr` now 404/tool-error on unknown
  repo instead of fabricating a "default" scope, and the
  `check-scope-literal-defaults.sh` no-arg default fix (the check previously
  scanned zero files when invoked bare). The exemption file went 12→6 with
  every removed line backed by a real fix; `task.rs` site re-anchored via the
  script's documented inline `// scope-literal:ok` marker at its true
  location. This is a net reduction with genuine repairs, not grandfathering.

## Independent evidence (logs under /tmp/stage/review-evidence/)

- All 18 mechanical gate scripts PASS (arch, ABAC registry, exempt handlers,
  mem port contracts, scope-literal defaults at frozen 6, inert enforcement,
  migration versions/portability, in-memory state stores, fabricated scope
  defaults, forwarded-header trust, lossy secret conversion, unbounded HTTP,
  byte-slice truncation, relative path defaults, fail-open ref resolution,
  dead message kinds, MCP write tools).
- `cargo test -p gyre-adapters --lib session` — **9/9 passed** against real
  SQLite storage: roundtrip, duplicate-id failure, credential+device matching
  (all four fields, newest-first), revoke owner-scoping, revoke-all,
  retention never deleting revoked rows, touch sliding expiry.
- `cargo test -p gyre-domain` — 507 + 8 passed.
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --lib --tests` — clean
  (0 errors), covering the previously-reported 16-error lib-test compile
  failure (durable finding e02879e3): the drift fixtures are adapted and the
  tree compiles.
- `cargo fmt -p <each package> --check` — clean for all five crates.
- `check-task-commit-attribution.sh` — PASS at the candidate's task file
  (all 16 task-labeled commits listed in frontmatter).

## Test-strength assessment

The gyre-server session tests drive the **real router + real auth extractor**
(`api_router().oneshot(...)` with `Authorization: Bearer <key>` and distinct
User-Agents), assert on HTTP status codes and listing contents, and include
controls proving failures come from revocation rather than broken setup
(e.g. the live-credential control in
`ws_auth_rejects_signed_out_everywhere_credential`). Killing properties are
explicitly documented per test (removing `credential_revoked` or the
`track_session` rejection from `authenticate_token` fails specific tests).
The throttle test asserts both the write (stale session) and the no-write
(fresh session) sides. These are behavior tests, not mirrored-logic tests.

## Verifier-required probes (blocked in-sandbox by the build wrapper)

The executor's wrapper rejects gyre-server codegen ("gyre-server code
generation plus the model exceeds 4 GiB"); this is an infrastructure
restriction, not a code defect. The independent verifier must run:

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib` — full suite; must
  include the 22 session tests in `auth.rs` and `api/users.rs`
  (`session_last_active_touch_is_throttled`,
  `expired_session_is_reminted_not_zombified`,
  `signout_everywhere_survives_retention_cleanup`,
  `ws_auth_rejects_signed_out_everywhere_credential`,
  `ws_auth_rejects_revoked_ws_device_session`,
  `sessions_endpoint_lists_session_created_by_auth`,
  `revoke_session_then_credential_is_rejected`,
  `revoking_one_device_session_does_not_sign_out_other_devices`,
  `revoke_all_sessions_signs_out_everywhere`,
  `revoke_all_blocks_credential_from_never_seen_device`,
  `my_session_listing_excludes_revoked_and_expired_sessions`,
  `admin_session_listing_excludes_revoked_and_expired_sessions`,
  `revoke_cannot_target_another_users_session`,
  `tenant_admin_can_view_and_revoke_another_users_sessions`,
  `non_admin_cannot_view_or_revoke_another_users_sessions`,
  `tenant_admin_revoke_all_signs_user_out_everywhere`, …).
- `SKIP_WEB_BUILD=1 cargo test --all` — the acceptance-criterion suite.
- Clippy on the workspace (in-sandbox server clippy was wrapper-blocked).
- The drifted-base drift-gate tests (`merge_processor`, `reconciliation`,
  `api/merge_requests`, `api/merge_deps`, `rbac`) that were adapted to the
  7-arg constructor.

Prior recorded run at tree-identical head (the only commit after it touches
`specs/tasks/task-111.md` alone): 1724 passed.

## Observations (non-blocking)

1. **Session identity design:** the spec's "session token generation
   utility" (plan item 1) is realized as keying sessions by the SHA-256 of
   the presented API key rather than minting a parallel session-token
   credential. The platform has no login endpoint that could issue session
   tokens (user credentials are API keys, provisioned via bootstrap/admin),
   the spec's endpoint table contains only the three /me session routes, and
   the acceptance criteria omit token generation. Every user-visible spec
   capability (view device/IP/last-active, revoke one, revoke all,
   TenantAdmin view/revoke) is enforced for real. Consequence worth noting:
   "sign out everywhere" invalidates the user's existing API keys (a fresh
   key must be provisioned to sign back in) — fail-closed, consistent with
   the spec's intent given keys are the only credential.
2. **Generated output:** `web/dist/` is untouched by this candidate (task is
   server-side); owned by the deterministic verifier per policy.
3. Minor (no action required): `credential_revoked` lists all of a user's
   sessions per authenticated request, and a concurrent first presentation
   from the same device can mint two rows (newest wins on lookup) —
   correctness unaffected.

## Verdict

Every acceptance criterion is satisfied by real, enforced production code
with killing tests; all in-sandbox probes pass. **Approved**, contingent on
the recorded verifier suites (the only evidence the sandbox cannot produce).
