---
title: "Implement session management"
spec_ref: "user-management.md §Session Management"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "user-management.md §Session Management"
commits: ["97ef01dd7c12d9f49f03f49ce64829916871df79", "db116ab7498d7ef385b229a9761d487bee10297e", "ed6a9adc819dd87a141af3fecee543c909233f97", "6a8d4a7cdd53c43eee53110c13b734d3d60d6168", "57f6d144a5bd01905efa6889dade4ce3487c34ea", "f811501fb395c914da41dc11ba2d9e91d3611e54", "4a71f98e3f43d72732ada3bf3ec08353a81b4a3e", "a4e0eaf8f87de1d3483e9cb2813e6e5434acffeb", "6f20bcb241f6dee4f991553edf35e3ec90c97b3e", "74749334f8f2846a81a9c108c72ebce383d5222f", "5a0816fe602097c22a86d51c7095b644ca1d4893", "0564d4e5b6f95c82bc20dd7cdd64a35ab46fe713", "5fc4fb1a1e470599b94208c7b7b06e1d3d7f3c8e", "30cb6ddd4231defa2c773cd7137ae20d3bdcfdc0", "2e1d71a89f82bd938262b41fd2ecc66be220f482", "66582a887e6caaf605c85ad05a2b8320791079fd", "1b6d9d2ea13cb5687f6cffcb8abd0e656145d581", "f41c1d83fc063d61a99dabbea5bd0ae6f602c52f", "4886ec076fdb44441c5090e66b7cd17389f9a70d", "52ad675e45e0645d92535c265ab86b1c2b343d65"]
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

Session management is implemented end-to-end against `user-management.md` §Session Management by this branch's reviewed history (commits above; head of the implementation work: `ed6a9adc`; latest repair: the merge commit `52ad675e`).

### Implementation

- **Domain:** `UserSession` in `gyre-domain/src/user_profile.rs` with all spec fields; only the SHA-256 of the session token is stored (`hash_api_key`), never plaintext; `is_active(now)` = not revoked and not expired.
- **Port:** `SessionRepository` in `gyre-ports/src/user_profile.rs` — create (duplicate-id fails), list_for_user, find_by_id, find_by_token_hash, find_by_credential_and_device, touch (slides TTL, throttled), revoke (owner-scoped), revoke_all_for_user, delete_expired_before (never deletes revoked rows — the durable sign-out record). Implemented by SQLite, PostgreSQL, and mem adapters on shared migration `2026-10-08-000063_user_sessions`.
- **Auth integration (`auth.rs`):** on successful API-key auth, the session for (credential, IP, User-Agent) is found or minted; `last_active_at` + sliding `expires_at` refreshed at most once per 60s; a revoked session for the device rejects auth with 401; `credential_revoked` rejects the key when every session for it is revoked ("sign out everywhere") — enforced on both the HTTP extractor and the WS/TTY `authenticate_token` paths. IP comes from `ConnectInfo` socket peer only (no forwarded-header trust, CWE-348).
- **API (`api/users.rs` + `api/mod.rs`):** `GET /users/me/sessions` (active only, no token hash exposed), `DELETE /users/me/sessions/:id` (404 on foreign id), `POST /users/me/sessions/revoke-all`, plus TenantAdmin variants with Admin-role **and tenant-containment** per-handler checks; target-user existence 404s.
- **Background cleanup:** `spawn_session_cleanup` (daily) deletes only unrevoked sessions expired >30 days.

### This round's repair (rebase onto the task-112 base `1ee52c43`)

The assignment base `1ee52c43` (task-112: notification delivery channels & routing) diverged from this branch at `b6892403`. Merged the base in as `52ad675e`, resolving seven content conflicts where task-112's `UserChannelPreferenceRepository` and task-111's `SessionRepository` land in the same files (`gyre-ports/src/lib.rs`, `gyre-ports/src/user_profile.rs`, `gyre-adapters/src/{postgres,sqlite}/user_profile.rs`, `gyre-server/src/api/mod.rs`, `gyre-server/src/lib.rs`, coverage matrix). Resolution keeps **both** sides: both port traits and re-exports, both schema imports, both `store!` wiring blocks, both route groups, both adapter impls, and both test sets (the sqlite channel-prefs round-trip test from the base sits next to the session tests). One semantic fallout fixed by hand: `upsert_batch` in the notification-preference impls now calls `UserNotificationPreferenceRepository::upsert(self, pref)` explicitly, because the merged storage types implement two `upsert` traits and the bare `self.upsert(...)` no longer resolves. The coverage header recounts the merged matrix (11 n/a, 14 task-assigned, 11 implemented — rows 12 and 16–18 both present). `web/dist` is restored to the assignment base; final integration rebuilds and publishes those assets remotely.

### Test evidence

In-sandbox this round (log: `/tmp/stage/review-evidence/merge-repair-round.log`):

- `SKIP_WEB_BUILD=1 cargo check -p gyre-ports -p gyre-adapters -p gyre-server -p gyre-domain` — clean (server type-checked; full server test compilation deferred per the 4 GiB budget).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib user_profile` — 13 passed, 0 failed (9 session-repository tests incl. duplicate-id, owner-scoped revoke, retention-never-deletes-revoked, TTL slide; channel-prefs round-trip from the base; notification/token tests).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-domain` — 509 + 8 passed, 0 failed.
- `SKIP_WEB_BUILD=1 cargo clippy -p gyre-ports -p gyre-adapters --all-targets` — exit 0; only the pre-existing async_trait `must_use` warnings, none new.
- Mechanical gates: `check-arch.sh`, `check-abac-route-registry.sh`, `check-abac-exempt-handlers.sh` (85 handlers), `check-mcp-write-tools.sh`, `check-migration-versions.sh`, `check-migration-sql-portability.sh` — all OK.
- `rustfmt --edition 2021 --check` on every resolved file — clean.

Required in the independent verifier (large gyre-server test compilation deferred per the 4 GiB budget):

- `SKIP_WEB_BUILD=1 cargo test --all` — repo-wide acceptance criterion (both task-111 session suites and task-112 channel/dispatcher suites).
- `SKIP_WEB_BUILD=1 cargo clippy` scoped to branch-changed files.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib api::users` and `auth` — the session-management surface.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib` notification/dispatcher modules — the task-112 surface carried by this merge.
- Full mechanical gate suite (CI).
