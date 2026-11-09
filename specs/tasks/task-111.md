---
title: "Implement session management"
spec_ref: "user-management.md §Session Management"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "user-management.md §Session Management"
commits: ["6f20bcb241f6dee4f991553edf35e3ec90c97b3e", "74749334f8f2846a81a9c108c72ebce383d5222f", "5a0816fe602097c22a86d51c7095b644ca1d4893", "0564d4e5b6f95c82bc20dd7cdd64a35ab46fe713", "5fc4fb1a1e470599b94208c7b7b06e1d3d7f3c8e", "30cb6ddd4231defa2c773cd7137ae20d3bdcfdc0", "2e1d71a89f82bd938262b41fd2ecc66be220f482", "66582a887e6caaf605c85ad05a2b8320791079fd", "1b6d9d2ea13cb5687f6cffcb8abd0e656145d581", "f41c1d83fc063d61a99dabbea5bd0ae6f602c52f", "4886ec076fdb44441c5090e66b7cd17389f9a70d"]
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

Session management is implemented end-to-end against `user-management.md` §Session Management (base `f4acb4eb`, merged up to base `fb1f82e31` in `dedbc7c9a`; TenantAdmin tenant-containment review-repair F1 in `6f20bcb24`; earlier review-repair killing tests in `5a0816fe6`):

- **Domain:** `UserSession` in `gyre-domain/src/user_profile.rs` with all spec fields (id, user_id, token_hash, ip_address, user_agent, created_at, last_active_at, expires_at, revoked); only the SHA-256 of the API key is stored (`hash_api_key`), never plaintext. `is_active(now)` = not revoked and not expired.
- **Port:** `SessionRepository` in `gyre-ports/src/user_profile.rs` — create (duplicate-id fails), list_for_user, find_by_id, find_by_token_hash, find_by_credential_and_device, touch (slides TTL), revoke (owner-scoped), revoke_all_for_user, delete_expired_before (never deletes revoked rows — they are the durable sign-out record).
- **Adapters:** SQLite + PostgreSQL + mem implementations against shared migration `2026-10-08-000059_user_sessions` (renumbered 000056→000057→000058→000059 across rebase/merge repairs — always a pure directory rename to the next unused same-date sequence, SQL byte-identical); diesel schema registered.
- **Auth integration (`auth.rs`):** on successful API-key auth, the session for (credential, IP, User-Agent) is found or minted; `last_active_at` + sliding `expires_at` are refreshed at most once per `SESSION_TOUCH_THROTTLE_SECS` (60s); a revoked session for the device rejects auth with 401; `credential_revoked` rejects the key when every session for it is revoked ("sign out everywhere", including never-seen devices) — enforced before `track_session` in the HTTP extractor and on the WS/TTY `authenticate_token` path. IP comes from `ConnectInfo` socket peer (no forwarded-header trust, CWE-348).
- **API (`api/users.rs` + `api/mod.rs`):** `GET /users/me/sessions` (active only, no token hash exposed), `DELETE /users/me/sessions/:id` (404 on foreign id), `POST /users/me/sessions/revoke-all`, plus TenantAdmin variants `GET|DELETE|POST /users/:user_id/sessions[...]` with Admin-role **and tenant-containment** per-handler checks (ABAC-exempt routes): `ensure_same_tenant_session_target` rejects with Forbidden unless the caller is the system principal or the target user's `tenant_id` matches the caller's tenant scope (fail-closed on a target with no tenant binding) — review-repair F1, see below. Target-user existence 404s.
- **Background cleanup:** `spawn_session_cleanup` (daily) deletes only unrevoked sessions expired >30 days.

### Test evidence (focused re-verification at head `32c13e542`; original full-suite run at `5a0816fe6`)

