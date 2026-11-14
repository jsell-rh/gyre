# Review — task-111 (Implement session management)

Spec: `specs/system/user-management.md` §Session Management.
Candidate: `c8ad3c50678aa07db29025d687765518af3fcae3` (base `adf4df3d6823d80641e83fb5c2bbe1caa9262e43`).
Verdict: **needs-revision** (one blocking finding; the session-management implementation itself is real and strong).

## Round 1

Evidence: probe commands, sources, exit codes under `/tmp/stage/review-evidence/`
(`cargo-probes.txt`, `server-tests-check.txt`, `server-check-errors.txt`,
`mechanical-checks.txt`, plus per-file diffs of the session surface).

### Verified working (no findings on the session surface)

- **Domain entity**: `UserSession` in `gyre-domain/src/user_profile.rs` with every
  spec field (id, user_id, token_hash, ip_address, user_agent, created_at,
  last_active_at, expires_at, revoked); only the SHA-256 of the credential is
  stored, never plaintext. `is_active(now)` = !revoked && unexpired.
- **Port + adapters**: `SessionRepository` in `gyre-ports/src/user_profile.rs`.
  Real Diesel implementations in SQLite (`sqlite/user_profile.rs`) and PostgreSQL
  (`postgres/user_profile.rs`), mirrored query-for-query; mem adapter in
  `gyre-server/src/mem.rs` enforces the create-duplicate-id contract in code
  (`check-mem-port-contracts.sh` OK). Migrations embedded once, portable SQL
  (`check-migration-sql-portability.sh` OK), next sequence number 000064
  (`check-migration-versions.sh` OK).
- **Real storage tests**: `cargo test -p gyre-adapters --lib session` → **9/9
  passed** against real SQLite (roundtrip, duplicate-id failure, all-four-column
  device matching, newest-match-after-remint, touch slides both last_active_at and
  expires_at, revoke scoped to owner, revoke-all isolation, retention never
  deletes revoked rows).
- **Auth integration**: sessions minted on successful API-key auth keyed by
  (credential hash, socket-peer IP, User-Agent); `track_session` throttles
  activity writes to ≥60s and slides the TTL; a revoked device session rejects
  auth with 401; `credential_revoked` rejects a key whose every session is
  revoked ("sign out everywhere", including devices never seen before); enforced
  on both the HTTP extractor and the WS/TTY `authenticate_token` path. IP comes
  from `ConnectInfo` only — forwarded headers are not consulted (CWE-348,
  `check-forwarded-header-trust.sh` OK). Expired-but-unrevoked sessions are
  re-minted (one live row per device) rather than zombified.
- **Endpoints**: `GET /api/v1/users/me/sessions` (active-only, token hash never
  exposed), `DELETE /api/v1/users/me/sessions/:id` (cross-user id → 404),
  `POST /api/v1/users/me/sessions/revoke-all`, plus TenantAdmin variants
  `/api/v1/users/:user_id/sessions[...]` with Admin-role check, target-user
  existence (404), and tenant containment (`ensure_same_tenant_session_target`,
  fail-closed on unbound targets). Killing tests in `api/users.rs` and `auth.rs`
  cover per-device vs sign-out-everywhere semantics, listing filters, WS path,
  throttle window, re-mint, cross-user 404, cross-tenant admin 403, and
  retention-survival of sign-out-everywhere.
- **Background cleanup**: `spawn_session_cleanup` wired in `main.rs` (daily,
  deletes only unrevoked rows expired >30 days; revoked rows are the durable
  sign-out record).
- **Route/ABAC registration**: all six routes registered in `api/mod.rs` with
  `RouteResourceMapping::exempt` entries + per-handler authorization
  (`check-abac-route-registry.sh`, `check-abac-exempt-handlers.sh` both OK — 86
  handlers checked).
- **Mechanical gates**: all 19 invariant scripts pass on the candidate
  (arch, scope-literal-defaults with the honestly-emptied exemption file, etc.).
  The `check-scope-literal-defaults.sh` edit is verified genuine: the four
  analytics `record()`/`record_cost()` sites now carry the storage handle's
  resolved tenant instead of a literal `"default"`, which justifies
  FROZEN_EXEMPTION_COUNT 6→0; the added no-arg `set -- crates/` default fixes a
  vacuous-invocation bug (previously scanned zero files).
- `cargo test -p gyre-domain -p gyre-ports` → pass. `cargo check -p gyre-server
  --bins` → clean; all integration test targets (`--test spec_enforcement`,
  `--test git_integration`, `--test api_integration`) → clean.

### Findings

- [ ] **F1 (blocking): the candidate's gyre-server lib test target does not
  compile — `cargo test --all` cannot pass.** `cargo check -p gyre-server --lib
  --tests` fails with 8 errors (4 sites × E0061/E0277): `merge_processor.rs`
  `mod tests`, inside `require_spec_ref_blocks_mr_without_spec_ref` (lines 11774
  `mr-no-spec-ref`, 11838 `mr-with-spec-ref`) and
  `require_approved_spec_blocks_unapproved_spec` (lines 11907
  `mr-unapproved-spec`, 11953 `mr-approved-spec`), call
  `gyre_domain::MergeRequest::new` with 6 arguments while the candidate's own
  commit 74749334 changed the constructor to 7 (workspace_id added after
  repository_id). The four tests arrived from the assigned base via task-145
  (commit f4b7f151, written against the base's 6-arg constructor); the final base
  merge 172b6bb0 took them unadapted. The implementation's claimed probe
  (`cargo check -p gyre-server`, no `--tests`) passes because production code
  compiles — the breakage is test-target-only, which is exactly why it slipped
  through. Remedy: pass the workspace id (each site already assigns
  `mr.workspace_id = Id::new("ws-1")` after construction) as the third argument
  at all four sites. Reproduce: `/tmp/stage/bin/cargo check -p gyre-server --lib
  --tests`.

### Non-blocking observations (no action required for approval)

- `track_session`/`credential_revoked` fail open (log + allow) on session-store
  errors. Deliberate and documented: the credential itself was validated against
  `api_keys`, and fail-closed would lock every user out on transient storage
  errors. Acceptable, but worth a future look if session revocation ever becomes
  a compliance control.
- Plan item "session token generation utility" is satisfied interpretively: the
  API key's SHA-256 doubles as the session `token_hash` (one session row per
  credential+device) instead of minting a separate crypto-random session token.
  Coherent with the platform's API-key auth model (there is no login endpoint
  that could hand out separate session tokens); only a hash is ever stored, per
  the spec comment.

### Verifier must run (after the F1 fix)

- `cargo test --all` (the acceptance-criterion gate; currently blocked by F1).
- Focused session suites: `cargo test -p gyre-server --lib session` (users.rs +
  auth.rs session tests), `cargo test -p gyre-adapters --lib session` (green
  in-sandbox, 9/9).
- `bash scripts/check-task-commit-attribution.sh` (timed out at 120s in this
  sandbox; needs a full-history scan).
