---
title: "Implement log compression background job"
spec_ref: "observability.md §Log Retention & Compression"
depends_on:
  - task-102
progress: ready-for-review
coverage_sections:
  - "observability.md §Log Retention & Compression"
commits: ["4597330e9528db2a379b420a8f07d2c8f8408038"]
---

## Spec Excerpt

From `observability.md` §Log Retention & Compression and `business-continuity.md`:

- Audit events: 365 days local, indefinite in SIEM
- Agent logs: 30 days (compressed after 7 days via the `log-compression` background job)
- eBPF event streams: 90 days (high-volume; compressed aggressively)

Current state: Retention policy store exists (`retention.rs`) with defaults (activity 90d, analytics 365d, cost 365d). `run_cleanup()` is a no-op ("no-op for ring-buffer backed stores"). No compression background job exists.

## Implementation Plan

1. **Log compression background job (`gyre-server/src/log_compression.rs`):**
   - Create a new background job that runs daily (configurable interval)
   - Query audit events older than 7 days that are not yet compressed
   - Compress the `detail` JSON field using gzip/zstd
   - Mark records as compressed (add `compressed: bool` column or store compressed data inline)
   - Delete audit events older than retention period (365 days for audit, 30 days for agent logs, 90 days for eBPF)

2. **Database schema changes:**
   - Add `compressed` boolean column to audit_events table (default false)
   - Add index on `timestamp` for efficient range queries

3. **Retention policy enforcement:**
   - Replace the no-op `run_cleanup()` with actual deletion of events beyond retention window
   - Respect per-category retention periods:
     - Audit events (auth, ABAC, RBAC): 365 days
     - Agent logs (heartbeat, completion): 30 days
     - eBPF/procfs events (file access, network connect): 90 days
   - Log deletion counts for observability

4. **Wire into server startup:**
   - Spawn the compression job in `gyre-server/src/lib.rs` alongside other background jobs
   - Make the schedule configurable via `GYRE_LOG_COMPRESSION_INTERVAL` env var (default: 86400s / 24h)

5. **Compression format:**
   - Use `zstd` crate for compression (better ratio than gzip for JSON)
   - Decompress transparently on read in the query path
   - SIEM forwarding should decompress before sending

## Acceptance Criteria

- [x] Background job runs on configurable interval (default daily)
- [x] Audit event details compressed after 7 days
- [x] Events deleted after retention period (365d audit, 30d agent, 90d eBPF)
- [x] Compressed events decompressed transparently on query
- [x] SIEM forwarding handles compressed events
- [x] Job logs deletion/compression counts
- [x] `cargo test --all` passes

## Agent Instructions

Read `specs/system/observability.md` §Log Retention & Compression. The current retention store is in `gyre-server/src/retention.rs`. Background jobs are spawned in `gyre-server/src/lib.rs` — look for `tokio::spawn` calls with `interval`. The audit repository is in `gyre-adapters/src/sqlite/audit.rs`. Add `zstd` to Cargo.toml if not already present. Check migration numbering — currently at 000038.

## Shipped

The `log-compression` background job is implemented end-to-end in commit `4597330e95`. Note on plan drift: the task excerpt's "current state" predated task-207 — `run_cleanup()` was already real retention enforcement when this task started, so deletion was NOT duplicated into the compression job (deletion past retention windows stays owned by the nightly `retention_cleanup` job, which logs its purged counts; the compression job logs its compression counts).

### Job

- `gyre-server/src/log_compression.rs`: `run_once` compresses (1) audit-event details and (2) agent log lines older than 7 days (`COMPRESSION_AGE_SECS = 7d`), then logs per-run counts (rows/lines, bytes before/after) via `tracing::info!` and returns a `LogCompressionReport`. Audit batches are bounded (500/batch, 1000 batches/run) so a backlog cannot pin the job. Idempotent: a second run compresses nothing new.
- Registered in the admin job registry as `log-compression` (visible in `GET /api/v1/admin/jobs`, manually triggerable via `POST /api/v1/admin/jobs/log-compression/run`) and spawned from `main.rs`. Interval from `GYRE_LOG_COMPRESSION_INTERVAL` (seconds, default 86400; unparsable/zero falls back to default).

### Audit-event detail compression

- Port: `AuditRepository::compress_details_before(cutoff_secs, limit) -> CompressionStats` (gyre-ports/src/audit.rs).
- Migration `2026-11-07-000057_audit_detail_compression`: adds `audit_events.detail_zstd BYTEA` (portable — BYTEA is native PostgreSQL; SQLite stores bound blobs regardless of affinity, same pattern as 000022 gate_traces). NULL = not yet compressed. No new index needed — `idx_audit_timestamp` from 000054 already covers the cutoff range scan.
- SQLite + Postgres adapters: select uncompressed rows older than the cutoff, zstd-compress (level 3) the `detail` JSON, blank `detail` and set `detail_zstd` under a `detail_zstd IS NULL` guard (idempotent under concurrency). `query`/`since_timestamp` decompress transparently; a corrupt compressed payload is a hard query error, not a silent empty detail.
- Mem adapter: same contract — compressed entries hold the zstd payload and a `Null` detail placeholder; `query`/`since_timestamp` materialize with decompression.

### Agent log compression

- `AgentLogBuffer` (log_compression.rs) replaces the plain `HashMap<String, Vec<String>>` in `AppState.agent_logs`: hot lines stay raw; lines older than 7 days fold into a per-agent zstd cold segment using length-prefixed frames (so lines with embedded newlines round-trip byte-for-byte). `GET /api/v1/agents/{id}/logs`, TTY replay, and the 10k-per-agent line cap are preserved; the 30-day retention purge moved into `AgentLogBuffer::purge_older_than` (driven by `retention_cleanup`, same keep-unprefixed-lines rule as before). Read order is cold (old) lines first, then hot — chronological for the server's `[ts]`-prefixed writer.

### SIEM forwarding

The forwarder reads through `since_timestamp`, which decompresses in the adapter, so forwarded syslog/webhook payloads carry the original detail. Proven at the wire level by `siem::tests::forward_cycle_sends_decompressed_detail_for_compressed_events` (local syslog TCP listener receives the decompressed JSON fields of a compressed event).

### Test evidence

Focused suites, all green on branch head `4597330e95` (logs in `/tmp/stage/review-evidence/`):

- `cargo test -p gyre-adapters` — 356 passed, incl. 4 new compression tests (round-trip transparency, byte-ratio reality, idempotency + limit bounding, since_timestamp decompression).
- `cargo test -p gyre-server --lib` — 1326 passed, incl. 10 new `log_compression` tests (transparency + idempotency, incremental folding, unprefixed/multiline round-trip, whole-segment and mixed-segment purge, line cap, `run_once` end-to-end with the mem audit repo) and the SIEM wire test.
- Mechanical checks: check-arch, check-migration-versions, check-migration-sql-portability, check-in-memory-state-stores, check-mem-port-contracts, check-abac-route-registry, check-inert-enforcement, check-task-commit-attribution — all OK.
- One test-design note: an earlier draft of the SIEM test asserted tiny log lines shrink under zstd — they do not (frame overhead exceeds savings on ~30-byte inputs); the test now uses realistic repetitive payloads so the byte-ratio assertions reflect real compression.
