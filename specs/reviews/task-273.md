# Review — task-273 (Admin Panel completion: SOPS/Vault secret backends, durable job history, maintenance jobs, token refresh, server logs, durable admin audit)

Spec: `specs/system/admin-panel.md` (gap spec-5b647653a4554eea3e93ac19).
Commits under review: `1bc00b6c`, `a2b31ac1`, `f6e31cf5`, `b101d27a`, `ee253c88`, `db320101` (+ merge `bce07ac1`, docs `5ff6596d`).
Candidate: `5ff6596d0b88ce3b8957ab2321d09dadc5326778` (base `b2523a47`).
Verdict: **needs-revision** — five verified blocking code defects (F1-F5), one blocking test gap (F6), plus a SOPS CLI-compatibility verification gap (F7) and seven non-blocking findings (F8-F14). The overall shape is real: no fakery found, all mechanical gates pass with no exemption growth, and the adapter-level implementations are genuine and well-tested — but the blocking items break the task contract's own named invariants.

## Round 1 — probes run (main checkout at candidate HEAD)

Mechanical gates (all PASS, outputs in `/tmp/stage/review-evidence/`):
`check-arch`, `check-abac-route-registry`, `check-migration-versions`,
`check-migration-sql-portability`, `check-mem-port-contracts`,
`check-in-memory-state-stores`, `check-unbounded-external-http`,
`check-tenant-filter`. Exemption files: byte-identical to base (all 33).

Focused tests (independent re-runs):
- `cargo test -p gyre-adapters --lib` → **637 passed, 0 failed** (sops
  cross-implementation vector incl. sops's own Go cipher vector; vault KV
  v2 / agent-JWT auth / fail-closed; job_run / oauth_state / agent_instance
  adapter restart-durability; audit tenant + q filters incl. zstd rows).
- `cd web && npm ci && npm test` → **127 files, 2571 passed, 41 skipped**
  (incl. the new TenantSettings server-logs and audit-search tab tests).

Server `--lib` suites and any live Vault/sops-CLI probe were NOT run here:
server test builds exceed the model sandbox's 4 GiB budget and the sops CLI
is not installed — recorded as verifier checks below.

## Verified working (no findings)

- **Durable job history core**: `JobRunRepository` port + SQLite/PG/mem
  adapters with portable migrations (000067-000070, next unused numbers);
  `record_run` persists through the store on every path (manual trigger,
  `spawn_job` scheduler, `record_cycle`); logs are drained from the
  run-scoped sink after the handler returns and before `record_run`; GET
  `/admin/jobs` reads `durable_history` (logs + derived duration);
  `job_history_cleanup` prunes only finished rows (`finished_at < cutoff`
  never matches NULL), retention via `GYRE_JOB_HISTORY_RETENTION_DAYS`.
- **repo_maintenance**: real `git gc --prune=now` per repo through the git
  CLI, 600 s per-repo bound, per-repo failure isolation, per-repo outcomes
  in run logs; test proves object reclamation via `count-objects` and
  corrupted-repo isolation.
- **remote_agent_cleanup core**: really calls `ComputeTarget::kill_process`;
  verifies Dead/Stopped/absent before killing; leaves live agents
  untouched (tested with a real `sleep` process); reconstructs
  container/kubernetes/local backends for restart orphans; SSH orphans
  honestly logged NOT REAPED.
- **Token refresh**: shape-based dispatch (OAuth doc by
  token_endpoint/client_id/refresh_token regardless of name — test uses
  `PROD_API_CREDENTIALS`; GCP SA by type=service_account) with no
  cross-match possible; real RS256 JWT-bearer grant via
  `ring::signature::RsaKeyPair::from_pkcs8` + `RSA_PKCS1_SHA256`, and the
  test stub **verifies the signature** against the SPKI public half;
  RFC 6749 rotation re-stores a rotated refresh_token; per-secret outcomes
  (refreshed / no-refresh-mechanism / failed) land in the durable run logs;
  both handlers reuse the shared 30 s-timeout `state.http_client`.
- **Server logs layer**: real `tracing_subscriber::Layer` with `on_event`
  and a str+debug `Visit`; installed alongside unchanged stdout/JSON/OTel
  layers; bound enforced (cap + pop_front); formatting outside the lock.
