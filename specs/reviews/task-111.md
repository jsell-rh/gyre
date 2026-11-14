# Review — task-111 (user-management §Session Management)

Spec: `specs/system/user-management.md` §Session Management — active sessions
(device, IP, last active) with individual revoke, "sign out everywhere", and
TenantAdmin view/revoke of any user's sessions.

Candidate: `794beaba` (base `b6892403`); implementation head `ed6a9adc`, latest
repair `97ef01dd` (clippy `empty_line_after_doc_comments` at
`gyre-domain/src/merge_request.rs:104`, comment-only). Verdict: **approved**.

## Scope of the diff (base → candidate)

Beyond the session-management surface, the assigned diff carries the branch's
reviewed rebase-repair history: `MergeRequest::new` now takes `workspace_id`
as an explicit constructor parameter (replacing the `bootstrap_workspace()`
binding — a real scope-fabrication fix, with the exemption file unchanged at
6 frozen entries), `RepoRepository`/`MergeRequestRepository` create() rows filed
under the storage handle's `self.tenant_id` (pre-existing in base; this branch
adds killing tests for the tenant-scoping), `Task::new` bootstrap-workspace
doc alignment, call-site adaptations across api/merge_requests.rs, spawn.rs,
release.rs, specs_assist.rs, merge_processor.rs, mcp.rs and test fixtures, and
a `check-scope-literal-defaults.sh` fix that gives the no-arg invocation a
`crates/` default (previously bare invocations scanned zero files — a
vacuous-check strengthening, not a weakening).

## What was verified

### Session management (the task contract)

- **Domain** (`gyre-domain/src/user_profile.rs`): `UserSession` carries every
  spec field (id, user_id, token_hash, ip_address, user_agent, created_at,
  last_active_at, expires_at, revoked); `is_active(now)` = !revoked &&
  expires_at > now. Only the SHA-256 of the credential is stored, never
  plaintext (hash computed by `hash_api_key` before any storage call).
- **Port** (`gyre-ports/src/user_profile.rs`): `SessionRepository` — create
  (duplicate id fails), list_for_user, find_by_id, find_by_token_hash,
  find_by_credential_and_device (all of user+credential+ip+ua), touch (slides
  TTL), revoke (owner-scoped), revoke_all_for_user, delete_expired_before
  (never deletes revoked rows — the durable sign-out record). Port contracts
  enforced identically by SQLite, PostgreSQL (diesel, real queries) and the
  server mem adapter (in-code duplicate guard); `check-mem-port-contracts` OK.
- **Migration** `2026-10-08-000063_user_sessions`: all nine spec columns,
  three indexes, portable SQL (portability gate OK), next unused sequence
  number (migration-version gate OK), up + down.
- **Auth integration** (`gyre-server/src/auth.rs`): on successful API-key auth
  the (credential-hash, IP, User-Agent) session is found or minted;
  `last_active_at` + sliding `expires_at` refreshed at most once per 60 s
  (`SESSION_TOUCH_THROTTLE_SECS`); a revoked session for the device returns
  401; `credential_revoked` rejects the key when every session for the
  credential is revoked (covers devices never seen before the revoke-all);
  both checks enforced on the HTTP extractor and the WS/TTY
  `authenticate_token` path. IP comes from `ConnectInfo` socket peer only —
  no forwarded-header trust (CWE-348 gate OK). Expired-but-not-revoked rows
  are re-minted with a fresh TTL rather than zombified.
- **API** (`api/users.rs`, `api/mod.rs`): `GET /users/me/sessions` (active
  only, `token_hash` never serialized), `DELETE /users/me/sessions/:id`
  (foreign id → 404), `POST /users/me/sessions/revoke-all`, plus TenantAdmin
  variants `GET/DELETE /users/:user_id/sessions[...]` with per-handler
  Admin-role AND tenant-containment checks (`ensure_same_tenant_session_target`,
  fail-closed on missing tenant binding, system principal exempt like sibling
  handlers); unknown target user → 404. All six routes registered and
  resolver-exempt in `abac_middleware.rs` with the per-handler authorization
  the repo invariant requires (`check-abac-exempt-handlers` OK, 85 handlers).
