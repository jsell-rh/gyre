---
title: "Implement log compression background job"
spec_ref: "observability.md §Log Retention & Compression"
depends_on:
  - task-102
progress: not-started
coverage_sections:
  - "observability.md §Log Retention & Compression"
commits: []
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

- [ ] Background job runs on configurable interval (default daily)
- [ ] Audit event details compressed after 7 days
- [ ] Events deleted after retention period (365d audit, 30d agent, 90d eBPF)
- [ ] Compressed events decompressed transparently on query
- [ ] SIEM forwarding handles compressed events
- [ ] Job logs deletion/compression counts
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/observability.md` §Log Retention & Compression. The current retention store is in `gyre-server/src/retention.rs`. Background jobs are spawned in `gyre-server/src/lib.rs` — look for `tokio::spawn` calls with `interval`. The audit repository is in `gyre-adapters/src/sqlite/audit.rs`. Add `zstd` to Cargo.toml if not already present. Check migration numbering — currently at 000038.
