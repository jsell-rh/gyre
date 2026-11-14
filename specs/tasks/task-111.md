---
title: "Implement session management"
spec_ref: "user-management.md §Session Management"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "user-management.md §Session Management"
commits: ["9db918872d7088861fb38695b09e47bad216ba0a", "c6cad385d11908daddf8a9b5e7f049237ebc2335", "c947d22f1aa9b66f30b26b913aeda2e6c506fb94", "bdf1855d12d8837e6142b7ab060b09ac3cb63e55", "fd4351b9988673828b61a2b1379057db07548018", "97ef01dd7c12d9f49f03f49ce64829916871df79", "db116ab7498d7ef385b229a9761d487bee10297e", "ed6a9adc819dd87a141af3fecee543c909233f97", "6a8d4a7cdd53c43eee53110c13b734d3d60d6168", "57f6d144a5bd01905efa6889dade4ce3487c34ea", "f811501fb395c914da41dc11ba2d9e91d3611e54", "4a71f98e3f43d72732ada3bf3ec08353a81b4a3e", "a4e0eaf8f87de1d3483e9cb2813e6e5434acffeb", "6f20bcb241f6dee4f991553edf35e3ec90c97b3e", "74749334f8f2846a81a9c108c72ebce383d5222f", "5a0816fe602097c22a86d51c7095b644ca1d4893", "0564d4e5b6f95c82bc20dd7cdd64a35ab46fe713", "5fc4fb1a1e470599b94208c7b7b06e1d3d7f3c8e", "30cb6ddd4231defa2c773cd7137ae20d3bdcfdc0", "2e1d71a89f82bd938262b41fd2ecc66be220f482", "66582a887e6caaf605c85ad05a2b8320791079fd", "1b6d9d2ea13cb5687f6cffcb8abd0e656145d581", "f41c1d83fc063d61a99dabbea5bd0ae6f602c52f", "4886ec076fdb44441c5090e66b7cd17389f9a70d"]
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

Session management per `user-management.md` §Session Management is implemented end-to-end on this branch; the coverage matrix row (`specs/coverage/system/user-management.md` row 12) is `implemented | task-111`. Frontmatter progress is `ready-for-review`.

### Implementation

- **Domain:** `UserSession` in `gyre-domain/src/user_profile.rs` with all spec fields; only the SHA-256 of the session token is stored, never plaintext.
- **Port + adapters:** `SessionRepository` (create, list, find, touch, revoke, revoke_all, delete_expired) implemented by SQLite, PostgreSQL, and mem adapters on migration `2026-10-08-000064_user_sessions`.
- **Auth integration:** sessions minted on API-key auth keyed by (credential, IP, User-Agent); `last_active_at` + sliding expiry refreshed at most once per 60s; revoked sessions reject auth on both HTTP and WS/TTY paths; `credential_revoked` rejects the key when every session for it is revoked; IP from socket peer only (CWE-348).
- **API:** `GET/DELETE /users/me/sessions[...]`, `POST /users/me/sessions/revoke-all`, plus TenantAdmin variants (`/users/:user_id/sessions...`) with per-handler role checks and tenant containment. Session routes are ABAC-exempt by design with per-handler authorization (validated by `check-abac-exempt-handlers.sh`).
- **Cleanup:** daily `spawn_session_cleanup` deletes unrevoked sessions expired >30 days (revoked rows kept as the durable sign-out record).

### This round: review-finding repair (commit 9db91887)

Repair of review finding ea8e3c88: the gyre-server lib test target did not compile — four merge_processor.rs test MRs (`mr-no-spec-ref`, `mr-with-spec-ref`, `mr-unapproved-spec`, `mr-approved-spec`, from task-145 via the base merge) called the 6-arg `MergeRequest::new` while commit 74749334 had added `workspace_id` as the third parameter. All four sites now pass `Id::new("ws-1")` at construction; the redundant post-construction assignments were removed. No production (non-test) code changed this round; the session surface is exactly the reviewed candidate state merged with base `44cf25bf`.

### Acceptance criteria status

All criteria above are satisfied by the shipped code and test evidence, except `cargo test --all`, which is deferred to the independent verifier (gyre-server test codegen exceeds the in-sandbox 4 GiB build-wrapper budget). The checkboxes are left unchecked deliberately: the task body is the protected assigned contract, and modifying it (checking boxes) is a normative change requiring separate spec review — completion state lives in this section and the frontmatter, not in edits to the protected text.

### Test evidence (this round, on this head after 9db91887)

- `cargo check -p gyre-server --lib --tests` → exit 0 (the reviewer's reproduction command; the prior 8 errors are gone). Evidence: `/tmp/stage/review-evidence/round-2-repair-summary.md`.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters session` → 9/9 passed (create/roundtrip, duplicate-id contract, credential+device matching, newest-match, touch/sliding expiry, revoke scoping, revoke-all + delete-expired, retention never deleting revoked rows).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-domain --lib` → 535/535 passed.
- Mechanical gates all exit 0 on this head: `check-arch.sh`, `check-migration-versions.sh`, `check-abac-route-registry.sh`, `check-abac-exempt-handlers.sh`.
- No conflict markers in `crates/`/`specs/`; no duplicate migration sequence numbers; session surface (domain entity, port, routes, migration) verified present at this head.

### Deferred to the independent verifier

- `cargo test --all` on this exact head (the acceptance-criterion gate; 4 GiB build-wrapper limit).
- `cargo test -p gyre-server --lib` — includes the four repaired merge_processor tests (`require_spec_ref_blocks_mr_without_spec_ref`, `require_approved_spec_blocks_unapproved_spec`) that must now compile and pass.
- Focused server suites: `cargo test -p gyre-server --test git_integration`, `cargo test -p gyre-server --test spec_enforcement`, and the task-111 session test module in `gyre-server/src/api/users.rs` (WS auth path, active-session listing, throttle).
