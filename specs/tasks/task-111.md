---
title: "Implement session management"
spec_ref: "user-management.md §Session Management"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "user-management.md §Session Management"
commits: ["4a71f98e3f43d72732ada3bf3ec08353a81b4a3e", "a4e0eaf8f87de1d3483e9cb2813e6e5434acffeb", "6f20bcb241f6dee4f991553edf35e3ec90c97b3e", "74749334f8f2846a81a9c108c72ebce383d5222f", "5a0816fe602097c22a86d51c7095b644ca1d4893", "0564d4e5b6f95c82bc20dd7cdd64a35ab46fe713", "5fc4fb1a1e470599b94208c7b7b06e1d3d7f3c8e", "30cb6ddd4231defa2c773cd7137ae20d3bdcfdc0", "2e1d71a89f82bd938262b41fd2ecc66be220f482", "66582a887e6caaf605c85ad05a2b8320791079fd", "1b6d9d2ea13cb5687f6cffcb8abd0e656145d581", "f41c1d83fc063d61a99dabbea5bd0ae6f602c52f", "4886ec076fdb44441c5090e66b7cd17389f9a70d"]
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

Session management is implemented end-to-end against `user-management.md` §Session Management (implementation is the branch's prior reviewed work; this round resolved the rebase onto base `68d51f87e` — merge `4aec81757` + rustfmt follow-up `4a71f98e3`):

- **Domain:** `UserSession` in `gyre-domain/src/user_profile.rs` with all spec fields (id, user_id, token_hash, ip_address, user_agent, created_at, last_active_at, expires_at, revoked); only the SHA-256 of the API key is stored (`hash_api_key`), never plaintext. `is_active(now)` = not revoked and not expired.
- **Port:** `SessionRepository` in `gyre-ports/src/user_profile.rs` — create (duplicate-id fails), list_for_user, find_by_id, find_by_token_hash, find_by_credential_and_device, touch (slides TTL), revoke (owner-scoped), revoke_all_for_user, delete_expired_before (never deletes revoked rows — they are the durable sign-out record).
- **Adapters:** SQLite + PostgreSQL + mem implementations against shared migration `2026-10-08-000060_user_sessions`; diesel schema registered; no duplicate migration versions.
- **Auth integration (`auth.rs`):** on successful API-key auth, the session for (credential, IP, User-Agent) is found or minted; `last_active_at` + sliding `expires_at` are refreshed at most once per 60s; a revoked session for the device rejects auth with 401; `credential_revoked` rejects the key when every session for it is revoked ("sign out everywhere", including never-seen devices) — enforced on both the HTTP extractor and the WS/TTY `authenticate_token` paths. IP comes from `ConnectInfo` socket peer only (no forwarded-header trust, CWE-348).
- **API (`api/users.rs` + `api/mod.rs`):** `GET /users/me/sessions` (active only, no token hash exposed), `DELETE /users/me/sessions/:id` (404 on foreign id), `POST /users/me/sessions/revoke-all`, plus TenantAdmin variants `GET|DELETE|POST /users/:user_id/sessions[...]` with Admin-role **and tenant-containment** per-handler checks (ABAC-exempt routes): `ensure_same_tenant_session_target` rejects cross-tenant targets (fail-closed on a target with no tenant binding). Target-user existence 404s.
- **Background cleanup:** `spawn_session_cleanup` (daily) deletes only unrevoked sessions expired >30 days.

### Rebase resolution (this round, commits `4aec81757` + `4a71f98e3`)

Rebased the branch onto base `68d51f87e` (task-115 spec approval signal chain & orchestrators). One content conflict, in `crates/gyre-server/src/main.rs`'s import list only: HEAD carried `spawn_session_cleanup` (this task's daily session retention job) while the base added `signal_chain` (task-115's task scheduler). Resolved by keeping both — the auto-merged `main()` body spawns both `signal_chain::spawn_task_scheduler` and `spawn_session_cleanup` — then ran rustfmt over the merged import list (`4a71f98e3`). No other file conflicted; all task-115 content (signal_chain module, `2026-10-10-000057_task_spec_ref` migration, orchestrator registry wiring in `lib.rs`/`mem.rs`/`middleware.rs`) came in clean and is untouched.

### Verification repair (prior round, commit `a4e0eaf8f`)

The verification stage (`checks.sh`, exit 1) failed on `check-migration-versions.sh`: after merging base `3d87a1507`, two migrations mapped to Diesel version `20261008000059` — the base's task-109 `2026-10-08-000059_spec_lifecycle_configs` and this branch's `2026-10-08-000059_user_sessions`. Diesel runs only one migration per version (alphabetically first), so the `user_sessions` DDL was silently dropped on fresh databases. Fixed by renumbering the branch's migration directory to `2026-10-08-000060_user_sessions` (next unused sequence; `up.sql` is idempotent `CREATE TABLE IF NOT EXISTS`, so databases that already applied either 000059 migration are unaffected); spec/coverage doc references updated to match. Both `check-migration-versions.sh` and `check-migration-sql-portability.sh` now pass.

### Merge resolution (prior round, merge `5ed14345a` onto base `3d87a1507`) — superseded by this round's rebase

Merged base `3d87a1507` (task-109 spec lifecycle configs, task-208 "My Stuff" amendment, task-104 audit events, R19 jj-export on complete) into the branch; four content conflicts, all resolved keeping both sides' real fixes:

- `api/mod.rs`: kept the session imports/routes; took the base's removal of `get_my_agents`/`get_my_mrs`/`get_my_tasks` (spec §"My Stuff" amendment strikes those endpoints).
- `api/spawn.rs`: took the base's R19 jj bookmark-set + git-export before MR stat computation; kept the branch's earlier-resolved `repo` (M34 workspace enforcement) so stats use it instead of re-looking-up the repository.
- `auth.rs` (both the HTTP extractor and WS `authenticate_token` API-key paths): kept session enforcement (`credential_revoked` + `track_session` reject first, with an `AuthFailure` audit event on rejection) and combined it with the base's task-104 gated `record_auth_success` (`auth_already_audited` dedupe) on the success path.
- `specs/coverage/system/user-management.md`: merged header audit — 11 n/a (task-208's row-22 reclassification), 21 task-assigned, 4 implemented (task-120's rows 2/3/11 + task-111's row 12).
- Merge-fallout repairs in auto-merged files: 5 base-added `merge_processor.rs` test call sites updated to the 7-arg `MergeRequest::new` (workspace `ws-1` hoisted into the constructor, value unchanged); `rbac.rs` `uninstrumented_role_guards_record_rbac_denied_audit_event` now seeds the `repo-1` repository its create_mr probe requires (create_mr is fail-closed on unknown repositories since the branch's scope-fabrication repair).

### Test evidence (at head `4a71f98e3`, post-rebase)

All runs with `SKIP_WEB_BUILD=1`; this round's logs under `/tmp/stage/review-evidence/task-111-rebase-*.log`:

- `cargo check -p gyre-server --all-targets` — clean (verifies the merged tree, incl. task-115's signal_chain, compiles with session management).
- `cargo test -p gyre-server --lib session` — **21 passed, 0 failed**; plus `-- signout_everywhere revoke_all_blocks tenant_admin_revoke_all ws_auth_rejects signal_chain` — **24 passed, 0 failed** (all session killing tests, plus the base's signal_chain suite confirming the rebase resolution). Session killing tests: `api/users.rs` — `sessions_endpoint_lists_session_created_by_auth`, `revoke_session_then_credential_is_rejected`, `revoking_one_device_session_does_not_sign_out_other_devices`, `revoke_all_sessions_signs_out_everywhere`, `revoke_all_blocks_credential_from_never_seen_device`, `my_session_listing_excludes_revoked_and_expired_sessions`, `admin_session_listing_excludes_revoked_and_expired_sessions`, `revoke_cannot_target_another_users_session`, `tenant_admin_can_view_and_revoke_another_users_sessions`, `non_admin_cannot_view_or_revoke_another_users_sessions`, `tenant_admin_revoke_all_signs_user_out_everywhere`, `admin_session_endpoints_unknown_user_is_404`, `cross_tenant_admin_cannot_view_or_revoke_another_tenants_user_sessions`; `auth.rs` — `session_last_active_touch_is_throttled`, `expired_session_is_reminted_not_zombified`, `signout_everywhere_survives_retention_cleanup`, `ws_auth_rejects_signed_out_everywhere_credential`, `ws_auth_rejects_revoked_ws_device_session`.
- `cargo test -p gyre-adapters --lib session` — **9 passed, 0 failed** (sqlite `user_profile::tests::session*`: roundtrip, duplicate-id, device match, newest-match, TTL slide, owner-scoped revoke, revoke-all scope, delete-expired scope, retention-never-deletes-revoked; fresh SQLite DB per test runs all pending embedded migrations, so both `user_sessions` and the base's `task_spec_ref` migration are exercised on fresh databases).
- `cargo fmt --all --check` clean; `check-task-commit-attribution.sh` OK at the post-commit head.
- Mechanical checks pass at the post-rebase head (logs under `/tmp/stage/review-evidence/task-111-rebase-check-*.log`): `check-migration-versions.sh`, `check-migration-sql-portability.sh`, `check-arch.sh`, `check-abac-route-registry.sh`, `check-mcp-write-tools.sh`, `check-mem-port-contracts.sh`.
- Prior rounds additionally ran the full suites green: `cargo test -p gyre-adapters --lib` — 421 passed; `cargo test -p gyre-server --lib` — 1619 passed; `cargo test -p gyre-domain` — 443 passed; clippy changed-lines clean; the full mechanical check set (scope-literal-defaults, abac-exempt-handlers, fabricated-scope-defaults, forged-scope-fields, forwarded-header-trust, inert-enforcement, lossy-secret-conversion, fail-open-ref-resolution, in-memory-state-stores, dead-message-kinds, byte-slice-truncation, relative-path-defaults, unbounded-external-http).

### Prior review repairs (retained from earlier attempts, unchanged this round)

- **TenantAdmin tenant containment** (`6f20bcb24`): `ensure_same_tenant_session_target` closes the cross-tenant admin hole; killing test `cross_tenant_admin_cannot_view_or_revoke_another_tenants_user_sessions`.
- **Verification findings** (`74749334f`): rustfmt, clippy borrows, and the merge-request scope-fabrication fixes (`MergeRequest::new` takes `workspace_id`; adapters persist under the handle's tenant scope) — the source of the 7-arg constructor and fail-closed `create_mr` that this merge's fallout repairs build on.
- **Killing tests for WS path and listing filters** (`5a0816fe6`).
