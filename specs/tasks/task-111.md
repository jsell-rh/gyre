---
title: "Implement session management"
spec_ref: "user-management.md §Session Management"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "user-management.md §Session Management"
commits: ["f811501fb395c914da41dc11ba2d9e91d3611e54", "4a71f98e3f43d72732ada3bf3ec08353a81b4a3e", "a4e0eaf8f87de1d3483e9cb2813e6e5434acffeb", "6f20bcb241f6dee4f991553edf35e3ec90c97b3e", "74749334f8f2846a81a9c108c72ebce383d5222f", "5a0816fe602097c22a86d51c7095b644ca1d4893", "0564d4e5b6f95c82bc20dd7cdd64a35ab46fe713", "5fc4fb1a1e470599b94208c7b7b06e1d3d7f3c8e", "30cb6ddd4231defa2c773cd7137ae20d3bdcfdc0", "2e1d71a89f82bd938262b41fd2ecc66be220f482", "66582a887e6caaf605c85ad05a2b8320791079fd", "1b6d9d2ea13cb5687f6cffcb8abd0e656145d581", "f41c1d83fc063d61a99dabbea5bd0ae6f602c52f", "4886ec076fdb44441c5090e66b7cd17389f9a70d"]
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

Session management is implemented end-to-end against `user-management.md` §Session Management (the implementation is the branch's prior reviewed work; this round resolved the rebase onto assigned base `d9c419a0` — task-191 budget enforcement, carrying task-122 team management and task-208 profile supersession):

- **Domain:** `UserSession` in `gyre-domain/src/user_profile.rs` with all spec fields (id, user_id, token_hash, ip_address, user_agent, created_at, last_active_at, expires_at, revoked); only the SHA-256 of the API key is stored (`hash_api_key`), never plaintext. `is_active(now)` = not revoked and not expired.
- **Port:** `SessionRepository` in `gyre-ports/src/user_profile.rs` — create (duplicate-id fails), list_for_user, find_by_id, find_by_token_hash, find_by_credential_and_device, touch (slides TTL), revoke (owner-scoped), revoke_all_for_user, delete_expired_before (never deletes revoked rows — they are the durable sign-out record).
- **Adapters:** SQLite + PostgreSQL + mem implementations on shared migration `2026-10-08-000063_user_sessions`; diesel schema registered; no duplicate migration versions.
- **Auth integration (`auth.rs`):** on successful API-key auth, the session for (credential, IP, User-Agent) is found or minted; `last_active_at` + sliding `expires_at` are refreshed at most once per 60s; a revoked session for the device rejects auth with 401; `credential_revoked` rejects the key when every session for it is revoked ("sign out everywhere", including never-seen devices) — enforced on both the HTTP extractor and the WS/TTY `authenticate_token` paths. IP comes from `ConnectInfo` socket peer only (no forwarded-header trust, CWE-348).
- **API (`api/users.rs` + `api/mod.rs`):** `GET /users/me/sessions` (active only, no token hash exposed), `DELETE /users/me/sessions/:id` (404 on foreign id), `POST /users/me/sessions/revoke-all`, plus TenantAdmin variants with Admin-role **and tenant-containment** per-handler checks (ABAC-exempt routes); target-user existence 404s.
- **Background cleanup:** `spawn_session_cleanup` (daily) deletes only unrevoked sessions expired >30 days.

### Rebase resolution (this round, merge commit `e433eff9` + fixture repair `f811501f`)

Merged assigned base `d9c419a0` into the branch. Three content conflicts, resolved semantically:

- `api/mod.rs` — union of the `users::` handler imports (branch's session handlers + base's `add_team_member`/`remove_team_member`); both sides' route registrations auto-merged without overlap (no duplicate paths).
- `api/spawn.rs` — kept the branch's M34 workspace-scope enforcement (`MergeRequest::new` takes `repo.workspace_id`; repo resolved once and reused for diff stats) inside base's extracted `create_mr_for_completion(&str params)` refactor.
- `specs/coverage/system/user-management.md` — merged audit note; row 10 (task-122, from base) and row 12 (task-111, branch) both `implemented`; coverage counts corrected to the merged table (6/36, 11 n/a, 19 task-assigned, 6 implemented).

Post-merge repairs:

- **Migration renumber:** `user_sessions` `2026-10-08-000060` → `000063` — base added `2026-10-08-000060_task_generation` under the same Diesel version (one migration runs per version; the duplicate would silently drop one). `check-migration-versions.sh` catches this; coverage doc references updated.
- **Exemption re-pins (no count change, frozen at 9):** base inserted code above the pinned legacy scope-literal sites, shifting `sqlite/repository.rs` and `postgres/repository.rs` 94→132 and `task.rs` 99→104. Base's own copy of `check-scope-literal-defaults.sh` predates the no-arg-scan fix and ran vacuously, so main never saw the drift; the branch's fixed script exposed it.
- **`enforcement-mode:ok` markers:** base's `constraint_check.rs` §2.4 context-binding mirror logs "(audit-only)" — the path is documented by its author as a Phase 2 observability mirror with enforcement in `enforce_push_constraints`, so the two log lines carry the check's sanctioned inline marker (no behavior change; the check failed on base itself).
- **Test fixtures (`f811501f`):** base's new tests still called the 6-arg `MergeRequest::new` (caught by `cargo check --all-targets`, which plain `cargo check` skips for `cfg(test)` code): `task134_review_probe.rs` (5 sites), `gate_executor.rs` (3), `merge_processor.rs` (3), `api/merge_requests.rs` (3), `ralph_loop.rs`, `search_indexer.rs`. Each now passes its test's real workspace at construction (`ws-1`/`ws-test`/`ws-probe`/`ws-spec-probe`), replacing post-construction `mr.workspace_id` assignments where they existed. No production code changed.

### Test evidence (at head `f811501f`; logs under `/tmp/stage/review-evidence/`)

In-sandbox (SKIP_WEB_BUILD=1, build wrapper):

- `cargo check -p gyre-server` — clean; `cargo check -p gyre-server --all-targets` — clean (this is what caught the 6-arg fixture calls); `cargo check -p gyre-adapters -p gyre-domain -p gyre-cli --all-targets` — clean; `cargo fmt --all` — no changes.
- `cargo test -p gyre-adapters --lib` — **521 passed, 0 failed** (incl. the 9 `user_profile::session*` tests: roundtrip, duplicate-id, device match, newest-match, TTL slide, owner-scoped revoke, revoke-all scope, delete-expired scope, retention-never-deletes-revoked).
- `cargo test -p gyre-domain` — **507 + 8 passed, 0 failed**; `cargo test -p gyre-common` — **124 passed, 0 failed**.
- Full mechanical check set: every AGENTS.md-enforced gate passes (arch, abac-route-registry, abac-exempt-handlers, mcp-write-tools, migration-versions, migration-sql-portability, mem-port-contracts, dead-message-kinds, byte-slice-truncation, relative-path-defaults, fail-open-ref-resolution, fabricated-scope-defaults, lossy-secret-conversion, scope-literal-defaults, inert-enforcement, forged-scope-fields, forwarded-header-trust, in-memory-state-stores, unbounded-external-http, task-commit-attribution), plus stale-enforcement-mode and no-em-dash. Logs: `/tmp/stage/review-evidence/check-*.log`. (34 non-AGENTS checks fail identically on both merge parents — pre-existing, not introduced by this round.)

Required in the independent verifier (gyre-server test compilation exceeds the in-sandbox 4 GiB budget):

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib` — full suite (previous round: 1724 passed at the prior base merge; includes the 22 session-management tests in `auth.rs` and `api/users.rs`: per-device vs sign-out-everywhere, listing filters, WS rejection paths, 60s throttle, re-mint).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --test task134_review_probe` — base's integration test whose fixtures this round adapted.
