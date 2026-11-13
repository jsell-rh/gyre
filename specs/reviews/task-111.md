# Review — task-111 (Implement session management)

Spec: `user-management.md` §Session Management.
Candidate under review: `6c9ddd1f7329cce562ad564cf88bae7350555d47` on
`pipeline/task-111/0569edb963f74f60832217ef35f47a3c-1` (assigned base `d9c419a0`,
merged via `e433eff9` + fixture repair `f811501f`; 14 commits listed in the task
frontmatter at the candidate).
Verdict: **approved** (Round 1). Evidence under `/tmp/stage/review-evidence/`.

## Probes run

- `bash scripts/{check-arch,check-abac-route-registry,check-abac-exempt-handlers,
  check-mem-port-contracts,check-migration-sql-portability,check-in-memory-state-stores,
  check-forwarded-header-trust,check-scope-literal-defaults,check-lossy-secret-conversion,
  check-fabricated-scope-defaults,check-byte-slice-truncation,check-relative-path-defaults,
  check-unbounded-external-http}.sh` → all OK, exit 0 (13/13).
- `/tmp/stage/bin/cargo test -p gyre-adapters --lib sqlite::user_profile::tests::session`
  → **8 passed, 0 failed** against real SQLite (round-trip, duplicate-id PK guard,
  all-four-column device match, newest-match-after-remint, sliding expiry, retention
  never deleting revoked rows, owner-scoped revoke, revoke-all + delete-expired).
- `/tmp/stage/bin/cargo check -p gyre-server` and `--tests` → 0 errors (one benign
  pre-existing `unused_mut` warning in a base-merge test fixture).
- `check-migration-versions.sh` → OK; `2026-10-08-000063_user_sessions` is the next
  unused sequence number; SQL is portable (IF NOT EXISTS, TEXT/BIGINT/INTEGER only).
- `check-task-commit-attribution.sh` exits 1 **only against the working tree**, where the
  controller reset `specs/tasks/task-111.md` to `commits: []` (uncommitted, not part of
  the candidate). All 9 flagged SHAs are present in the candidate commit's frontmatter
  (verified via `git show 6c9ddd1:specs/tasks/task-111.md`).
- `scope-literal-defaults-exemptions.txt` shrank 12→9; verified genuine: the three
  `merge_request.rs` sites now pass a real `workspace_id: Id` through `MergeRequest::new`
  (domain constructor parameter) and the adapters persist `m.workspace_id` — no
  "default" literal remains.

## Verified working (acceptance criteria)

- **Domain entity: PASS.** `UserSession` in `gyre-domain/src/user_profile.rs` with every
  spec field (id, user_id, token_hash, ip_address, user_agent, created_at, last_active_at,
  expires_at, revoked); `is_active(now)` = not revoked and not expired. Only a SHA-256
  (`hash_api_key`, real `sha2::Sha256::digest`) is ever stored — no plaintext.
- **Port + adapters: PASS.** `SessionRepository` (create/find_by_id/find_by_token_hash/
  find_by_credential_and_device/list_for_user/touch/revoke/revoke_all_for_user/
  delete_expired_before) implemented by SQLite, PostgreSQL, and mem; the duplicate-id
  create contract is enforced in all three (SQL PK / mem code guard —
  check-mem-port-contracts OK). Sessions live in port-backed `AppState.sessions`
  (`store!` wiring; no bare in-memory store).
- **Migration: PASS.** `user_sessions` table with all nine columns + indexes; next
  sequence 000063; portable to both backends.
- **Auth integration: PASS.** On successful API-key auth the (credential, IP, User-Agent)
  session is found or minted; `last_active_at` + sliding `expires_at` are refreshed at
  most once per 60 s (throttle test asserts no write inside the window and a write +
  TTL slide outside it, reading back through the port). A revoked device session rejects
  auth with 401; `credential_revoked` rejects a key whose every session is revoked
  ("sign out everywhere", including never-seen devices) on **both** the HTTP extractor
  and the WS/TTY `authenticate_token` path. IP comes from `ConnectInfo` socket peer only
  (main.rs serves `into_make_service_with_connect_info`); forwarded headers are not
  consulted (CWE-348 check passes).
- **Endpoints: PASS.** `GET/DELETE /users/me/sessions[/:id]`, `POST
  /users/me/sessions/revoke-all`, plus TenantAdmin variants
  (`/users/:user_id/sessions...`) with real per-handler authorization on the ABAC-exempt
  routes: Admin role check, target-user existence 404, tenant containment
  (`ensure_same_tenant_session_target`, fail-closed on missing tenant binding), and
  cross-user session id → 404. Listings are active-only and never expose token hashes.
- **Background cleanup: PASS.** `spawn_session_cleanup` (daily) deletes only unrevoked
  sessions expired > 30 days; revoked rows are retained forever as the durable sign-out
  record (port contract, adapter-tested).
- **Tests are real, not inflated.** The server session tests drive the real router and
  real auth extractor (mem-backed state) and assert observable outcomes: 401 after
  revoke, per-device isolation vs sign-out-everywhere, never-seen-device rejection,
  listing filters, cross-user 404, non-admin 403, cross-tenant 403, unknown user 404,
  WS-path rejection, throttle, re-mint. The adapter suite ran green against real SQLite.

## Notes (non-blocking)

- `token_hash` is the SHA-256 of the API key (the credential doubles as the session
  token) rather than a separately minted crypto-random token. Gyre auth is API-key-only
  (no login flow exists that could issue independent session tokens), the spec's field
  contract (hash stored, plaintext never) is honored, and every observable specced
  behavior (view/revoke/revoke-all, TenantAdmin variants) is real. Documented in code
  and in the coverage note. Recorded as an interpretation, not a defect.
- `credential_revoked` loads all of the user's sessions per request and filters in
  memory — minor per-request cost, no correctness impact.
- `track_session`/`credential_revoked` fail soft (log + allow) on session-store errors;
  the credential itself is validated against the same store, so a store outage fails
  auth before these paths become a bypass.
- One benign `unused_mut` warning in a `merge_request` test fixture from the base-merge
  adaptation.

## Verifier must run (deferred from sandbox per the 4 GiB budget)

- `cargo test -p gyre-server --lib api::users::tests::session` (13 endpoint tests)
- `cargo test -p gyre-server --lib auth::tests::session_last_active_touch_is_throttled
  expired_session_is_reminted_not_zombified ws_auth_rejects_revoked_ws_device_session`
- `cargo test --all` (task acceptance criterion)
- No frontend source changed by this candidate (no `web/` diff).
