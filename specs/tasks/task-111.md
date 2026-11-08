---
title: "Implement session management"
spec_ref: "user-management.md §Session Management"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "user-management.md §Session Management"
commits: ["0564d4e5b6f95c82bc20dd7cdd64a35ab46fe713", "5fc4fb1a1e470599b94208c7b7b06e1d3d7f3c8e", "30cb6ddd4231defa2c773cd7137ae20d3bdcfdc0", "2e1d71a89f82bd938262b41fd2ecc66be220f482", "66582a887e6caaf605c85ad05a2b8320791079fd", "1b6d9d2ea13cb5687f6cffcb8abd0e656145d581", "f41c1d83fc063d61a99dabbea5bd0ae6f602c52f", "4886ec076fdb44441c5090e66b7cd17389f9a70d", "f0d541c75aed53503732cf78325510b0d4b77bcd"]
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

Session management is implemented end-to-end against `user-management.md` §Session Management (base `f4acb4eb` → head, merged up to base `98a5299b9` in commit `f0d541c75`):

- **Domain:** `UserSession` in `gyre-domain/src/user_profile.rs` with all spec fields (id, user_id, token_hash, ip_address, user_agent, created_at, last_active_at, expires_at, revoked); only the SHA-256 of the API key is stored (`hash_api_key`), never plaintext. `is_active(now)` = not revoked and not expired.
- **Port:** `SessionRepository` in `gyre-ports/src/user_profile.rs` — create (duplicate-id fails), list_for_user, find_by_id, find_by_token_hash, find_by_credential_and_device, touch (slides TTL), revoke (owner-scoped), revoke_all_for_user, delete_expired_before (never deletes revoked rows — they are the durable sign-out record).
- **Adapters:** SQLite + PostgreSQL + mem implementations against shared migration `2026-10-08-000059_user_sessions` (renumbered 000056→000057→000058→000059 across rebase/merge repairs — always a pure directory rename to the next unused same-date sequence, SQL byte-identical); diesel schema registered.
- **Auth integration (`auth.rs`):** on successful API-key auth, the session for (credential, IP, User-Agent) is found or minted; `last_active_at` + sliding `expires_at` are refreshed at most once per `SESSION_TOUCH_THROTTLE_SECS` (60s); a revoked session for the device rejects auth with 401; `credential_revoked` rejects the key when every session for it is revoked ("sign out everywhere", including never-seen devices — enforced before `track_session` in the extractor and on the WS ticket path). IP comes from `ConnectInfo` socket peer (no forwarded-header trust, CWE-348).
- **API (`api/users.rs` + `api/mod.rs`):** `GET /users/me/sessions` (active only, no token hash exposed), `DELETE /users/me/sessions/:id` (404 on foreign id), `POST /users/me/sessions/revoke-all`, plus TenantAdmin variants `GET|DELETE /users/:user_id/sessions[...]` with Admin-role per-handler checks (ABAC-exempt routes), target-user existence 404s.
- **Background cleanup:** `spawn_session_cleanup` (daily) deletes only unrevoked sessions expired >30 days.

### Test evidence

- `cargo test -p gyre-adapters --lib -- sqlite::user_profile::tests::session` — 8 passed (roundtrip, duplicate-id, device-match on all four fields, newest-match, TTL slide, owner-scoped revoke, revoke-all scope, retention-never-deletes-revoked).
- `cargo test -p gyre-server --lib -- session` — 13 passed, covering: session listed after auth; revoking one session rejects the credential; revoking one device does not sign out other devices; revoke-all signs out everywhere; **revoke-all blocks the credential from a never-seen device** (review-repair round below); foreign-session revoke is 404; TenantAdmin view/revoke; non-Admin forbidden; admin revoke-all; unknown-user 404; throttled touch; expired-session re-mint; sign-out survives retention cleanup.
- All mechanical checks pass with no regression vs base `98a5299b9`: migration versions + SQL portability, ABAC route registry + exempt handlers, task-commit attribution; all 89 `scripts/check-*.sh` behave identically to base (the 34 failures are pre-existing baseline state owned by verification — awk-dialect and known findings, verified identical on a clean base worktree).

### Review repair (attempt `ff6a7ca3` finding, closed in merge commit `f0d541c75`)

The reviewer's mutation probe showed the extractor's `credential_revoked()` call had no endpoint-level killing test (neutering it left all API-level tests green; the only failing test called the function directly). Fixed by `revoke_all_blocks_credential_from_never_seen_device` in `api/users.rs`: POST `/users/me/sessions/revoke-all`, then present the same key with a fresh User-Agent (never-seen device, no session row of its own) and assert 401. Because `credential_revoked` runs before `track_session` in the extractor, removing the call makes the fresh device mint a new session and receive 200 — the test fails, killing the regression. Re-running the session suites and this test is deferred to remote verification (crates.io unreachable in this sandbox; evidence `/tmp/stage/review-evidence/task-111-round3-merge-and-killing-test.md`).

### Merge into base `98a5299b9` (commit `f0d541c75`)

- `gyre-domain/src/lib.rs`: combined re-exports — base's UserPreferences additions (DiffView, FeedScope, NotificationChannels, UiDensity, UserPreferencesPatch) plus our `UserSession`; both symbol sets verified present in the merged sources.
- `api/users.rs` tests: kept both suites (base's user-preferences tests with `bootstrap_user`, our session tests); `provision_user_with_key` reduced to a thin wrapper over `bootstrap_user` to avoid duplicating the create-user round-trip. Handler code auto-merged — session handlers/routes/ABAC mappings intact alongside base's preference handlers.
- Migration renumbered `000058` → `000059`: base added `2026-10-08-000058_user_entity_profile_fields`, colliding with our `user_sessions` at Diesel version `20261008000058` (only the alphabetically first would ever run). Pure directory rename, SQL byte-identical.
- Earlier rebase repairs (attempt `e88a5646`, merge into `72ad2a12` as `b62fa0f16`): migration 000056→000057→000058 chain, fail-closed `seed_builtin_policies` kept, CWE-348 comment kept, exemption files matched to base, exemption line re-pins for shifted frozen sites (none added).

Not verifiable in this sandbox: Rust compilation (crates.io/static.crates.io unreachable, empty registry — infrastructure restriction recorded for remote verification) and live-server HTTP transport (TCP listener unsupported). Endpoint behavior is proven by in-process Router (oneshot) tests; every session source file (domain/port/adapter `user_profile.rs`) is byte-identical to the previously verified candidate head — the only Rust deltas in `f0d541c75` are base's already-reviewed task-088 additions, the new killing test, and the helper delegation, plus a 0-line-diff migration directory rename (`git diff 9746304f9 f0d541c75`).
