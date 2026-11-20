# Review — task-273 (Admin Panel completion: SOPS/Vault secret backends, durable job history with logs, maintenance jobs, complete token refresh, server-log viewer, durable searchable admin audit)

Spec: `specs/system/admin-panel.md` (all previously-unimplemented bullets), cross-reference
`specs/system/platform-model.md` §7 Storage Backend.
Candidate under review: `5c71eecd13ad0525a26107fc6d54ea8b29fe36be` (branch
`pipeline/task-273/69d233b1f1e149c4a29950140651d01e-1`), assigned base
`e9f6a31c3903ff61597af88d0b70b41d94bb911a`. Product commits: `db320101`,
`ee253c88`, `b101d27a`, `f6e31cf5`, `a2b31ac1`, `1bc00b6c` (all recorded in the
task's `commits:` frontmatter at the candidate — the working-tree copy was reset
by the controller to `progress: not-started` / `commits: []`, which makes
`check-task-commit-attribution.sh` fail against the working tree only; the
committed candidate is compliant).

Verdict: **approved** — all eight deliverables are real production
implementations; every runnable probe passes; no fakery found. One
environment-restricted probe (sops-CLI decryptability) is recorded below as a
mandatory verifier check, per the sandbox having no `sops` binary and no
network.

## Probes run (evidence under /tmp/stage/review-evidence/)

Mechanical invariants (all exit 0, no new exemption entries — `git diff
base..candidate -- 'scripts/*exemption*'` is empty):

- `check-arch.sh`, `check-migration-versions.sh`,
  `check-migration-sql-portability.sh`, `check-mem-port-contracts.sh`,
  `check-in-memory-state-stores.sh`, `check-abac-route-registry.sh`,
  `check-unbounded-external-http.sh`, `check-forwarded-header-trust.sh`,
  `check-forged-scope-fields.sh`, `check-lossy-secret-conversion.sh`,
  `check-byte-slice-truncation.sh`, `check-relative-path-defaults.sh`,
  `check-fail-open-ref-resolution.sh`, `check-fabricated-scope-defaults.sh`,
  `check-scope-literal-defaults.sh`, `check-inert-enforcement.sh`,
  `check-dead-message-kinds.sh`, `check-tenant-filter.sh`,
  `check-abac-exempt-handlers.sh`, `check-mcp-write-tools.sh`.

Focused builds/tests:

- `cargo test -p gyre-adapters --lib` → **635 passed, 0 failed, 6 ignored**
  (includes: sops format suite incl. the claimed sops v3.9.4 Go-suite AES-GCM
  vector and the wrong-AAD rejection test; the full Vault suite — KV v2 CRUD
  against a real stub HTTP server with token validation, agent-JWT login
  observation, closed-port fail-closed, duplicate-contract, binary round-trip;
  job_run restart durability (`history_survives_storage_rebuild_restart_durability`);
  oauth_state duplicate/consume/expired-delete; agent_instance; audit
  tenant-scoping, LIKE-wildcard escaping, and zstd-compressed free-text `q`
  match).
- `cargo check -p gyre-server` → clean (5m24s; large server test compilation
  deliberately left to the verifier per the 4 GiB budget rule).
- `cd web && npm ci && npx vitest run src/__tests__/TenantSettings.test.js` →
  **58/58 passed** (Server Logs tab rendering with level/target/message, Audit
  search wiring asserting `api.adminAudit` receives `q`, Jobs run-history
  expansion with duration/logs/failure).

Personal source verification (beyond five parallel deep-dive reviews):

- `admin_audit` (`api/admin.rs`): queries `state.audit.query(&filter)` (durable
  repository) with `tenant_id: Some(auth.tenant_id.clone())` — the `AuditQuery`
  struct has no tenant field at all, so tenant scope is structurally
  server-derived. `telemetry_buffer` is gone from the handler.
- `admin_server_logs` + `RouteResourceMapping::api("/api/v1/admin/server/logs",
  "admin", None)` (abac_middleware.rs:456) — admin-resource class, rejected for
  non-Admin roles by the `builtin-admin-only-deny` policy; no exemption-file
  entry.
- OAuth callback: `RouteResourceMapping::exempt` + the `require_auth_middleware`
  bypass, with the single-use expiring state row as the per-handler capability
  (validated: unknown/replayed/expired rejected; consumed before the network
  exchange), pinned by a full-`build_router` regression test
  (`callback_reachable_through_full_middleware_stack_without_bearer`).
- `repo_maintenance` (`maintenance_jobs.rs`): real `git gc --prune=now --quiet`
  per stored repo, 10-minute per-repo timeout, per-repo failure isolation with
  outcomes in the run-scoped `JobRunLog`.
- Spawn fail-open posture (`secret resolution failed; continuing without
  GYRE_CRED_*`) confirmed **pre-existing at the base** (`git show
  e9f6a31c:.../spawn.rs` line 2216) — not a task-273 regression.