- **Admin audit durability**: `/admin/audit` now queries the durable audit
  repository, always tenant-scoped (`tenant_id = ?` bare equality, never a
  NULL-union — tested); `tenant_id` derived server-side at emit and at
  adapter `record` (workspace → tenant, user → tenant), no client-supplied
  path (the POST body has no tenant_id field); migration 000070 backfills
  from workspaces portably; `q` filter enforced in SQL by both adapters
  with LIKE escaping, ASCII-lowercase on both sides, zstd rows decompressed
  and matched in-adapter, filtering applied before limit with paging.
- **Vault core**: KV v2 data/metadata path handling correct; agent
  resolution logs in with the agent's JWT and uses the returned client
  token for both lists and reads; agents structurally never receive Vault
  tokens; single 30 s-timeout client; fail-closed without DB fallback
  (structurally impossible in the adapter); base64 value transport, zero
  `from_utf8_lossy` on secret material; `build_state` swaps the repository
  on `GYRE_SECRET_BACKEND=vault` and panics on misconfiguration.
- **OAuth crypto**: CSPRNG 256-bit state; PKCE S256 correct
  (BASE64URL_NOPAD(SHA256(verifier))); consume-before-exchange ordering;
  server-derived admin/tenant scope with cross-tenant workspace rejection
  (tested); callback reachable through the full middleware stack (tested);
  hourly `oauth_state_cleanup` deletes only expired rows (tested);
  enroll/callback ABAC-registered with no exemption growth.

## Findings

### Blocking

- [F1] **Vault path traversal via unvalidated secret name** —
  `crates/gyre-adapters/src/vault/mod.rs:293-298` interpolates the
  caller-supplied `name` raw into the KV path; `api/secrets.rs:129-134`
  validates only trim + length. reqwest/url collapse `../` segments, so a
  tenant admin can write outside their tenant subtree in Vault
  (cross-tenant credential injection when the server token's policy is
  broader than one tenant subtree); a `/` in a name also silently breaks
  listing. Fix: reject non-path-safe names in the adapter (or
  URL-encode/percent-encode the component).
- [F2] **OAuth state consumption is a read-then-write race** — the
  contract explicitly requires atomic consume via
  `UPDATE ... WHERE consumed=0`; `mark_consumed`
  (sqlite/pg/mem `oauth_state.rs`) is an unconditional UPDATE and the
  handler checks `consumed` in Rust first. Two concurrent callbacks with
  the same state both proceed. Fix: conditional update in all three
  adapters + port contract "fails if already consumed", map to the
  replay-rejection 400.
- [F3] **Relative default `redirect_uri`** — `api/oauth.rs:85` defaults to
  `/api/v1/oauth/claude-max/callback`; RFC 6749 §3.1.2 requires an absolute
  URI, so the documented minimal config produces a flow that can never
  complete, silently (all tests set the env var). Same failure class as the
  relative-path-defaults invariant. Fix: require an absolute
  `GYRE_CLAUDE_MAX_REDIRECT_URI`.
- [F4] **Orchestrator spawns bypass the durable agent-instance store** —
  `orchestrator.rs:183` `launch_orchestrator_process` inserts into
  `process_registry` only (zero `agent_instances` references), so restart-
  orphaned orchestrator containers are invisible to `remote_agent_cleanup`
  forever. Deliverable (6) requires recording at spawn in **all** spawn
  paths.
- [F5] **Admin kill marks terminated without killing after a restart** —
  `admin.rs:478-508` attempts the kill only when an in-memory
  `process_registry` entry exists but calls
  `agent_instances.mark_terminated(&id, "admin_kill")` unconditionally;
  post-restart this permanently hides a still-running orphan from the
  sweep (which only lists running records). Fix: reconstruct the backend
  from the durable record when the fast path is empty (the sweep already
  does this), or don't mark terminated when no kill was attempted.
- [F6] **No HTTP-level test for `GET /api/v1/admin/server/logs`** — the AC
  explicitly requires non-Admin 403 and a retrievable log line; zero tests
  hit the route (only buffer unit tests exist), and the handler's
  level/since/limit handling is untested.
