---
title: "Implement session management"
spec_ref: "user-management.md §Session Management"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "user-management.md §Session Management"
commits: ["0564d4e5b6f95c82bc20dd7cdd64a35ab46fe713", "5fc4fb1a1e470599b94208c7b7b06e1d3d7f3c8e", "30cb6ddd4231defa2c773cd7137ae20d3bdcfdc0", "2e1d71a89f82bd938262b41fd2ecc66be220f482", "66582a887e6caaf605c85ad05a2b8320791079fd", "1b6d9d2ea13cb5687f6cffcb8abd0e656145d581", "f41c1d83fc063d61a99dabbea5bd0ae6f602c52f", "4886ec076fdb44441c5090e66b7cd17389f9a70d", "5a0816fe602097c22a86d51c7095b644ca1d4893"]
---

## Spec Excerpt

From `user-management.md` §Session Management:

```rust
pub struct UserSession {
    pub id: Id,
    pub user_id: Id,
    pub token_hash: String,     // SHA-256 of session token (never store plaintext)
    pub ip_address: String,
    pub user_agent: String,
    pub created_at: u64,
    pub last_active_at: u64,
    pub expires_at: u64,
    pub revoked: bool,
}
```

Users can:
- View all active sessions (device, IP, last active)
- Revoke individual sessions
- Revoke all sessions ("sign out everywhere")
- TenantAdmins can view and revoke any user's sessions

**API endpoints:**

| Endpoint | Method | Purpose |
|---|---|---|
| `GET /api/v1/users/me/sessions` | GET | My active sessions |
| `DELETE /api/v1/users/me/sessions/{id}` | DELETE | Revoke a session |
| `POST /api/v1/users/me/sessions/revoke-all` | POST | Revoke all sessions |

## Implementation Plan

1. **Domain entity in `gyre-domain`:**
   - Add `UserSession` struct with all spec fields
   - Session token generation utility (crypto-random, SHA-256 hashed for storage)

2. **Port trait in `gyre-ports`:**
   - `SessionRepository` — create, find_by_user, find_by_id, find_by_token_hash, update_last_active, revoke, revoke_all_for_user, delete_expired

3. **SQLite adapter:**
   - Migration for `user_sessions` table (id, user_id, token_hash, ip_address, user_agent, created_at, last_active_at, expires_at, revoked)
   - Implement `SessionRepository`

4. **Session tracking integration:**
   - On successful auth, create a session record (extract IP from request, User-Agent header)
   - Update `last_active_at` on authenticated requests (throttled — at most once per minute to avoid write amplification)
   - Check session revocation status in auth middleware (reject revoked sessions)

5. **API endpoints:**
   - `GET /api/v1/users/me/sessions` — list active (non-revoked, non-expired) sessions for current user
   - `DELETE /api/v1/users/me/sessions/{id}` — revoke session (set `revoked: true`)
   - `POST /api/v1/users/me/sessions/revoke-all` — revoke all sessions for current user

6. **Background cleanup:**
   - Periodic job to delete expired sessions (e.g., sessions expired > 30 days ago)

7. **Register routes** in `api/mod.rs` and add ABAC mappings.

## Acceptance Criteria

