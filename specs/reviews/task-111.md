# Review — task-111 (Implement session management)

Spec: `user-management.md` §Session Management.
Candidate under review: `05b8e4bfa` (base `fb1f82e31b`). All 11 frontmatter commits present on the branch; attribution check passes.
Verdict: **approved** (round 2 — F1 tenant containment from round 1 fixed in `6f20bcb24` and verified below).

## What was verified

Diff inspection (`git diff fb1f82e31..05b8e4bfa`) plus focused probes, evidence under `/tmp/stage/review-evidence/`:

- **Domain/port:** `UserSession` in `gyre-domain/src/user_profile.rs` has every spec field; `is_active(now)` = `!revoked && expires_at > now`. `SessionRepository` port (`gyre-ports/src/user_profile.rs`) covers create (duplicate-id must fail), list_for_user, find_by_id, find_by_token_hash, find_by_credential_and_device, touch, owner-scoped revoke, revoke_all_for_user, delete_expired_before — with the port contract that revoked rows are never deleted (they are the durable sign-out record) documented and enforced in all three adapters.
- **Adapters:** SQLite, PostgreSQL, and mem implementations are behaviorally equivalent (same filters, same newest-match ordering, same retention predicate `expires_at < cutoff AND revoked = 0`; mem guards the duplicate-id contract in code — `check-mem-port-contracts.sh` PASS). Migration `2026-10-08-000059_user_sessions` uses the next unused sequence for its date (000056–000059 chain intact; `check-migration-versions.sh` PASS), is portable SQL (CREATE TABLE IF NOT EXISTS / INTEGER, no dialect-only constructs; `check-migration-sql-portability.sh` covered by mechanical run), and `schema.rs` registers `user_sessions`.
- **Auth integration (`auth.rs`):** on API-key auth the (credential-hash, IP, User-Agent) session is found or minted; `last_active_at` + sliding `expires_at` are written at most once per `SESSION_TOUCH_THROTTLE_SECS` (60s); a revoked per-device session returns 401; `credential_revoked` rejects the key when every session for that credential hash is revoked (sign-out-everywhere, including never-seen devices). Enforced on both the HTTP extractor and the WS/TTY `authenticate_token` path (WS device = ("","")). IP comes from `ConnectInfo` socket peer only — no forwarded-header trust (CWE-348; `check-forwarded-header-trust.sh` PASS). Both paths also fail closed on users without a tenant binding.
- **API:** `GET /users/me/sessions` (active-only, no token_hash exposed), `DELETE /users/me/sessions/:id` (find + ownership check, foreign id → 404), `POST /users/me/sessions/revoke-all`, plus TenantAdmin variants `GET|DELETE|POST /users/:user_id/sessions[...]` with Admin-role, target-existence-404, and tenant-containment (`ensure_same_tenant_session_target`: Forbidden unless system principal or target's `tenant_id` equals caller scope; fail-closed on unbound target) per-handler checks on ABAC-exempt routes. Routes registered in `api/mod.rs`; six `RouteResourceMapping::exempt` entries added in `abac_middleware.rs` (`check-abac-route-registry.sh`, `check-abac-exempt-handlers.sh` PASS).
- **Background cleanup:** `spawn_session_cleanup` runs daily, deletes only unrevoked sessions expired >30 days.

## Probe runs (all at candidate `05b8e4bfa`)

- `cargo test -p gyre-adapters --lib -- sqlite::user_profile::tests::session` → **8 passed, 0 failed** (roundtrip, duplicate-id, device-match all four fields, newest-match, TTL slide, owner-scoped revoke, revoke-all scope, retention-never-deletes-revoked).
- `cargo test -p gyre-server --lib -- api::users::tests` → **30 passed, 0 failed** (includes all 13 session endpoint tests: listing, per-device revoke vs sign-out-everywhere, never-seen-device credential rejection, active-only filters for /me and admin, cross-user 404, TenantAdmin view/revoke, non-admin Forbidden, unknown-user 404, cross-tenant containment).
- `cargo test -p gyre-server --lib` → **1553 passed, 0 failed**.
- `cargo test -p gyre-domain --lib` → **442 passed, 0 failed**.
- Mechanical: `check-arch`, `check-abac-route-registry`, `check-migration-versions`, `check-scope-literal-defaults`, `check-abac-exempt-handlers`, `check-mem-port-contracts`, `check-forged-scope-fields`, `check-forwarded-header-trust`, `check-in-memory-state-stores` → all PASS.

## Independent mutation probes (source restored after each; worktree clean)

- Neutering `ensure_same_tenant_session_target` (same_tenant = true) → `cross_tenant_admin_cannot_view_or_revoke_another_tenants_user_sessions` **FAILS** (200 vs 403). Round-1 F1 fix has real killing strength.
- Neutering the extractor's `credential_revoked` call → `revoke_all_blocks_credential_from_never_seen_device` **FAILS** (200 vs 401).
- Neutering the extractor's `track_session` rejection → `revoke_session_then_credential_is_rejected` **FAILS**.

## Notes (no findings)

- The "session token" is modeled as the API key itself: `token_hash` stores the SHA-256 of the raw key (never plaintext), and sessions are keyed per (credential, device). This is a sound reading of the spec for a platform whose only long-lived user credential is the API key; short-lived IdP JWTs create no session rows (and there is no server-issued session token to hash). The spec's "never store plaintext" invariant holds.
- Only the API-key auth path creates sessions, which satisfies the acceptance criterion "sessions created on auth" for the bearer-credential path; JWT/agent principals have no session rows, so the /me endpoints simply return empty lists for them (and `resolve_user_id` fabricating an id from `agent_id` cannot touch another user's sessions — revoke is ownership-checked and no session carries such a user_id).
- WS/TTY sessions are tracked with empty device attributes (that transport carries no headers), documented in code.
