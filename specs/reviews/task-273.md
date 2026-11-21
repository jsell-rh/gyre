# Review — task-273 (Admin Panel completion: SOPS/Vault secret backends, durable job history with logs, maintenance jobs, complete token refresh, server-log viewer, durable searchable admin audit)

Spec: `specs/system/admin-panel.md` (gap `42df7a7e48e1904b76eface1`).
Candidate under review: `3c0b9a77` (branch `pipeline/task-273/3d9acd2e8ba94269bde1d21f5c5961cb-1`, base `678a8ac3`; ~16.1k-line diff over 71 files, carrying the implementation plus multiple prior review/verification repair rounds).
Verdict: **needs-revision** — three findings (F1 SOPS legacy-migration lost-update race, F2 silently-ignored `GYRE_SECRET_BACKEND` values, F3 coverage-row citation errors). Everything else verified genuine by independent probes; the findings are small, precisely-scoped repairs.

## Probes run (evidence under `/tmp/stage/review-evidence/`)

Mechanical invariants (all exit 0, exemption files byte-identical to base):
`check-arch`, `check-abac-route-registry`, `check-abac-exempt-handlers` (91 handlers),
`check-migration-versions`, `check-migration-sql-portability`, `check-mem-port-contracts`,
`check-in-memory-state-stores`, `check-unbounded-external-http`, `check-lossy-secret-conversion`,
`check-fabricated-scope-defaults`, `check-relative-path-defaults`, `check-fail-open-ref-resolution`,
`check-forged-scope-fields`, `check-forwarded-header-trust`, `check-inert-enforcement`,
`check-scope-literal-defaults`, `check-byte-slice-truncation`, `check-dead-message-kinds`,
`check-mcp-write-tools`, `check-tenant-filter` (passes only via three new SKIP_LIST entries in
`scripts/check-tenant-filter.sh` — all three rationales verified legitimate: audit `query_rows`/`count_rows`
apply `AuditQueryFilter.tenant_id` lexically in their own bodies; `oauth_state.find_by_state` is the
pre-auth single-use capability; `user_profile.list_for_user` is the user-keyed judgment ledger).

Focused tests (`/tmp/stage/bin/cargo`, explicit `-p gyre-adapters`):
- `sops::` 13 passed — includes the sops v3.9.4 Go `aes/cipher_test.go` GCM vector and the cross-implementation document vector.
- `vault::` 16 passed — real HTTP stub speaking KV v2 (cas, versioning, destroy, AppRole TTL, JWT-auth scoping, namespace headers), incl. `unreachable_vault_fails_closed`, `agent_resolution_policy_denial_scopes_instead_of_aborting`, `concurrent_create_of_same_scoped_name_succeeds_exactly_once`.
- `job_run` 8, `oauth_state` 6, `agent_instance` 7, `secret` 23, `audit` 37 — all passed, incl. the storage-rebuild restart-durability tests, `mark_consumed_is_atomic_single_use`, tenant-scoped and zstd-compressed free-text-search tests.
- `cargo check -p gyre-server` → exit 0 (5m31s).
- Frontend: `npm ci` + `npx vitest run src/__tests__/TenantSettings.test.js` → 61 passed.

**Independent SOPS-format probe** (`sops-node-probe.js`): a from-scratch Node/OpenSSL reader —
implementing the age v1 spec (bech32 identity decode, X25519 ECDH, HKDF-SHA256 with the
`age-encryption.org/v1/X25519` label, ChaCha20-Poly1305 stanza unwrap of the 16-byte file key,
payload STREAM chunk with final-flag nonce) and the SOPS v3.9 leaf format (AES-256-GCM, 32-byte IV,
AAD `data:`) with zero gyre code — fully decrypts the candidate's cross-implementation ground-truth
vector and verifies the MAC composition (SHA-512(plaintext) uppercase hex sealed under the
lastmodified AAD). This is independent confirmation that the stored ciphertext is genuine
SOPS format per the acceptance criterion's "spec-conformant native age decryption" clause.

## Verified working (no findings)

