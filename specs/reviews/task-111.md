# Review — task-111 (Implement session management)

Spec: `specs/system/user-management.md` §Session Management — `UserSession` entity (token hash only, never plaintext), users view/revoke own sessions, "sign out everywhere", TenantAdmins view/revoke any user's sessions, three `/api/v1/users/me/sessions*` endpoints.

Commits under review (cumulative diff `git diff f4acb4eb..bf3fd120`, +2763/−64 across domain/ports/adapters/server): `2e1d71a8`, `66582a88`, `f41c1d83`, `4886ec07` (frontmatter) plus follow-ups `0745f499` (test-callsite fix + exemption re-pin) and `30cb6ddd` (stray `web/dist` artifact removal); the remaining commits are `process:` bookkeeping only. Working tree at HEAD `bf3fd120` matches the assigned candidate exactly; tree pristine before and after all review probes.

Verdict: **approved** (round 1).

## Evidence (full log: `/tmp/stage/review-evidence/task-111-review-probes.md`)

- **Focused tests, unmodified candidate:** `cargo test -p gyre-adapters --lib -- sqlite::user_profile::tests::session` — 8/8 passed; `cargo test -p gyre-server --lib -- session` — 12/12 passed; `-- auth::tests` — 39/39 passed (incl. `signout_everywhere_survives_retention_cleanup`); `gyre-domain`/`gyre-ports` — 363 passed; `cargo check -p gyre-cli` clean.
- **Mutation probes (tests are not self-confirming):**
  - Disabling `track_session`'s revoked-device rejection → `revoking_one_device_session_does_not_sign_out_other_devices` FAILS (200 vs 401). Per-device revocation enforcement is load-bearing and covered.
  - Disabling `credential_revoked` entirely → the `-- session`-filtered suite still passes (every test device was already seen — the per-device layer covers them), but `auth::tests::signout_everywhere_survives_retention_cleanup` FAILS ("all-revoked credential must be rejected"). Both enforcement layers are independently covered; no single-layer hole.
- **End-to-end "never-seen device" probe (temporary test, removed after run):** after `POST /users/me/sessions/revoke-all`, the same API key presented from a brand-new User-Agent is rejected 401 through the real router + extractor. This proves the `credential_revoked` layer end-to-end, which the shipped `/me` tests alone do not (their post-revoke retries all use already-seen devices).
- **Mechanical gates all pass on the candidate tree:** arch, ABAC route registry + exempt handlers, migration versions + SQL portability, mem port contracts, scope-literal/fabricated-scope defaults, inert enforcement, in-memory state stores, forwarded-header trust, forged scope fields, lossy secret conversion, unbounded external HTTP, dead message kinds, task commit attribution.

## Verified working (no findings)

- **All acceptance criteria are real production code, not stubs.** `UserSession` in `gyre-domain` with every spec field; `SessionRepository` port with the full method set; SQLite + PostgreSQL + mem implementations against portable migration `2026-10-08-000056_user_sessions` (next unused sequence, verified `ls migrations | sort | tail`); routes registered in `api/mod.rs` with resolver-exempt ABAC mappings and genuine per-handler authorization (owner-scoped `/me` with foreign-id 404; Admin-role gate + target-user-existence 404 on the `:user_id` variants — `check-abac-exempt-handlers.sh` passes); `spawn_session_cleanup` wired in `main.rs` (daily, deletes only unrevoked rows expired >30 days).
- **Only the SHA-256 of the API key is stored** (`hash_api_key`) in `token_hash`; the response DTO (`UserSessionResponse`) has no hash field, and `sessions_endpoint_lists_session_created_by_auth` asserts `token_hash` is absent from the JSON.
- **Revocation is enforced, not audit-only.** Both the HTTP extractor and the WS ticket path check `credential_revoked` + `track_session` before returning an authenticated identity; rejection is a 401 (`Session has been revoked`). Proven by mutation probes above.
- **Throttled touch is real:** `SESSION_TOUCH_THROTTLE_SECS = 60`; `session_last_active_touch_is_throttled` proves a write happens beyond the window (and slides `expires_at`) and — the harder half — that no write happens inside the window (seeded values asserted unchanged).
- **Scope safety:** IP from `ConnectInfo` socket peer only (`main.rs` switched to `into_make_service_with_connect_info`), no forwarded-header trust (CWE-348 check passes); revoke is owner-scoped in the adapter (`WHERE id AND user_id`) and double-checked in the handler (find → ownership compare → 404); cross-user revoke attempt is 404 and the victim's session survives (tested).
- **Port-contract parity:** duplicate-id create fails in mem (code guard) as well as SQLite (PK) — `check-mem-port-contracts.sh` passes; `delete_expired_before` never deletes revoked rows in all three adapters (the durable sign-out record), tested in both the SQLite adapter suite and the retention test.
- **Exemption-file changes are pure line-number re-pins** of the same frozen sites (auth.rs grew); counts unchanged (24/7/1/1), no entries added. The `check-scope-literal-defaults.sh` edit adds a no-arg default scan target (`crates/`) — previously a bare invocation scanned zero files vacuously — a strengthening verified by the check passing on this tree with the frozen count intact.

Minor (not blocking, recorded for completeness):

- The Shipped notes say `cargo test -p gyre-server --lib -- session` — 12 passed, covering "… sign-out survives retention cleanup". That test exists and passes, but its name does not match the `session` filter (it runs under `auth::tests`); the 12 filtered passes include 2 incidental `spec_patrol` "supersession" tests. Evidence-labeling inaccuracy only — the underlying test is real and kills the `credential_revoked` mutation.
- Session-store storage failures fail open (`track_session`/`credential_revoked` log and allow): a deliberate, documented tradeoff — the credential itself was validated against `api_keys`, and hard-failing would turn a session-store outage into a platform-wide auth outage. Not hollow enforcement; noted so it stays a conscious decision.
- The WS ticket path tracks sessions with empty device attributes (no request headers on that path), so WS auth appears as an extra `("", "")` session row. Documented in-code; cosmetic.
- Live-server HTTP transport not probed (sandbox TCP listener unsupported, per capabilities.json) — endpoint behavior proven via in-process Router oneshot tests through the real extractor and middleware.