- At head `32c13e542` (after the merge into `774737638`): `cargo test -p gyre-server --lib` — **1543 passed, 0 failed** (full crate suite; includes all session tests named below plus base's tenant-binding tests); `cargo test -p gyre-adapters --lib` — **401 passed, 0 failed** (includes the 8 `sqlite::user_profile::tests::session*` adapter tests); `cargo test -p gyre-domain --lib` — **442 passed, 0 failed**; `cargo check --all` clean; `cargo clippy --lib` (server+adapters+domain) — no warnings on merge-touched lines; `cargo fmt --all --check` clean; all mechanical checks pass including `check-scope-literal-defaults.sh` (9 entries — both sides' real fixes kept), `check-arch.sh`, `check-abac-route-registry.sh`, `check-migration-versions.sh`, `check-fabricated-scope-defaults.sh`, `check-inert-enforcement.sh`, `check-mem-port-contracts.sh`, `check-forged-scope-fields.sh`, `check-forwarded-header-trust.sh`, `check-lossy-secret-conversion.sh`, `check-fail-open-ref-resolution.sh`, `check-in-memory-state-stores.sh`, `check-mcp-write-tools.sh`, `check-dead-message-kinds.sh`, `check-byte-slice-truncation.sh`, `check-relative-path-defaults.sh`, `check-migration-sql-portability.sh`, `check-abac-exempt-handlers.sh`, `check-unbounded-external-http.sh`.
- `cargo test -p gyre-adapters --lib -- sqlite::user_profile::tests::session` — 8 passed (roundtrip, duplicate-id, device-match on all four fields, newest-match, TTL slide, owner-scoped revoke, revoke-all scope, retention-never-deletes-revoked).
- Session coverage is listed by exact test name rather than by a `-- session` filter count, because substring filters also match unrelated name-colliding tests (e.g. ws `ws_session_cap`, spec_patrol `supersession`) while missing tests whose names lack the substring:
  - `api/users.rs`: `sessions_endpoint_lists_session_created_by_auth`; `revoke_session_then_credential_is_rejected`; `revoking_one_device_session_does_not_sign_out_other_devices`; `revoke_all_sessions_signs_out_everywhere`; `revoke_all_blocks_credential_from_never_seen_device`; `my_session_listing_excludes_revoked_and_expired_sessions`; `admin_session_listing_excludes_revoked_and_expired_sessions`; `revoke_cannot_target_another_users_session`; `tenant_admin_can_view_and_revoke_another_users_sessions`; `non_admin_cannot_view_or_revoke_another_users_sessions`; `tenant_admin_revoke_all_signs_user_out_everywhere`; `admin_session_endpoints_unknown_user_is_404`; `cross_tenant_admin_cannot_view_or_revoke_another_tenants_user_sessions`.
  - `auth.rs`: `session_last_active_touch_is_throttled`; `expired_session_is_reminted_not_zombified`; `signout_everywhere_survives_retention_cleanup`; `ws_auth_rejects_signed_out_everywhere_credential`; `ws_auth_rejects_revoked_ws_device_session`.
- Mutation-verified killing strength (logs under `/tmp/stage/review-evidence/task-111-mutation-*.log`): removing the `credential_revoked` check, the `track_session` rejection, or both from `authenticate_token` each fails at least one WS-path test; neutering the `.filter(|s| s.is_active(now))` in either listing handler fails both listing tests.

### Review repairs

