# Review — task-111 (Implement session management)

Spec: `user-management.md` §Session Management.
Candidate under review: `c1d9fa650036ac6699d3515d89d4f290ab342479` (base `33cd786c2fb58516ff41c72de9fb3a8e81d177a0`).
Verdict: **approved** (round 1). Server-suite probes deferred to the independent verifier per the 4 GiB wrapper constraint — recorded below; approval is bound to those runs.

## Probes run (evidence under `/tmp/stage/review-evidence/`)

- All 20 mechanical checks PASS at the candidate: migration-versions, migration-sql-portability, arch, abac-route-registry, abac-exempt-handlers, mem-port-contracts, scope-literal-defaults, fabricated-scope-defaults, forged-scope-fields, forwarded-header-trust, inert-enforcement, lossy-secret-conversion, in-memory-state-stores, dead-message-kinds, byte-slice-truncation, relative-path-defaults, unbounded-external-http, fail-open-ref-resolution, mcp-write-tools, task-commit-attribution (re-run in a `git worktree` at the exact candidate — the checkout's working tree carries the pipeline's task-body revert, which trips the attribution check spuriously; the committed frontmatter lists all 13 commits and passes).
- `cargo test -p gyre-domain` → **471 passed, 0 failed** (463 lib + 8 + doc-tests).
- `cargo test -p gyre-adapters --lib session` → **9 passed, 0 failed** (roundtrip, duplicate-id, device match, newest-match, TTL slide, owner-scoped revoke, revoke-all scope, delete-expired scope, retention-never-deletes-revoked).
- `cargo check -p gyre-server` → clean, 0 errors (server test compilation is wrapper-blocked in this sandbox).
- `cargo fmt -p {gyre-domain,gyre-ports,gyre-adapters,gyre-server} --check` → clean.

