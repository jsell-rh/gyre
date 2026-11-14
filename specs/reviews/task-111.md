# Review — task-111 (User Management §Session Management)

Spec: `specs/system/user-management.md` §Session Management.
Candidate: `04e291dfb1ba737bfe46885dc84f0d44a80ddecf` (base `4b9220bca`).
Verdict: **approved** — real end-to-end implementation; server-test execution deferred to the independent verifier (blocked in-sandbox by the 4 GiB build wrapper, by design).

## Round 1

### Probes (evidence: `/tmp/stage/review-evidence/`)

- All 19 mechanical invariant gates exit 0 (architecture, ABAC registry + exempt-handler parity, migration versions + SQL portability, scope-literal/forged-scope/forwarded-header/in-memory-store/mem-port-contract checks, etc.).
- `cargo test -p gyre-adapters --lib session` → 9 passed (SQLite `SessionRepository`: roundtrip, duplicate-id PK guard, credential+device 4-tuple match, newest-match-after-remint, touch-slides-expiry, retention-never-deletes-revoked, revoke scoped to owner, revoke-all scoped to user).
- `cargo test -p gyre-adapters --lib` → 534 passed; `cargo test -p gyre-domain` → 534+8 passed; `cargo check -p gyre-server --lib` → clean; `cargo fmt --check` → clean.
- `check-task-commit-attribution.sh` fails only against the executor's working-tree contract reset (`task-111.md` reverted to `commits: []`); at the candidate commit the frontmatter lists all 23 branch commits and covers every task-labeled commit.
- Server test binaries cannot be built in-sandbox (wrapper exit 64). Required verifier commands recorded in `probes.md`: `cargo test --all` on this head, `cargo test -p gyre-server --lib session`, `cargo test -p gyre-server --test git_integration`.

### Verified working (acceptance criteria)

- **Domain entity**: `UserSession` in `gyre-domain/src/user_profile.rs` with every spec field; only a SHA-256 hash is ever stored.
- **Port + adapters**: `SessionRepository` (create, list_for_user, find_by_id, find_by_token_hash, find_by_credential_and_device, touch, revoke, revoke_all_for_user, delete_expired_before) implemented for real in SQLite, PostgreSQL, and mem — real Diesel queries, scoped revoke/revoke-all, duplicate-id guard in all three adapters (mem guards in code per the port contract). 8 adapter tests green in-sandbox.
- **Migration** `2026-10-08-000064_user_sessions`: all spec columns + indexes, unique sequence number (base's duplicate 000059–000062 pairs are pre-existing base drift, not this task's), portable SQL (passes the portability gate on both backends).
- **Auth integration**: sessions minted on successful API-key auth on both the HTTP extractor (IP from socket peer `ConnectInfo` only, no forwarded-header trust — CWE-348) and the WS/TTY `authenticate_token` path; `last_active_at` refreshed at most once per 60 s with sliding `expires_at`; revoked device session rejects auth (401); `credential_revoked` rejects a key whose every session is revoked ("sign out everywhere", including never-seen devices — the stolen-credential case). Expired-but-unrevoked rows are re-minted, not zombified.
- **Endpoints**: `GET/DELETE /api/v1/users/me/sessions[/:id]`, `POST .../revoke-all`, plus TenantAdmin variants `/users/:user_id/sessions...` — Admin role check, tenant containment (cross-tenant → 403, fail-closed on missing tenant binding), unknown user → 404 (no existence probe), cross-user session id → 404, active-only listings, token hash never exposed.
- **Routes + ABAC**: registered in `api/mod.rs`; ABAC-exempt resolver entries with per-handler authorization (validated by `check-abac-exempt-handlers.sh`, 85 handlers).
- **Background cleanup**: daily `spawn_session_cleanup` deletes only unrevoked sessions expired >30 days; revoked rows are retained as the durable sign-out record so cleanup can never re-authorize a signed-out credential (adapter + auth tests prove both directions).
- **Tests**: 17 killing server tests reviewed line-by-line — they assert observable outcomes (401/403/404/200, row counts, cross-device isolation), with control cases proving setup validity; not self-confirming.
- **Coverage row 12** updated to `implemented` with an accurate note.

### Non-scope work on the branch (reviewed, sound)

The large non-session diff is base-merge resolution (the assigned base moved repeatedly) plus two principled repairs: 74749334 fixes real "default"-scope fabrication sites (MergeRequest::new workspace_id constructor param, adapters persisting under the handle's tenant scope) and empties the scope-literal exemption file 6→0 while fixing the check's vacuous no-arg scan — strictly stronger enforcement, direction-of-travel correct. c6cad385 repairs the two git_integration failures from verification finding 0124f1f5 (test-fixture-only: seeds real repos now required by create_mr's scope resolution). After c6cad385 only `specs/tasks/task-111.md` changed.

### Observations (non-blocking)

- The session's `token_hash` stores the SHA-256 of the presented API key rather than a separately minted crypto-random session token (task plan item 1 sketched a token-generation utility). The spec's storage contract (hash only, never plaintext) and every user-facing behavior are preserved; sessions are keyed per (credential, device) with documented rationale. The acceptance checklist does not require a separate mint.
- Session-store errors fail soft in `track_session`/`credential_revoked` (logged, request allowed): the credential itself is still validated against `api_keys` independently; documented availability trade-off.
- Only API-key auth is session-tracked (JWT/OIDC auth is stateless and untracked); the spec section does not specify a credential type and the port/handler docs record the choice.

### Deferred to verifier

`cargo test --all` on this exact head is the one acceptance criterion not provable in-sandbox (wrapper-blocked). Everything code-visible that gate covers has independent in-sandbox evidence: the full suite last ran at ancestor c3ccfad6 failing only the two tests c6cad385 repaired (fixture-only change, causal story verified against the create_mr scope-resolution change that broke them), and candidate code is byte-identical to c6cad385 for everything except the task markdown.
