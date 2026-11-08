# Review — task-111 (Implement session management)

Spec: `specs/system/user-management.md` §Session Management (lines 276-296).
Base `98a5299b9`, candidate `97e101603` (head of `pipeline/task-111/dd62b5fda101450cb647c424789b97eb-1`).
This is the third review round; the first (`ff6a7ca3`) found the missing extractor-path killing test, the second (`88ab7af7`) found the missing WS/TTY-path killing tests, the missing active-only-listing killing tests, and a docs misattribution. All are closed in this candidate.
Verdict: **approved**.

## Test runs (this round, at candidate `97e101603`, shared target dir)

- `cargo test -p gyre-server --lib` → **1435 passed, 0 failed** (matches the Shipped claim; full test list by name captured in `task-111-server-session-tests.log`).
- `cargo test -p gyre-adapters --lib -- sqlite::user_profile::tests::session` → **8 passed, 0 failed** (roundtrip, duplicate-id, device-match on all four fields, newest-match, TTL slide, owner-scoped revoke, revoke-all scope, retention-never-deletes-revoked).
- Mechanical checks at candidate: `check-arch.sh`, `check-abac-route-registry.sh`, `check-abac-exempt-handlers.sh`, `check-migration-versions.sh`, `check-migration-sql-portability.sh`, `check-inert-enforcement.sh`, `check-mem-port-contracts.sh`, `check-scope-literal-defaults.sh`, `check-task-commit-attribution.sh` — all OK.

## Independent mutation probes (logs under /tmp/stage/review-evidence/)

Each enforcement site was independently neutered at the candidate and the focused tests re-run; every mutation was caught (source restored byte-identical after each probe, `git status --porcelain` clean):

| Enforcement site (file) | Mutation | Killing test that failed | Log |
|---|---|---|---|
| `auth.rs` extractor `credential_revoked` (~742) | `if false &&` guard | `revoke_all_blocks_credential_from_never_seen_device` | `task-111-mutation-extractor-credential-revoked-disabled.log` |
| `auth.rs` `authenticate_token` `credential_revoked` (~873) | `if false &&` guard | `ws_auth_rejects_signed_out_everywhere_credential` | `task-111-mutation-ws-credential-revoked-disabled.log` |
| `auth.rs` `authenticate_token` `track_session` rejection (~878) | `let _ =` discard | `ws_auth_rejects_revoked_ws_device_session` | `task-111-mutation-ws-track-session-disabled.log` |
| `api/users.rs` `list_my_sessions` active filter (801) | `.filter(\|s\| true)` | `my_session_listing_excludes_revoked_and_expired_sessions` | `task-111-mutation-my-listing-filter-disabled.log` |
| `api/users.rs` `list_user_sessions` active filter (873) | `.filter(\|s\| true)` | `admin_session_listing_excludes_revoked_and_expired_sessions` | `task-111-mutation-admin-listing-filter-disabled.log` |

Prior-round findings verified closed:

- **ff6a7ca3 (extractor path):** `revoke_all_blocks_credential_from_never_seen_device` exists, passes in the suite, and kills the extractor `credential_revoked` removal (probe above). The never-seen-device scenario (fresh User-Agent, no session row of its own) is genuinely exercised.
- **88ab7af7 F1 (WS/TTY path):** both function-level tests against `authenticate_token` (used by `ws.rs:520` and `tty.rs:46`) exist, pass, and each kills its single-check removal independently (probes above).
- **88ab7af7 F2 (listing filter):** both listing tests exist, pass, and each kills its handler's filter removal (probes above).
- **88ab7af7 F3 (docs):** the Shipped section now lists coverage by exact test name; every named test was confirmed present and passing in this round's run (`task-111-server-session-tests.log`).

## Implementation review (no findings)

- **Domain:** `UserSession` in `gyre-domain/src/user_profile.rs` with all nine spec fields; only the SHA-256 of the API key is stored (`hash_api_key`); `is_active(now)` = `!revoked && expires_at > now`.
- **Port:** `SessionRepository` (`gyre-ports/src/user_profile.rs`) with create/list/find_by_id/find_by_token_hash/find_by_credential_and_device/touch/revoke/revoke_all_for_user/delete_expired_before; the port documents the duplicate-id, owner-scoped-revoke, and never-delete-revoked contracts, and all three adapters enforce them (mem guards in code — `check-mem-port-contracts.sh` OK).
- **Adapters:** SQLite, PostgreSQL, and mem all implement the port against shared migration `2026-10-08-000059_user_sessions` (unique Diesel version — `check-migration-versions.sh` OK; portable SQL — `check-migration-sql-portability.sh` OK). `find_by_credential_and_device` orders newest-first so re-minted rows are found rather than re-minted per request.
- **Auth integration:** sessions minted on successful API-key auth in both the HTTP extractor and `authenticate_token` (WS/TTY); `last_active_at` + sliding `expires_at` refreshed at most once per `SESSION_TOUCH_THROTTLE_SECS` (60s, tested by `session_last_active_touch_is_throttled`); expired-but-unrevoked rows are re-minted, not zombified (`expired_session_is_reminted_not_zombified`); IP from `ConnectInfo` socket peer only (no forwarded-header trust, CWE-348); revocation storage failures fail soft with a log but never bypass in either direction on the key-validation path.
- **API:** `GET /users/me/sessions` (active only, no token_hash exposed in `UserSessionResponse`), `DELETE /users/me/sessions/:id` (existence + ownership check, foreign id is 404), `POST /users/me/sessions/revoke-all`, plus TenantAdmin variants gated per-handler on `UserRole::Admin` with target-user-existence 404s — routes are ABAC-exempt with per-handler authorization, and `check-abac-exempt-handlers.sh` passes.
- **Background cleanup:** `spawn_session_cleanup` (daily, wired in `main.rs:98`) deletes only unrevoked rows expired >30 days; revoked rows are never deleted (they are the durable sign-out record) — enforced in all three adapters and covered by `signout_everywhere_survives_retention_cleanup` and `session_retention_never_deletes_revoked_rows`.
- **Attribution:** all nine frontmatter commits are ancestors of the candidate; `check-task-commit-attribution.sh` OK.

## Verdict

Every acceptance criterion is met with production code, every enforcement site has an independently mutation-verified killing test, and all prior review findings are closed. Approved.
