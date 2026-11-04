# Review — task-111 (Implement session management)

Spec: `user-management.md` §Session Management.
Candidate under review: `b501527853195ab657b3e8189e6f05bf422665b0` against base `75ada1c4073ee9d56d3a1aebf5c780582d8867cb` (full base..candidate diff inspected).
Verdict: **approved**.

## Round 1

Probes (evidence in `/tmp/stage/review-evidence/`, run on the exact candidate SHA with `/tmp/stage/bin/cargo`):

- `cargo test -p gyre-adapters --lib -- sqlite::user_profile::tests::session` → **8 passed, 0 failed** (`adapters-session-tests.txt`).
- `cargo test -p gyre-server --lib -- session` → **12 passed, 0 failed** (`server-session-tests.txt`; includes the two incidental `spec_patrol` matches — the 10 session tests are the relevant ones, plus `auth::tests::signout_everywhere_survives_retention_cleanup` covered by the filter-independent suite).
- **Mutation probe** (`mutation-probe.txt`): with `track_session`'s revoked-device branch forced to `true` and `credential_revoked` forced to `false`, 4 server session tests FAIL (`revoke_all_sessions_signs_out_everywhere`, `revoke_session_then_credential_is_rejected`, `revoking_one_device_session_does_not_sign_out_other_devices`, `tenant_admin_can_view_and_revoke_another_users_sessions`). The enforcement tests are not self-confirming — they kill disabled enforcement. Mutation reverted; suite re-run green; tree verified clean at the candidate SHA afterwards.
- Mechanical checks all pass on the candidate tree: arch, abac-route-registry, abac-exempt-handlers (95 handlers), migration-versions, migration-sql-portability, mem-port-contracts, in-memory-state-stores, scope-literal-defaults, fabricated-scope-defaults, unwritten-store-fields, forwarded-header-trust, lossy-secret-conversion, inert-enforcement, task-commit-attribution (`mechanical-checks.txt`).

Verified working (no findings):

- **Spec entity + port:** `UserSession` in `gyre-domain/src/user_profile.rs` with all 9 spec fields; `SessionRepository` in `gyre-ports` with the full specced surface (create/list/find_by_id/find_by_token_hash/find_by_credential_and_device/touch/revoke/revoke_all_for_user/delete_expired_before). SQLite, PostgreSQL, and mem implementations are real queries against real storage (diesel rows ↔ domain structs), not stand-ins.
- **Migration:** `2026-10-08-000056_user_sessions` is the next unused sequence number; plain portable SQL (BIGINT/INTEGER, no dialect-only functions) — passes the portability check that exists precisely for the shared embedded dir.
- **Auth integration:** sessions minted on successful API-key auth (HTTP extractor: IP from `ConnectInfo` socket peer — no forwarded-header trust, CWE-348 respected — and User-Agent from headers; WS path: same per-device rule with empty device attributes). `last_active_at` writes throttled to ≥60s (test proves the no-write case inside the window AND the write case outside it, including TTL slide). Revoked per-device session rejects auth with 401; all-revoked credential ("sign out everywhere") rejects the key including never-seen devices — both proven by the mutation probe above to be load-bearing. Expired-but-unrevoked rows are re-minted as exactly one fresh row (test proves no re-mint-per-request explosion).
- **API endpoints:** all three specced `/users/me/sessions` routes plus TenantAdmin variants. `/me` handlers are caller-scoped; foreign session id → 404 (tested); admin handlers check the Admin role per-handler (ABAC-exempt, matching the sibling tokens/notifications pattern), unknown target user → 404 (tested, also for non-admins → 403 first, so no user-existence probing); non-admin view/revoke → 403 with the victim's session surviving (tested). Response never exposes `token_hash` (asserted in test).
- **Background cleanup:** `spawn_session_cleanup` daily, deletes only unrevoked rows expired >30 days; revoked rows are retained as the durable sign-out record, and the sign-out-survives-retention test proves cleanup cannot re-authorize a signed-out credential.
- **Retention of frozen exemption baselines:** the exemption-file edits are line-number re-pins only (24 scope-literal, 8 fabricated-scope, 3 attribution sites — counts unchanged, verified by the passing checks with frozen counts). The `check-scope-literal-defaults.sh` change (default `crates/` arg when invoked with none) makes a previously vacuous no-arg invocation actually scan — it strictly strengthens the check; pre-commit runs it with `pass_files: false`, so before this change the pre-commit path scanned nothing.
- **`main.rs`** now serves with `into_make_service_with_connect_info`, which is what makes the socket-peer IP available; the WS upgrade path is unaffected.

Non-blocking observations (recorded, not findings):

- `0745f499`'s message describes a crates test-callsite fix not present in that commit's diff (only exemption re-pins + task file). The callsite is correct in the final tree (proven by the passing adapters suite), so this is a misdescribed commit message, not a functional defect.
- The candidate head `b5015278` (a pipeline `process:` commit) dropped the rebase-repair SHAs `9879f07d`/`fd46822f` from the `commits:` frontmatter that `c5409fb4` had added. Both are scripts/specs-only commits (merge resolution + exemption line re-pin), skipped by the attribution check's product-surface filter, and their net effect is fully visible in the base..candidate diff reviewed here — no review-scope impairment for this round. Future rounds scope to the 4 implement checkpoints, which carry the product surface.
- `credential_revoked` runs `list_for_user` on every API-key auth (full session-list read per request). Correctness is unaffected; it is a per-request read on an indexed column — a scaling note for verification, not a defect.
- Sessions track API-key auth only; JWT/agent-token auth is not session-tracked. This matches the platform model where API keys are the user credential mechanism (no password login exists), is documented in the task's Shipped section and `docs/api-reference.md`, and the spec excerpt does not define a separate session-token issuance flow.

Live-server HTTP transport was not probed (TCP listener unsupported in this sandbox — infrastructure, not a code defect); endpoint behavior is proven by the in-process Router (oneshot) tests above, which exercise the real extractor, handlers, and stores.
