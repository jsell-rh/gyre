# Review — task-111 (user-management.md §Session Management)

Assigned candidate: `710ed99288be992669583e9b49ed038d50b02b10` (branch
`pipeline/task-111/89e19b854d3240f3860ff7ba0ad37a94-2`), base
`18adceb684926b31138c6d05be1403b1621d3b02`. This round re-reviewed the full
session-management surface after the branch's merge onto base `18adceb68`
(`defaa2003`) and the review-repair commit `6be0fbfb2`. Verdict: **approved**.

## Prior-round finding repaired (3627486c)

`check-scope-literal-defaults.sh` exited 1 at the previous candidate because the
pre-existing exempted site `crates/gyre-domain/src/task.rs`
(`workspace_id: Id::new("default")` in `Task::new`) shifted from line 92 to 99
across the rebase onto base `68d51f87e`, leaving the exemption pin stale.
`6be0fbfb2` re-pins `task.rs:92` → `task.rs:99` — verified: same single
pre-existing site, exemption count still 9, no new entries, and the check now
exits 0 (reproduced live this round; `/tmp/stage/review-evidence/probe-scope-literal-fix.log`).
The dead `activity.rs` pins were correctly left untouched per the frozen-file
convention (main carries the same dead pins).

## Independent verification (this round)

- **Spec contract vs. code**: `UserSession` in `gyre-domain/src/user_profile.rs`
  carries every spec field; only the SHA-256 of the credential is ever stored
  (`hash_api_key`), never plaintext. `SessionRepository` port
  (`gyre-ports/src/user_profile.rs`) implements the full plan surface
  (create/list/find_by_id/find_by_token_hash/find_by_credential_and_device/
  touch/revoke/revoke_all_for_user/delete_expired_before) with the port
  contracts documented and enforced by every adapter (mem guards duplicate-id
  and never-deletes-revoked in code; `check-mem-port-contracts.sh` passes).
  Migration `2026-10-08-000060_user_sessions` uses the next unused sequence
  number (no duplicates; `check-migration-versions.sh` passes) and is portable
  SQL (`check-migration-sql-portability.sh` passes).
- **Auth integration** (`auth.rs`): sessions minted on successful API-key auth
  keyed per (credential, IP, User-Agent); `last_active_at` + sliding
  `expires_at` refreshed at most once per 60 s (throttle test proves no write
  inside the window and a write + TTL slide outside it); per-device revocation
  rejects auth with 401; `credential_revoked` enforces "sign out everywhere"
  (including never-seen devices) on BOTH the HTTP extractor and the WS/TTY
  `authenticate_token` paths; expired-but-unrevoked sessions are re-minted, not
  zombified; IP comes from `ConnectInfo` socket peer only (no forwarded-header
  trust; `check-forwarded-header-trust.sh` passes).
- **Endpoints** (`api/users.rs`, `api/mod.rs`): all spec routes registered
  (`GET/DELETE /users/me/sessions[...]`, `POST /users/me/sessions/revoke-all`)
  plus TenantAdmin variants with per-handler Admin-role AND tenant-containment
  checks (`ensure_same_tenant_session_target` fails closed on a target with no
  tenant binding; system principal exempt as in `create_user`). Listings are
  active-only and the response type has no `token_hash` field. Foreign session
  ids 404. `check-abac-route-registry.sh` and `check-abac-exempt-handlers.sh`
  both pass (routes are resolver-exempt with per-handler auth, the pattern used
  by the sibling token endpoints).
- **Background cleanup**: `spawn_session_cleanup` (daily, wired in `main.rs`)
  deletes only unrevoked sessions expired >30 days; revoked rows are the
  durable sign-out record — the `signout_everywhere_survives_retention_cleanup`
  test proves cleanup cannot re-authorize a signed-out credential.
- **Mutation probe (killing-test proof)**: in a scratch worktree at the
  candidate, both revocation enforcement paths were neutered (per-device
  revoked arm → `true`; `credential_revoked` → early `false`). 7 tests failed
  (`revoke_all_blocks_credential_from_never_seen_device`,
  `revoke_all_sessions_signs_out_everywhere`,
  `revoke_session_then_credential_is_rejected`,
  `tenant_admin_can_view_and_revoke_another_users_sessions`,
  `tenant_admin_revoke_all_signs_user_out_everywhere`,
  `ws_auth_rejects_revoked_ws_device_session`,
  `ws_auth_rejects_signed_out_everywhere_credential`) — the tests kill real
  enforcement bugs, they are not self-confirming. Worktree removed; checkout
  verified pristine at `710ed9928` afterwards (`git status` clean).
- **Test runs** (focused, `/tmp/stage/bin/cargo`, `SKIP_WEB_BUILD=1`; logs
  under `/tmp/stage/review-evidence/`):
  - `cargo test -p gyre-server --lib` filtered to session/revoke/tenant
    suites → **67 passed, 0 failed** (includes all 17 session killing tests
    plus base-content suites).
  - `cargo test -p gyre-adapters --lib session` → **9 passed, 0 failed**
    (roundtrip, duplicate-id, device match, newest-match, TTL slide,
    owner-scoped revoke, revoke-all scope, delete-expired scope,
    retention-never-deletes-revoked).
  - `cargo test -p gyre-domain --lib` → **463 passed, 0 failed**.
  - `cargo fmt --check -p gyre-server -p gyre-domain -p gyre-ports -p gyre-adapters`
    → clean.
- **Mechanical gates at the candidate** — all exit 0 (per-check logs under
  `/tmp/stage/review-evidence/mech-*.out`): scope-literal-defaults (the
  repaired finding), task-commit-attribution, abac-route-registry,
  abac-exempt-handlers, migration-versions, migration-sql-portability, arch,
  mem-port-contracts, in-memory-state-stores, forged-scope-fields,
  forwarded-header-trust, inert-enforcement, lossy-secret-conversion,
  fail-open-ref-resolution, fabricated-scope-defaults, dead-message-kinds,
  byte-slice-truncation, relative-path-defaults, unbounded-external-http,
  mcp-write-tools.
- **Attribution**: the new commits since the last frontmatter update
  (`defaa2003` merge, `6be0fbfb2` fix, `9e2d3c0f1`/`710ed9928` process) are
  either merge/process commits or touch only `scripts/` (non-product-surface),
  so none require frontmatter recording; `check-task-commit-attribution.sh`
  passes with the frozen exemption count unchanged.

## Notes (non-blocking)

- **Design interpretation, accepted**: `token_hash` stores the SHA-256 of the
  presented API key rather than a separately generated crypto-random session
  token. Gyre has no login flow that mints session tokens — the API key IS the
  bearer credential, sessions are keyed per credential+device, and the spec's
  essential property (never store plaintext) holds. This interpretation is
  documented in the port trait docs and was consistent across the branch's
  prior review rounds.
- Deferred to verification (workspace-wide invocations are outside review
  scope): full `cargo test --all`, all-targets Clippy, and the web vitest
  suite. Prior rounds ran each green; this round's focused probes and the
  clean per-package compile of the merged tree found no regression.

**Conclusion**: every acceptance criterion is satisfied by real production
code with killing tests verified by mutation. The prior round's only finding
is repaired and reproduced green. No findings.
