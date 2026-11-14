---
title: "Implement session management"
spec_ref: "user-management.md §Session Management"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "user-management.md §Session Management"
commits: ["bdf1855d", "fd4351b9", "97ef01dd", "db116ab7", "ed6a9adc", "6a8d4a7c", "57f6d144", "f811501f", "4a71f98e", "a4e0eaf8", "6f20bcb2", "74749334", "5a0816fe", "0564d4e5", "5fc4fb1a", "30cb6ddd", "2e1d71a8", "66582a88", "1b6d9d2e", "f41c1d83", "4886ec07"]
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

- [x] `UserSession` domain entity in gyre-domain
- [x] `SessionRepository` port trait and SQLite implementation
- [x] DB migration creates `user_sessions` table
- [x] Sessions created on auth, `last_active_at` updated (throttled)
- [x] GET endpoint lists current user's active sessions
- [x] DELETE endpoint revokes individual session
- [x] POST revoke-all revokes all sessions for user
- [x] Revoked sessions rejected in auth middleware
- [x] Routes registered with ABAC mappings
- [ ] `cargo test --all` passes (deferred to the independent verifier; see Shipped)

## Agent Instructions

Read `specs/system/user-management.md` §Session Management for full requirements. Auth middleware is in `gyre-server/src/auth.rs`. User model is in `gyre-domain/src/user.rs`. Route registration is in `gyre-server/src/api/mod.rs`. ABAC mappings in `gyre-server/src/abac_middleware.rs`. Check migration numbering.

## Shipped

Session management per `user-management.md` §Session Management is implemented end-to-end by this branch's reviewed history. This round merges the new assignment base `32c4faef` (task-135 gate chain configuration and gate agent lifecycle, task-011 spec save git write-through) and repairs the four test failures that verification finding `4fb29ee2` (`cargo test --all`, exit 101) attributed to that merge.

### Implementation (unchanged this round)

- **Domain:** `UserSession` in `gyre-domain/src/user_profile.rs` with all spec fields; only the SHA-256 of the session token is stored, never plaintext.
- **Port + adapters:** `SessionRepository` (create, list, find, touch, revoke, revoke_all, delete_expired) implemented by SQLite, PostgreSQL, and mem adapters.
- **Auth integration:** sessions minted on API-key auth keyed by (credential, IP, User-Agent); `last_active_at` + sliding expiry refreshed at most once per 60s; revoked sessions reject auth on both HTTP and WS paths; IP from socket peer only.
- **API:** `GET/DELETE /users/me/sessions[...]`, `POST /users/me/sessions/revoke-all`, plus TenantAdmin variants with tenant-containment checks.
- **Cleanup:** daily `spawn_session_cleanup` deletes unrevoked sessions expired >30 days.

### This round's repair (`bdf1855d`, on merge `a52bc417` of base `32c4faef`)

The base `32c4faef` and this branch both descend from `bfc36c9c`; git auto-merged them without textual conflicts, but four tests the base brought in build the MR router from `test_state()` without seeding the `repo-1` fixture (`closing_mr_emits_analytics_event`, `merging_mr_emits_analytics_event_with_gate_count`, `scoped_gate_review_records_persona_attribution`, `unscoped_review_has_no_gate_attribution`). `create_mr` has refused unknown repositories since the M34 scope-fabrication repair (`74749334`), so `create_test_mr` got a 404 and panicked unwrapping `json["id"]` — the exact failure class repaired before in `db116ab7`. All four now use the same `seed_repo(&state).await` fixture every other test in the module already uses; no product code changed. The attribution test's mint-site comment was also refreshed to describe the seeded-repo workspace binding instead of the base's pre-merge bootstrap-workspace one.

### Test evidence

In-sandbox this round (log: `/tmp/stage/review-evidence/task-111-repair-round.log`):

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --all-targets` — clean (the merge plus the four fixture repairs type-check, including the base's new `mint_scoped` call sites and `Review` attribution fields).
- All mechanical invariant gates pass on the merged tree: architecture/hierarchy lint, ABAC route registry + exempt-handler parity + context parity, MCP write-tool gating, migration versions and SQL portability, dead message kinds, byte-slice truncation, relative path defaults, fail-open ref resolution, mem port contracts, fabricated scope defaults (lookup and construction), lossy secret conversion, inert enforcement, forged scope fields, forwarded-header trust, in-memory state stores, unbounded external HTTP.
- `cargo fmt --check` on the changed file — clean.

Deferred to the independent verifier (gyre-server codegen exceeds the in-sandbox 4 GiB budget; the wrapper blocks it by design):

- `cargo test --all` — the authoritative rerun of finding `4fb29ee2`. Expected: the four repaired `api::merge_requests::tests` now pass; the remaining suite passed verbatim on this exact merged tree in that finding's run (2181 passed, only those 4 failed).
