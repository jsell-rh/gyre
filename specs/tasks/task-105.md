---
title: "Implement log compression background job"
spec_ref: "observability.md §Log Retention & Compression"
depends_on:
  - task-102
progress: complete
coverage_sections:
  - "observability.md §Log Retention & Compression"
commits: ["9b175db00e22916c573465ab626f7bc13e7aaf32", "0ef762b86b7f4426f525dcece52bb71023dbedcb", "eda138a7bcccf283512c072474e4dd07893a3640", "4597330e9528db2a379b420a8f07d2c8f8408038"]
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

### Round 2: contract repair + base merges

- The assigned task document was restored by the executor (acceptance-criteria checkboxes left unchecked as assigned — protected text); the production fix above is preserved unchanged. Source conflicts resolved across assignment-base merges: base `a884329d7` (task-076, merge commit `615182576`, one textual conflict in `crates/gyre-adapters/src/sqlite/audit.rs` — kept our side), base `233664cf8` (task-087, clean, merge commit `68dacd0ac`), base `4d6759a0b` (task-084, clean, merge commit `cf1f047b0`).

### Round 3: review repair

Commit `eda138a7b` fixed both round-2 review findings: removed an unused import in the SIEM wire test (CI clippy-diff gate), and made `AgentLogBuffer::trim_to_cap` evict lines in overall age order (cold-segment front first, then hot front) so a cold segment at the 10k cap no longer discards every newly appended line. Regression test: `line_cap_evicts_oldest_when_cold_segment_is_at_cap`.

### Round 4: review repair

Commit `0ef762b86` fixes all three round-3 findings on the merged head (`cf1f047b0`):

- **F1 (CI-blocking):** `format![...]` → `format!(...)` in the round-3 regression test — `clippy::nonstandard_macro_braces` on a changed line fails the CI clippy-diff gate.
- **F2 (CI-blocking):** added the repo-established `#[allow(clippy::double_must_use)] // async_trait generates a must_use future for each method.` above `AuditRepository` (same pattern as `storage.rs` / `meta_spec_repository.rs` / `spec_assertion_result_repo.rs`) — the new `compress_details_before` method's async_trait-generated warning span overlapped changed lines.
- **F3 (observability):** `AgentLogBuffer::compress_older_than` counted the ENTIRE rebuilt cold segment, so every incremental pass re-counted already-cold lines (a daily run folding 1 line into a 10k-line segment reported 10k lines compressed, growing unboundedly over the 7–30 day window). All `AgentLogCompressionStats` fields (`lines`, `bytes_before`, `bytes_after`) now count only the lines newly folded THIS pass, matching the audit-event half of the report (which counts only newly compressed rows); field docs updated. `compress_folds_incrementally_and_keeps_new_lines_hot` now asserts the scoped counts (`lines == 1`, `bytes_before` == the new line's frame bytes) and FAILS on the pre-fix code (kill-check verified: reverting just the `lines` counting reproduces `left: 2, right: 1`).

### Round 5: verification repair (rustfmt)

Commit `9b175db00` fixes the single failing gate from verification finding `0237fecc` (`bash /tmp/stage/checks.sh`, exit 1): `scripts/check-rustfmt-diff.py` against base `72ad2a128d` reported changed lines 209–210, 719–720, 735–738 in `log_compression.rs` needing formatting (everything else in that run passed — clippy changed-lines clean, all mechanical checks OK, web build succeeded). The fix applies `rustfmt --edition 2021 --config skip_children=true` to the file: three whitespace-only hunks (the round-4 `decode` let-binding, `buf.append(...)` wrapping, and the `assert!(...contains(...))` wrapping in the cap-eviction test), no semantic change. Post-fix, the exact gate script exits 0: "rustfmt: changed lines clean (14 Rust files checked)". Evidence saved under `/tmp/stage/review-evidence/` (`rustfmt-fix.md`, `rustfmt-gate.txt`). Note for remote verification: this implement sandbox has no crates.io network, so Rust suites were not re-run here — the change is rustfmt-whitespace-only on previously compiling lines; the prior head's suites stand (`cargo test -p gyre-server --lib`, 1360 passed).

### Test evidence

Post-repair probes on branch head `0ef762b86` (command lines, exit codes, and output tails saved under `/tmp/stage/review-evidence/`):

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib` — 1360 passed, incl. the 11 `log_compression` tests (with the round-4 scoped-count assertions) and the SIEM wire test.
- `python3 scripts/check-clippy-diff.py 4d6759a0b` (the actual CI gate script, workspace `--all-targets --all-features` clippy, changed-line rule vs the assignment base) — exit 0: "clippy: changed lines clean (14 Rust files, 1084 existing warnings outside changes)". Both round-3 CI-blocking findings (F1 braces, F2 double_must_use) are gone; two `cloned_ref_to_slice_refs` hits introduced by the first draft of the round-4 test assertions were also fixed (bind `vec![...]` and pass `&vec` instead of `&[x.clone()]` literals).
- F3 kill-check: with only the old whole-segment `lines` counting temporarily restored, `compress_folds_incrementally_and_keeps_new_lines_hot` fails (`assertion left: 2 right: 1`); restored fix passes.
- Earlier probes on prior heads stand: `cargo test -p gyre-adapters --lib` — 356 passed (incl. the 4 audit-detail compression tests) on `615182576`; mechanical checks (check-arch, check-abac-route-registry, check-mcp-write-tools, check-dead-message-kinds, check-migration-versions, check-task-commit-attribution) — all OK.
- One test-design note (from round 1): an earlier draft of the SIEM test asserted tiny log lines shrink under zstd — they do not (frame overhead exceeds savings on ~30-byte inputs); the test uses realistic repetitive payloads so the byte-ratio assertions reflect real compression.
