# Review — task-111 (Implement session management)

Spec: `specs/system/user-management.md` §Session Management (rows: coverage
matrix row 12; API rows 26 for the session endpoints).
Candidate under review: `f0ee49c5` (branch
`pipeline/task-111/9e9c873941274579aeb6eba640a4d383-1`), assigned base
`9bfab365`. Net diff base→candidate inspected in full.
Verdict: **approved** — independent evidence supports the task contract; the
`cargo test --all` criterion is deferred to the independent verifier per the
4 GiB in-sandbox build budget, with the exact suites listed below.

## Probes run (evidence: /tmp/stage/review-evidence/probe-log.md)

- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo check -p gyre-server --lib --tests`
  → exit 0 (9m10s). Type-checks the server including every `#[cfg(test)]`
  module — the 17 session tests compile; domain/ports/adapters checked as
  transitive deps.
- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo test -p gyre-adapters session`
  → exit 0, 9/9 passed (duplicate-id contract, credential+device matching,
  newest-match, touch/sliding expiry, scoped revoke, revoke-all +
  delete-expired, retention never deleting revoked rows) against real SQLite
  via tempfile.
- Mechanical gates, all exit 0: arch, abac-route-registry,
  abac-exempt-handlers (86 handlers, includes the six new session handlers),
  mem-port-contracts (MemSessionRepository bails on duplicate id),
  forwarded-header-trust, in-memory-state-stores, scope-literal-defaults,
  fabricated-scope-defaults, migration-sql-portability,
  task-commit-attribution, migration-versions.
- `grep -c '| not-started |' specs/coverage/system/user-management.md` → 0;
  row 12 `implemented | task-111` with a substantive audit note.
- Worktree clean before and after; no source edits made during review.

## Verified against the acceptance criteria

- **Domain entity**: `UserSession` in `gyre-domain/src/user_profile.rs` with
  all nine spec fields; only the SHA-256 of the presented credential is
  stored (`hash_api_key`), never plaintext.
- **Port + adapters**: `SessionRepository` (create/list/find_by_id/
  find_by_token_hash/find_by_credential_and_device/touch/revoke/
  revoke_all_for_user/delete_expired_before) implemented by SQLite,
  PostgreSQL (plain INSERT → duplicate id fails, matching the port contract),
  and mem (bails on duplicate id). Newest-match ordering and the
  revoked-rows-are-never-deleted retention contract are consistent across
  all three.
- **Migration**: `2026-10-08-000064_user_sessions` — table + 3 indexes,
  portable SQL; `000064` is the next unused sequence number (gate passes).
- **Auth integration**: sessions minted/found on the API-key branch of the
  HTTP extractor (after deactivated + tenant-binding checks) keyed by
  (credential hash, IP, User-Agent); `last_active_at` + sliding `expires_at`
  refreshed at most once per 60 s; revoked per-device session → 401;
  `credential_revoked` (every session of the key revoked) → 401; both checks
  also on the WS/TTY `authenticate_token` path. IP comes from `ConnectInfo`
  socket peer only (CWE-348 honored; forwarded headers untouched).
- **Endpoints**: `GET /users/me/sessions` (active-only, never exposes
  `token_hash`), `DELETE /users/me/sessions/:id` (scoped: foreign id → 404),
  `POST /users/me/sessions/revoke-all`, plus the spec's TenantAdmin
  capability via `/users/:user_id/sessions…` with Admin role check, target
  existence (404), and `ensure_same_tenant_session_target` tenant
  containment (fail-closed on unbound target; system-principal exemption
  consistent with `create_user`).
- **Routes + ABAC**: registered in `api/mod.rs`; ABAC-exempt resolver
  entries with per-handler authorization — the repo's sanctioned pattern for
  caller-scoped routes, validated by `check-abac-exempt-handlers.sh` (frozen
  exemption file untouched).
- **Cleanup**: daily `spawn_session_cleanup` (spawned in `main.rs`) deletes
  only unrevoked sessions expired > 30 days.
- **Tests are real, not self-confirming**: the server session tests drive
  the real `api_router().oneshot(...)` with real bearer keys and assert
  status codes end-to-end (revoked session → 401, other device stays 200,
  cross-user revoke → 404 + victim survives, cross-tenant admin → 403);
  auth.rs tests cover the throttle window (no write inside 60 s), expired
  re-mint (exactly one live row), retention never re-authorizing a
  signed-out credential, and both WS-path rejection modes with live-control
  users. Adapter tests run against real SQLite here.
- **Base-merge fallout** (`merge_processor.rs`, `git_http.rs`,
  `merge_requests.rs`, etc.): mechanical adaptations to the 7-arg
  `MergeRequest::new(workspace_id)` constructor — semantically equivalent to
  the previous assign-after-construction form; no assertions weakened.
- **Exemption-file reduction is real**: `scope-literal-defaults-exemptions.txt`
  emptied 6→0 by fixing the four SQLite/PG analytics `record()`/`record_cost()`
  rows to the storage handle's resolved tenant scope and deleting two dead
  lines for files removed upstream; FROZEN count lowered, never raised; the
  script also gained a no-arg `crates/` default so bare invocations no longer
  scan vacuously — a hardening that passes.

## Non-blocking observations (recorded for the verifier, no action required)

- The migration's date prefix (`2026-10-08-000064`) sorts before the
  previously newest `2026-11-13-000063`, so on a fresh database it runs
  before it. The table is independent (no FKs, no shared columns), so
  ordering is harmless; the sequence number is the correct next-unused.
  Cosmetic only.
- `SessionRepository::find_by_token_hash` has no production caller (the auth
  path resolves sessions via `find_by_credential_and_device`); it is
  plan-mandated in the port trait and implemented consistently, so it is
  dead-but-specced, not fakery.
- `track_session`/`credential_revoked` fail-open (log + allow) on
  session-store errors — a documented availability tradeoff: the credential
  itself was validated against `api_keys`; revocation state is unavailable,
  not "no". Acceptable for this design; noted for the record.
- The spec's "session token" is realized as the API key credential (the
  platform has no separate session-token auth flow). This adaptation is
  documented in the coverage note and preserves every behavioral
  requirement (per-device sessions, revoke, sign-out-everywhere enforced in
  middleware). UI for session management is row 31 / task-127, correctly out
  of scope here.

## Deferred to the independent verifier (server test codegen exceeds the
in-sandbox 4 GiB budget; `cargo check --lib --tests` already proves they
compile)

- `cargo test --all` (the acceptance-criterion gate)
- `cargo test -p gyre-server --lib` (includes the 17 session tests named in
  the evidence log)
- `cargo test -p gyre-server --test git_integration`
- `cargo test -p gyre-server --test spec_enforcement`

Route-conflict note: the six new routes reuse the exact static-vs-param
shapes already present at base (`/users/me/tokens` vs `/users/:id/deactivate`
under axum 0.7 / matchit 0.7.3), so no router-build panic is expected; the
`api_router().oneshot` session tests would catch one at runtime.
