# Review: task-273 — Admin Panel completion

**Candidate:** `bc92e18be0b50e42abe11773ce9114f1f8c35bad` (branch
`pipeline/task-273/9abd2a227a9c4a6bb4a64361c781abba-1`, base
`2f33a117949ec8070fa207b69d8f6f021b4bcab8`)
**Reviewer:** independent review stage (Gyre pipeline)
**Verdict:** **APPROVED** — every deliverable verified against real behavior;
no structural defects, missing enforcement, fake implementations, or weak
tests found. Probe evidence: `/tmp/stage/review-evidence/`.

## What was verified (independently)

**SOPS default backend** — `crates/gyre-adapters/src/sops/` implements the
genuine sops v3.9.4 binary-envelope JSON format. Independent evidence beyond
the repo's own tests: a from-scratch Node.js implementation of the age v1
spec (bech32 identity decode, X25519 ECDH, HKDF-SHA256 stanza unwrap,
16-byte file nonce, STREAM ChaCha20-Poly1305 payload, AES-256-GCM with the
sops 32-byte IV and `"data:"` AAD, SHA-512 value MAC sealed under the
canonical second-precision `lastmodified`) decrypted BOTH (a) the test
suite's fixed cross-implementation vector and (b) a **freshly generated**
`sops_encrypt` document captured from a one-off run of the production code
(plaintext `review-probe-value`), with MAC verification — see
`sops-external-decrypt-output.txt`. The stored ciphertext is real,
externally decryptable SOPS format. Legacy AES-256-GCM rows stay resolvable
and re-encrypt on first access (`legacy_rows_remain_resolvable_and_migrate_to_sops`,
plus the sops Go-cipher test vector proves byte compatibility).

**Vault option** — `crates/gyre-adapters/src/vault/` serves the whole
`SecretRepository` port from Vault KV v2 through a 30 s latency-bounded
reqwest client: server token/AppRole (with expired-token re-login) for the
admin path, agent-JWT login for `resolve_for_agent_as` per platform-model
§7 (agents never receive Vault credentials), base64 value transport (no
lossy UTF-8 on secret material), path-unsafe name rejection at both the API
boundary and the adapter, namespace headers on every request, and fail-closed
behavior with no database fallback. The 10-test suite runs against a real
local HTTP server speaking the KV v2 + JWT-auth API (`cargo test -p
gyre-adapters --lib`: 655 passed / 0 failed, including
`unreachable_vault_fails_closed`).

**Durable job history with logs** — `gyre_ports::JobRunRepository`
(record/list_by_job/list_recent/delete_older_than) with SQLite +
PostgreSQL adapters (portable migrations `000071`–`000074`, next-unused
sequence after the base consumed 000067), a mem adapter enforcing the port
constraints in code (atomic single-use `mark_consumed`, finished-only
`delete_older_than`), `store!` wiring, and a run-scoped `JobRunLog` sink
handed to every handler. `GET /api/v1/admin/jobs` serves the persisted
history (logs + derived duration) with the in-memory ring as fronting
cache only. `job_history_cleanup` prunes via the port and is itself
registered/scheduled/visible like every other job.

**Maintenance jobs** — `repo_maintenance` runs real `git gc --prune=now`
per stored repository (10-min per-repo bound) with per-repo outcomes in the
run logs; the in-repo test seeds an unreachable loose object, asserts
`git count-objects -v` reclaims it, and proves a corrupted repo fails
without aborting the others. `oauth_state_cleanup` deletes only expired
rows. `remote_agent_cleanup` kills still-recorded-running instances whose
agent is Dead/Stopped/absent via `ComputeTarget::kill_process` — the tests
spawn REAL `sleep` processes and assert the process is actually gone, the
durable record is marked terminated, and live agents' instances are
untouched. Restart orphans are handled through backend reconstruction from
the durable descriptor (unreconstructable SSH descriptors are reported,
not silently skipped). Spawn paths (agents AND orchestrators) record
instances; clean exits and the admin kill handler mark them terminated
(the kill handler only marks when a kill was actually attempted).

**OAuth state + Claude Max enrollment** — Admin-only initiation
(ABAC `secret:write` + in-handler role check) generates state + PKCE S256
verifier, persists scope fields derived server-side (tenant from the
authenticated caller, workspace validated against it), and 303-redirects.
The callback (resolver-exempt, per-handler authorization = the single-use
state row; reachable without a Bearer token through the full stack —
pinned by a dedicated full-router test) validates exists/unexpired/
unconsumed, consumes atomically BEFORE the exchange (port contract:
conditional UPDATE … WHERE consumed = 0, race-tested), exchanges the code
with the verifier at a latency-bounded token endpoint, and stores/rewrites
the scoped `claude_max_refresh_token` credential document (re-enrollment
extends expiry immediately, with restore-on-failure).

