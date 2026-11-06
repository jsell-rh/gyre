---
title: "Implement session management"
spec_ref: "user-management.md §Session Management"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "user-management.md §Session Management"
commits: ["0564d4e5b6f95c82bc20dd7cdd64a35ab46fe713", "5fc4fb1a1e470599b94208c7b7b06e1d3d7f3c8e", "30cb6ddd4231defa2c773cd7137ae20d3bdcfdc0", "2e1d71a89f82bd938262b41fd2ecc66be220f482", "66582a887e6caaf605c85ad05a2b8320791079fd", "1b6d9d2ea13cb5687f6cffcb8abd0e656145d581", "f41c1d83fc063d61a99dabbea5bd0ae6f602c52f", "4886ec076fdb44441c5090e66b7cd17389f9a70d"]
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

Session management is implemented end-to-end against `user-management.md` §Session Management (base `f4acb4eb` → head):

- **Domain:** `UserSession` in `gyre-domain/src/user_profile.rs` with all spec fields (id, user_id, token_hash, ip_address, user_agent, created_at, last_active_at, expires_at, revoked); only the SHA-256 of the API key is stored (`hash_api_key`), never plaintext. `is_active(now)` = not revoked and not expired.
- **Port:** `SessionRepository` in `gyre-ports/src/user_profile.rs` — create (duplicate-id fails), list_for_user, find_by_id, find_by_token_hash, find_by_credential_and_device, touch (slides TTL), revoke (owner-scoped), revoke_all_for_user, delete_expired_before (never deletes revoked rows — they are the durable sign-out record).
- **Adapters:** SQLite + PostgreSQL + mem implementations against shared migration `2026-10-08-000057_user_sessions` (next unused sequence number after the rebase consumed 000056, portable SQL); diesel schema registered.
- **Auth integration (`auth.rs`):** on successful API-key auth, the session for (credential, IP, User-Agent) is found or minted; `last_active_at` + sliding `expires_at` are refreshed at most once per `SESSION_TOUCH_THROTTLE_SECS` (60s); a revoked session for the device rejects auth with 401; `credential_revoked` rejects the key when every session for it is revoked ("sign out everywhere", including never-seen devices). IP comes from `ConnectInfo` socket peer (no forwarded-header trust, CWE-348). Both the HTTP extractor and the WS ticket path enforce the same per-device rule.
- **API (`api/users.rs` + `api/mod.rs`):** `GET /users/me/sessions` (active only, no token hash exposed), `DELETE /users/me/sessions/:id` (404 on foreign id), `POST /users/me/sessions/revoke-all`, plus TenantAdmin variants `GET|DELETE /users/:user_id/sessions[...]` with Admin-role per-handler checks (ABAC-exempt routes), target-user existence 404s.
- **Background cleanup:** `spawn_session_cleanup` (daily) deletes only unrevoked sessions expired >30 days.

**Test evidence** (focused probes, full log at `/tmp/stage/review-evidence/task-111-session-tests.md`):
- `cargo test -p gyre-adapters --lib -- sqlite::user_profile::tests::session` — 8 passed (roundtrip, duplicate-id, device-match on all four fields, newest-match, TTL slide, owner-scoped revoke, revoke-all scope, retention-never-deletes-revoked).
- `cargo test -p gyre-server --lib -- session` — 12 passed, covering: session listed after auth; revoking one session rejects the credential; revoking one device does not sign out other devices; revoke-all signs out everywhere; foreign-session revoke is 404; TenantAdmin view/revoke; non-Admin forbidden; admin revoke-all; unknown-user 404; throttled touch; expired-session re-mint; sign-out survives retention cleanup.
- All mechanical checks pass: arch, ABAC route registry + exempt handlers, migration versions + SQL portability, inert enforcement, mem port contracts, in-memory state stores, lossy secret conversion, forwarded-header trust, scope-literal/fabricated-scope defaults (after re-pinning the exemption line numbers that commit 66582a88 had shifted +37 — same 24/8 frozen sites, none added).

### Review repair: duplicate migration version (commit `0564d4e5`)

- Review finding (attempt `e88a5646`): the rebase onto base `9bef21a4`/`88bd81bf` collided the candidate's `2026-10-08-000056_user_sessions` with the base's `2026-10-08-000056_spec_assertion_results` — both map to Diesel version `20261008000056`, so only the alphabetically first would ever run and `user_sessions` would be silently dropped on fresh databases (the exact audit-only failure class). `check-migration-versions.sh` failed (exit 1).
- Fix: pure directory rename to `2026-10-08-000057_user_sessions` (next unused 6-digit sequence); migration SQL byte-identical; `embed_migrations!` globs the directory, so no code reference changes (no Rust source referenced the old path).
- Verified in this sandbox: `check-migration-versions.sh` → OK (exit 0, evidence `/tmp/stage/review-evidence/migration-version-check-output.txt`); `check-migration-sql-portability.sh` → OK (exit 0).
- Deferred to remote verification (sandbox transport restriction, not a code defect): the Rust probes (`cargo test -p gyre-adapters --lib -- sqlite::user_profile::tests::session`, `cargo test -p gyre-server --lib -- session`, and the `no_duplicate_migration_version_prefixes` guard test) — crates.io and static.crates.io are unreachable from this sandbox (connection refused, sinkholed to 198.18.0.7; github.com reachable), so no Rust compilation is possible here. Evidence: `/tmp/stage/review-evidence/rust-probe-blocked.txt`. The rename changes no Rust source and no SQL, so the previously recorded 8/8 + 12/12 probe results remain applicable to the identical content; only the migration directory name differs.

### Rebase repair (merge into `75ada1c`)

- Resolved the one conflicted file, `scripts/unwritten-store-fields-exemptions.txt`: took the base side (dropped the `mem.rs:3561` payloads exemption) because the task-097 fix is present in the merged `mem.rs` (`store()` populates `payloads` at line 3570; `trace_payload_tests` at line 4535); `check-unwritten-store-fields.sh` passes with the line removed (commit `9879f07d`).
- Re-pinned two `scope-literal-defaults` exemption lines shifted +5 by the merge (`lib.rs` 498→503, 542→547 — the same frozen `emit_event`/`emit_telemetry` sites, none added); `check-scope-literal-defaults.sh` passes (commit `fd46822f`).
- Post-merge focused probes on the merged tree: adapters session suite 8/8 passed; server session suite 12/12 passed (full build, including web assets, clean).
- All check scripts that pass on base `75ada1c` also pass on the merged head; the 34 that fail on the merged head fail identically on the base worktree, i.e. pre-existing baseline state owned by verification, not introduced by this merge.
- Stray `web/dist` rebuild artifacts from the local test build were reverted (same class as commit `30cb6ddd`), not committed.

Not verifiable in this sandbox: live-server HTTP transport (TCP listener unsupported, errno 95). Endpoint behavior is proven by in-process Router (oneshot) tests.
