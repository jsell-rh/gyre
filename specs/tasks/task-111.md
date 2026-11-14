---
title: "Implement session management"
spec_ref: "user-management.md §Session Management"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "user-management.md §Session Management"
commits: ["97ef01dd7c12d9f49f03f49ce64829916871df79", "db116ab7498d7ef385b229a9761d487bee10297e", "ed6a9adc819dd87a141af3fecee543c909233f97", "6a8d4a7cdd53c43eee53110c13b734d3d60d6168", "57f6d144a5bd01905efa6889dade4ce3487c34ea", "f811501fb395c914da41dc11ba2d9e91d3611e54", "4a71f98e3f43d72732ada3bf3ec08353a81b4a3e", "a4e0eaf8f87de1d3483e9cb2813e6e5434acffeb", "6f20bcb241f6dee4f991553edf35e3ec90c97b3e", "74749334f8f2846a81a9c108c72ebce383d5222f", "5a0816fe602097c22a86d51c7095b644ca1d4893", "0564d4e5b6f95c82bc20dd7cdd64a35ab46fe713", "5fc4fb1a1e470599b942084c7b7b06e1d3d7f3c8e", "30cb6ddd4231defa2c773cd7137ae20d3bdcfdc0", "2e1d71a89f82bd938262b41fd2ecc66be220f482", "66582a887e6caaf605c85ad05a2b8320791079fd", "1b6d9d2ea13cb5687f6cffcb8abd0e656145d581", "f41c1d83fc063d61a99dabbea5bd0ae6f602c52f", "4886ec076fdb44441c5090e66b7cd17389f9a70d", "fd4351b9988673828b61a2b1379057db07548018"]
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

Session management per `user-management.md` §Session Management is implemented end-to-end by this branch's reviewed history (head of the implementation work: `ed6a9adc`; prior verification repairs through the task-112 base merge `52ad675e`). This round repairs the three findings from the last verification run, which merged the candidate onto the new assignment base `bfc36c9c` (task-146: analytics event schema).

### Implementation (unchanged this round)

- **Domain:** `UserSession` in `gyre-domain/src/user_profile.rs` with all spec fields; only the SHA-256 of the session token is stored, never plaintext.
- **Port + adapters:** `SessionRepository` (create, list, find, touch, revoke, revoke_all, delete_expired) implemented by SQLite, PostgreSQL, and mem adapters.
- **Auth integration:** sessions minted on API-key auth keyed by (credential, IP, User-Agent); `last_active_at` + sliding expiry refreshed at most once per 60s; revoked sessions reject auth on both HTTP and WS paths; IP from socket peer only.
- **API:** `GET/DELETE /users/me/sessions[...]`, `POST /users/me/sessions/revoke-all`, plus TenantAdmin variants with tenant-containment checks.
- **Cleanup:** daily `spawn_session_cleanup` deletes unrevoked sessions expired >30 days.

### This round's repair (`fd4351b9`, on merge `92ed0c5d` of base `bfc36c9c`)

The base `bfc36c9c` (task-146) and this branch both descend from `1ee52c43`; git auto-merged them without textual conflicts, leaving three semantic failures that the verifier caught (10 compile errors, duplicate migration version, fabricated scope literals):

1. **`MergeRequest::new` call sites (10 × E0061/E0277):** this branch's task-111 verification repair made the constructor take `workspace_id` as its 3rd parameter; task-146's new test hunks in `api/specs.rs` and `merge_processor.rs` still used the old 6-arg form with a post-hoc `mr.workspace_id` assignment. All five sites now pass `workspace_id` in the constructor (redundant assignments dropped, `mut` adjusted).
2. **Migration version collision (20261008000063):** task-146's `analytics_scope_columns` (from the base) and task-111's `user_sessions` both claimed sequence 000063 — the alphabetically-later `user_sessions` would silently never run. Renumbered `user_sessions` to the next unused sequence **000064**; coverage-matrix references updated.
3. **Fabricated `tenant_id: "default"` scope literals (4):** task-146's analytics edits moved the four construction literals in `sqlite/postgres analytics.rs` off their frozen exemption lines (115/225 → 154/427, 171/443). Fixed for real rather than re-pointing exemptions (explicitly forbidden): the analytics `record()`/`record_cost()` rows now carry the storage handle's resolved `self.tenant_id` — the same form as the merge_request/repository `create()` rows from the earlier task-111 repair. All six exemption entries deleted (four fixed analytics sites, two dead `activity.rs` lines referencing files removed by task-207) and `FROZEN_EXEMPTION_COUNT` lowered 6→0 in both the shell header and the Python body.

### Test evidence

In-sandbox this round (log: `/tmp/stage/review-evidence/task-111-repair-round.log`):

- `SKIP_WEB_BUILD=1 cargo check --workspace --all-targets` — clean (reproduces and clears the verifier's 10 errors; server test *execution* deferred per the 4 GiB budget).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib` — 530 passed, 0 failed (includes the 9 session-repository tests and all 16 analytics tests exercising the re-scoped `record()`/`record_cost()` insert paths).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-domain` — 510 + 8 passed, 0 failed.
- `SKIP_WEB_BUILD=1 cargo clippy -p gyre-ports -p gyre-adapters --all-targets` — no errors; only pre-existing async_trait `must_use` warnings (all in base's unchanged `gyre-ports/src/analytics.rs` trait), none in edited files.
- All 19 mechanical gates pass, including the three that failed in verification: `check-migration-versions.sh`, `check-scope-literal-defaults.sh`, and the workspace compile; rustfmt clean on all changed files; `check-task-commit-attribution.sh` OK.

Required in the independent verifier (large gyre-server test compilation deferred per the 4 GiB budget):

- `cargo test -p gyre-server --lib merge_processor` and `cargo test -p gyre-server --lib api::specs` — in particular the five repaired tests: `reject_spec_records_mr_closed_analytics_events`, `gate_failure_during_selection_records_queue_processed_failed`, `spec_policy_block_records_queue_processed_failed`, `circuit_breaker_cancel_records_queue_processed_skipped`, `queue_merge_records_mr_merged_analytics_event`.
- The task-111 session test suites already run green in prior rounds (`users` sessions, `auth` session tracking) and are unaffected by this round's changes.