- Migrations `000067_job_runs` / `000068_oauth_states` / `000069_agent_instances`
  / `000070_audit_events_tenant_id`: next unused sequence numbers, plain
  portable SQL (no `INSERT OR IGNORE` / JSON1 / `jsonb_*` / `::cast`), same
  embedded dir for both backends.

## Verified working (per deliverable)

1. **SOPS default** — genuine age X25519 (reference `age` crate, bech32
   round-trip, armored `enc` decryptable by the reference implementation),
   AES-256-GCM with sops's 32-byte IVs, `ENC[AES256_GCM,...]` field format,
   SHA-512 value MAC (key paths excluded — sops's documented quirk) sealed
   under the RFC3339 `lastmodified` AAD; `GYRE_SECRET_BACKEND` default `sops`
   in both DB adapters; identity from `GYRE_SOPS_AGE_KEY` / key file /
   auto-generated persisted in kv_store; legacy AES-256-GCM rows re-encrypted
   on first access (get_value / resolve_for_agent, tested); fail-closed
   decrypt errors; binary values never through `from_utf8_lossy`.
2. **Vault option** — whole `SecretRepository` port served from KV v2
   (data/metadata split, version-per-write, LIST semantics, metadata delete);
   `resolve_for_agent_as` authenticates with the **agent's own worker-scoped
   JWT** via `/v1/auth/{mount}/login` so Vault policies scope reads
   (platform-model §7), server token confined to the admin path (AppRole with
   re-login-on-rejection); one shared 30 s-timeout reqwest client for every
   request; structurally no DB fallback (fail-closed test against a closed
   port; startup panics on misconfiguration); no Vault credential ever crosses
   the container boundary.
3. **Job history** — `gyre_ports::JobRunRepository` (record/list_by_job/
   list_recent/delete_older_than) with SQLite + PostgreSQL + mem adapters
   (mem enforces port constraints in code); run-scoped `JobRunLog` sink handed
   to every handler, persisted in the `job_runs.logs` column;
   `record_run` is the single funnel for manual triggers, scheduler cycles and
   `record_cycle` (success and failure); GET /admin/jobs reads durable-first
   (in-memory ring is a fronting cache) and serves logs + derived duration;
   `job_history_cleanup` registered + scheduled daily 03:00 UTC, retention via
   `GYRE_JOB_HISTORY_RETENTION_DAYS`, deletes only finished rows (tested in
   all three adapters).
4. **Repo maintenance** — real `git gc --prune=now` per repo (see above).
5. **OAuth state** — durable `OAuthStateRepository` + portable migration;
   Admin-only enroll (`require_admin` in-handler on top of ABAC secret:write)
   with state + PKCE S256 from CSPRNG, tenant/admin derived server-side,
   cross-tenant workspace rejected; callback validates and single-use-consumes
   the state, exchanges the code with the verifier at the persisted token
   endpoint over the shared 30 s-timeout client, stores the full credential
   document as the scoped `claude_max_refresh_token`; `oauth_state_cleanup`
   registered + scheduled hourly, deletes only expired rows (tested).
6. **Token refresh** — dispatch is credential-document **shape**-based
   (`parses_as_oauth_doc` / `type == "service_account"`), not name suffix;
   every expiring secret gets a per-secret outcome line in the durable run log
   (refreshed / no-refresh-mechanism / failed: reason) plus the existing
   SecretExpiring notification; the GCP SA handler performs a real
   RS256 JWT-bearer grant (ring `RsaKeyPair::sign` over the PKCS#8 key,
   stub-endpoint test cryptographically verifies the signature and rejects a
   forged one) and re-stores the document with Google-issued fields preserved
   verbatim.
7. **Server logs** — genuine `tracing_subscriber::Layer` capture into a
   bounded ring buffer (capacity-capped, eviction tested), installed alongside
   the unchanged stdout/JSON/OTel layers in every subscriber branch;
   Admin-only GET /admin/server/logs with level/since/limit; Server Logs tab
   in TenantSettings (tested).
8. **Admin audit** — durable rewrite per above; `AuditEvent.tenant_id`
   derived server-side at emit (HTTP ingest from the auth extractor;
   server-emitted rows derived in both SQL adapters from workspace→tenant then
   user→tenant, NULL kept NULL — no fabricated scope); migration 000070
   backfills from workspace scope; `tenant_id` + free-text `q` enforced in SQL
   by BOTH adapters (parameterized `lower(detail) LIKE` with escaped
   wildcards; zstd-compressed rows pass the SQL prefilter and are decompressed
   and matched in the adapter — tested); TenantSettings Audit tab renders the
   durable events with the same filter set including search.

The three new `check-tenant-filter.sh` SKIP_LIST entries were each verified
against the actual callers: they cover only deployment-wide non-row-serving
reads (cross-tenant COUNT/GROUP BY aggregates behind ABAC, the SIEM/search
cross-tenant stream), the pre-auth single-use OAuth state capability, and the
user-keyed judgment ledger — no cross-tenant row leak; the row-serving audit
reads apply the tenant filter lexically in their own bodies and are pinned by
behavioral tests.

## False positive resolved during review

An earlier pass flagged `vault/tests.rs` `crud_is_served_from_vault_kv_v2` as
using a mangled `"secr..."` store key that could never match (panic). This was
an artifact of **output redaction in the review tooling**: `od -c` on the same
source shows the file contains the full key
`secret/data/gyre-secrets/t1/repo/repo-1/DATABASE_URL`, which matches the
stub's inserted keys. The test passes (verified with
`cargo test ... -- --exact --nocapture`). No defect.

