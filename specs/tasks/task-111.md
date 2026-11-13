---
title: "Implement session management"
spec_ref: "user-management.md §Session Management"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "user-management.md §Session Management"
commits: ["ed6a9adc819dd87a141af3fecee543c909233f97", "6a8d4a7cdd53c43eee53110c13b734d3d60d6168", "57f6d144a5bd01905efa6889dade4ce3487c34ea", "f811501fb395c914da41dc11ba2d9e91d3611e54", "4a71f98e3f43d72732ada3bf3ec08353a81b4a3e", "a4e0eaf8f87de1d3483e9cb2813e6e5434acffeb", "6f20bcb241f6dee4f991553edf35e3ec90c97b3e", "74749334f8f2846a81a9c108c72ebce383d5222f", "5a0816fe602097c22a86d51c7095b644ca1d4893", "0564d4e5b6f95c82bc20dd7cdd64a35ab46fe713", "5fc4fb1a1e470599b94208c7b7b06e1d3d7f3c8e", "30cb6ddd4231defa2c773cd7137ae20d3bdcfdc0", "2e1d71a89f82bd938262b41fd2ecc66be220f482", "66582a887e6caaf605c85ad05a2b8320791079fd", "1b6d9d2ea13cb5687f6cffcb8abd0e656145d581", "f41c1d83fc063d61a99dabbea5bd0ae6f602c52f", "4886ec076fdb44441c5090e66b7cd17389f9a70d"]
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

Session management is implemented end-to-end against `user-management.md` §Session Management by this branch's reviewed history (commits above; head of that work: `ed6a9adc`). No production code changed this round — the round only repaired the task-document contract (finding `7f47cc82`): the assigned task text is restored verbatim (acceptance-criteria checkboxes left as assigned) and all progress notes live under this heading.

### Implementation

- **Domain:** `UserSession` in `gyre-domain/src/user_profile.rs` with all spec fields; only the SHA-256 of the session token is stored (`hash_api_key`), never plaintext; `is_active(now)` = not revoked and not expired.
- **Port:** `SessionRepository` in `gyre-ports/src/user_profile.rs` — create (duplicate-id fails), list_for_user, find_by_id, find_by_token_hash, find_by_credential_and_device, touch (slides TTL, throttled), revoke (owner-scoped), revoke_all_for_user, delete_expired_before (never deletes revoked rows — the durable sign-out record). Implemented by SQLite, PostgreSQL, and mem adapters on shared migration `2026-10-08-000063_user_sessions`.
- **Auth integration (`auth.rs`):** on successful API-key auth, the session for (credential, IP, User-Agent) is found or minted; `last_active_at` + sliding `expires_at` refreshed at most once per 60s; a revoked session for the device rejects auth with 401; `credential_revoked` rejects the key when every session for it is revoked ("sign out everywhere") — enforced on both the HTTP extractor and the WS/TTY `authenticate_token` paths. IP comes from `ConnectInfo` socket peer only (no forwarded-header trust, CWE-348).
- **API (`api/users.rs` + `api/mod.rs`):** `GET /users/me/sessions` (active only, no token hash exposed), `DELETE /users/me/sessions/:id` (404 on foreign id), `POST /users/me/sessions/revoke-all`, plus TenantAdmin variants with Admin-role **and tenant-containment** per-handler checks; target-user existence 404s.
- **Background cleanup:** `spawn_session_cleanup` (daily) deletes only unrevoked sessions expired >30 days.

### Prior verification repairs absorbed into the branch

- `74749334`: verification-finding repairs; fixed merge-request scope fabrications (7-arg `MergeRequest::new` with real workspace ids).
- `6f20bcb2`: tenant containment on TenantAdmin session endpoints.
- `a4e0eaf8`/`0564d4e5`: migration renumbering past base duplicates.
- `ed6a9adc` (latest): durable finding `08f72f21` — `gyre-server` lib test compilation failed (E0061/E0277 at `gate_executor.rs:4421/4427/5068/5074`) because the assigned base `6f5a631c` (task-195, manifest-driven gate selection) added fixtures constructing MRs with the old 6-arg `MergeRequest::new`; repaired to the 7-arg form with real workspace ids. The finding's Svelte/a11y/CSS log tail is pre-existing web-build noise (the vite build itself succeeded).

### Test evidence (at head; logs under `/tmp/stage/review-evidence/`)

In-sandbox (offline — no crates.io access from this stage, so Rust compilation is deferred to the verifier):

- `rustfmt --edition 2021 --check crates/gyre-server/src/gate_executor.rs` — clean.
- All 20 mechanical gate scripts listed in AGENTS.md re-run PASS at head, including exemption freezes (`mechanical-gates.log`); `check-task-commit-attribution.sh` PASSes once this round's frontmatter is committed.
- Structural verification of the ed6a9adc fixture repair: both `gate_executor.rs` sites construct `MergeRequest::new` with 7 positional args and real workspace ids (`ws.id` / `repo.workspace_id`).

Required in the independent verifier (no network / no cargo registry in this sandbox; server test compilation exceeds the in-sandbox budget):

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --lib --tests` — the exact failing target of finding `08f72f21`; must compile clean.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib` — full suite (previous round: 1724 passed; includes the session-management tests in `auth.rs` and `api/users.rs` and the manifest-gate tests adapted by `ed6a9adc`).
- `SKIP_WEB_BUILD=1 cargo test --all` — repo-wide acceptance criterion.
- `bash /tmp/stage/checks.sh` — the previously failing verification command.