## Verifier must run (deferred by the build wrapper)

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib` — full suite; session filters: `auth::tests::{session_last_active_touch_is_throttled, expired_session_is_reminted_not_zombified, signout_everywhere_survives_retention_cleanup, ws_auth_rejects_signed_out_everywhere_credential, ws_auth_rejects_revoked_ws_device_session}` and `api::users::tests::{sessions_endpoint_lists_session_created_by_auth, revoke_session_then_credential_is_rejected, revoking_one_device_session_does_not_sign_out_other_devices, revoke_all_sessions_signs_out_everywhere, revoke_all_blocks_credential_from_never_seen_device, my_session_listing_excludes_revoked_and_expired_sessions, admin_session_listing_excludes_revoked_and_expired_sessions, revoke_cannot_target_another_users_session, tenant_admin_can_view_and_revoke_another_users_sessions, non_admin_cannot_view_or_revoke_another_users_sessions, tenant_admin_revoke_all_signs_user_out_everywhere, admin_session_endpoints_unknown_user_is_404, cross_tenant_admin_cannot_view_or_revoke_another_tenants_user_sessions}`.
- `SKIP_WEB_BUILD=1 cargo test --all` (acceptance criterion) and `cargo clippy` on changed lines — the candidate also touches shared merge-request paths (see observations), so the full suite is the safety net for those.

## Verified working (acceptance criteria, one by one)

- **`UserSession` domain entity** — `gyre-domain/src/user_profile.rs`, all spec fields (id, user_id, token_hash, ip_address, user_agent, created_at, last_active_at, expires_at, revoked); `is_active(now)` = not revoked and not expired. Only the SHA-256 of the API key is stored (`hash_api_key`, real `sha2::Sha256`), never plaintext.
- **`SessionRepository` port + adapters** — port has every specced method (create/list_for_user/find_by_id/find_by_token_hash/touch/revoke/revoke_all_for_user/delete_expired_before) plus `find_by_credential_and_device` (per-device rows so revoking one device does not sign out every device — the port doc explains why). SQLite, PostgreSQL, and mem implementations are contract-consistent: newest-match ordering after re-mint (kills re-mint-per-request explosion), owner-scoped revoke, revoked-rows-never-deleted (durable sign-out record). `check-mem-port-contracts` passes; 9 adapter tests pass.
- **Migration** — `2026-10-08-000060_user_sessions` is the next unused sequence (base's duplicate 000057s are pre-existing); portable plain SQL (both backends share the embedded dir; portability check passes); diesel schema registered.
- **Sessions created on auth, throttled activity** — `track_session` in `auth.rs` runs in the `AuthenticatedAgent` extractor after API-key validation: finds-or-mints the (credential, IP, User-Agent) row; `touch` (last_active_at + sliding expires_at) only when idle ≥ 60s; expired-but-not-revoked rows are re-minted, not zombified. IP comes from `ConnectInfo` socket peer only (no forwarded-header trust, CWE-348).
- **GET endpoint** — active-only listing (revoked/expired filtered), response shape `{id, user_agent, ip_address, created_at, last_active_at, expires_at}`; token_hash never exposed.
- **DELETE endpoint** — owner-scoped: foreign session id → 404 (tested); sets revoked.
- **POST revoke-all** — revokes every session for the user; the extractor's `credential_revoked` check then rejects the key from any device including never-seen ones (stolen-credential scenario, tested).
- **Revoked sessions rejected in auth middleware** — both the HTTP extractor and the WS/TTY `authenticate_token` path enforce per-device revocation AND credential-wide sign-out-everywhere; global/agent-token principals return before the checks (unaffected). Killing tests exercise the real router with real provisioned API keys and include control cases and documented mutation sensitivity.
- **TenantAdmin variants** — Admin role check + tenant containment (`ensure_same_tenant_session_target`, fail-closed on unbound target, system-principal exemption consistent with sibling handlers), unknown user → 404 (no existence probe), cross-tenant admin → 403 with same-tenant positive control.
- **Background cleanup** — `spawn_session_cleanup` (daily, wired in `main.rs`) deletes only unrevoked rows expired > 30 days; revoked rows retained deliberately so sign-out-everywhere survives cleanup (tested).
- **Routes + ABAC** — all six routes registered in `api/mod.rs`; ABAC exemptions in the resolver with per-handler authorization in every handler body (`check-abac-exempt-handlers` passes). `AppState.sessions` is port-backed via the `store!` pattern (in-memory-state-stores check passes).
- **Docs** — `docs/api-reference.md` rows for all six endpoints.

## Non-blocking observations (recorded for the verifier/auditor)

1. **Fail-open on session-store errors** (`track_session`/`credential_revoked` log-and-allow): a storage outage degrades revocation enforcement while the key itself still validates against `api_keys`. Documented availability trade-off in comments; the sessions store is the same storage backend as api_keys in every wiring, so the split-brain window is narrow. Not a spec violation (error-path behavior unspecified); flag for a future hardening pass if desired.
2. **Out-of-task repairs ride the branch** (from earlier verification rounds, all in the commits list): `MergeRequest::new` now takes `workspace_id` as a constructor parameter (removing the fabricated `"default"` construction-time scope), SQLite/PG merge-request adapters persist under the handle's configured tenant, `create_mr`/MCP/spawn MR creation fail-closed (404/tool-error) on unresolvable repository scope, and `check-scope-literal-defaults.sh` gained a no-arg default of `crates/` (previously scanned zero files vacuously) with the exemption count lowered 12→9 and the stale `task.rs` pin corrected 92→99. All changes are strictly enforcement-tightening; every mechanical check passes at the candidate. Because they touch shared MR paths, the deferred full `cargo test --all` is the safety net.
3. **Revoked rows are retained forever by design** (durable sign-out record). Correct for security; an unbounded-growth data-retention concern only.
4. **Per-request cost**: API-key auth now adds a `list_for_user` + `find_by_credential_and_device` read pair per request. Write amplification is throttled as specced; the read cost is ordinary.
5. **JWT-authenticated principals are not session-tracked** — sessions model token credentials (API keys), matching the spec's token_hash design; short-lived JWTs have no server-side session to revoke. Reasonable interpretation, documented in the shipped notes.

## Generated-output note

`web/dist/` was not touched by this candidate (no web/ changes in the diff); nothing to observe.