## Verifier checks required (environment-restricted here)

1. **sops-CLI decryptability (the one probe this sandbox cannot run — no
   `sops` binary, no network).** Create a secret with `GYRE_SECRET_BACKEND`
   unset, export the stored value + the age identity
   (`GYRE_SOPS_AGE_KEY` / the kv_store-persisted auto identity), and run
   `sops -d`. The in-repo evidence (claimed genuine sops v3.9.4
   `aes/cipher_test.go` vector passing under AAD `bar:`, reference-age armor,
   32-byte IVs, value-only SHA-512 MAC) supports compatibility, but the MAC's
   `lastmodified` AAD and the exact value-AAD bytes have no
   genuinely-independent in-repo ground truth (the "independent
   implementation" vector was generated by the task author from the same
   reading of the sops source). If `sops -d` fails, the SOPS backend is
   sops-*shaped* but not sops-compatible and the row-2 coverage claim fails.
2. Full workspace `cargo test --all` (gyre-server test modules in
   `maintenance_jobs.rs`, `api/oauth.rs`, `secret_rotation.rs`, `api/admin.rs`,
   `telemetry.rs`, `jobs.rs` were not compiled here — 4 GiB budget rule).
3. PostgreSQL adapter behavior (repo pins PG SQL shapes but has no live PG
   harness): migrations 000067–000070 + job_run/oauth_state/agent_instance/audit
   against a real PG.
4. Full web suite + e2e; `web/dist` regeneration (verifier-owned).
5. `check-task-commit-attribution.sh` against the committed task file (the
   working-tree reset by the controller makes it fail locally; the candidate
   commit lists all six task-labeled commits).
6. A live Vault dev-server run of the KV v2 + JWT-auth flows (the stub server
   is faithful but is not Vault).

## Minor findings (non-blocking, recorded for follow-up)

- `api/oauth.rs`: default `redirect_uri` is the path-only
  `/api/v1/oauth/claude-max/callback` when `GYRE_CLAUDE_MAX_REDIRECT_URI` is
  unset — an absolute URI is required by RFC 6749; derive from `base_url` or
  fail closed like the missing-client-id case.
- `oauth_states.provider_config` persists the `client_secret` in plaintext
  (short-TTL rows, hourly cleanup) — consider at-rest encryption.
- State consumption is check-then-mark (TOCTOU); an atomic
  `UPDATE ... WHERE consumed = 0` would close it (practically mitigated by
  provider-side code single-use).
- `remote_agent_cleanup` cannot kill SSH-backend restart orphans (reconnect
  details not persisted in `AgentInstanceRecord`) — honestly logged as NOT
  REAPED, but the spec's headline orphan case is only fully realized for
  container/kubernetes/local backends.
- Local-backend orphan kills SIGTERM the persisted PID without identity
  verification (PID-reuse hazard after a restart).
- The mem audit adapter does not mirror the SQL adapters' tenant derivation
  (server-emitted rows stay NULL in pure-mem mode — fail-closed, but a
  mem/SQL contract divergence the port docs don't record).
- Migration 000070 backfills tenant only from workspace scope; legacy
  user-only rows remain NULL and invisible to tenant-scoped admin queries
  (fail-closed direction).
- `q` search pages through the filtered set (O(table) with decompression) —
  perf only; invalid `?level=` on /admin/server/logs acts as no-filter.
- `JobRegistry::durable_history` swallows durable-store errors via
  `unwrap_or_default` (silent degradation to the ≤50-entry cache; unlogged).
- `secret_rotation.rs` `run_once` skips a scope's secrets with only a tracing
  warn when `list_by_scope` errors (no per-secret job-log line for that
  failure mode); re-enrollment via `rotate()` does not extend `expires_at`.
- Server Logs UI lacks a `since` control though the endpoint supports it.

Test-inflation check: none found. The Vault suite's stub validates tokens and
observes which token read each path (asserting agent reads never use the
server token); the GCP SA test cryptographically verifies the RS256 signature;
the job-history durability test rebuilds the storage handle on the same file;
the audit tests pin tenant exclusion of other-tenant AND NULL-tenant rows and
wildcard escaping; the frontend tests assert rendered content and filter
wiring. One evidentiary caveat is recorded above: the SOPS "independent
implementation" vector is same-reading mirrored logic, not independent ground
truth — hence verifier check 1.
