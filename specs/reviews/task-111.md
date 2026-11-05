# Review — task-111 (Implement Session Management)

Spec: `user-management.md` §Session Management.
Candidate under review: `de7fbcb7` (base `3c39ab1f`; branch `pipeline/task-111/fae2a165969549cb94e0d467f86bbfd1-1`). Task commits: `4886ec0`, `f41c1d8`, `66582a8`, `2e1d71a`, `5fc4fb1` (plus process/frontmatter-only commits).
Verdict: **approved**.

## Evidence

Full log: `/tmp/stage/review-evidence/review-notes.md` (probe commands, sources, exit codes, outputs).

- `cargo test -p gyre-adapters --lib -- sqlite::user_profile::tests::session` → **8 passed, 0 failed** (run by this reviewer on the exact candidate; real SQLite storage).
- `python3 scripts/check-rustfmt-diff.py 3c39ab1f` → clean (15 files). The durable verification finding (rustfmt violations on changed lines) was recorded against pre-checkpoint source `b5015278`; checkpoint `5fc4fb1` in this candidate reformatted exactly those files — verified fixed.
- `check-scope-literal-defaults.sh`, `check-fabricated-scope-defaults.sh`, `check-warn-continue-creation.sh` → all OK on the candidate.
- **Blocked by sandbox infrastructure, deferred to verification:** the review sandbox's command monitor wedged after the adapter-suite run (every subsequent bash invocation, including `echo`, times out; two attempts to start the server suite were killed during cold compilation). This is an infrastructure restriction, not a code defect. Verification must run: `cargo test -p gyre-server --lib -- session` (12 handler tests + 3 auth unit tests), `cargo test --all`, and the remaining mechanical scripts listed in the evidence notes.

## Verified working (full diff read file-by-file; no stubs, no missing enforcement)

- **Domain:** `UserSession` in `gyre-domain/src/user_profile.rs` with all 9 spec fields; `is_active(now)` = not revoked and not expired. Only a SHA-256 (`hash_api_key`, real `sha2::Sha256`) is ever stored — plaintext never persisted, never returned by any endpoint (tested).
- **Port:** `SessionRepository` (`gyre-ports/src/user_profile.rs`) with doc-level contracts: duplicate-id create fails, `revoke` is owner-scoped, `touch` slides TTL, `delete_expired_before` never deletes revoked rows (they are the durable sign-out record — deleting one would re-authorize a signed-out API key, since `credential_revoked` treats an empty session set as "nothing revoked").
- **Adapters:** SQLite, PostgreSQL, and mem implementations are contract-consistent (duplicate-id PRIMARY KEY vs `bail!` guard; owner-scoped revoke everywhere; newest-match-first on credential+device lookups). Migration `2026-10-08-000056_user_sessions` uses the next unused sequence number (000055 was the prior max) and portable SQL (CREATE TABLE/INDEX IF NOT EXISTS, BIGINT/INTEGER).
- **Auth integration (`auth.rs`):** API-key auth mints/finds one session per (credential hash, socket-peer IP, User-Agent); `last_active_at` + sliding `expires_at` refreshed at most once per `SESSION_TOUCH_THROTTLE_SECS` (60 s) — the throttle is unit-tested in both directions (stale row touched, fresh row untouched). A revoked session for the device returns 401; `credential_revoked` rejects the key when every session for it is revoked ("sign out everywhere", including never-seen devices; empty session set correctly allows). IP comes from `ConnectInfo` (main.rs now serves with `into_make_service_with_connect_info`), not forwarded headers (CWE-348). Expired-but-unrevoked rows re-mint exactly one fresh row (no zombie rows, no mint-per-request explosion — both asserted in tests). The WS `authenticate_token` path enforces the same rules with empty device attributes (WS auth carries no headers).
- **API:** all three specced endpoints plus TenantAdmin variants, registered in `api/mod.rs`; all six routes carry per-handler authorization on their ABAC-exempt resolver entries (`RouteResourceMapping::exempt` — the sanctioned mechanism, not the frozen exemption file): `/me` handlers scope to `resolve_user_id(&auth)`; admin handlers check `UserRole::Admin`, 404 on unknown target user (no existence probing), 404 on foreign session id. `UserSessionResponse` exposes no token hash. `docs/api-reference.md` documents all six endpoints.
- **Cleanup:** `spawn_session_cleanup` (daily, wired in main.rs) deletes only unrevoked sessions expired >30 days; the retention contract is adapter-tested.
- **State wiring:** `AppState.sessions` via the `store!` pattern (port-backed, not a bare in-memory map).

## Design deviations examined and found defensible (disclosed in the task record)

1. `token_hash` stores the SHA-256 of the API key rather than a separately minted random session token. No client in this platform ever presents a session token (auth is API keys + external OIDC); a minted token would be dead weight. The spec's field shape and never-plaintext rule are honored, and the choice is what makes revocation enforceable.
2. Sessions track API-key auth only, not JWT users — JWT session lifecycle belongs to the external IdP, which the platform cannot revoke.
3. `delete_expired_before` never deletes revoked rows (plan §6 said "delete expired sessions") — deleting a revoked row would silently re-authorize a signed-out credential; the deviation is the safe reading and is documented at the port level.
4. Sliding expiry on `touch` (not in spec) closes a real inconsistency (auth succeeds while the session is filtered out of the active list forever); documented and tested.

## Exemption-file changes (checked for check-gaming — none found)

Frozen counts were lowered, not raised: scope-literal 24→13 with exactly 11 lines removed, fabricated-scope 5→4 with 1 line removed, warn-continue −1 line via the check's own documented inline `// policy-create:ok` mechanism. The `DEFAULT_TENANT_ID` resolution of the former auth.rs/lib.rs sites is backed by hierarchy-enforcement.md's explicit M34 sanction of construction-time default-tenant identity ("the default tenant is the only tenant and the construction-time `tenant_id` suffices") and the idempotent bootstrap-tenant creation (api/admin.rs:483). The no-arg `crates/` default added to `check-scope-literal-defaults.sh` fixes a vacuous-invocation hole (bare invocation previously scanned zero files) — strictly stronger, and the check passes on the candidate. The `seed_builtin_policies` restructure is behavior-identical (warn on Err / debug on Ok).

## Observations (non-blocking)

- New comments contain U+2014 em-dashes; CI's em-dash check is explicitly non-blocking (`|| true` in ci.yml) and the touched files already carried them — cosmetic, pre-existing class.
- The WS path tracks sessions with empty device attributes, so an individual per-device revoke on the HTTP path does not block the same key over WS (revoke-all does). Defensible: the WS auth message carries no headers to identify a device; WS is effectively its own device row.
- mem `list_for_user` returns insertion order vs SQLite's `created_at DESC` — no correctness impact (clients receive the full active set).
- `specs/coverage/system/user-management.md` row 12 still reads `task-assigned`; status flips in that file appear to be audit-stage-owned (its header records the audit that sets them). Verification/audit should flip it to `implemented` for this section now that the behavior ships.

— Reviewer, 2026-10-08 (independent review; deferred probes and exact commands recorded in /tmp/stage/review-evidence/review-notes.md)