- [ ] `UserSession` domain entity in gyre-domain
- [ ] `SessionRepository` port trait and SQLite implementation
- [ ] DB migration creates `user_sessions` table
- [ ] Sessions created on auth, `last_active_at` updated (throttled)
- [ ] GET endpoint lists current user's active sessions
- [ ] DELETE endpoint revokes individual session
- [ ] POST revoke-all revokes all sessions for user
- [ ] Revoked sessions rejected in auth middleware
- [ ] Routes registered with ABAC mappings
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/user-management.md` §Session Management for full requirements. Auth middleware is in `gyre-server/src/auth.rs`. User model is in `gyre-domain/src/user.rs`. Route registration is in `gyre-server/src/api/mod.rs`. ABAC mappings in `gyre-server/src/abac_middleware.rs`. Check migration numbering.

## Shipped

Session management is implemented end-to-end against `user-management.md` §Session Management (base `f4acb4eb`, merged up to base `98a5299b9` in `f0d541c75`; review-repair killing tests in `5a0816fe6`):

- **Domain:** `UserSession` in `gyre-domain/src/user_profile.rs` with all spec fields (id, user_id, token_hash, ip_address, user_agent, created_at, last_active_at, expires_at, revoked); only the SHA-256 of the API key is stored (`hash_api_key`), never plaintext. `is_active(now)` = not revoked and not expired.
- **Port:** `SessionRepository` in `gyre-ports/src/user_profile.rs` — create (duplicate-id fails), list_for_user, find_by_id, find_by_token_hash, find_by_credential_and_device, touch (slides TTL), revoke (owner-scoped), revoke_all_for_user, delete_expired_before (never deletes revoked rows — they are the durable sign-out record).
- **Adapters:** SQLite + PostgreSQL + mem implementations against shared migration `2026-10-08-000059_user_sessions` (renumbered 000056→000057→000058→000059 across rebase/merge repairs — always a pure directory rename to the next unused same-date sequence, SQL byte-identical); diesel schema registered.
- **Auth integration (`auth.rs`):** on successful API-key auth, the session for (credential, IP, User-Agent) is found or minted; `last_active_at` + sliding `expires_at` are refreshed at most once per `SESSION_TOUCH_THROTTLE_SECS` (60s); a revoked session for the device rejects auth with 401; `credential_revoked` rejects the key when every session for it is revoked ("sign out everywhere", including never-seen devices) — enforced before `track_session` in the HTTP extractor and on the WS/TTY `authenticate_token` path. IP comes from `ConnectInfo` socket peer (no forwarded-header trust, CWE-348).
- **API (`api/users.rs` + `api/mod.rs`):** `GET /users/me/sessions` (active only, no token hash exposed), `DELETE /users/me/sessions/:id` (404 on foreign id), `POST /users/me/sessions/revoke-all`, plus TenantAdmin variants `GET|DELETE|POST /users/:user_id/sessions[...]` with Admin-role per-handler checks (ABAC-exempt routes), target-user existence 404s.
- **Background cleanup:** `spawn_session_cleanup` (daily) deletes only unrevoked sessions expired >30 days.

### Test evidence (run at head `5a0816fe6` in this sandbox)

- `cargo test -p gyre-server --lib` — **1435 passed, 0 failed** (full crate suite; includes every session test named below).
- `cargo test -p gyre-adapters --lib -- sqlite::user_profile::tests::session` — 8 passed (roundtrip, duplicate-id, device-match on all four fields, newest-match, TTL slide, owner-scoped revoke, revoke-all scope, retention-never-deletes-revoked).
- Session coverage is listed by exact test name rather than by a `-- session` filter count, because substring filters also match unrelated name-colliding tests (e.g. ws `ws_session_cap`, spec_patrol `supersession`) while missing tests whose names lack the substring:
  - `api/users.rs`: `sessions_endpoint_lists_session_created_by_auth`; `revoke_session_then_credential_is_rejected`; `revoking_one_device_session_does_not_sign_out_other_devices`; `revoke_all_sessions_signs_out_everywhere`; `revoke_all_blocks_credential_from_never_seen_device`; `my_session_listing_excludes_revoked_and_expired_sessions`; `admin_session_listing_excludes_revoked_and_expired_sessions`; `revoke_cannot_target_another_users_session`; `tenant_admin_can_view_and_revoke_another_users_sessions`; `non_admin_cannot_view_or_revoke_another_users_sessions`; `tenant_admin_revoke_all_signs_user_out_everywhere`; `admin_session_endpoints_unknown_user_is_404`.
  - `auth.rs`: `session_last_active_touch_is_throttled`; `expired_session_is_reminted_not_zombified`; `signout_everywhere_survives_retention_cleanup`; `ws_auth_rejects_signed_out_everywhere_credential`; `ws_auth_rejects_revoked_ws_device_session`.
- Mutation-verified killing strength (logs under `/tmp/stage/review-evidence/task-111-mutation-*.log`): removing the `credential_revoked` check, the `track_session` rejection, or both from `authenticate_token` each fails at least one WS-path test; neutering the `.filter(|s| s.is_active(now))` in either listing handler fails both listing tests.
- Mechanical checks pass: `check-arch.sh`, `check-abac-route-registry.sh`, `check-migration-versions.sh`, `check-inert-enforcement.sh`, `check-task-commit-attribution.sh`.

### Review repairs

- **Attempt `ff6a7ca3`** (extractor path, closed in `f0d541c75`): the extractor's `credential_revoked()` call had no endpoint-level killing test. `revoke_all_blocks_credential_from_never_seen_device` posts revoke-all, then presents the same key with a fresh User-Agent (never-seen device, no session row of its own) and asserts 401 — removing the extractor call mints a new session and returns 200, failing the test.
- **Attempt `88ab7af7`** (this round, commit `5a0816fe6`):
  - WS/TTY path: `authenticate_token` (ws.rs / tty.rs present a raw token from the client's Auth message) had no killing test for its revocation enforcement. `ws_auth_rejects_signed_out_everywhere_credential` signs a credential out everywhere (its only row is an HTTP device) and asserts the WS path rejects it — the WS ("", "") device pair has no revoked row of its own, so only the credential-wide check can reject it. `ws_auth_rejects_revoked_ws_device_session` revokes only the WS device's session while a sibling HTTP session keeps the credential alive, and asserts the WS path still rejects — only the per-device `track_session` rejection catches this. Mutation-probed: each single-check removal and the both-checks removal fail at least one of the two tests.
  - Active-only listing filter: `my_session_listing_excludes_revoked_and_expired_sessions` and `admin_session_listing_excludes_revoked_and_expired_sessions` seed an expired row and revoke one device's session, then assert both the /me and TenantAdmin listings return only the live sessions. Mutation-probed: neutering the filter in either handler fails the corresponding test.
  - Docs: the previous Shipped list misattributed coverage to a `-- session` filtered run; coverage is now stated by exact test name (above).

### Merge history

Merged up to base `98a5299b9` in `f0d541c75`: `gyre-domain/src/lib.rs` combined re-exports (base's UserPreferences additions plus `UserSession`); `api/users.rs` kept both test suites (base's user-preferences tests with `bootstrap_user`, our session tests; `provision_user_with_key` reduced to a thin wrapper); migration renumbered `000058`→`000059` (base added `2026-10-08-000058_user_entity_profile_fields`, colliding with `user_sessions` at Diesel version `20261008000058` — only the alphabetically first would ever run; pure directory rename, SQL byte-identical). Earlier rebase repairs (merge into `72ad2a12` as `b62fa0f16`): migration 000056→000057→000058 chain, fail-closed `seed_builtin_policies` kept, CWE-348 comment kept, exemption files matched to base, exemption line re-pins for shifted frozen sites (none added).
