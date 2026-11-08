# Review — task-111 (Implement session management)

Spec: `specs/system/user-management.md` §Session Management.
Comparison base: `72ad2a128d87c0cfaed5d1f6d2ea8cb5f1ad5781` → candidate `9746304f9f2816cecdb007226b751b05d0e30293`.
Diff: 21 files, +2822/−83 — domain entity, port, SQLite/PG/mem adapters, migration `2026-10-08-000058_user_sessions`, auth-middleware integration (HTTP extractor + WS ticket path), 6 REST endpoints with ABAC-exempt per-handler authorization, daily retention cleanup, docs.

Verdict: **needs-revision** (one test-coverage finding; implementation itself is real and verified).

## What was verified (independent evidence, this sandbox)

crates.io was reachable this round, so this exact merged head was compiled and exercised for the first time:

- **Full suites**: `cargo test -p gyre-server --lib` → 1415 passed / 0 failed; `cargo test -p gyre-adapters --lib` → 378 passed / 0 failed / 12 ignored; `cargo test -p gyre-domain` → 434 passed / 0 failed. No regressions from the auth-extractor changes anywhere in the server crate.
- **Focused session suites**: server 12 task tests + adapters 8 task tests, all passing through the real Router (oneshot) and real SQLite adapter respectively.
- **Mechanical checks**: all 19 relevant `scripts/check-*.sh` exit 0 (arch, ABAC route registry + exempt handlers, migration versions + SQL portability, mem port contracts, scope-literal/fabricated-scope defaults, forwarded-header trust, in-memory state stores, lossy secrets, inert enforcement, task-commit attribution, ...).
- **Migration numbering**: `000058` is the next unused `2026-10-08` sequence on base — the two rebase renumber repairs are correct and collision-free.
- **Design soundness of the API-key-derived session model**: one session row per (credential hash, IP, User-Agent); revoked row for the device rejects that device; `credential_revoked` rejects the key when every session for it is revoked (covers never-seen devices after "sign out everywhere"); revoked rows are never deleted by retention (durable sign-out record); IP from `ConnectInfo` socket peer only (CWE-348); token hash never exposed by the API; scoped revokes 404 on foreign ids; Admin-role per-handler checks on the TenantAdmin variants.
- **`check-scope-literal-defaults.sh` modification is a strengthening, not tampering**: on base, every invocation path (pre-commit `pass_filenames: false`, bare CI line) ran the check vacuously — `find_files([])` scans zero files. I confirmed this from the base script and the pre-commit/CI configs. The candidate adds a no-arg `crates/` default so the check actually scans; exemption entries went 24 → 13 (removals only; `bootstrap_default_tenant` pre-exists on base, so `DEFAULT_TENANT_ID` names a real entity and the removed auth.rs/lib.rs exemptions were genuinely fixed).

## Mutation probes (temporary edits, restored byte-identical; evidence in /tmp/stage/review-evidence/)

1. **`credential_revoked` disabled** (early `return false`): all 13 API-level session tests STILL PASS; only the direct unit test `signout_everywhere_survives_retention_cleanup` fails.
2. **Per-device revocation arm disabled** (`return true` on revoked session): `revoking_one_device_session_does_not_sign_out_other_devices` FAILS — per-device enforcement wiring is guarded by a killing test.

## Finding (blocking)

**F1 — the `credential_revoked` call in the auth extractor has no endpoint-level killing test.**
Reproduction: neuter `credential_revoked` (or delete its call in `FromRequestParts` for API keys, `crates/gyre-server/src/auth.rs`) and run the full session suite — everything passes except the direct unit test, which calls the function itself and therefore cannot catch removal of the extractor call. Root cause: `revoke_all_sessions_signs_out_everywhere` and `tenant_admin_revoke_all_signs_user_out_everywhere` re-present devices whose sessions were just revoked, so the per-device `track_session` rejection alone carries those assertions; the credential-wide path (a key presented from a *never-seen* device after revoke-all — the stolen-credential scenario "sign out everywhere" exists for) is never exercised through the middleware. A one-line regression would silently reduce "sign out everywhere" to "sign out known devices" with a green suite. Fix: one API-level test — after `POST /users/me/sessions/revoke-all`, present the same key with a new User-Agent and assert 401 (it currently mints a new session and returns 200 if the wiring is removed).

## Minor observations (not blocking)

- `track_session`/`credential_revoked` fail open on session-store errors (documented availability tradeoff). In practice both `api_keys` and `sessions` back onto the same store, so the divergent-failure window is nil; noted for the record.
- Per-device revocation is keyed on (IP, User-Agent); a client that changes either mints a fresh session. Inherent to API-key-derived sessions (no per-session token exists to reject); the implemented and tested semantics match the port contract's documented model.
- The candidate also replaced the postgres `user_profile.rs` bail-stubs for `UserNotificationPreferenceRepository`/`UserTokenRepository` with real Diesel implementations (needed so `SessionRepository` satisfies check-mem-port-contracts across adapters). Code mirrors the tested SQLite implementations; PG runtime behavior is unverifiable here (no PG server) — deferred to verification like all PG paths.
- Coverage row (`specs/coverage/system/user-management.md` #12) remains `task-assigned`, consistent with the ready-for-review convention (the flip happens at completion, cf. task-199/197 process commits).
