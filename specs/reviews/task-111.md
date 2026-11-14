# Review — task-111 (Implement session management)

Spec: `user-management.md` §Session Management.
Candidate under review: `c3ccfad6d2050661b31d8d99e444e39e132da95d` (assigned base `c844ea52ad9389b1435ba5a240f87ee297f34764`).
Verdict: **pass** (this round).

## Round 1 — 2026-11-13

### Probe evidence (logs under /tmp/stage/review-evidence/)

- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters session` → **9 passed, 0 failed** (SQLite `SessionRepository` suite: create/roundtrip, duplicate-id contract, credential+device 4-tuple match, newest-match-after-remint, touch slides `last_active_at` AND `expires_at`, retention never deletes revoked rows, revoke scoped to owner, revoke-all user-scoped, delete-expired).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters` (full) → **534 passed, 0 failed, 6 ignored** — includes the rebase-repaired `merge_requests`/`repository` fixtures and the new `create_is_scoped_to_the_handles_tenant` killing test.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-domain -p gyre-common -p gyre-ports` → **643 passed, 0 failed**.
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` → **clean** (5m49s; lib + all test targets type-check; `--all-targets` and server test codegen are blocked by the stage wrapper by design).
- `cargo fmt --check` → clean.
- All 19 mechanical invariant gates pass (architecture, ABAC route registry, ABAC exempt-handler parity — 85 handlers checked including the six new session routes, MCP write-tool gating, migration versions, migration SQL portability, dead message kinds, byte-slice truncation, relative path defaults, fail-open ref resolution, mem port contracts, fabricated scope defaults, lossy secret conversion, scope-literal defaults, inert enforcement, forged scope fields, forwarded-header trust, in-memory state stores, unbounded external HTTP).
- Modified `check-scope-literal-defaults.sh` probed non-vacuous: a temp file with `tenant_id: "default".to_string()` at construction is still caught (exit 1); the no-arg default now scans `crates/` (previously a bare invocation scanned zero files vacuously). Exemption count lowered 6→0 with all six sites fixed for real (SQLite/PG analytics `record()`/`record_cost()` now carry the storage handle's resolved tenant; two dead activity.rs lines deleted) — the documented allowed direction, verified by the new scoped-create adapter test.

### Verified against the task contract

- **`UserSession` domain entity** (`gyre-domain/src/user_profile.rs`): all nine spec fields exactly as specified; only a SHA-256 hash is ever stored (`token_hash` = `hash_api_key` of the presented credential), plaintext never persisted; `is_active(now)` = `!revoked && expires_at > now`.
- **`SessionRepository` port** (`gyre-ports/src/user_profile.rs`) with create/list/find_by_id/find_by_token_hash/find_by_credential_and_device/touch/revoke/revoke_all_for_user/delete_expired_before, each with a documented contract; implemented by **SQLite, PostgreSQL (shared migration schema), and mem** — the duplicate-id create contract is enforced in all three (SQLite/PG PRIMARY KEY, mem explicit bail; `check-mem-port-contracts.sh` green).
- **Migration `2026-10-08-000064_user_sessions`**: next unused sequence (no duplicate Diesel version prefix; `check-migration-versions.sh` green), portable SQL (TEXT/BIGINT/INTEGER, `IF NOT EXISTS`, no dialect-only functions; portability gate green), three indexes (user_id, token_hash, expires_at), matching `schema.rs` table.
- **Session tracking on auth** (`auth.rs`): on API-key auth the (credential_hash, socket-peer IP, User-Agent) tuple is looked up; found+live → throttled touch (≥60s cadence, also slides `expires_at`); found+expired-not-revoked → re-minted as exactly one fresh row; not found → minted; found+revoked → request rejected 401. IP comes from `ConnectInfo` only — no forwarded-header trust (CWE-348 gate green). WS/TTY `authenticate_token` path enforces both the credential-wide and per-device checks (killing tests document the mutations they kill).
- **"Sign out everywhere"**: `credential_revoked` rejects a key whose every session is revoked, including from never-seen devices; revoked rows are never deleted by retention (port contract + killing test `signout_everywhere_survives_retention_cleanup`), so the sign-out survives cleanup forever.
- **Endpoints**: `GET /users/me/sessions` (active-only, never exposes token hash), `DELETE /users/me/sessions/:id` (cross-user id → 404), `POST /users/me/sessions/revoke-all`, plus TenantAdmin variants with Admin-role check, target-user 404, and tenant containment (`ensure_same_tenant_session_target`, fail-closed on missing tenant binding, system-principal exemption consistent with sibling handlers). All six routes registered in `api/mod.rs` with resolver `exempt` entries and real per-handler authorization (exempt-handler parity gate green). Documented in `docs/api-reference.md`.
- **Background cleanup**: `spawn_session_cleanup` spawned from `main.rs`, daily, deletes only unrevoked rows expired >30 days.
- **Test quality**: server tests drive the real router through the real extractor (mem-backed) and assert the enforcement outcomes (revoked → 401, per-device vs everywhere, listing filters, cross-tenant admin → 403, unknown user → 404); adapter tests are contract-level and include the retention/never-delete-revoked and scoped-revoke killing tests. No self-confirming or assertionless tests found in the session surface.

### Deferred to the independent verifier (blocked in-sandbox by the stage build wrapper, not code defects)

- `cargo test --all` on this exact head — in particular `cargo test -p gyre-server` (session suite: `api::users::tests::*session*`, `auth::tests::session_*`, `ws_auth_*`) and the rebase-repaired suites (`merge_requests`, `rbac`, `mcp`, `git_http` fixtures adapted to the base's 7-arg `MergeRequest::new`). `cargo check -p gyre-server --tests` is clean on this head.
- `scripts/check-task-commit-attribution.sh` (needs full history walk; timed out in-sandbox at 120s — the committed frontmatter lists this branch's product commits).

### Non-findings (observations, not blockers)

- Sessions are keyed by the SHA-256 of the API key rather than a separately minted crypto-random session token: in this platform API keys are the only locally-issued user credential, so the key doubles as the session token. Consequence (documented in code): revoking a session permanently blocks that credential from that device, and "sign out everywhere" fully invalidates the key — recovery is a new key (self-mintable while any other auth path is live, admin-provisionable, or SSO). The spec's user capabilities are all enforced for real; recorded as a deliberate design decision.
- Keycloak-JWT (SSO) user auth is not session-tracked — IdP-managed sessions cannot be meaningfully revoked server-side; minting unenforceable rows would be audit-only theater. Correct scoping.
- `credential_revoked`/`track_session` fail-open (log + allow) on session-store errors: the key itself was validated against `api_keys`; documented tradeoff to avoid bricking auth on a session-store outage. `api_keys` deletion remains the authoritative credential kill.
- `credential_revoked` runs a `list_for_user` scan on every API-key-authenticated request; revoked rows are retained forever by contract, so the per-request scan grows with the user's session history. Performance note only.
- Two simultaneous first presentations from the same device can race-mint duplicate rows (no unique constraint on the 4-tuple); benign — lookups are newest-first and revocation is per-row.
- `find_by_token_hash` has no production caller (plan-specified port method, tested); noted as unused surface.

Summary: every acceptance criterion is satisfied by real, enforceable production code with killing tests at every layer I could execute in-sandbox; the remaining criterion (`cargo test --all`) is a recorded verifier item per stage design. No structural defects, missing enforcement, unsafe scope handling, fake implementations, or weak tests found.

— Reviewer, 2026-11-13
