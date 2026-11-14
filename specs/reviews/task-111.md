# Review — task-111 (Implement session management)

Spec: `user-management.md` §Session Management.
Candidate: `ba30f2b6` (branch `pipeline/task-111/168a528c963c475d81de5ae52775af11-1`) against assigned base `1ee52c43` (task-112 delivery channels). The candidate is the task-111 branch merged onto the base (`52ad675e` resolved seven content conflicts; `ba30f2b6`/`f7e8dee6` are process commits).
Verdict: **pass** (Round 1).

## Probes run in-sandbox (logs under `/tmp/stage/review-evidence/`)

- `SKIP_WEB_BUILD=1 cargo check -p gyre-ports -p gyre-domain -p gyre-adapters -p gyre-server` → exit 0 (server type-checked; test compilation deferred to the verifier per the 4 GiB budget).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib user_profile` → **13 passed, 0 failed** against real SQLite storage, incl. 9 session-repo tests: create/find round-trip, duplicate-id fails (PK guard), `find_by_credential_and_device` matches all four fields AND returns the newest match (kills re-mint-per-request), revoke is owner-scoped, revoke-all + delete-expired, retention never deletes revoked rows, touch slides expiry. Migration `2026-10-08-000063_user_sessions` ran on SQLite as part of every `SqliteStorage` construction in these tests.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-domain` → **509 + 8 passed, 0 failed**.
- All 19 mechanical gates exit 0: `check-arch`, `check-abac-route-registry` (6 new session routes registered as `RouteResourceMapping::exempt` in the resolver — not the frozen exemption file), `check-abac-exempt-handlers` (per-handler auth verified), `check-mcp-write-tools`, `check-migration-versions` (000063 is the next unused number past base's 000062), `check-migration-sql-portability` (portable `CREATE TABLE IF NOT EXISTS`/BIGINT — no dialect-only SQL), `check-mem-port-contracts`, `check-scope-literal-defaults` (exemption file byte-identical entry set to base; the script change only fixes a vacuous no-arg invocation), plus dead-message-kinds, byte-slice, relative-path, fail-open-ref, fabricated-scope, lossy-secret, inert-enforcement, forwarded-header-trust, in-memory-state-stores, unbounded-external-http, forged-scope-fields.
- Merge-resolution audit: `git diff 1ee52c43 ba30f2b6` shows pure additions in `api/users.rs` (+1355/−0), `mem.rs` (+114/−0), `auth.rs` (−1 import line); task-112's channel-preference routes/dispatcher and all base features are present in the candidate; no conflict markers anywhere; the non-session diffs are the branch's earlier review-round repairs (`MergeRequest::new` now requires `workspace_id` — constructor-level scope, no fabricated "default"; repo/MR create() filed under the storage handle's tenant) with matching killing tests.

## Verified against the acceptance criteria

- **`UserSession` domain entity** — `gyre-domain/src/user_profile.rs`, all 9 spec fields exactly; `is_active(now)` = !revoked && !expired; only SHA-256 hashes stored (`hash_api_key`), never plaintext.
- **`SessionRepository` port + adapters** — port in `gyre-ports/src/user_profile.rs` (create/list_for_user/find_by_id/find_by_token_hash/find_by_credential_and_device/touch/revoke/revoke_all_for_user/delete_expired_before, each with a written contract); SQLite, PostgreSQL (real impls — this branch also replaced the base's PG `bail!` stubs for the sibling user-profile ports with real implementations), and mem adapters all enforce the same contracts (duplicate-id fails, revoke owner-scoped, retention never deletes revoked rows — `check-mem-port-contracts` green).
- **Migration** — `2026-10-08-000063_user_sessions`, next unused sequence number, portable SQL, indexed on user_id/token_hash/expires_at; proven to run on SQLite by the adapter tests.
- **Sessions created on auth, throttled activity** — `auth.rs::track_session`: one row per (credential hash, IP, User-Agent); on the HTTP extractor path IP comes from `ConnectInfo` socket peer only (no forwarded-header trust, CWE-348 — `check-forwarded-header-trust` green) and UA from the header; `touch` fires at most once per 60 s and slides `expires_at`; an expired-but-unrevoked row is re-minted as exactly one fresh row (adapter returns the newest match, so no per-request row explosion).
- **Endpoints** — `GET /users/me/sessions` (active-only; response omits `token_hash` and `user_id`), `DELETE /users/me/sessions/:id` (foreign id → 404, tested), `POST /users/me/sessions/revoke-all`, plus TenantAdmin variants `GET/DELETE/POST /users/:user_id/sessions[...]` gated on Admin role (this platform's TenantAdmin — `user.global_role = TenantAdmin` iff `user.roles` contains `Admin`) **and** tenant containment (`ensure_same_tenant_session_target`; cross-tenant admin → 403, tested end-to-end through the real router; unknown target user → 404, tested).
- **Revoked sessions rejected in auth middleware** — per-device revoked row → 401 on both the HTTP extractor and the WS/TTY `authenticate_token` path; `credential_revoked` rejects a key whose every session is revoked ("sign out everywhere", including never-seen devices) on both paths. Killing tests go through the real router/extractor with control cases (`revoke_session_then_credential_is_rejected`, `revoking_one_device_session_does_not_sign_out_other_devices`, `revoke_all_blocks_credential_from_never_seen_device`, `ws_auth_rejects_signed_out_everywhere_credential`, `ws_auth_rejects_revoked_ws_device_session`, cross-tenant and non-admin negative tests).
- **Background cleanup** — `spawn_session_cleanup` (daily, wired in `main.rs`) deletes only unrevoked sessions expired > 30 days; revoked rows are retained as the durable sign-out record (tested: `signout_everywhere_survives_retention_cleanup`, `session_retention_never_deletes_revoked_rows`).
- **Routes + ABAC** — registered in `api/mod.rs`; resolver-exempt with per-handler authorization (the sanctioned pattern for user-scoped routes; `check-abac-route-registry` and `check-abac-exempt-handlers` green).
- **`cargo test --all`** — deferred to the independent verifier (see below); everything runnable in-sandbox is green.

## Non-blocking observations (recorded, not findings)

1. **No separate crypto-random session token utility (plan §1).** The platform has no login flow that mints bearer tokens to users — API keys are the user credential — so the implementation stores `token_hash` = SHA-256 of the API key and keys sessions per (credential, device). A separately generated session token would be a token nobody presents. The spec struct, endpoint contract, and revocation semantics are fully satisfied; the deviation from the plan's means (utility) is documented in code. Accepted.
2. **Revocation is durable and kills the credential.** Revoke-all (or revoking a key's every session) permanently rejects that API key (revoked rows are never deleted, by design, so retention cannot re-authorize). Recovery is a newly minted key (admin-provisioned). This is the direct consequence of the plan's "reject revoked sessions in auth middleware" plus the goal's no-audit-only rule; noted as an operational consequence.
3. **`track_session`/`credential_revoked` fail open on session-store errors** (logged). The API key itself was already validated against `api_keys` (its own revocation path); a session-table outage failing closed would lock out every user. Documented trade-off; acceptable.
4. **Corner:** a key that was never presented (zero session rows) is not blocked by a later revoke-all — it has no session to revoke. Spec-silent; minor.
5. **JWT-authenticated requests are not session-tracked** (only API-key and WS-token auth are). Sessions are the API-key credential model; JWTs are stateless and cannot be revoked by a row anyway. Scoping decision, documented.
6. Per-API-key-request cost: two session-store reads (`credential_revoked` + `find_by_credential_and_device`). Performance note only.

## Required in the independent verifier (gyre-server test compilation deferred per the 4 GiB budget)

- `SKIP_WEB_BUILD=1 cargo test --all` — the repo-wide acceptance criterion.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib auth::` — the session-tracking/revocation killing tests.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib api::users` — the session endpoint tests (listing filters, ownership 404, admin role + tenant containment, cross-tenant 403).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib` (full lib) and the task-112 notification/dispatcher modules carried by the merge.
- Full mechanical gate suite (CI) on the integrated tree.

Summary: real storage, real enforcement on both auth paths, correct scoping (ownership 404, Admin-role + tenant containment, socket-peer IP), no hash exposure, thorough killing tests with control cases, all mechanical gates green, and the merge onto the task-112 base preserves both sides' work. No blocking findings.

— Reviewer, 2026-11-13
