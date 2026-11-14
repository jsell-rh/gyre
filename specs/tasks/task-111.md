---
title: "Implement session management"
spec_ref: "user-management.md §Session Management"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "user-management.md §Session Management"
commits: ["c947d22f1aa9b66f30b26b913aeda2e6c506fb94", "bdf1855d12d8837e6142b7ab060b09ac3cb63e55", "fd4351b9988673828b61a2b1379057db07548018", "97ef01dd7c12d9f49f03f49ce64829916871df79", "db116ab7498d7ef385b229a9761d487bee10297e", "ed6a9adc819dd87a141af3fecee543c909233f97", "6a8d4a7cdd53c43eee53110c13b734d3d60d6168", "57f6d144a5bd01905efa6889dade4ce3487c34ea", "f811501fb395c914da41dc11ba2d9e91d3611e54", "4a71f98e3f43d72732ada3bf3ec08353a81b4a3e", "a4e0eaf8f87de1d3483e9cb2813e6e5434acffeb", "6f20bcb241f6dee4f991553edf35e3ec90c97b3e", "74749334f8f2846a81a9c108c72ebce383d5222f", "5a0816fe602097c22a86d51c7095b644ca1d4893", "0564d4e5b6f95c82bc20dd7cdd64a35ab46fe713", "5fc4fb1a1e470599b94208c7b7b06e1d3d7f3c8e", "30cb6ddd4231defa2c773cd7137ae20d3bdcfdc0", "2e1d71a89f82bd938262b41fd2ecc66be220f482", "66582a887e6caaf605c85ad05a2b8320791079fd", "1b6d9d2ea13cb5687f6cffcb8abd0e656145d581", "f41c1d83fc063d61a99dabbea5bd0ae6f602c52f", "4886ec076fdb44441c5090e66b7cd17389f9a70d"]
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

Session management per `user-management.md` §Session Management is implemented end-to-end by this branch's reviewed history. This round merges the new assignment base `c844ea52` (task-129 ABAC evaluation engine, API, and audit integration) and repairs the compile fallout that merge introduced in three base test fixtures. No session-management behavior changed this round.

### Implementation (unchanged this round)

- **Domain:** `UserSession` in `gyre-domain/src/user_profile.rs` with all spec fields; only the SHA-256 of the session token is stored, never plaintext.
- **Port + adapters:** `SessionRepository` (create, list, find, touch, revoke, revoke_all, delete_expired) implemented by SQLite, PostgreSQL, and mem adapters.
- **Auth integration:** sessions minted on API-key auth keyed by (credential, IP, User-Agent); `last_active_at` + sliding expiry refreshed at most once per 60s; revoked sessions reject auth on both HTTP and WS paths; IP from socket peer only.
- **API:** `GET/DELETE /users/me/sessions[...]`, `POST /users/me/sessions/revoke-all`, plus TenantAdmin variants with tenant-containment checks.
- **Cleanup:** daily `spawn_session_cleanup` deletes unrevoked sessions expired >30 days.

### This round's repair (`c947d22f`, on merge `0c135ca9` of base `c844ea52`)

The base `c844ea52` and this branch both descend from `32c4faef`; git auto-merged them without textual conflicts, but the base's three merge-processor decision-recording tests (`trust-audit`, `att-audit`, `fail-closed`) construct `MergeRequest` with the pre-M34 6-arg signature and patch `workspace_id` after the fact, while this branch's `MergeRequest::new` takes the workspace `Id` as its third argument — the same failure class repaired in `6a8d4a7c`/`ed6a9adc`/`f811501f`. All three fixtures now inline their workspace `Id` at the construction site (identical final state to the removed post-construction assignment); the trust-audit fixture's now-unneeded `mut` was dropped. No product code changed. The merged `test_state()` keeps both sides' wiring: the base's seeded builtin ABAC policies (`shared_workspace_policy_pair_seeded`) and this branch's `MemSessionRepository` session store; session routes remain ABAC-exempt with per-handler enforcement, and `check-abac-route-registry`/`check-abac-exempt-handlers` pass on the merged tree.

### Test evidence

In-sandbox this round (log: `/tmp/stage/review-evidence/task-111-c844ea52-merge-round.log`):

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --all-targets` — clean (the merge plus the three fixture repairs type-check, including the base's new `policy_cache`, `evaluate_push_attestation_abac`, and reserved-system-identity call sites).
- `cargo fmt --check` — clean.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-domain -p gyre-common -p gyre-ports` — 643 passed, 0 failed.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters` — 534 passed, 0 failed (includes the SQLite `SessionRepository` suite).
- All mechanical invariant gates pass on the merged tree: architecture/hierarchy lint, ABAC route registry + exempt-handler parity, MCP write-tool gating, migration versions and SQL portability, dead message kinds, byte-slice truncation, relative path defaults, fail-open ref resolution, mem port contracts, fabricated scope defaults (lookup and construction), lossy secret conversion, inert enforcement, forged scope fields, forwarded-header trust, in-memory state stores, unbounded external HTTP (logs in `/tmp/stage/review-evidence/check-*.log`).

Deferred to the independent verifier (gyre-server test codegen exceeds the in-sandbox 4 GiB budget; the wrapper blocks it by design):

- `cargo test --all` — expected: the three repaired `merge_processor` tests compile and pass; the full suite last ran green on this branch's tree at base `32c4faef` (finding `4fb29ee2`: 2181 passed, only the 4 `merge_requests` failures since repaired in `bdf1855d`), and the only compile-level interaction with the new base was the constructor drift fixed in `c947d22f`.
