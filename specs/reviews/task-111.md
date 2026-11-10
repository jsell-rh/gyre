# Review — task-111 (Implement session management)

Spec: `specs/system/user-management.md` §Session Management.
Candidate under review: `de7b03517255e6c85f85cc07a3d2f0cbe0cdbd58` (base `3d87a1507`).
Verdict: **approved** — no findings. All acceptance criteria are backed by real production code, and the killing tests were mutation-verified in this round.

## Probes run (this round, at candidate HEAD; logs under `/tmp/stage/review-evidence/`)

All Rust runs with `SKIP_WEB_BUILD=1` and `/tmp/stage/bin/cargo`:

- `cargo test -p gyre-server --lib session` → **21 passed, 0 failed** (`server-session-tests.log`; includes the 13 `api/users.rs` session tests and the 3 `auth.rs` session tests plus unrelated name-matches).
- `cargo test -p gyre-server --lib -- signout_everywhere revoke_all_blocks tenant_admin_revoke_all ws_auth_rejects` → **5 passed, 0 failed** (`server-signout-tests.log`).
- `cargo test -p gyre-adapters --lib session` → **9 passed, 0 failed** (`adapters-session-tests.log`; the 8 `sqlite::user_profile::tests::session*` tests construct fresh SQLite DBs through the constructor that runs all pending embedded migrations, so migration `2026-10-08-000060_user_sessions` is exercised on fresh databases).
- `cargo test -p gyre-domain --lib` → **448 passed, 0 failed** (`domain-tests.log`).
- **Mutation check** (`mutation-revocation-disabled.log`): temporarily disabled both revocation enforcements in `auth.rs` (`track_session` returning `true` for revoked device sessions; `credential_revoked` returning `false` unconditionally) → `revoke_session_then_credential_is_rejected`, `revoking_one_device_session_does_not_sign_out_other_devices`, and `revoke_all_sessions_signs_out_everywhere` all **FAIL** (200 vs expected 401). Source restored byte-identical afterwards (`git status` clean; re-ran a session test green to confirm). The tests are genuine killing tests, not self-confirming.
- Mechanical checks at candidate HEAD: `check-migration-versions.sh` OK (the prior verification failure — duplicate Diesel version `20261008000059` vs base's task-109 migration — is repaired by renumbering to `2026-10-08-000060_user_sessions`, next unused sequence); `check-migration-sql-portability.sh` OK; `check-abac-route-registry.sh` OK; `check-abac-exempt-handlers.sh` OK (89 handlers); `check-mem-port-contracts.sh` OK; `check-scope-literal-defaults.sh` OK; `check-task-commit-attribution.sh` OK; `check-arch.sh` OK.

## Verified (per acceptance criterion)

- **`UserSession` domain entity** (`gyre-domain/src/user_profile.rs`): all nine spec fields; only the SHA-256 of the API key is ever stored (`hash_api_key`) — `UserSessionResponse` exposes id/user_agent/ip_address/timestamps only, no token hash (asserted by `sessions_endpoint_lists_session_created_by_auth`).
- **`SessionRepository` port + adapters**: full port (create with duplicate-id failure contract, list_for_user, find_by_id, find_by_token_hash, find_by_credential_and_device matching all four fields, touch with sliding TTL, owner-scoped revoke, revoke_all_for_user, delete_expired_before that never deletes revoked rows). SQLite, PostgreSQL, and mem implementations; duplicate guard enforced in the mem adapter in code (check passes); durable wiring via the production `store!` macro in `lib.rs` (SQLite/PG when `GYRE_DATABASE_URL` is set).
- **Migration**: `2026-10-08-000060_user_sessions` with all spec columns + indexes; unique Diesel version (check green); portable SQL (check green); idempotent `CREATE TABLE IF NOT EXISTS`, so databases that already applied the pre-renumber 000059 are unaffected.
- **Sessions created on auth, throttled updates**: `track_session` finds-or-mints per (credential hash, IP, User-Agent); `session_last_active_touch_is_throttled` asserts both no `last_active_at`/`expires_at` write inside the 60s window and a write + TTL slide outside it; `expired_session_is_reminted_not_zombified` covers the idle-expiry return path.
- **Endpoints**: GET/DELETE/revoke-all under `/users/me/sessions...` scoped to the caller (foreign session id → 404, tested); TenantAdmin variants under `/users/:user_id/sessions...` with Admin-role **and** tenant-containment per-handler checks (`ensure_same_tenant_session_target`, fail-closed on unbound target; cross-tenant, non-admin, and unknown-user cases all tested). Listings are active-only.
- **Revoked sessions rejected in auth middleware**: enforced on both the HTTP extractor and the WS/TTY `authenticate_token` path, via per-device revoked-session rejection and `credential_revoked` for "sign out everywhere" including never-seen devices — mutation-verified killing tests (above). IP from `ConnectInfo` socket peer only (CWE-348).
- **Routes + ABAC**: registered in `api/mod.rs`; resolver `RouteResourceMapping::exempt` entries added in `abac_middleware.rs` (not the frozen exemption file) with per-handler authorization, which `check-abac-exempt-handlers.sh` confirms.
- **Background cleanup**: `spawn_session_cleanup` (daily, spawned in `main.rs`) deletes only unrevoked sessions expired >30 days; `session_retention_never_deletes_revoked_rows` and `signout_everywhere_survives_retention_cleanup` guard the durable-revoked-row invariant.
- **Focused suites pass** (above). Full workspace `cargo test --all` is deferred to the verification stage per review scope; the changed crates' focused suites and all mechanical invariants are green.

## Notes (non-blocking, considered and dismissed)

- `track_session`/`credential_revoked` fail soft on session-store errors (allow the already-independently-validated credential, log a warning). Documented availability choice at every site; the API key itself is validated against `api_keys` regardless, and the spec is silent on storage-failure semantics.
- Revoking a credential's only session also blocks that credential from never-seen devices until a new key is minted — the natural consequence of the documented "every session for the credential revoked ⇒ credential signed out" model; consistent with, not contrary to, the spec.
- The Session Management UI page (§UI Pages table) is task-127's scope (coverage row 31), not this task's coverage section.
- The `check-scope-literal-defaults.sh` verifier edit strengthens the check (no-arg invocations previously scanned zero files vacuously) and its frozen exemption count went 12→9 by *removing* three genuinely fixed `merge_request.rs` sites (`MergeRequest::new` now requires `workspace_id`; adapters persist under the handle's tenant scope — verified in the diff). No entries were added, no count raised.