- **Review repair (attempt `e7f17f9c`, commit `6f20bcb24`)** — F1 (major, unsafe scope handling): the TenantAdmin session endpoints `list_user_sessions`, `revoke_user_session` and `revoke_all_user_sessions` checked only the Admin role and target-user existence — never the loaded target user's tenant against the caller's tenant — so an Admin-role API-key principal of tenant t-1 could list (user_agent, ip_address, last_active_at) and revoke the sessions of a tenant-t-2 user. Fixed with `ensure_same_tenant_session_target`, called by all three handlers after the target load: Forbidden unless `auth.is_system_principal()` or the target user's `tenant_id` matches `auth.tenant_id` (fail-closed on `None`), mirroring the `create_user` precedent for the repo-wide ABAC-exempt route invariant. Killing test `cross_tenant_admin_cannot_view_or_revoke_another_tenants_user_sessions` seeds tenants t-1 and t-2, authenticates a t-1 victim, then asserts a t-2 Admin gets Forbidden on all three endpoints while the victim's session survives and a same-tenant t-1 Admin still succeeds (so the fix is tenant containment, not a blanket rejection of API-key principals — previous admin tests only used the exempt system principal). Mutation-verified both directions (evidence `/tmp/stage/review-evidence/mutation-neutered-check.txt`, `mutation-blanket-reject.txt`): neutering the check fails the test, and so does an over-broad "reject every non-system principal" rewrite. Post-repair at head `6f20bcb24`: `cargo test -p gyre-server --lib` — **1553 passed, 0 failed** (full crate; evidence `server-lib-full.txt`, `users-module.txt` — 30/30); `cargo fmt --all --check` clean; `cargo clippy -p gyre-server --lib` — no warnings on changed lines; `check-abac-exempt-handlers.sh`, `check-arch.sh`, `check-abac-route-registry.sh` all pass.
- **Verification repair (attempt `7a9f98d6`, commit `74749334f`)** — checks.sh on candidate `04f38854` failed on three findings, all closed:
  - **rustfmt on changed lines** (`api/users.rs`, `auth.rs`): fixed; `cargo fmt --all --check` is clean.
  - **clippy `needless_borrows_for_generic_args`** (`auth.rs:3197`, `UserSession::new`'s `impl Into<String>` token-hash arg; plus the same lint on the new `mcp.rs` `tool_error` lines): borrows removed.
  - **"new verification exemptions forbidden"** — three re-pinned entries in `scope-literal-defaults-exemptions.txt` (`sqlite/postgres/merge_request.rs:166`, `gyre-domain/merge_request.rs:124`). The re-pins were correct pins for real sites (base's `:163`/`:119` pins were stale — base's check scanned zero files because of the no-arg argv bug this branch fixed), but the frozen-baseline guard rightly forbids entries absent from base, and reverting either the pins or the check fix would weaken a verifier. The three legacy fabrication sites are therefore **fixed for real** and their entries deleted (13→10, `FROZEN_EXEMPTION_COUNT` lowered in the script):
    - `MergeRequest::new` now takes `workspace_id: Id` as a constructor parameter; the seven production call sites (REST `create_mr`, spawn-complete, MCP `create_mr`, `specs_assist`, release prepare, admin seed ×2) pass the repository's real workspace and refuse to create the MR when the repository cannot be resolved (404 / tool error) instead of fabricating `Id::new("default")`. The ~78 test call sites pass the workspace they previously set via post-construction assignment (hoisted into the constructor) or the `"default"` value the constructor used to fabricate — behavior preserved exactly.
    - SQLite/PG `MergeRequestRepository::create` persists under the storage handle's configured tenant scope (mirroring `TaskRepository::create`) instead of `tenant_id: "default"` — previously a tenant-scoped handle wrote rows invisible to its own tenant-filtered reads. Killing test `sqlite::merge_request::tests::create_persists_under_the_handles_tenant_scope` (mutation-verified: restoring the literal fails it).
    - `api/merge_requests.rs` + `api/merge_deps.rs` test modules seed the `repo-1` repository that `create_mr` now requires (unknown repository → 404, fail-closed).
  - Post-repair evidence at head `74749334f`: `cargo test -p gyre-server --lib` — **1501 passed, 0 failed**; `cargo test -p gyre-adapters --lib` — 392 passed, 0 failed; `cargo test -p gyre-domain` — 436+8 passed; `cargo check --all --tests` clean (no new warnings on changed lines); `cargo fmt --all --check` clean; all mechanical checks pass including `check-scope-literal-defaults.sh` (OK at 10 entries) and `check-task-commit-attribution.sh`.
- **Attempt `ff6a7ca3`** (extractor path, closed in `f0d541c75`): the extractor's `credential_revoked()` call had no endpoint-level killing test. `revoke_all_blocks_credential_from_never_seen_device` posts revoke-all, then presents the same key with a fresh User-Agent (never-seen device, no session row of its own) and asserts 401 — removing the extractor call mints a new session and returns 200, failing the test.
- **Attempt `88ab7af7`** (this round, commit `5a0816fe6`):
  - WS/TTY path: `authenticate_token` (ws.rs / tty.rs present a raw token from the client's Auth message) had no killing test for its revocation enforcement. `ws_auth_rejects_signed_out_everywhere_credential` signs a credential out everywhere (its only row is an HTTP device) and asserts the WS path rejects it — the WS ("", "") device pair has no revoked row of its own, so only the credential-wide check can reject it. `ws_auth_rejects_revoked_ws_device_session` revokes only the WS device's session while a sibling HTTP session keeps the credential alive, and asserts the WS path still rejects — only the per-device `track_session` rejection catches this. Mutation-probed: each single-check removal and the both-checks removal fail at least one of the two tests.
  - Active-only listing filter: `my_session_listing_excludes_revoked_and_expired_sessions` and `admin_session_listing_excludes_revoked_and_expired_sessions` seed an expired row and revoke one device's session, then assert both the /me and TenantAdmin listings return only the live sessions. Mutation-probed: neutering the filter in either handler fails the corresponding test.
  - Docs: the previous Shipped list misattributed coverage to a `-- session` filtered run; coverage is now stated by exact test name (above).

### Merge history

Merged up to base `774737638` (task-177) in `32c13e542` (rebase repair, id `eddfa21be`): six content conflicts, all resolved by keeping **both sides' real fixes**:
- `auth.rs`: took base's tenant-scope evolution everywhere (`system_principal_tenant()` for system/agent/federated principals; fail-closed `user.tenant_id` binding on both API-key paths) while keeping our session enforcement (`credential_revoked` + `track_session`) on the HTTP extractor and the WS/TTY `authenticate_token` path; kept both test suites (our 5 session tests + base's `is_system_principal_discriminates_spoof_shapes` and tenant-binding tests); our WS session tests now provision tenant-bound users (`user.tenant_id = Some(...)`, mirroring base's `api_key_auth_works` pattern) because base's F1 rejects unbound API-key users.
- `gyre-server/src/lib.rs`: took base's `message_tenant_id()` workspace-record resolution (skip-and-log) over our `DEFAULT_TENANT_ID` literal; removed the now-unused `DEFAULT_TENANT_ID` const (base never had it; `system_principal_tenant()` supersedes it).
- `scripts/check-scope-literal-defaults.sh` + exemptions: combined both real fixes — ours (3 merge_request.rs sites, 13→10) and base's (`agent.rs:147` via `bootstrap_workspace`, 13→12) → **9 entries**, `FROZEN_EXEMPTION_COUNT` lowered to 9 in both the shell and python halves; kept our no-arg scan fix. Non-vacuity verified: the 9 remaining exempt sites all resolve to live flagged lines.
- `gyre-domain/src/lib.rs` / `main.rs`: combined re-exports/imports (`UserSession` + base's builtin-persona exports; `seed_builtin_personas` + `spawn_session_cleanup`).
- Auto-merge repairs in non-conflicted files: `postgres/user_profile.rs` judgment ledger adapted to base's nullable `spec_approvals.approved_at` (same `is_not_null` filter + `Option<i64>` load the sqlite adapter already had — the PG side was ours and predated base's migration `2026-11-08-000058_spec_approval_ledger`); `merge_processor.rs` base-added stale-spec tests updated to the 7-arg `MergeRequest::new` (workspace hoisted into the constructor, value unchanged `ws-1`); `api/users.rs` session tests' attacker provisioning now passes `tenant_id: t-1` because base made `tenant_id` REQUIRED on `POST /api/v1/users` (the behavior under test — non-admin Forbidden, cross-user revoke 404 — is unchanged).

Earlier: merged up to base `fefde2946` in `99c293d06` (rebase repair, finding `47ccac97b`): the only conflict was `auth.rs` `JwtClaims::tenant_id`, where both sides had reworked the JWT tenant-claim handling — the base's task-103 fix (serde schema default `default_tenant_claim`, exemption entry removed, count 4→3) supersedes the branch's interim `unwrap_or(crate::DEFAULT_TENANT_ID)` use-site form; both hunks took the base side, the branch's own scope-literal fixes (`DEFAULT_TENANT_ID` construction sites, exemption lines removed, count 22→13) and all session-management code auto-merged unchanged. Migration numbering unchanged (`2026-10-08-000059_user_sessions`; the base's new `2026-11-06/07-000057_*` migrations use different dates, no Diesel version collision).

Earlier: merged up to base `98a5299b9` in `f0d541c75`: `gyre-domain/src/lib.rs` combined re-exports (base's UserPreferences additions plus `UserSession`); `api/users.rs` kept both test suites (base's user-preferences tests with `bootstrap_user`, our session tests; `provision_user_with_key` reduced to a thin wrapper); migration renumbered `000058`→`000059` (base added `2026-10-08-000058_user_entity_profile_fields`, colliding with `user_sessions` at Diesel version `20261008000058` — only the alphabetically first would ever run; pure directory rename, SQL byte-identical). Earliest rebase repairs (merge into `72ad2a12` as `b62fa0f16`): migration 000056→000057→000058 chain, fail-closed `seed_builtin_policies` kept, CWE-348 comment kept, exemption files matched to base, exemption line re-pins for shifted frozen sites (none added).
