# Review — task-111 (Implement session management)

Spec: `specs/system/user-management.md` §Session Management.
Candidate: `df0c1da2699f715cb267637ef9d19c884cf288b4` (base `44cf25bf`, includes repair
commit `9db91887` and two process commits).
Verdict: **approved** — no findings.

## Round 3 (this review)

Probes run (evidence: `/tmp/stage/review-evidence/`, summary in `review-summary.md`):

- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo check -p gyre-server --lib --tests` → exit 0
  (7m18s). This independently verifies the round-2 repair: the four merge_processor test
  MRs now compile against the 7-arg `MergeRequest::new`; the prior 8 errors are gone.
- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo test -p gyre-adapters session` → 9/9 passed.
- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo test -p gyre-domain --lib` → 535/535 passed.
- Mechanical gates, all exit 0 on the candidate head: `check-arch.sh`,
  `check-migration-versions.sh`, `check-migration-sql-portability.sh`,
  `check-abac-route-registry.sh`, `check-abac-exempt-handlers.sh` (86 handlers),
  `check-mem-port-contracts.sh`, `check-task-commit-attribution.sh`,
  `check-scope-literal-defaults.sh`.
- `cargo test -p gyre-server --lib` could not execute in-sandbox (build wrapper rejects
  gyre-server test codegen against the 4 GiB budget) — infrastructure limit, not a code
  defect. Required verifier commands recorded in the evidence summary and below.

Verified against the acceptance criteria (full diff inspection 44cf25bf..df0c1da2):

- **`UserSession` domain entity** — `gyre-domain/src/user_profile.rs`, all nine spec
  fields; only the SHA-256 of the credential is stored, never plaintext.
- **`SessionRepository` port + SQLite (and PostgreSQL/mem) implementations** — every
  plan method present with documented contracts; mem enforces the duplicate-id guard in
  code (SQLite/PG via PK); `delete_expired_before` never deletes revoked rows in any of
  the three adapters (port-contract parity).
- **Migration** — `2026-10-08-000064_user_sessions`, sequence 000064 is the next unused;
  portable SQL (both-backend portability gate passes).
- **Sessions created on auth, throttled `last_active_at`** — minted in the shared HTTP
  extractor and the WS/TTY `authenticate_token` path, one row per (credential, IP,
  User-Agent); touch throttled to ≥60s and also slides `expires_at` (daily-active device
  never crosses its expiry wall); expired-not-revoked rows re-mint rather than zombify.
  IP comes from the `ConnectInfo` socket peer only — no forwarded-header trust (CWE-348).
- **GET/DELETE/POST endpoints** — all three `/users/me/sessions...` routes live, plus
  TenantAdmin variants with per-handler Admin-role checks and tenant containment
  (`ensure_same_tenant_session_target`, fail-closed on missing tenant binding); listings
  are active-only and never expose `token_hash`; cross-user session ids 404.
- **Revoked sessions rejected in auth middleware** — revoked device session → 401 on both
  HTTP and WS paths; `credential_revoked` additionally rejects a key whose every session
  is revoked, covering devices never seen before ("sign out everywhere").
- **Routes registered with ABAC mappings** — six resolver entries (exempt, with
  per-handler authorization validated by `check-abac-exempt-handlers.sh`); route-registry
  gate passes.
- **Background cleanup** — daily `spawn_session_cleanup` deletes only unrevoked sessions
  expired >30 days; revoked rows are retained as the durable sign-out record.
- **Tests are real, not inflated** — 16 server session tests drive the real router with
  real API keys through the real extractor (per-device vs sign-out-everywhere, listing
  filters, cross-user 404, TenantAdmin allow/deny, cross-tenant deny, WS path, throttle
  both directions, re-mint, signout-survives-retention); each carries a positive control
  and mutation-resistant assertions (the doc comment names the production change that
  fails the test). 9 adapter tests cover the port contracts including the
  retention-never-deletes-revoked invariant.
- **`cargo test --all`** — deferred to the independent verifier per stage protocol
  (in-sandbox 4 GiB build-wrapper limit on gyre-server test codegen); compilation of the
  full lib test target independently verified via `cargo check -p gyre-server --lib
  --tests` (exit 0).

Verification-repair scope (stricter, not weaker — accepted): `check-scope-literal-defaults.sh`
now scans `crates/` when invoked with no args (previously vacuous) and its frozen exemption
count was lowered 6→0 with the last four analytics tenant-scope sites fixed for real;
`mcp.rs` create_mr scope resolution switched from fail-open to fail-closed.

Non-blocking observations (recorded in the evidence summary, no action required):
session-store storage errors fail-open with a logged warning (documented tradeoff — the
credential was independently validated against `api_keys`); coverage-row prose test counts
drift by one (17/8 vs actual 16/9); "sign out everywhere" is per-credential, so a second
API key of the same user that never minted a session still authenticates — consistent with
the documented per-(credential, device) model and the literal acceptance criterion.

Required verifier commands:
1. `cargo test --all`
2. `cargo test -p gyre-server --lib` (covers the repaired merge_processor tests and the
   task-111 session modules; `--lib sessions` / `--lib session` as focused filters)
3. `cargo test -p gyre-server --test git_integration` and `--test spec_enforcement`
