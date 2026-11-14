# Review — task-111 (Implement session management)

Spec: `specs/system/user-management.md` §Session Management.
Candidate: `12252f0efa964dbb7ce672ed4bcafe4075e1b1f9` (branch `pipeline/task-111/c8a89897faf446ef915209d17cc99c9b-1`), assigned base `6ffd679985be9826193f4dc089d9b1f16766273c` (task-130 token scoping). Cumulative branch history reviewed against the task contract; this round's head is the rebase repair (`a5cc47de` merge + process commits `75857b36`/`12252f0e`).

Verdict: **approved** — every acceptance criterion is backed by production code and killing tests; probes below. Required verifier runs recorded at the end (gyre-server test compilation is wrapper-blocked in this sandbox).

## Probes run (evidence under `/tmp/stage/review-evidence/`)

- Mechanical gate sweep (all `scripts/check-*.sh`, log `mechanical-gates.log`): all 20 task-relevant invariant gates PASS — `check-arch`, `check-abac-route-registry`, `check-abac-exempt-handlers`, `check-migration-versions`, `check-migration-sql-portability`, `check-mcp-write-tools`, `check-scope-literal-defaults` (frozen count 6), `check-fabricated-scope-defaults`, `check-mem-port-contracts`, `check-task-commit-attribution`, `check-forwarded-header-trust`, `check-in-memory-state-stores`, `check-inert-enforcement`, `check-forged-scope-fields`, `check-fail-open-ref-resolution`, `check-byte-slice-truncation`, `check-relative-path-defaults`, `check-dead-message-kinds`, `check-lossy-secret-conversion`, `check-unbounded-external-http`.
- 34 other gates fail with awk-syntax/node errors **identically on the base commit** (verified via detached worktree, log `compare-gates-base.log`; sandbox has mawk 1.3.4) — environment, not candidate defects.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-domain` → **507 + 8 passed, 0 failed** (covers the resolved `MergeRequest::new` 7-arg and `Task::new` bootstrap constructors).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib session` → **9 passed, 0 failed** (duplicate-id fails, roundtrip, credential+device all-four match, newest-match-after-remint, retention never deletes revoked, revoke-all + delete-expired, owner-scoped revoke, touch slides expiry).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` → clean (3m39s). `--all-targets` and `cargo test -p gyre-server` are rejected by the build wrapper (4 GiB budget) — deferred to verifier, see below.
- **Mutation probe** (`mutation-probe.log`): removed the `revoked.eq(0)` guard from SQLite `SessionRepository::delete_expired_before` → `session_retention_never_deletes_revoked_rows` and `session_revoke_all_and_delete_expired` **FAIL** (7 passed / 2 failed); file restored byte-identical (sha256 verified, tree clean). The retention contract test is not self-confirming.

## Acceptance criteria verification

- **`UserSession` domain entity** — `gyre-domain/src/user_profile.rs`, all nine spec fields; only a SHA-256 (`hash_api_key`) is ever stored, never plaintext; `is_active(now) = !revoked && expires_at > now`.
- **`SessionRepository` port + SQLite impl** — `gyre-ports/src/user_profile.rs` (create/list/find_by_id/find_by_token_hash/find_by_credential_and_device/touch/revoke/revoke_all_for_user/delete_expired_before), implemented by SQLite, PostgreSQL, and mem adapters; mem guards the create-duplicate-fails and never-delete-revoked contracts in code (mem-port-contracts gate passes).
- **Migration** — `2026-10-08-000063_user_sessions`, next unused sequence after 000062, portable SQL (runs on SQLite + PostgreSQL; portability gate passes), indexed on user_id/token_hash/expires_at.
- **Sessions created on auth, throttled refresh** — `auth.rs::track_session`: one row per (credential, IP, User-Agent); `last_active_at` + sliding `expires_at` written at most once per 60 s; expired-not-revoked rows re-minted (exactly one live row per device, tested). IP from `ConnectInfo` socket peer only — no forwarded-header trust (CWE-348 gate passes).
- **GET /users/me/sessions** — active-only listing, never exposes `token_hash` (tested).
- **DELETE /users/me/sessions/{id}** — owner-scoped; foreign id → 404 (tested cross-user).
- **POST /users/me/sessions/revoke-all** — revokes all; "sign out everywhere" rejects the credential itself including never-seen devices via `credential_revoked` (tested with the stolen-key/new-device scenario), on both the HTTP extractor and the WS/TTY `authenticate_token` paths (two dedicated WS tests with live-credential controls).
- **Revoked sessions rejected in auth middleware** — per-device revoked row → 401 while sibling devices stay signed in (tested both directions); revoked rows are never resurrected or deleted, so retention cleanup cannot re-authorize a signed-out credential (tested).
- **Routes registered with ABAC mappings** — six routes in `api/mod.rs` with per-handler authorization on ABAC-exempt mappings: `/me/*` scope to the caller by construction; `/:user_id/*` require Admin role **and** tenant containment (`ensure_same_tenant_session_target`, fail-closed on missing tenant binding, system-principal exemption consistent with `create_user`), unknown user → 404. Tested: non-admin 403, cross-tenant admin 403 with a same-tenant positive control, admin view/revoke/revoke-all.
- **`cargo test --all`** — deferred to verifier (recorded below); domain + adapters packages pass locally, server type-checks clean, and the prior verification round at the pre-rebase head ran the full suite (2055 passed / 4 failed, the four fixed by `db116ab7`).
- **Background cleanup** — `spawn_session_cleanup` wired in `main.rs`: daily, deletes only unrevoked rows expired > 30 days.

## Rebase-resolution review (this round's diff vs base `6ffd6799`)

- `MergeRequest::new` keeps the 7-arg explicit-`workspace_id` form (stronger than the incoming bootstrap binding); the incoming `bootstrap_workspace` helper is removed and the removal documented; all call sites (REST/MCP/admin seed/tests) pass a resolved or seeded workspace — the MCP `handle_create_mr` now refuses unknown repositories instead of fabricating a scope.
- `Task::new` takes the mainline bootstrap-workspace form; `check-scope-literal-defaults` passes at frozen count 6.
- SQLite/Postgres `merge_request.rs`/`repository.rs` `create()` persist under the storage handle's tenant scope with killing tests (`create_persists_under_the_handles_tenant_scope`, `create_is_scoped_to_the_handles_tenant` — cross-tenant isolation asserted).
- `mcp.rs` `seed_mr` fixture correctly adapted to the 7-arg constructor (verified by reading the fixture; `mcp_scope_worker_list_mrs_no_repo_clamped_to_own_repo` is a verifier-required run).
- `scripts/check-scope-literal-defaults.sh` gains a no-arg default of `crates/` — a genuine fix for a vacuous-invocation bug (previously the argv-iterating body scanned zero files when invoked without args); frozen count unchanged.

## Design notes (non-blocking, recorded)

- Sessions are bound to the platform's user credential (the personal API key): `token_hash` stores the SHA-256 of the API key, and one row exists per (credential, IP, User-Agent). The spec's field comment ("SHA-256 of session token") is satisfied — only a hash is stored — but there is no separately issued per-session bearer token, because Gyre's user auth protocol presents the long-lived API key on every request. Consequences, both documented in the code and honestly stated in the coverage row: (a) revoking one device's session signs that device out but cannot cryptographically bar a holder of the raw key who spoofs a new User-Agent while sibling sessions remain live — the credential-wide guarantee is "sign out everywhere" (revoke-all), which rejects the key from every device including never-seen ones (tested); (b) JWT/SSO principals get no session rows. Full per-session cryptographic revocation would require a session-token issuance protocol the spec section does not define; the task plan's acceptance criteria do not require one.
- `track_session`/`credential_revoked` fail open (logged) on session-store errors — the credential itself was already validated against `api_keys`, so a session-store outage does not lock every user out. Reasonable availability trade-off, documented at both call sites.

## Required in the independent verifier

- `SKIP_WEB_BUILD=1 cargo test --all` — repo-wide acceptance criterion (server test compilation exceeds the sandbox's 4 GiB wrapper).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib auth` — session tracking/revocation tests (throttle, re-mint, retention, WS path).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib api::users` — session endpoint tests (listing filters, cross-user 404, admin tenant containment).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib mcp::tests::mcp_scope_worker_list_mrs_no_repo_clamped_to_own_repo` — the task-130 fixture adapted this round to the 7-arg constructor.
- Full CI mechanical gate suite (locally: 20/20 task-relevant gates pass; the 34 failures are base-identical mawk/node environment issues).

## Generated-output observation (non-blocking, per review policy)

`web/dist/` was not regenerated in this checkpoint — expected: the deterministic verifier owns the bundle rebuild and binds publication to the tested tree. No authored frontend source (`web/src/`) is touched by this candidate (the session surface is backend + API only), so there is no stale-bundle source defect to report.
