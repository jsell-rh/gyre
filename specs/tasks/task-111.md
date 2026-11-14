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

Session management per `user-management.md` §Session Management is implemented end-to-end by this branch's reviewed history; the coverage matrix row (`specs/coverage/system/user-management.md` row 12) is `implemented | task-111`. This round (repair `5c7b2879`) and the two prior rounds were contract-restore repairs over an unchanged tree; no product behavior changed.

### Implementation

- **Domain:** `UserSession` in `gyre-domain/src/user_profile.rs` with all spec fields; only the SHA-256 of the session token is stored, never plaintext.
- **Port + adapters:** `SessionRepository` (create, list, find, touch, revoke, revoke_all, delete_expired) implemented by SQLite, PostgreSQL, and mem adapters on migration `2026-10-08-000064_user_sessions`.
- **Auth integration:** sessions minted on API-key auth keyed by (credential, IP, User-Agent); `last_active_at` + sliding expiry refreshed at most once per 60s; revoked sessions reject auth on both HTTP and WS/TTY paths; `credential_revoked` rejects the key when every session for it is revoked; IP from socket peer only (CWE-348).
- **API:** `GET/DELETE /users/me/sessions[...]`, `POST /users/me/sessions/revoke-all`, plus TenantAdmin variants (`/users/:user_id/sessions...`) with per-handler role checks and tenant containment. Session routes are ABAC-exempt by design with per-handler authorization (validated by `check-abac-exempt-handlers.sh`).
- **Cleanup:** daily `spawn_session_cleanup` deletes unrevoked sessions expired >30 days (revoked rows kept as the durable sign-out record).

### This round: contract restore (repair `5c7b2879`)

- The executor's checkout again restored the pristine assigned task document (progress reset to `not-started`, `commits:` emptied, checkboxes unchecked, `## Shipped` dropped) — the same contract-reset shape as the prior rounds' repairs `2d09a4ab` and `34e4e281`. The contract body is byte-identical to the checkpoint version, so the checkpoint metadata (progress `ready-for-review`, the 23-commit attribution list, checked criteria, these notes) was restored over it unchanged. This also keeps `check-task-commit-attribution.sh` passing — the gate fails while the restored `commits: []` hides the branch's task-labeled commits from review scoping.
- **No source change:** `git diff 3ba9bba5..HEAD -- . ':!specs/tasks/task-111.md'` is empty — every commit after base merge `3ba9bba5` touches only this task document. All session surfaces were re-verified present by direct inspection this round at head `0f6cb439`: `UserSession` entity, `SessionRepository` port + SQLite/PostgreSQL/mem impls, migration `2026-10-08-000064_user_sessions`, auth-path session tracking, all six session routes with their ABAC-exempt mappings and per-handler authorization, and `spawn_session_cleanup`. Probe evidence: `/tmp/stage/review-evidence/task-111-repair-contract-restore.md`.

Prior-round context, preserved: the executor merged base `3f7871b6` (task-148's `gyre_analytics_decide` MCP tool) into this branch as `3ba9bba5` — `git diff c0988beb..3ba9bba5` touched only `api/analytics.rs`, `mcp.rs`, docs, and task-148's spec files, with no conflict markers. Earlier repair history (base-merge conflict resolution `44049f85` against the task-124 base, fixture repairs, migration renumbering) is preserved in the branch history and the commits list below; it is not re-diagnosed here.

### Test evidence

This round (no source change; direct inspection only — full gates are not repeated inside a repair round):

- `git diff --stat 3ba9bba5..HEAD -- . ':!specs/tasks/task-111.md'` — empty; no session surface differs from the previously verified tree.
- Source-surface presence probes at head `0f6cb439` (entity, port + SQLite/PostgreSQL/mem impls, migration, routes, ABAC-exempt mappings, cleanup spawn) — all present; log: `/tmp/stage/review-evidence/task-111-repair-contract-restore.md`.
- Differential probe of the gate this repair fixes: `bash scripts/check-task-commit-attribution.sh` — exit 0 with the restored `commits:` frontmatter; exit 1 when the frontmatter is reset to `commits: []` (the executor's restored state, 23 unlisted commits), proving the restore is load-bearing, not cosmetic. Logs: `/tmp/stage/review-evidence/task-111-attribution-checkpoint.log` (pristine, exit 1) and `/tmp/stage/review-evidence/task-111-attribution-pristine-vs-checkpoint.log` (checkpoint, exit 0).

Prior-round evidence on the identical source tree (head `3ba9bba5`, the only later commits are task-doc restores):

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` — exit 0 (6m21s, includes domain/ports/adapters; validates the task-148 base merge against the session surfaces).
- `cargo test -p gyre-adapters session` — 9/9 passed (create/roundtrip, duplicate-id contract, credential+device matching, newest-match, touch/sliding expiry, revoke scoping, revoke-all + delete-expired, retention never deleting revoked rows).
- Mechanical gates on the merged tree: `check-arch.sh`, `check-abac-route-registry.sh`, `check-abac-exempt-handlers.sh`, `check-migration-versions.sh`, `check-mem-port-contracts.sh` — all exit 0; `check-task-commit-attribution.sh` exits 0 with the restored commits frontmatter (it fails against the executor's `commits: []` restore, as expected).
- Earlier round (head `44049f85`, unchanged surfaces): `cargo test -p gyre-domain --lib` 535/535, `cargo fmt --check` exit 0, `check-forwarded-header-trust.sh` / `check-forged-scope-fields.sh` / `check-in-memory-state-stores.sh` exit 0.

### Deferred to the independent verifier

- `cargo test --all` on this exact head (the acceptance-criterion gate; gyre-server test codegen exceeds the in-sandbox 4 GiB budget, blocked by the build wrapper by design).
- Focused server suites for the merged surfaces: `cargo test -p gyre-server --test git_integration` (prior round's repaired fixtures) and the task-111 session test module (WS auth path, active-session listing, throttle).