- [F7] **SOPS document-level MAC lacks sops-CLI ground truth** (verification
  gap): value layer proven against sops's own Go cipher vector and the age
  wrap uses the reference `age` crate, but the MAC construction
  (SHA-512 over values only; AAD = raw `lastmodified` string) rests on a
  Node-reimplementation vector with no CLI-generated fixture, and the
  fractional-second `lastmodified` hazard (Go marshals RFC3339Nano; sops's
  own MAC AAD uses second-precision RFC3339) would make `sops -d` fail on
  every document while in-repo tests stay green. The sops CLI is not
  available in the review sandbox — **the verifier must run both
  directions** (`sops -d` a stored document with the age identity;
  `sops_decrypt` a CLI-encrypted document). Canonicalizing `lastmodified`
  to second-precision RFC3339 before MAC AAD use removes the hazard.

### Non-blocking (recorded for the revision round)

- [F8] Vault `X-Vault-Namespace` dropped on index reads and both deletes
  (`with_auth(..., None)` at mod.rs:352/579/595): read fails open
  (`get_value` → `Ok(None)`), delete treats 404 as success.
- [F9] Vault AppRole TTL permanently breaks `get_value`/`list_by_scope`/
  `rotate`/`resolve_for_agent` (raw `server_token()` without the retry
  wrapper at mod.rs:421/550/614/647).
- [F10] OAuth re-enrollment rotate path discards the new `expires_at`
  (port `rotate` doesn't update expiry), leaving a just-re-enrolled
  credential invisible to `resolve_for_agent` until the next rotation
  cycle; the rotation job's delete+create pattern should be matched.
- [F11] Durable wiring untested: no test attaches a store through
  `set_durable_store` or exercises `/admin/jobs` against DB rows —
  deleting the durable `record`/`durable_history`/`record_spawned` calls
  would leave every test green; `durable_history` also swallows DB read
  errors (`.unwrap_or_default()`, no log).
- [F12] `GYRE_SERVER_LOG_BUFFER_SIZE=0` disables the ring-buffer bound
  (`len == capacity` never true again after first push); lowercase/invalid
  `?level=` silently no-ops the filter.
- [F13] `secret_rotation` scope-list failures silently skip secrets with a
  green run (warn! only, no run-log line, no notifications — violates "no
  expiring credential is silently skipped"); `attempt_rotation`'s
  delete+create can permanently lose the credential if create fails.
- [F14] mem audit adapter skips the tenant derivation the SQL twins
  perform at `record` — mem-mode server events vanish from the admin audit
  tab; de facto port contract implemented by 2 of 3 adapters.
- (notes) Test-hygiene: several sqlite secret tests mutate env without
  `ENV_LOCK` (flake hazard vs the legacy-backend test); the SQLite
  "tampering" test decrypts with a wrong identity so the ciphertext flip
  is not what fails (duplicate of the wrong-identity test; the genuine
  tamper test lives in sops/tests.rs). Agent-only audit events
  (AuthFailure, agent-subject AbacDenied) land NULL-tenant and have no
  admin-visible surface in the tenant-scoped viewer. `/admin/audit`
  `count` is the page length, not the filtered total.

## Checks the independent verifier must run

1. `cargo test -p gyre-server --lib -- secret_rotation:: maintenance_jobs::
   api::admin::tests::admin_jobs api::admin::tests::admin_audit api::oauth::
   telemetry::jobs` and the merge-touched integration tests (e2e_ralph_loop,
   git_integration, graph_integration, spec_enforcement); `cd web && npm test`.
2. sops CLI probe (F7), both directions, with a real age identity.
3. HTTP-level server-logs tests (F6): non-Admin 403 through the full
   middleware stack + captured-line retrieval with filters.
4. After the F2 fix: a test asserting the conditional-update semantics.

Test-inflation check: the new adapter/handler tests are largely
load-bearing (signature-verifying GCP stub, shape-dispatch under a
non-suffix name, count-objects reclamation, real process kills, replayed/
expired-state rejection, zstd q-match, tenant exclusion). The gaps are
absence gaps (F6, F11), not self-confirming suites.
