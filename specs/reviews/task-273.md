# Review — task-273 (Admin Panel completion: SOPS/Vault backends, durable job history, maintenance jobs, token refresh, server logs, admin audit)

- **Candidate:** `0ec8c590293ba389c57fda277bec77775ed720ab` (base `e1cf9d8d22352f8434addbb32b77d27b56b883db`)
- **Task commits:** all four (`db320101`, `ee253c88`, `b101d27a`, `f6e31cf5`) verified as ancestors of the candidate.
- **Verdict:** ❌ **not approved** (see `/tmp/stage/verdict.json`; evidence in `/tmp/stage/review-evidence/task-273-r8/`)

## What was verified as real (independent evidence)

- **SOPS backend** (`gyre-adapters/src/sops/`): genuine age X25519 data-key wrap
  via the reference `age` crate, AES-256-GCM with sops's 32-byte IVs and path
  AAD, fail-closed constant-time MAC verification. The value cipher is proven
  byte-compatible with sops's own Go test-suite vector. SQLite adapter: SOPS
  default, identity from env/keyfile/auto-generated-persisted, legacy AES-GCM
  rows re-encrypted on first access (tested). `cargo test -p gyre-adapters
  --lib -- sops:: vault:: sqlite::secret` → **40 passed, 0 failed**.
- **Vault backend**: wire-level axum stub speaking KV v2 + AppRole/JWT login;
  30 s bounded client, structurally no DB fallback (fail-closed tested with a
  dead listener), agent resolution authenticates with the agent's JWT and the
  server token never appears on agent-scoped reads (asserted), base64 value
  transport with no lossy UTF-8 conversion, tenant-scoped path model with
  cross-tenant id lookups 404ing.
- **Durable job history**: `JobRunRepository` (SQLite replace-into / PG
  on-conflict-upsert / mem retain-replace — consistent semantics),
  migrations 000067–000070 are the next unused numbers and portable to both
  backends, `store!` wiring, durable store authoritative for GET
  /admin/jobs with logs + derived duration, run-scoped `JobRunLog` sink,
  newest-first on both the durable and fallback paths.
- **Maintenance jobs**: all four registered and scheduled; `repo_maintenance`
  runs real `git gc --prune=now` with a 10-minute per-repo bound and failure
  isolation; `oauth_state_cleanup` deletes only expired rows;
  `remote_agent_cleanup` kills via `ComputeTarget::kill_process` and marks
  terminated (restart orphans reconstructed from the durable descriptor;
  unreconstructable SSH backends honestly reported, not silently skipped);
  `job_history_cleanup` prunes finished rows only, retention configurable.
- **Token refresh**: shape-based dispatch (GCP SA first, then OAuth doc);
  the SA handler performs a real RS256 JWT-bearer grant (ring over the SA's
  PKCS#8 key) and preserves the full Google-issued document on refresh;
  per-secret outcomes (refreshed / no-refresh-mechanism / failed) land in the
  durable run logs; no-refresh secrets still emit SecretExpiring.
- **Server logs / admin audit / frontend**: real tracing `ServerLogLayer`
  ring buffer installed alongside existing layers; Admin-only
  /admin/server/logs; `admin_audit` now queries the durable repository with
  server-derived tenant scoping and SQL-enforced `q` filtering (portable
  lower()+escaped LIKE) including zstd-compressed detail matching in both
  adapters; TenantSettings gained Server Logs, searchable Audit, and
  Jobs-with-logs tabs with substantive DOM-level tests.
- **Mechanical gates**: all 10 run, exit 0, `git diff base..HEAD -- scripts/`
  empty (no exemption growth). Commit attribution complete.

## Round 8 — blocking findings

1. **OAuth callback unreachable in deployment.** `/api/v1/oauth/claude-max/callback`
   is registered inside `api_router()` behind `require_auth_middleware`
   (only `/api/v1/version` is public). The provider's redirect carries no
   Bearer token → 401 before the handler runs; the ABAC exemption is
   ineffective because `require_auth` is the outermost layer. The unit tests
   use a bare router so CI stays green while the deployed flow is broken.
   Fix class: mount on the outer public Router like
   `/api/v1/invite/:token/accept` (lib.rs:1023).
2. **Failing test / wrong redirect semantics.** `enroll_claude_max` returns
   `Redirect::temporary` (307) but the test asserts `StatusCode::FOUND`
   (302) — `cargo test -p gyre-server --lib -- api::oauth` fails. 307 also
   preserves the POST method, which is wrong for redirecting to a GET
   authorization endpoint.

## Non-blocking observations (recorded for the verifier)

- Tenant filter unions `tenant_id IS NULL`, making unattributed audit events
  visible to every tenant admin — deliberate and tested, but contradicts the
  migration-000070 comment and the `AuditEvent.tenant_id` domain doc.
- SOPS MAC hashes value bytes only; sops's own tree-walk MAC includes key
  paths, so `sops -d` on a Gyre-stored document will likely report MAC
  mismatch. The acceptance criterion's alternative (native age decryption of
  data key + payload) is satisfied and proven; the module doc's
  "decryptable with the sops CLI" claim should be verified with the real CLI
  or amended.
- `JobRegistry::durable_history` swallows repository errors
  (`.unwrap_or_default()`), degrading /admin/jobs to the in-memory cache
  without signal.
- `mark_consumed` is not a compare-and-swap: two concurrent callback replays
  can both pass the `consumed` check (bounded by the provider's single-use
  code).
- Vault `create` is check-then-write without KV v2 `cas`; a crash between the
  data and index writes orphans a data path in Vault.
- Vault token retry-on-401/403 covers writes/index reads but not
  `get_value`/`find_by_id`/`list_by_scope`/`rotate` (stale cached AppRole
  token fails those until another op re-logs-in).
- Spawn continues without `GYRE_CRED_*` when secret resolution errors
  (pre-existing posture; the repository itself fails closed).

## Verifier must run

- `cargo test -p gyre-server --lib -- api::oauth` — **expected to fail**
  (finding 2).
- `cargo test -p gyre-server --lib -- secret_rotation:: maintenance_jobs::
  api::admin::tests::admin_jobs telemetry::`
- Restart probes (job history durability, admin audit durability) and the
  real `sops` CLI decryptability probe per the task's verification intent.
