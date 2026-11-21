# Review: task-273 — Admin Panel completion

**Candidate:** `03cebdc7546d0bf4c8d93a3969c4c3a0fa2e9c06` (base `678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce`)
**Round:** repair of the review of candidate `5e3a928e` (findings: SSH kill failure semantics; unbounded kill subprocesses), closed in `f8fb4508`.
**Verdict:** **APPROVED** — fresh, independent; no earlier approval transferred.

## Repair-round verification (commit `f8fb4508`)

Both findings are genuinely closed, verified by reading the diff and running the suites:

1. **SSH kill failure semantics.** `SshDockerTarget::kill_process` propagates every remote `docker rm --force` failure (only "No such object" is the already-gone success, mirroring `container_already_gone`); `SshTarget::kill_process` classifies every ssh-invoked kill (transport diagnostics and unexpected remote failures are errors; "No such process" is the idempotent success) and verifies the kill TERM → `kill -0` poll → KILL → poll, so a SIGTERM-trapping remote process can no longer be recorded as killed. `kill_and_mark` (shared by `remote_agent_cleanup` and the admin kill path) marks the durable `agent_instances` record terminated only after a verified kill.
2. **Latency bounds.** All kill subprocesses on the cleanup/admin-kill paths are bounded: ssh kill round trips and remote `docker rm --force` 30 s, local `<runtime> rm --force` 30 s, `kubectl delete pod` 60 s (> `--grace-period=30`), with `kill_on_drop(true)`.

Also confirmed: the `mem.rs` `#[cfg(test)]` fix (cargo check shows no dead-code warnings).

## Whole-candidate verification (all eight deliverables)

Independent inspection (full-file reads of `sops/`, `vault/`, `maintenance_jobs.rs`, `secret_rotation.rs`, `api/oauth.rs`, audit adapters, `telemetry.rs`, `jobs.rs`, ABAC middleware) plus focused probes:

- **SOPS default** — real: `age` crate X25519 data-key wrap (armored, spec-conformant), RustCrypto AES-256-GCM with sops's 32-byte IVs and `data:` path AAD, SHA-512 MAC sealed with second-precision `lastmodified` AAD, sops v3.9.x JSON envelope; `GYRE_SECRET_BACKEND` unset ⇒ SOPS; unrecognized backend panics at startup (fail-closed); legacy AES-256-GCM rows re-encrypt on first access behind a CAS on the exact ciphertext bytes (lost-update test included).
- **Vault option** — real: whole `SecretRepository` port served from Vault KV v2 via a 30 s-bounded reqwest client; agent resolution logs in with the agent's JWT (server token never used for agent reads — test-asserted); no DB fallback exists structurally; unreachable Vault fails closed (test-asserted); binary values base64-transported, no `from_utf8_lossy`.
- **Durable job history** — real: `JobRunRepository` port + SQLite/PG (portable migration `000071`) + mem parity + `store!` wiring; run-scoped `JobRunLog` sink persisted into `JobRun.logs`; empty durable history is authoritative over the in-memory ring (test-pinned); restart-durability adapter test passes.
- **`repo_maintenance` / `oauth_state_cleanup` / `remote_agent_cleanup` / `job_history_cleanup`** — all registered AND scheduled; real git `gc --prune=now` with per-repo outcomes and failure isolation (tests compare `git count-objects -v`); only-expired state-row deletion; orphan kills through real `ComputeTarget::kill_process` with pid-identity verification for local, `backend_config` reconstruction for container/k8s, and explicit escalation (never silent skip) for unreconstructable SSH.
- **OAuth state** — real flow: Admin-only enroll (server-derived tenant/workspace, PKCE S256), atomic single-use `mark_consumed` (`UPDATE … WHERE consumed = 0`) before the exchange, replay/expired rejected, client_secret never persisted in `oauth_states`, bounded token exchange, scoped `claude_max_refresh_token` stored idempotently.
- **Token refresh** — shape-based dispatch (no name-suffix matching remains); GCP SA RS256 JWT-bearer via `ring` over the PKCS#8 key with a stub provider that verifies the signature; every expiring secret in the scanned scopes gets a per-secret durable run-log outcome; scope-list failures fail the run.
- **Server logs** — `ServerLogLayer` installed in all four subscriber branches; Admin-only `GET /api/v1/admin/server/logs` with validated level/since/limit; non-Admin 403 test through the full stack.
- **Admin audit** — durable repository query (TelemetryBuffer gone from the handler), `tenant_id` derived server-side at emit + portable backfill migration, tenant scoping and free-text `q` enforced in SQL by both adapters with transparent zstd decompression; caller-tenant scoping test-pinned.

Cross-cutting: all seven invariant scripts pass with **zero exemption-file growth**; migrations 000070–000075 are duplicate-free and portable; every new route is in the ABAC resolver (the OAuth callback is resolver-exempt by the documented state-row-capability pattern, with the per-handler authorization the spec requires); commit attribution complete (all product-surface commits listed; the unlisted ones are docs/process/Cargo.lock only); coverage matrix 11/11 with concrete source evidence and an honest "awaiting independent verification" note.

## Probe evidence

Under `/tmp/stage/review-evidence/`: `probe1-compute-tests.log` (55 passed/0 failed), `probe2-sops-vault-tests.log` (29/0), `probe3-sqlite-adapter-tests.log` (62/0), `probe4-server-check.log` (EXIT=0), `probe5-npm-ci.log`, `probe6-tenantsettings-tests.log` (61/0), plus the seven invariant-script logs.

## Non-blocking observations (for the record / verifier)

1. **Task-scoped expiring secrets** are not scanned by `secret_rotation_check` (tenant/workspace/repo only). Pre-existing scope coverage on the base, not a task-273 regression; task-scoped secrets are revoked at task teardown (`revoke_task_secrets`). Worth a follow-up if task-scoped credentials with `expires_at` become a supported pattern.
2. **No live `sops` CLI round-trip test** in the repo (the binary isn't available in this sandbox). The acceptance criterion's alternative — spec-conformant native age decryption — is satisfied (real `age` crate, sops Go cipher test vector, independent Node-generated document vector, tamper/wrong-identity tests). Verifier may optionally run `sops -d` against a `sops_encrypt` output with the identity to add mechanical confirmation.
3. `load_sops_identity`'s re-read-`None` fallback to the freshly generated (unpersisted) identity is effectively unreachable (the insert-if-absent race loser implies the row exists); failing closed there would be strictly better.
4. Vault `_idx/{id}` index write is check-then-write without CAS — unreachable via the API (secret ids are server-generated UUIDs; the scoped-name uniqueness is CAS-enforced and concurrency-tested).
5. Vault agent-resolution policy denial yields the permitted subset (`CascadeDenial::YieldPermitted`) — documented and test-pinned as the platform-model §7 reading ("a denial scopes, it does not abort"); flagging as a product decision, not a defect.
6. On a Vault outage the spawn handler logs a warning and continues the spawn without `GYRE_CRED_*` (port fails closed; pre-existing handler posture unchanged by this task).

## Verifier-required tests (blocked here by the 4 GiB build budget)

`cargo test -p gyre-server --lib kill_and_mark`, `--lib mem_audit_search_tests`, `--lib maintenance_jobs`, `--lib jobs::`, `--lib api::admin`, `--lib api::oauth`, `--lib secret_rotation`; full `cargo test --all` and `cd web && npm test`; optionally a `sops -d` CLI round-trip per observation 2.
