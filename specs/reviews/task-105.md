# Review — task-105 (Implement log compression background job)

Spec: `specs/system/observability.md` §Log Retention & Compression (lines 164-170), cross-referenced with `specs/system/business-continuity.md` §5 (retention table, lines 135-145).
Candidate under review: `2d54e3a0d6c984ffc3798748cfb6d9e57b9438b8` (base `72ad2a128d`). Rounds 1-4 findings (unused import, cap-eviction order, clippy braces/double_must_use, scoped compression counts) were repaired in commits `eda138a7b`, `0ef762b86`; round-5 candidate adds only the whitespace-only rustfmt fix `9b175db00` (verified via `git show`: three pure-formatting hunks in `log_compression.rs`) plus process/docs commits.
Verdict: **approved**.

## Round 5 evidence (independent, this review)

Sandbox note: crates.io is reachable only through the executor's cargo wrapper (`/tmp/stage/bin/cargo`); workspace-wide invocations are wrapper-blocked, so probes are package-scoped. All commands, exit codes, and outputs saved under `/tmp/stage/review-evidence/`.

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib` → **1415 passed, 0 failed** (incl. all 11 `log_compression` tests and the SIEM wire test).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib` → **373 passed, 0 failed, 12 ignored** (incl. the 4 audit-detail compression tests; migration 000057 therefore executed on SQLite).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-ports` → 0 tests (crate has none; compiles as dependency).
- `python3 scripts/check-rustfmt-diff.py 72ad2a128d` → exit 0, "changed lines clean (14 Rust files checked)" — the round-4 verification failure is fixed.
- `cargo clippy -p gyre-server --lib --tests` → exit 0 (warnings only; none in changed `log_compression`/`siem`/`retention`/`tty` code — the `sort_by` hits in `mem.rs` are the pre-existing pattern, also present at base). The workspace clippy-diff gate itself is wrapper-blocked here; verification ran it clean at `8d4d26ae` (only rustfmt failed), and `9b175db00` is whitespace-only, so the changed-line result carries over.
- All 20 mechanical checks (`check-arch`, `check-abac-route-registry`, `check-abac-exempt-handlers`, `check-mcp-write-tools`, `check-dead-message-kinds`, `check-migration-versions`, `check-migration-sql-portability`, `check-byte-slice-truncation`, `check-relative-path-defaults`, `check-fail-open-ref-resolution`, `check-mem-port-contracts`, `check-fabricated-scope-defaults`, `check-lossy-secret-conversion`, `check-scope-literal-defaults`, `check-inert-enforcement`, `check-forged-scope-fields`, `check-forwarded-header-trust`, `check-unbounded-external-http`, `check-in-memory-state-stores`, `check-task-commit-attribution`) → all OK on the candidate (`mech-checks.txt`).
- **Kill-check (restored)**: temporarily made `AuditEventRow::into_event` in `crates/gyre-adapters/src/sqlite/audit.rs` fail-open (return `Null` detail for compressed rows instead of decompressing) — `audit_compress_details_before_compresses_only_old_uncompressed`, `audit_compress_details_is_idempotent_and_limit_bounded`, and `audit_since_timestamp_decompresses_compressed_events` all **FAIL** (3 failed / 11 passed); restored via `git checkout`, tree verified clean (`git status` empty, HEAD `2d54e3a0d`), suite re-run green (14 passed). The decompression tests genuinely gate the production behavior.

## Contract verification (against the task acceptance criteria)

- **Configurable interval, default daily** — `log-compression` registered in the admin job registry (`jobs.rs`, `run_at_utc_hour: None`, interval from `GYRE_LOG_COMPRESSION_INTERVAL`, default 86400, unparsable/zero → default; unit-tested `interval_resolution`), spawned from `main.rs` after `start_job_registry` (same pattern as `retention_cleanup`); triggerable via `POST /api/v1/admin/jobs/log-compression/run` through the pre-existing generic route.
- **Audit details compressed after 7 days** — `COMPRESSION_AGE_SECS = 7d`; `AuditRepository::compress_details_before` on SQLite/Postgres/mem with a NULL-guarded idempotent flip (`detail` blanked, `detail_zstd` set, level-3 zstd); batches bounded 500×1000/run so a backlog cannot pin the job.
- **Retention deletion (365d audit / 30d agent / 90d eBPF-stream)** — owned by the pre-existing nightly `retention_cleanup` (task-207), which the shipped notes disclose as intentional plan drift (the task excerpt's "no-op run_cleanup" predated task-207). Verified defaults: audit_events 365, agent_logs 30, activity_events (the coverage-matrix mapping of eBPF streams) 90, all enforced by `run_cleanup` with purged-count logging; retention suite 29/29 green.
- **Transparent decompression on query** — `query`/`since_timestamp` decompress in all three adapters; corrupt payload is a hard error, not silent empty detail; kill-check above proves the tests bite.
- **SIEM forwarding of compressed events** — forwarder reads via `since_timestamp`; proven at the wire level by `siem::tests::forward_cycle_sends_decompressed_detail_for_compressed_events` (real local TCP listener receives the decompressed JSON fields).
- **Job logs counts** — `tracing::info!` per run with rows/lines and bytes before/after; round-4 fix correctly scopes agent-log stats to newly folded lines only (regression test asserts `lines == 1` on an incremental fold).
- **`cargo test --all`** — the diff touches only `gyre-ports`, `gyre-adapters`, `gyre-server` (plus docs/specs); all three pass their full lib suites here. Workspace-wide run deferred to verification (wrapper restriction, not a code defect).

## Notes (non-blocking)

- `AgentLogBuffer::append` at the 10k cap with a cold segment decodes + re-encodes the whole segment per appended line (O(segment) per append on the hot path). Agent log lines are human-paced, so this is a performance tradeoff, not a correctness defect; worth an eye if log volume grows.
- `AgentLogBuffer::get`/`replay` decompress the entire cold segment even for a paged read of a few lines — same class, non-blocking.
- Postgres `compress_details_before` has no integration test (no PG in CI anywhere, repo-wide pattern; SQL is symmetric with the tested SQLite path and `check-migration-sql-portability.sh` passes).
- The coverage row was honestly updated to `implemented` (not `verified`), awaiting verification.
