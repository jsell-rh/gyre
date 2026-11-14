---
title: "Implement session management"
spec_ref: "user-management.md §Session Management"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "user-management.md §Session Management"
commits: ["c6cad385d11908daddf8a9b5e7f049237ebc2335", "c947d22f1aa9b66f30b26b913aeda2e6c506fb94", "bdf1855d12d8837e6142b7ab060b09ac3cb63e55", "fd4351b9988673828b61a2b1379057db07548018", "97ef01dd7c12d9f49f03f49ce64829916871df79", "db116ab7498d7ef385b229a9761d487bee10297e", "ed6a9adc819dd87a141af3fecee543c909233f97", "6a8d4a7cdd53c43eee53110c13b734d3d60d6168", "57f6d144a5bd01905efa6889dade4ce3487c34ea", "f811501fb395c914da41dc11ba2d9e91d3611e54", "4a71f98e3f43d72732ada3bf3ec08353a81b4a3e", "a4e0eaf8f87de1d3483e9cb2813e6e5434acffeb", "6f20bcb241f6dee4f991553edf35e3ec90c97b3e", "74749334f8f2846a81a9c108c72ebce383d5222f", "5a0816fe602097c22a86d51c7095b644ca1d4893", "0564d4e5b6f95c82bc20dd7cdd64a35ab46fe713", "5fc4fb1a1e470599b94208c7b7b06e1d3d7f3c8e", "30cb6ddd4231defa2c773cd7137ae20d3bdcfdc0", "2e1d71a89f82bd938262b41fd2ecc66be220f482", "66582a887e6caaf605c85ad05a2b8320791079fd", "1b6d9d2ea13cb5687f6cffcb8abd0e656145d581", "f41c1d83fc063d61a99dabbea5bd0ae6f602c52f", "4886ec076fdb44441c5090e66b7cd17389f9a70d"]
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
- [ ] `cargo test --all` passes (deferred to the independent verifier — 4 GiB build wrapper)

## Agent Instructions

Read `specs/system/user-management.md` §Session Management for full requirements. Auth middleware is in `gyre-server/src/auth.rs`. User model is in `gyre-domain/src/user.rs`. Route registration is in `gyre-server/src/api/mod.rs`. ABAC mappings in `gyre-server/src/abac_middleware.rs`. Check migration numbering.

## Shipped

Session management per `user-management.md` §Session Management is implemented end-to-end by this branch's reviewed history; the coverage matrix row (`specs/coverage/system/user-management.md` row 12) is `implemented | task-111`. This round resolved the rebase repair against the task-124 base and changed no product behavior.

### Implementation

- **Domain:** `UserSession` in `gyre-domain/src/user_profile.rs` with all spec fields; only the SHA-256 of the session token is stored, never plaintext.
- **Port + adapters:** `SessionRepository` (create, list, find, touch, revoke, revoke_all, delete_expired) implemented by SQLite, PostgreSQL, and mem adapters on migration `2026-10-08-000064_user_sessions`.
- **Auth integration:** sessions minted on API-key auth keyed by (credential, IP, User-Agent); `last_active_at` + sliding expiry refreshed at most once per 60s; revoked sessions reject auth on both HTTP and WS/TTY paths; `credential_revoked` rejects the key when every session for it is revoked; IP from socket peer only (CWE-348).
- **API:** `GET/DELETE /users/me/sessions[...]`, `POST /users/me/sessions/revoke-all`, plus TenantAdmin variants (`/users/:user_id/sessions...`) with per-handler role checks and tenant containment. Session routes are ABAC-exempt by design with per-handler authorization (validated by `check-abac-exempt-handlers.sh`).
- **Cleanup:** daily `spawn_session_cleanup` deletes unrevoked sessions expired >30 days (revoked rows kept as the durable sign-out record).

### This round: base merge repair (44049f85)

Repair finding `dd6ed4f1` (rebase of the task-111 branch onto base `5984177c`, task-124's head): the two branches implemented disjoint rows of `user-management.md` from common base `4b9220bc`. Two content conflicts, resolved in merge commit `44049f85`:

- `crates/gyre-server/src/api/mod.rs` — the `use users::{...}` import list: union of both handler sets (task-111 session handlers + task-124 user-management/invitation handlers). Route registrations and ABAC resolver mappings for both sides auto-merged cleanly and were verified present.
- `specs/coverage/system/user-management.md` — the "Last audited" recount: merged into one note covering both branches' rows. Row 26 (§API > User Management, task-124's "stays open until [the session endpoints] land") is closed by this merge — task-124's five endpoints plus task-111's three session endpoints are all live in this tree (row-28 precedent for cross-task completion). Mechanical recount on the merged table: 11 n/a, 0 not-started, 12 task-assigned, 13 implemented; `specs/coverage/SUMMARY.md` row updated to match (52%).

No product code changed this round beyond git's auto-merge of the two parents (both independently type-checked and tested on their own branches).

### Test evidence

Evidence log: `/tmp/stage/review-evidence/task-111-merge.md` (this round, head `44049f85`):

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` — exit 0 (7m07s, includes domain/ports/adapters).
- `cargo test -p gyre-adapters session` — 9/9 passed (create/roundtrip, duplicate-id contract, credential+device matching, newest-match, touch/sliding expiry, revoke scoping, revoke-all + delete-expired, retention never deleting revoked rows).
- `cargo test -p gyre-domain --lib` — 535/535 passed.
- `cargo fmt --check` — exit 0.
- Mechanical gates on the merged tree: `check-arch.sh`, `check-abac-route-registry.sh`, `check-abac-exempt-handlers.sh` (86 handlers), `check-migration-versions.sh`, `check-mem-port-contracts.sh`, `check-forwarded-header-trust.sh`, `check-forged-scope-fields.sh`, `check-in-memory-state-stores.sh` — all exit 0.

Pre-existing on the base (not introduced by this merge, not repaired here): the base's `2026-11-13-000063_user_deactivated_at` reuses 6-digit sequence 000063 already taken by `2026-10-08-000063_analytics_scope_columns`. The full Diesel versions differ (`2026-11-13-000063` ≠ `2026-10-08-000063`), so both migrations run and `check-migration-versions.sh` passes; recorded for the base branch's owner.

### Deferred to the independent verifier

- `cargo test --all` on this exact head (the acceptance-criterion gate; gyre-server test codegen exceeds the in-sandbox 4 GiB budget, blocked by the build wrapper by design).
- Focused server suites for the merged surfaces: `cargo test -p gyre-server --test git_integration` (prior round's repaired fixtures) and the task-111 session test module (WS auth path, active-session listing, throttle).