- **Route-insertion safety**: the new `/users/:user_id/sessions` family
  alongside `/users/me/sessions` cannot panic axum 0.7.9's matchit 0.7.3 —
  verified against the vendored matchit source (static and wildcard children
  coexist on one node; static `/me` wins precedence; all three admin routes
  share the `:user_id` wildcard name).
- **Background cleanup** (`lib.rs` `spawn_session_cleanup`, wired in
  `main.rs`): daily, deletes only unrevoked sessions expired > 30 days.
- **Tests are behavior-killing, not mirrors**: the server suite drives the
  REAL router (`api_router().oneshot`) through the real auth extractor with
  real API keys — per-device revoke rejects only that device while siblings
  stay signed in; revoke-all rejects the credential from a never-seen device;
  listings exclude revoked and expired rows; token hash absent from
  responses; cross-tenant Admin gets 403 on all three admin endpoints and the
  victim's session survives; WS-path tests cover both the credential-wide and
  per-device rejections with a live-credential control. Adapter tests cover
  duplicate-id failure, owner-scoped revoke, retention never deleting revoked
  rows, and touch sliding expiry. Domain/port/adapters tests were re-run in
  this review (see evidence); server test compilation is deferred to the
  verifier per the 4 GiB budget, with the exact commands recorded below.
- **Docs**: `docs/api-reference.md` rows for all six endpoints; coverage row
  12 `task-assigned` → `implemented` with accurate anchors; `specs/tasks/task-111.md`
  records commits, shipped notes, and the deferred verifier commands.

### Probes run in this review (logs under /tmp/stage/review-evidence)

- 18 mechanical gate scripts: all exit 0 (including scope-literal-defaults,
  migration versions/portability, ABAC registry + exempt handlers, mem port
  contracts, task-commit attribution).
- `cargo test -p gyre-domain` → 507 + 8 passed, 0 failed.
- `cargo test -p gyre-adapters --lib session` → 9 passed, 0 failed.
- `cargo check -p gyre-server` → clean.
- `cargo clippy -p gyre-domain/-ports/-adapters --all-targets` → clean;
  `cargo clippy -p gyre-server --lib` → pre-existing warnings only, **zero
  `empty_line_after_doc_comments`** (the 651f1e90 finding is repaired).

### Required in the independent verifier

- `SKIP_WEB_BUILD=1 cargo test --all` (repo-wide acceptance criterion).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib api::users` (session endpoints).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib auth` (session tracking).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib mcp::tests` (7-arg fixture).
- Remote clippy-diff vs base (the check class that failed in run 651f1e90).

## Design notes (considered, not defects)

- `token_hash` stores the SHA-256 of the API key (the credential presented)
  rather than a distinct per-session token: the spec's API surface has no
  session-token issuance endpoint and sessions are minted on API-key auth
  (the task plan's own model), so the credential is the session token;
  plaintext is never stored. Documented in the port and auth code.
- Per-device revocation is keyed on (credential, IP, User-Agent); a device
  that changes its User-Agent string mints a new session. Inherent to the
  API-key-as-session-token model; the stronger gesture ("sign out
  everywhere") is credential-wide via `credential_revoked` and cannot be
  evaded by device spoofing. The split is deliberate (revoking one device
  must not sign out every device) and documented in the port contract.
- `track_session`/`credential_revoked` fail soft on storage errors (log and
  allow): the credential itself was already validated against `api_keys`;
  documented at both call sites. An availability tradeoff, not a bypass of
  an enforcement the spec demands.
- WS/TTY sessions are tracked with the ("", "") device pair (no HTTP
  headers on that path) — all WS clients of one credential share one row;
  documented, and the WS tests cover both revocation dimensions.
- Generated `web/dist/` was not reviewed (verifier-owned); no authored web
  source changed in this diff.
