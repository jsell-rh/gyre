# Review — task-111 (Implement session management)

Spec: `user-management.md` §Session Management (coverage row 12).
Candidate: `d6a414ede3cc9de71c326e5072d7fa44ef4a5fb3` (base `fefde2946`).
Verdict: **approved**.

## Round 1 (independent review at candidate `d6a414ede`)

All probes run at the exact candidate with `/tmp/stage/bin/cargo`; logs under
`/tmp/stage/review-evidence/task-111-*.log`. Working tree verified clean and at
the candidate SHA before and after every mutation probe (source restored with
`git checkout --` after each).

### Focused test runs

- `cargo test -p gyre-adapters --lib -- sqlite::user_profile::tests::session` → **8 passed, 0 failed** (exit 0) — roundtrip, duplicate-id, four-field device match, newest-match, TTL slide, owner-scoped revoke, revoke-all scope, retention-never-deletes-revoked.
- `cargo test -p gyre-server --lib -- api::users::tests::` → **26 passed, 0 failed** (exit 0) — includes all 12 session endpoint tests plus the base's user-profile tests.
- `cargo test -p gyre-server --lib -- auth::tests::` → **49 passed, 0 failed** (exit 0) — includes the 5 session tracking tests (`session_last_active_touch_is_throttled`, `expired_session_is_reminted_not_zombified`, `signout_everywhere_survives_retention_cleanup`, `ws_auth_rejects_signed_out_everywhere_credential`, `ws_auth_rejects_revoked_ws_device_session`) and the JWT tenant-claim/jti tests pinning the merge-conflict region.
- `cargo fmt -p gyre-{domain,ports,adapters,server} --check` → exit 0 for all four (workspace-wide fmt is deferred to verification per the executor wrapper).
- Mechanical gates (20 scripts): `check-arch`, `check-abac-route-registry`, `check-abac-exempt-handlers` (94 handlers), `check-migration-versions`, `check-migration-sql-portability`, `check-mem-port-contracts`, `check-scope-literal-defaults`, `check-inert-enforcement`, `check-task-commit-attribution`, `check-forged-scope-fields`, `check-forwarded-header-trust`, `check-in-memory-state-stores`, `check-lossy-secret-conversion`, `check-fail-open-ref-resolution`, `check-unbounded-external-http`, `check-fabricated-scope-defaults`, `check-byte-slice-truncation`, `check-relative-path-defaults`, `check-dead-message-kinds`, `check-mcp-write-tools` — **all exit 0**.

### Independent mutation probes (each disables production enforcement; each must fail a test)

| # | Mutation | Killing test (result) |
|---|---|---|
| M1 | `credential_revoked` gate disabled in HTTP extractor (`if false && ...`) | `revoke_all_blocks_credential_from_never_seen_device` **FAILED** (200 vs 401) |
| M2 | `track_session` rejection neutered in HTTP extractor (`let _ = track_session(...)`) | `revoking_one_device_session_does_not_sign_out_other_devices` **FAILED** (200 vs 401) |
| M3 | Active-only listing filter neutered in `list_my_sessions` (`filter(\|s\| true \|\| s.is_active(now))`) | `my_session_listing_excludes_revoked_and_expired_sessions` **FAILED** (3 vs 2 rows) |
| M4 | Admin role gate disabled in `list_user_sessions` (`if false && !roles.contains(Admin)`) | `non_admin_cannot_view_or_revoke_another_users_sessions` **FAILED** (200 vs 403) |
| M5 | `credential_revoked` gate disabled in WS/TTY `authenticate_token` | `ws_auth_rejects_signed_out_everywhere_credential` **FAILED** |

All five enforcement points are therefore guarded by killing tests verified
independently of the implementing agent's claims.

### Code conformance (read against the spec + task plan)

- **Domain:** `UserSession` in `gyre-domain/src/user_profile.rs` with every spec field; `is_active(now)` = not revoked and not expired. Only SHA-256 hashes are persisted.
- **Port:** `SessionRepository` (`gyre-ports/src/user_profile.rs`) with all planned methods plus `find_by_credential_and_device` (one row per credential+device — documented rationale: revoking one device must not sign out every device).
- **Adapters:** SQLite, PostgreSQL, and mem implementations with identical contracts (newest-match ordering after re-mint, owner-scoped revoke, revoked rows never deleted by retention). Migration `2026-10-08-000059_user_sessions` is portable SQL (portability check passes) with no Diesel version collision (000058 belongs to the base's `user_entity_profile_fields`).
- **Auth integration:** both the HTTP extractor and the WS/TTY `authenticate_token` path enforce (a) per-device revocation via `track_session` and (b) credential-wide "sign out everywhere" via `credential_revoked` — the two checks are complementary and independently mutation-killed. IP from `ConnectInfo` socket peer only; forwarded headers not consulted (CWE-348). `last_active_at` + sliding `expires_at` refreshed at most once per 60 s (`SESSION_TOUCH_THROTTLE_SECS`), proven by `session_last_active_touch_is_throttled` (asserts both the write and its absence inside the window).
- **API:** `GET/DELETE /users/me/sessions[/:id]`, `POST /users/me/sessions/revoke-all`, plus TenantAdmin variants. Per-handler authorization on the ABAC-exempt routes: owner-scope mismatch → 404 (no cross-user id probing), Admin role required → 403, unknown target user → 404. Listings are active-only and the response type has no `token_hash` field (test asserts absence).
- **Background cleanup:** `spawn_session_cleanup` (daily) deletes only unrevoked sessions expired > 30 days; revoked rows retained as the durable sign-out record (`signout_everywhere_survives_retention_cleanup`).
- **Wiring:** `AppState.sessions` is port-backed via the `store!` pattern (no bare in-memory state store); cleanup spawned from `main.rs`.
- **Coverage:** row 12 of `specs/coverage/system/user-management.md` marked `implemented` with an accurate note; task frontmatter lists the product-surface commits (attribution check passes).

### Non-blocking observations (documented in code, no action required)

- The session's `token_hash` column stores the SHA-256 of the API key rather than a separately minted session token: the spec defines no session-token issuance flow (clients continue presenting the long-lived API key), the never-store-plaintext invariant holds, and every behavioral requirement is enforced and mutation-verified. Reasonable interpretation, documented at `track_session`.
- `track_session`/`credential_revoked` fail open on session-store *storage* errors (logged). The credential itself was already validated against `api_keys` at that point; this is an explicit availability tradeoff, not a bypass of revocation logic. Noted for the record.

Approved. Deferred to verification (per the executor's wrapper): workspace-wide `cargo test --all`, `cargo fmt --all --check`, all-target Clippy, frontend suites.
