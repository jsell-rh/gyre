# Review — task-105 (Implement log compression background job)

Spec: `specs/system/observability.md` §Log Retention & Compression (+ `business-continuity.md` §5 retention windows).
Candidate: `8d4d26ae4778d898bdfe8f234dfdb3c7262be26f` on `pipeline/task-105/80d166762be0428fa894f52a5af955ee-1`, diffed against assigned base `4d6759a0bfd637e899bbf65e725f91b07f745c69`. Task commits: `4597330e95`, `eda138a7b` (round-3 repair), `0ef762b86` (round-4 repair).
Verdict: **approved** (round 4; rounds 2–3 findings verified fixed).

Evidence: commands, exit codes, and outputs under `/tmp/stage/review-evidence/` (see `INDEX.md`). Review-stage cargo wrapper rejects workspace-wide invocations, so probes are package-scoped; the CI clippy gate's exact changed-line rule was replicated against the base over lib+test clippy captures of all three changed crates.

## Probes run

- `cargo test -p gyre-server` (all targets): 1360 lib + 163 integration tests passed — includes the 11 `log_compression` tests, the SIEM wire-level test (`forward_cycle_sends_decompressed_detail_for_compressed_events`, local TCP listener receives the decompressed JSON of a compressed event), tty replay, and the 29 retention tests (30d agent-log purge through `AgentLogBuffer::purge_older_than`, 90d activity sweep, 365d audit DELETE).
- `cargo test -p gyre-adapters --lib`: 356 passed, incl. `audit_compress_details_before_compresses_only_old_uncompressed` (real zstd byte shrinkage + full detail round-trip through `query`), `audit_compress_details_is_idempotent_and_limit_bounded`, `audit_since_timestamp_decompresses_compressed_events`. `-p gyre-ports` and `-p gyre-cli` also pass.
- Clippy changed-line rule (replica of `scripts/check-clippy-diff.py` vs base): **0 warnings on changed lines** — round-3's two CI-blocking findings (`nonstandard_macro_braces` in the regression test, `double_must_use` on the extended `AuditRepository` trait) are confirmed fixed on this head.
- Mechanical invariants: check-arch, check-migration-versions (000057 is the next unused number), check-in-memory-state-stores, check-byte-slice-truncation, check-mem-port-contracts (mem adapter enforces the same compression contract), check-lossy-secret-conversion, check-task-commit-attribution, check-dead-message-kinds, check-inert-enforcement, scope/fabricated-default checks, check-fail-open-ref-resolution, check-unbounded-external-http — all OK.
- **F3 kill-check (independent)**: temporarily restored the pre-fix whole-segment `stats.lines` counting in `compress_older_than` → `compress_folds_incrementally_and_keeps_new_lines_hot` fails (`left: 2, right: 1`); restored byte-identical (sha256 verified, tree clean), test passes. The round-4 regression test genuinely kills the over-counting bug.

## Verified working

- **Job**: registered as `log-compression` in the admin job registry (interval from `GYRE_LOG_COMPRESSION_INTERVAL`, default 86400s, unparsable/zero → default — unit-tested), spawned from `main.rs` via `spawn_job`; `run_once` compresses audit details + agent log lines older than 7 days (`COMPRESSION_AGE_SECS`), batch-bounded (500×1000), idempotent, logs per-run counts via `tracing::info!` and returns a `LogCompressionReport`.
- **Audit detail compression (real)**: port method `AuditRepository::compress_details_before` implemented in SQLite, Postgres, and mem. Migration `2026-11-07-000057` adds `detail_zstd BYTEA` (portable across both backends — same pattern as 000022; portability check passes); the update blanks `detail` and sets `detail_zstd` under an `IS NULL` guard (idempotent under concurrency). Every read path (`query`, `since_timestamp`) decompresses transparently; a corrupt compressed payload is a hard error, not a silent `{}`. Tests assert real byte ratios, so a structural no-op would fail them.
- **Agent log compression (real)**: `AgentLogBuffer` (hot raw lines + per-agent zstd cold segment with length-prefixed frames so embedded newlines round-trip byte-for-byte) replaces the plain `Vec` buffer; `GET /api/v1/agents/{id}/logs`, TTY replay, the 10k line cap (now evicting in overall age order — round-3 F2 regression test passes), and the 30d retention purge (moved into `purge_older_than`, driven by `retention_cleanup`) all preserved. Stats count only newly folded lines (round-4 F3 fix; kill-check above).
- **SIEM forwarding**: forwarder reads through `since_timestamp`, which decompresses in the adapter — proven at the wire level by the local-syslog-listener test.
- **Plan drift (deletion) is legitimate**: `run_cleanup()` was already real retention enforcement (task-207, in base) when this task started; deletion past retention windows stays owned by the nightly `retention_cleanup` job (365d audit / 30d agent / 90d activity), verified by the passing retention tests. Duplicating deletion into the compression job would double-own retention.
- Docs (`api-reference.md`, `server-config.md`) and coverage row 9 (`task-assigned` → `implemented`, "Awaiting independent verification") updated honestly; `zstd` was already a workspace dependency at base; no Cargo.toml changes needed.

## Non-blocking note for the coverage auditor

observability.md's "eBPF event streams: 90 days" line sits in tension with the same spec's routing of eBPF events into the audit API (365d local) and with business-continuity.md §5, which has no separate eBPF row. The platform's established reading (base coverage row 9, task-207 history) maps the 90d eBPF window onto `activity_events` (TelemetryBuffer, swept at 90d by `retention_cleanup`); eBPF-originated audit events ride the 365d audit window and are zstd-compressed after 7 days by this job. The tension predates this candidate (zero deletion code in this diff); if the stricter per-event-type 90d purge is wanted, it needs a spec-lifecycle amendment and a new task. Not a defect of this candidate.

## Deferred to verifier

- Literal `cargo test --all` and `python3 scripts/check-clippy-diff.py 4d6759a0b` (workspace-wide invocations rejected by the review-stage wrapper). Package-scoped equivalents cover every crate with source changes on lib + test targets, plus gyre-cli, and the clippy changed-line rule was replicated exactly (0 hits).
- Two non-zero mechanical-check exits (`check-assertionless-tests.sh` exit 2, `check-conditional-test-guards.sh` web hits) were verified to reproduce identically on the base commit (temp worktree) — pre-existing/environmental, not candidate defects.
