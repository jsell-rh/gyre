# Review — task-111 (Implement session management)

Spec: `specs/system/user-management.md` §Session Management.
Comparison base: `98a5299b905609adf4f309bceb855e5938d4405e` → candidate `38dd5c33fe57206af08cceaff8b76ae9994e575b` (branch head, includes merge `f0d541c75` and process commits).
Verdict: **needs-revision** (not approved) — two unguarded enforcement behaviors (same class as the round-1 finding), one record-accuracy note. The core implementation is real and largely proven; the gaps are missing killing tests, not fake code.

Evidence: `/tmp/stage/review-evidence/` (summary: `task-111-review-summary.md`; every probe log with command context is in that directory). Unlike the previous round, crates.io was reachable here, so the deferred Rust probes were actually executed.

## What was verified as real and working

- **Diff scope**: 21 files, +2985/−198. Domain entity (`UserSession`, all spec fields), `SessionRepository` port (create/list/find_by_id/find_by_token_hash/find_by_credential_and_device/touch/revoke/revoke_all_for_user/delete_expired_before), SQLite + PostgreSQL + mem implementations against shared migration `2026-10-08-000059_user_sessions` (next unused sequence; SQL portable), diesel schema, `sessions` port-backed store in `AppState`, daily `spawn_session_cleanup`, routes + ABAC-exempt mappings with per-handler auth (caller-scoped `/me` handlers; Admin-role checks + target-user 404s on the TenantAdmin variants), no token hash in any response.
- **Good runs (clean checkout, cold build)**: `cargo test -p gyre-server --lib -- session` → 13 passed; `revoke_all_blocks_credential_from_never_seen_device` → 1 passed (the round-1 repair is real); `signout_everywhere_survives_retention_cleanup` + `tenant_admin_revoke_all_signs_user_out_everywhere` → 2 passed; `cargo test -p gyre-adapters --lib -- sqlite::user_profile::tests::session` → 8 passed (real SQLite temp-file round-trips); full `cargo test -p gyre-server --lib` → **1431 passed, 0 failed**; full `cargo test -p gyre-adapters --lib` → **383 passed, 0 failed, 12 ignored**. No regressions outside the session surface.
- **Mutation probes (isolated worktree, never the review checkout)**:
  - Neutering the extractor's `credential_revoked()` call → `revoke_all_blocks_credential_from_never_seen_device` FAILS (exit 101). Killed.
  - Fail-opening the per-device revoked-session rejection in `track_session` → `revoking_one_device_session_does_not_sign_out_other_devices` FAILS (exit 101). Killed.
- **Mechanical checks**: all 89 `scripts/check-*.sh` produce identical outcomes on base and candidate (55 PASS / 34 pre-existing FAIL on both; `diff` of outcome lists is empty). Migration numbering, SQL portability, ABAC registry, exempt handlers, mem port contracts, task-commit attribution, arch, forwarded-header trust, in-memory state stores, forged scope fields — all pass on the candidate.

## The `check-scope-literal-defaults.sh` change is legitimate (scrutinized)

The candidate modifies a verifier script and its exemption file — normally a red flag. Verified safe:

- At base, a no-arg invocation scans **zero files** and passes vacuously; an explicit `crates/` scan at base FAILS on 6 drifted exemption pins. The candidate's `set -- crates/` no-arg default converts the vacuous invocation into a real scan that genuinely passes — a tightening, not a bypass.
- Frozen exemption count went **down** 24 → 13; only deletions and line re-pins, no additions.
- The 11 deleted entries (task-099 F1 auth.rs/lib.rs sites) were re-attributed to `crate::DEFAULT_TENANT_ID`; `bootstrap_default_tenant` — creating a real Tenant row id `"default"` — pre-exists at base (lib.rs:1376 at `98a5299b9`, invoked from main.rs:47) and is spec-documented (hierarchy-enforcement.md §Bootstrap). The semantic claim in the new exemption header checks out.

## Findings (block approval)

1. **WS/TTY auth path session enforcement is unguarded** (`crates/gyre-server/src/auth.rs`, `authenticate_token` ~866–883). Mutation removing BOTH the `credential_revoked` check and the `track_session` rejection left 137 session/revoke/ws/ticket-filtered tests green. The path is independently reachable: `ws.rs:520` authenticates a raw token from the client's `Auth` message on `/ws` (and `tty.rs:46`), so a signed-out-everywhere credential would still open a WebSocket if this regressed. Repair: a test that revokes all sessions for a key and asserts `authenticate_token` rejects it (the mirror of the round-1 extractor repair).
2. **Active-only listing filter is unguarded** (`crates/gyre-server/src/api/users.rs`, `list_my_sessions` and `list_user_sessions`). Mutation removing `.filter(|s| s.is_active(now))` left all 30 session/revoke tests green. The plan/AC require listing *active* (non-revoked, non-expired) sessions; nothing asserts a revoked or expired session is absent from the listing. Repair: after revoking one device's session, assert from the sibling device that it no longer appears in `GET /users/me/sessions`.
3. **Record accuracy** (`specs/tasks/task-111.md` Shipped): the "13 passed" coverage list is wrong in composition — the actual 13 include 3 unrelated name-collisions (`spec_patrol` ×2, `ws_session_cap`) and omit "sign-out survives retention cleanup" and "admin revoke-all", whose test names don't match the `session` filter (both exist and pass when run by name). Correct the list so verification scoping isn't misled.

## Notes for the next round

- The revoked-rows-are-never-deleted retention rule is a deliberate, documented deviation from the spec's "delete expired sessions" (a deleted revoked row would re-authorize a signed-out key under `credential_revoked`); it is port-contract-documented, adapter-tested (`session_retention_never_deletes_revoked_rows`), and mutation-consistent with the sign-out enforcement. Sound, but the unbounded growth of revoked rows is a retention tradeoff the spec lifecycle may want to bless explicitly.
- `credential_revoked` fails open on session-store errors (logged); the key itself is still validated against `api_keys`. Documented design choice, acceptable.
- Full workspace `cargo test --all` (AC #10) is deferred to the controller's verification gates per the review-stage wrapper; the two touched crates' full lib suites pass.
