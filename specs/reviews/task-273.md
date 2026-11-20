# Review: task-273 — Admin Panel completion

**Candidate:** `3ce715c38d0d61b8824de2800850f76c855e7793`
(branch `pipeline/task-273/94ee543f217e40e29bfaf5866c169b39-1`, base `9f998fe8`)
**Verdict:** approved (fresh, independent; no earlier approval transferred)
**Date:** 2026-11-20

## Scope

Full diff from base (~13.5k insertions): SOPS/Vault secret backends, durable
job history with logs, four maintenance jobs (repo_maintenance,
oauth_state_cleanup, remote_agent_cleanup, job_history_cleanup), document-shape
token-refresh dispatch incl. the GCP SA RS256 JWT-bearer handler, the Claude
Max OAuth enrollment flow, the server-log viewer, and the durable searchable
admin audit — plus the review-repair round (commits `9b81c981`…`d6f32bd0`).

## What was verified (independently, in this sandbox)

**Static review of every major surface:** sops/mod.rs (+tests), vault/mod.rs
(+tests), sqlite+postgres secret/audit/job_run/oauth_state/agent_instance
adapters, ports (secret/job_run/oauth_state/agent_instance/audit),
jobs.rs, maintenance_jobs.rs, secret_rotation.rs, api/{oauth,admin,secrets,
audit,spawn,orchestrator,mod,error}.rs, telemetry.rs, mem.rs parity, lib.rs
wiring, main.rs scheduling, migrations 000071–000074, TenantSettings.svelte +
tests, abac_middleware.rs registrations.

**SOPS ground truth:** fetched the actual getsops/sops **v3.9.4 source**
(sops.go, cmd/sops/common/common.go — via the GitHub API; saved under
`/tmp/stage/review-evidence/`) and confirmed the implementation's MAC
algorithm byte-for-byte: SHA-512 over the decrypted **value bytes only**
(`walkBranch` feeds `hash.Write(ToBytes(value))`; keys never enter the
hash), `%X` uppercase hex, MAC sealed with `LastModified.Format(time.RFC3339)`
(second-precision AAD — the canonicalization handling Go's nanosecond JSON
marshal is correct), value AAD `strings.Join(path, ":") + ":"` = `"data:"`,
32-byte GCM nonce, `ENC[AES256_GCM,...]` format, age-armored data-key stanza
exactly as sops's age key source stores it. The cross-implementation vector
test and sops's own Go cipher test vector (key `f`×32, AAD `bar:` → `foo`)
both pass. This is a genuine SOPS implementation, not a lookalike.

