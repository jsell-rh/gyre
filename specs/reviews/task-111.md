# Review — task-111 (user-management.md §Session Management)

Spec: `specs/system/user-management.md` §Session Management (lines 276-296).
Candidate under review: `c1bf489af47fd3c132cfba92e1e035db77188e99` (pipeline
checkpoint of the task-111 branch, base `9bef21a41729a6b696b48a587b1f2e619d0f6ff2`).
Verdict: **needs-revision** (1 finding).

Probes run (evidence in `/tmp/stage/review-evidence/`):

- `bash scripts/check-migration-versions.sh` → **FAIL, exit 1** (duplicate
  version 20261008000056) — the finding below. Passes on the assigned base, so
  it is candidate-introduced.
- All other mechanical checks pass on the candidate: arch, ABAC route registry,
  ABAC exempt handlers (94 checked), migration SQL portability, scope-literal
  defaults, fabricated-scope defaults, unwritten store fields, mem port
  contracts, in-memory state stores, inert enforcement, lossy secret
  conversion, forwarded-header trust, forged scope fields, warn-continue
  creation, api-auth, mirrored-logic tests, conditional test guards,
  assertionless tests (`mechanical-checks.txt`).
- Rust build/test probes **blocked by sandbox infrastructure**: no route to
  index.crates.io (connect error 7), no populated cargo registry cache, no
  prebuilt target dir. Not a code defect. Deferred verifier checks and the
  static test review are recorded in `build-probe-blocked.txt`.

Verified working (no findings):

- **Domain/port/adapters**: `UserSession` with all spec fields
  (`gyre-domain/src/user_profile.rs`); `SessionRepository` port
  (`gyre-ports/src/user_profile.rs`) with the plan's full method set; SQLite,
  PostgreSQL, and mem implementations all enforce the port contracts
  (owner-scoped revoke, newest-match on device lookup, revoked rows never
  deleted by retention — the durable "sign out everywhere" record). Migration
  SQL is portable (check passes); diesel schema registered.
- **Auth integration** (`auth.rs`): sessions minted per (credential, IP from
  `ConnectInfo` socket peer — no forwarded-header trust, user_agent) on
  successful API-key auth; `last_active_at` + sliding `expires_at` refreshed
  at most once per 60 s (throttle); a revoked session for the device rejects
  auth with 401; `credential_revoked` implements "sign out everywhere"
  including never-seen devices; expired-but-unrevoked sessions re-mint exactly
  once. The WS ticket path enforces the same rule. Storage failures fail soft
  with a warn log (documented; the credential itself was validated against
  `api_keys`).
- **API** (`api/users.rs`): all three spec endpoints plus TenantAdmin variants;
  per-handler authorization on the ABAC-exempt routes (caller-scoped `/me`
  with 404 on foreign ids; Admin role + target-user existence checks on the
  admin variants); `token_hash` never exposed in responses. Routes registered
  in `api/mod.rs` with ABAC registry entries.
- **Background cleanup**: `spawn_session_cleanup` (daily, wired in `main.rs`)
  deletes only unrevoked sessions expired > 30 days.
- **Verifier/exemption changes** are genuine fixes, not check-weakening: the
  `check-scope-literal-defaults.sh` no-arg default makes the check scan
  `crates/` instead of scanning zero files vacuously; the 11 removed
  exemption entries correspond to sites now naming `DEFAULT_TENANT_ID`, a real
  tenant materialized at startup by base's task-197 (`bootstrap_default_tenant`,
  `main.rs:48`) — not a fabricated scope; the rewritten
  `seed_builtin_policies` binds its result (passes inert-enforcement) and
  carries the documented inline per-line marker.
- **Tests (static review; execution deferred — see build-probe-blocked.txt)**:
  server session tests drive the real `api_router()` via oneshot with the real
  auth extractor and assert both directions (revoked → 401 / sibling device →
  200, foreign id → 404, non-admin → 403, unknown user → 404, throttle window,
  re-mint exactly-once, sign-out survives retention); adapter tests cover
  roundtrip, duplicate-id, four-field device match, TTL slide, scoped revoke,
  revoke-all scope, retention-keeps-revoked. No mirrored-logic or
  conditional-guard patterns (checks pass).

Findings:

- [-] **F1 (major, blocking): duplicate Diesel migration version 000056.** The
  candidate adds `crates/gyre-adapters/migrations/2026-10-08-000056_user_sessions/`
  while the assigned base already contains
  `2026-10-08-000056_spec_assertion_results/`. `scripts/check-migration-versions.sh`
  fails (exit 1) → pre-commit + CI red. Per the repo invariant, only one
  migration per version runs — one of the two 000056 tables is silently never
  created on real SQLite/PostgreSQL deployments. If `user_sessions` is the
  dropped one, the feature is silently hollow on persistent storage: every
  `SessionRepository` call errors and the fail-soft auth paths log-and-allow,
  so revocation would have no effect in production while in-process tests
  (mem adapter / whichever migration won locally) stay green — exactly the
  failure class the check exists to prevent. The task's Agent Instructions
  say "Check migration numbering", and the Shipped notes' claims "next unused
  sequence number" and "All mechanical checks pass: ... migration versions"
  are false for this candidate: they held on the pre-rebase base `f4acb4eb`
  but the rebase onto `9bef21a4` (which already consumed 000056) invalidated
  them without re-checking. Remedy: renumber to
  `2026-10-08-000057_user_sessions` (next unused), then re-run the check and
  the deferred probes (adapter + server session suites, and a
  fresh-`SqliteStorage` round-trip proving the table is created).

Minor (not blocking, recorded for completeness):

- Coverage matrix row 12 (`specs/coverage/system/user-management.md`) is still
  `task-assigned` — appropriate to flip when the task completes, not a defect
  of this candidate.
- Sessions are tracked only for API-key auth (JWT/global-token identities get
  no session rows; `/me/sessions` returns an empty list for them). The spec's
  `token_hash` field implies bearer-token sessions, and the plan's "on
  successful auth" is ambiguous; this is a defensible reading, noted so the
  verifier can confirm the interpretation with the spec owner if desired.