- **SOPS default backend** (`crates/gyre-adapters/src/sops/`): real sops v3.9.x binary-store envelope — reference `age` crate for the data-key wrap, AES-256-GCM with sops's 32-byte IVs and `data:` path AAD, SHA-512 MAC sealed under canonical RFC-3339 `lastmodified` (Go fractional-seconds quirk handled and tested), fail-closed MAC verification. Identity from `GYRE_SOPS_AGE_KEY`/key file/auto-generated persisted in kv_store (never API-exposed); legacy AES-GCM rows decrypt via the preserved ring path and migrate on first access (test-pinned, incl. `resolve_for_agent` on migrated rows). `resolve_for_agent` still feeds `GYRE_CRED_*` at spawn; non-UTF-8 skipped, never lossy.
- **Vault option** (`crates/gyre-adapters/src/vault/`): the whole `SecretRepository` port served from KV v2 (data/metadata/destroy paths, `options.cas=0` duplicate enforcement with a concurrency test); agent resolution logs in with the agent's own JWT (`/v1/auth/<mount>/login`) so Vault policies scope the cascade (policy-denial-yields-permitted-subset, proven by test; server path treats 403 as failure); 30 s reqwest timeout; structurally no DB fallback (unreachable → error, tested); values base64 — no `from_utf8_lossy`; agents never receive Vault tokens; AppRole re-login on expiry with X-Vault-Namespace on both login types.
- **Durable job history**: `JobRunRepository` port + SQLite/PG adapters (portable migration 000071) + mem parity; run-scoped `JobRunLog` sink taken into every recorded run (scheduler, manual trigger, `record_cycle`); `GET /admin/jobs` reads the durable store (empty-is-authoritative — pruned runs do not resurrect from the 50-entry ring, test-pinned); logs + derived duration rendered in the Jobs tab (frontend test). `job_history_cleanup` registered + scheduled, `delete_older_than` on finished rows only, retention configurable.
- **repo_maintenance**: real `git gc --prune=now` per repo (registered + repos_root scan with dedup), per-repo outcomes in run logs, failures isolated; test with three real git repos proves `count-objects` reclamation and corrupted-repo isolation.
- **OAuth enrollment + state**: Admin-only ABAC `secret:write` + in-handler role check; tenant/admin derived server-side; PKCE S256 verifier persisted and used; 256-bit state, atomic single-use consumption (conditional UPDATE in SQL, check+flip under lock in mem — concurrent-consumption test), expiry enforced, replay/expired → 400; code exchanged via the 30 s-bounded shared client; `claude_max_refresh_token` stored/rotated via the port's atomic `replace`; `client_secret` redacted from the oauth_states row (re-derived from env for the exchange; test-pinned); callback resolver-exempt with the state row as the only capability, full-middleware-stack reachability test; `oauth_state_cleanup` deletes only expired rows. `check-abac-route-registry` and `check-abac-exempt-handlers` pass with exemption files unchanged vs base.
- **Token refresh generalization**: shape-based dispatch (OAuth doc by `token_endpoint`/`client_id`/`refresh_token`, GCP SA by `type=service_account`), name-independent (tested under `PROD_API_CREDENTIALS`); GCP handler does real ring RS256 over the SA PKCS#8 key with correct RFC 7523 claims, stub verifies the signature against the public key; full SA document preserved on refresh; every `try_rotate` branch writes a per-secret outcome line (refreshed / no-refresh-mechanism / failed: reason / deferred) to the durable run log; missing `expires_in` fails honestly; rotation cadence bounded by lifetime/5 clamped [600, 3600] (safe for Google's 3600 s).
- **Remote agent cleanup**: every spawn path (container/ssh-docker/ssh-bare/k8s/local + orchestrator) records a durable instance (port + adapters, portable migrations 000073/000075; `backend_config` carries no credentials); terminated marked on clean exit/admin kill only on verified kills (failed kill leaves the record running, test-pinned); sweep kills Dead/Stopped/absent instances via `ComputeTarget::kill_process` with pid-identity verification against `/proc` start times (recycled-pid refusal test), live agents untouched (real-process test); restart orphans reconstructed for container/k8s/local; real `sleep`-process kill test and stub-kubectl k8s test.
- **Server logs**: real `tracing` Layer over a bounded ring (existing stdout/JSON/OTel layers unchanged); `GET /admin/server/logs` with level/since/limit, ABAC-registered `admin` resource, non-Admin 403 through the full stack (tested); Server Logs tab + i18n.
- **Admin audit**: `admin_audit` queries the durable repository with `tenant_id` from the caller (never the query string) and free-text `q`; tenant_id derived server-side at emit (workspace → user → agent; unresolvable dropped, tested); migration 000070 backfills from workspace and user scope; `q` enforced as portable escaped `LOWER() LIKE` in both SQL adapters with non-ASCII terms skipping the ASCII prefilter (Unicode-fold re-filter; café/CAFÉ test) and zstd-compressed rows decompressed and matched (adapter-level restart-durability test); TenantSettings Audit tab renders the durable events with search + full filter set.
- **Cross-cutting**: hexagonal boundary clean; new durable state all port-backed via `store!`; latency-bounded outbound HTTP everywhere (Vault 30 s; OAuth + rotation ride the 30 s AppState client; only test code uses `Client::new()`); `check-task-commit-attribution` passes at the candidate commit (the working-tree failure is the stage's protected-task-text restoration of `specs/tasks/task-273.md`, not part of the candidate).

## Findings (verdict-blocking)

- **F1 (code, concurrency/integrity): SOPS legacy-migration lost-update race.** `decrypt_at_rest` (`crates/gyre-adapters/src/sqlite/secret.rs:290-339`, PG twin) re-encrypts a legacy row with `UPDATE secrets WHERE id = ?` — unguarded on the ciphertext it read, outside any transaction. A rotate/replace committing between the read and the migration UPDATE is silently reverted to the pre-rotation plaintext while the rotate API returned success. One-time window per pre-upgrade secret; consequence is a silently defeated credential rotation. Fix: `WHERE id = ? AND encrypted_value = ?` (bail/retry on 0 rows) or a wrapping transaction.
- **F2 (code, fail-open config): unrecognized `GYRE_SECRET_BACKEND` values silently ignored.** `build_secret_repository` (`crates/gyre-server/src/lib.rs:1752-1776`) handles only `"vault"`; a typo'd value with full Vault env config silently boots on the database store — no error, no log naming the ignored value — contradicting the spec's "never silently fall back to the database store" and the function's own doc-comment rationale (enforced only for the exact string `vault`). Fix: reject values outside {sops, legacy/aesgcm, vault} at startup.
- **F3 (docs/evidence): coverage-row citations wrong.** `specs/coverage/system/admin-panel.md` row 3 cites nonexistent migration `2026-11-13-000074_audit_events_tenant_id` (actual: `...000070_audit_events_tenant_id`); row 10 claims "sequence 000070 is unused" (false — 000070 is the audit migration); row 5 says "302-redirects" (code returns 303). Fix the citations.

## Non-blocking observations (recorded for follow-up, not verdict-blocking)

- SSH-backend restart-orphans are unreapable (connection details deliberately not persisted): reported as NOT REAPED, retried 7 days, then `retired_unreapable` with escalation — honestly documented in code and coverage row 6; in-process SSH orphans and all local/container/k8s restart-orphans are killed. A follow-up could persist a non-secret SSH reconstruction descriptor (host/user/identity path) as is done for k8s.
- `REFRESH_THRESHOLD_MIN_SECS = 600` inverts the cadence bound for providers granting ≤ ~900 s tokens (refresh every 5-min cycle); no backoff for persistently failing refreshes of already-expired credentials. Safe for Google's 3600 s.
- Audit migration 000070 backfills workspace and user scope but not the agent-principal scope that record-time derivation performs — legacy agent-principal rows stay NULL and invisible to tenant-scoped admin queries.
- `repo_maintenance`'s 10-minute timeout drops the `git gc` future without killing the child (`kill_on_drop` not set) — a slow repo's gc keeps running detached.
- Test seams (implementation verified piecewise, no single end-to-end run): no AppState-rebuild-then-`GET /admin/jobs` test; no enroll-route-through-full-middleware-stack test; no single multi-secret run-accounting test for the rotation job.
- Two doc inaccuracies: `docs/server-config.md` claims a default for `GYRE_CLAUDE_MAX_REDIRECT_URI` (code is deliberately fail-closed with no default); `docs/api-reference.md` "302" vs 303 (same as F3's row-5 note).

## Verifier must run (deferred: large gyre-server test compilation exceeds the sandbox budget)

- `cargo test --all` (workspace) and `cargo clippy --all-targets` — in particular the gyre-server test modules covering this task: `maintenance_jobs` (repo_maintenance/remote_agent_cleanup/job_history_cleanup/oauth_state_cleanup incl. the real-process and real-git tests), `jobs` (durable registry), `secret_rotation` (shape dispatch, GCP RS256 stub, cadence), `api::oauth` (enrollment/callback/replay/expiry/concurrency), `api::admin` (server-logs 403/filters, admin_audit tenant scoping + q, jobs durability semantics, kill-path instance semantics), `telemetry` (ServerLogLayer), `mem` (audit compressed search, port-contract parity).
- Frontend: `cd web && npm ci && npm test` (TenantSettings suite verified here: 61 passed).
- The five verification-intent probes from the task text (restart/SOPS/server-logs/maintenance-jobs/admin-audit) against base `678a8ac3` (must fail) and candidate (must pass); the SOPS probe may use the Node reader saved at `/tmp/stage/review-evidence/sops-node-probe.js` as a template.
- Re-run the mechanical invariant scripts listed above in the merged tree (no new exemption entries).

## Round history note

This round reviewed the candidate after the `93385c75` "repair the three review findings" commit; those three prior findings (admin-kill target reconstruction from `backend_config`, Vault cas-based duplicate enforcement, mem-audit compressed-detail search) were independently confirmed fixed by reading the repair diff and re-running the vault/mem/audit test modules.