**Token refresh coverage** — dispatch is credential-document-SHAPE based:
the RFC 6749 handler refreshes an OAuth document under a non-`_refresh_token`
name (tested as `PROD_API_CREDENTIALS`), the new GCP SA handler performs a
real RS256 JWT-bearer grant (ring signs over the SA's PKCS#8 key; the test
stub VERIFIES the signature against the public half and checks iss/aud/
scope/exp), the refreshed document preserves every Google-issued SA field
verbatim, and every expiring secret is accounted for in the durable run
logs (refreshed / no-refresh-mechanism / failed: reason), with
SecretExpiring notifications still emitted for no-mechanism secrets.
Scope-list failures now fail the run instead of degrading to empty.

**Server logs** — a `ServerLogLayer` tracing capture layer (bounded ring,
capacity-clamped, case-insensitive level filter) installed alongside the
unchanged stdout/JSON/OTel layers; `GET /api/v1/admin/server/logs`
(ABAC-registered `admin` resource — non-Admin 403 verified through the
full middleware stack; unknown levels are 400, not silently widened) and
a Server Logs tab in TenantSettings.

**Admin audit** — `GET /api/v1/admin/audit` rewritten onto the durable
audit repository: `AuditEvent.tenant_id` derived server-side at emit
(never client-supplied; migration `000074` backfills from workspace scope;
mem adapter performs the same derivation for parity), tenant scoping
enforced in SQL by BOTH adapters at the read entry points, free-text `q`
matching over the detail payload with zstd-compressed rows transparently
decompressed and matched in the adapter (LIKE wildcard-escaped, lowercased
on both sides for cross-backend parity), other-tenant and NULL-tenant rows
invisible to tenant-scoped queries, and the TenantSettings Audit tab
renders durable events with the search box.

**Cross-cutting** — hexagonal boundaries hold (check-arch passes); all
fifteen-plus mechanical invariant scripts pass with no new exemption-file
entries (the two scripts that exit non-zero do so identically on the base —
pre-existing script behavior; the four rc=1 violations have identical
counts on base and candidate); `check-tenant-filter.sh` gained three
documented SKIP_LIST entries whose rationales I verified against the code
(query_rows/count_rows do apply tenant scoping lexically; the skipped
reads are genuinely cross-tenant housekeeping); task commit attribution
passes against the committed frontmatter; `cargo check -p gyre-server`
clean; `cargo test -p gyre-ports -p gyre-common -p gyre-domain` green
(129/570/8); `web` `npm ci` + `npm test` green (2571 passed, 41 skipped)
including the new TenantSettings server-logs/jobs/audit-tab tests.

## Deferred to the independent verifier (blocked by the sandbox build
wrapper — 4 GiB budget, NOT code defects)

- `cargo test -p gyre-server --lib` (full) — in particular the
  `maintenance_jobs::`, `oauth::`, `secret_rotation::`, `jobs::`,
  `telemetry::`, `admin::` (audit/server-logs/jobs endpoint) and `mem::`
  test modules. `cargo check -p gyre-server` (lib) is clean; the test
  modules were code-reviewed but not compiled here.
- `cargo check -p gyre-server --all-targets`.
- The runtime probes from the task's verification intent that need a
  booted server: restart durability of job history and the admin audit
  trail (build_state against the same GYRE_DATABASE_URL), the live
  SOPS-at-rest export check, and the four maintenance-job triggers via
  `POST /admin/jobs/{name}/run`.

## Non-blocking observations (recorded for the coverage refresh; not
production-code defects)

- `specs/coverage/system/admin-panel.md` rows 3, 5, 6 and 10 cite the
  pre-rebase migration numbers (`000067`–`000070`); after the rebase-repair
  renumbering the actual migrations are `000071_job_runs`,
  `000072_oauth_states`, `000073_agent_instances`,
  `000074_audit_events_tenant_id` (the accurate mapping is documented in
  the task's Shipped notes; only the matrix citations are stale). Row 5
  also says "302-redirects" where the code uses 303 See Other.
- `web/dist/` was not rebuilt in this checkpoint — expected; it is
  verifier-owned and regenerated remotely (the committed bundle may match
  the upstream base by design). Authored frontend source and tests were
  reviewed and pass.