**Vault backend:** KV v2 data/metadata paths, AppRole login with relogin-on-
403 retry, agent-JWT login for `resolve_for_agent_as` (spawn.rs passes the
agent's freshly minted JWT; DB backends ignore it via the port default),
30 s total reqwest timeout, base64 value transport (no `from_utf8_lossy` on
secret material anywhere in the new code), path-safety name validation
(`/`, `\`, `..`, control chars rejected at both the API and the adapter),
typed `VaultPermissionDenied` (403) so agent-resolution yields the permitted
subset (§7 scoping) while everything else fails closed — structurally no DB
fallback; misconfiguration panics at startup.

**Durable job history:** `JobRunRepository` port + SQLite/PostgreSQL/mem
adapters; `JobRun.logs` captured through the run-scoped `JobRunLog` sink
handed to every handler; `set_durable_store` attaches before the schedulers
spawn; persist retries ×3 with a loud `durable_persist_failures` counter;
`GET /admin/jobs` serves the durable result authoritatively (ring only on a
loud read error or pure-mem mode). The Jobs tab renders status, duration,
failures and expandable logs.

**Maintenance jobs:** all four registered in `start_job_registry` and
scheduled in main.rs. `repo_maintenance` runs real `git gc --prune=now`
(bounded 10 min/repo) over RepoRepository rows AND unregistered repos under
repos_root (canonical-path dedup), per-repo outcomes in run logs, failures
isolated. `remote_agent_cleanup` kills via `ComputeTarget::kill_process`
with restart-orphan reconstruction; local pids verified against
`/proc/<pid>/stat` start time (recycled pids refused, exited orphans marked,
unreapable SSH retried 7 days then retired with escalation);
`LocalTarget::kill_process` now verifies the process is actually gone
(SIGTERM → wait → SIGKILL → wait → error).

**OAuth enrollment:** Admin-only initiation with server-derived
tenant/admin scoping (workspace validated against the caller's tenant),
state + PKCE S256 persisted durably, atomic single-use consumption
(`UPDATE … WHERE consumed = 0` in SQL, one-lock flip in mem) BEFORE the
code exchange, latency-bounded token exchange, scoped
`claude_max_refresh_token` stored via atomic `replace` on re-enrollment.
The public callback is resolver-exempt with the state row as the
per-handler capability — same trust model as invitation magic links;
`check-abac-exempt-handlers.sh` passes.

**Token refresh:** dispatch matches credential-document shape (GCP SA
first, then OAuth doc) regardless of name; the SA handler performs a real
RS256 JWT-bearer grant (ring over the PKCS#8 key, original document
preserved verbatim with only `access_token` overwritten, no fabricated
lifetime); `should_refresh` bounds cadence by granted lifetime (one-fifth,
clamped [10 min, 1 h]) so a 3600 s Google token refreshes ~every 48 min,
not every cycle; per-secret outcomes (refreshed / deferred /
no-refresh-mechanism / failed: reason) land in the durable run logs.

**Server logs:** `ServerLogLayer` installed alongside the unchanged
stdout/JSON/OTel layers in all four subscriber branches; bounded ring
(capacity clamped ≥1); Admin-only `GET /admin/jobs`-style route
`/api/v1/admin/server/logs` ABAC-registered with validated level/since/limit
filters (unknown level → 400); Server Logs tab with working filters.

**Admin audit:** `GET /admin/audit` queries the durable repository with
tenant derived from the authenticated caller (never the query string);
`tenant_id` derived at emit (workspace → user fallback) with portable
backfill migration 000074 (both correlated UPDATEs); free-text `q`
enforced in SQL by both adapters (Unicode `to_lowercase` on both sides,
escaped LIKE wildcards) with transparent zstd decompression + Rust re-filter
and correct paging; the Audit tab renders the full filter set incl. search.

**Probes executed (evidence in `/tmp/stage/review-evidence/`):**

- `cargo test -p gyre-adapters` focused: vault:: 12, sops 18, sqlite::audit
  22, sqlite::secret 23, sqlite::job_run 5, sqlite::oauth_state 3,
  sqlite::agent_instance 4, compute::local 4 — all pass.
- `cargo check -p gyre-server` — clean (postgres module compiled).
- web: `npm ci` + `npx vitest run src/__tests__/TenantSettings.test.js` —
  61/61 pass (incl. q/outcome/since/scope filter tests that assert the
  parameters actually reach the endpoints).
- All 18 mechanical invariant scripts pass; `git diff` on
  `scripts/*-exemptions.txt` is empty (no exemption growth).
- Working tree left clean (build-regenerated `web/dist` restored — the
  bundle is verifier-owned).

**Deferred to the verifier** (4 GiB budget): gyre-server test execution;
the required module filters are recorded in
`/tmp/stage/review-evidence/required-server-tests.txt`.

## Observations (non-blocking)

- `scripts/check-tenant-filter.sh` gained three SKIP_LIST entries with
  documented rationale (row-serving audit reads enforce `tenant_id`
  lexically in `query_rows`/`count_rows`; skips cover the pre-auth OAuth
  state capability lookup, cross-tenant housekeeping sweeps and aggregate
  stats). This script is not one of the frozen exemption files, and the
  rationales match the code.
- `secret_rotation_check` scans tenant/workspace/repo scopes, not task
  scope — pre-existing scan behavior, unchanged by this task; task-scoped
  secrets are revoked at task completion/kill (`revoke_task_secrets`).
- Generated `web/dist` matched/absent states are verifier-owned; no
  authored-frontend defect found.

## Conclusion

Every acceptance-criterion behavior is backed by real production code that
I verified directly (including against the actual sops v3.9.4 source), with
meaningful tests that exercise the behavior (shape dispatch, atomic state
consumption, pid verification, MAC tampering, fail-closed Vault, durable
history). The repair round's changes (atomic `replace`, cadence bound,
authoritative durable history, Vault policy-denial scoping, audit backfill
via users, repos_root maintenance, pid-verified kills, UI filters) are all
present and correct. No fake implementations, no missing enforcement, no
unsafe scope handling, no test inflation found. Approved.
