# Review — task-111 (user-management.md §Session Management)

Spec: `specs/system/user-management.md` §Session Management.
Candidate: `6794b439ab623a7d66e456383447a6901a4fb021` (base `88bd81bfcda47da2366c8080c13e69d062682559`, branch `pipeline/task-111/2931f7e7509a48709cf9d27e68f26cc8-12`).
Verdict: **approved** (full probe log: `/tmp/stage/review-evidence/review-summary.md`).

Prior round (attempt `e88a5646`, checkpoint `8855b967`) found a duplicate Diesel migration
version (`20261008000056` colliding with the base's `spec_assertion_results`). This round
confirms the repair: the migration is `2026-10-08-000057_user_sessions` (next unused sequence
for its date), `check-migration-versions.sh` and `check-migration-sql-portability.sh` pass, and
the `no_duplicate_migration_version_prefixes` guard test passes on the exact candidate.

## Verified against the task contract

- **Domain + port**: `UserSession` in `gyre-domain/src/user_profile.rs` with every spec field
  and `is_active`; `SessionRepository` in `gyre-ports/src/user_profile.rs` covering the plan's
  full method set (create/find_by_user/find_by_id/find_by_token_hash/touch/revoke/
  revoke_all_for_user/delete_expired, plus `find_by_credential_and_device` for the
  one-row-per-credential+device model). Only the SHA-256 of the key is stored (`hash_api_key`).
- **Adapters**: SQLite, PostgreSQL, and mem implementations; diesel schema registered; portable
  migration SQL. Adapter session suite 8/8 (roundtrip, duplicate-id failure, four-field device
  match, newest-match, TTL slide, owner-scoped revoke, revoke-all scope, revoked-rows-never-
  deleted retention).
- **Auth integration**: sessions minted on successful API-key auth in both the HTTP extractor
  and the WS `authenticate_token` path; `last_active_at` + sliding `expires_at` refreshed at
  most once per 60s (throttle test asserts no write inside the window, write + TTL slide
  outside it); revoked session for the device rejects with 401; `credential_revoked` rejects a
  key whose every session is revoked ("sign out everywhere", including never-seen devices); IP
  from `ConnectInfo` socket peer only (main.rs now serves with
  `into_make_service_with_connect_info`; no forwarded-header trust). Expired-but-unrevoked
  sessions re-mint as exactly one fresh row rather than zombie.
- **API**: `GET/DELETE /users/me/sessions[...]`, `POST /users/me/sessions/revoke-all`, plus
  TenantAdmin variants `GET|DELETE /users/:user_id/sessions[...]` with per-handler Admin-role
  checks (ABAC-exempt, `check-abac-exempt-handlers.sh` passes), target-user 404s, foreign-session
  404s, and no `token_hash` in responses. Server session suite 13/13 on the exact candidate.
- **Background cleanup**: `spawn_session_cleanup` wired in `main.rs` (daily; deletes only
  unrevoked rows expired >30 days — the durable sign-out record is never deleted, proven by
  `signout_everywhere_survives_retention_cleanup`).
- **Mutation proof (test strength)**: disabling `track_session`'s revoked-arm alone fails
  `revoking_one_device_session_does_not_sign_out_other_devices` (exit 101); disabling
  `credential_revoked` alone fails `signout_everywhere_survives_retention_cleanup` (exit 101).
  Each enforcement layer is independently pinned by a test that fails when it alone is removed;
  the endpoint-level tests that survive single-layer mutations do so via the other, independent
  layer (defense in depth, not hollow assertions).
- **Mechanical checks**: 22 repo invariant scripts pass on the candidate; the one non-zero
  (`check-assertionless-tests.sh`, exit 2) fails identically on the base — pre-existing.
  Exemption files shrank only (scope-literal 24→13 with the underlying sites genuinely fixed via
  `DEFAULT_TENANT_ID` + `bootstrap_default_tenant`; one fabricated-scope exemption removed
  matching a real code fix) — no new exemptions.
- **Docs**: `docs/api-reference.md` rows for all six endpoints.

## Non-blocking notes

- The Shipped notes describe the exemption state as "same 24/8 frozen sites, none added"; the
  actual diff lowers scope-literal exemptions 24→13 and removes one fabricated-scope exemption.
  The direction is safe (fewer exemptions, real fixes behind each removal, frozen counts lowered
  in lockstep) — a documentation inaccuracy, not a code defect.
- Live-server HTTP transport was not probed (sandbox TCP listener unsupported); endpoint
  behavior is proven by in-process Router (oneshot) tests. `cargo test --all` (full workspace)
  is left to verification; both crates carrying candidate changes compile and pass focused
  suites.
