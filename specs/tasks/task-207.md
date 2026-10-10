---
title: "Business Continuity §5 — Data Retention: Real Enforcement for All 7 Data Types"
spec_ref: "business-continuity.md §5. Data Retention Policies"
depends_on: []
review: specs/reviews/task-207.md
progress: ready-for-review
coverage_sections:
  - "business-continuity.md §5. Data Retention Policies"
commits: ["d365e47f45f04b633516eef3a058d26a528662a0", "e57cbd2c13b1f4470940769ffdbf6d693d9066f1", "16e7c07affb1d27f3732c6208501607fb446957b", "5f58013e398a726ecb2d583e1e3294f042c353bf", "2b1fa2ae823dfc912d85cbd41d4e9069a9cc02b4", "3a3c727b1df1480c95b3c0929cc3297d2e5ae161", "d69ef5baf6ccae0700b4aaf74499189080333f17", "dbc06219e0500d33c08b0c578f6c9e3679f7bc88", "6a908460b4d37971938a6f9cc4bfca182fefd592", "574d0c3c6f2f6309ec550c1f23aa1daab9157475"]
---

## Spec Excerpt

**§5. Data Retention Policies** — Configurable via `GET/PUT /api/v1/admin/retention`:

| Data Type | Default Retention | Rationale |
|---|---|---|
| Activity events | 90 days | Dashboard history; older events rarely queried |
| Agent logs | 30 days | Debugging; compress after 7 days |
| Audit events | 365 days | Compliance; forward to SIEM for longer retention |
| DB snapshots | 24h×24 + 7d×7 + 4w×4 | See snapshot policy above |
| Merge attestations | Forever | Non-repudiation; git notes survive DB loss |
| Notifications | 90 days (read), 365 days (unread) | Inbox hygiene |
| Analytics events | 365 days | Trend analysis |

Retention jobs run nightly at `02:00 UTC` via the background job scheduler. Jobs are idempotent and safe to run multiple times.

## Current State (audited 2026-09-29 — this is why the section is `not-started`)

- `run_cleanup()` (crates/gyre-server/src/retention.rs:64-76) is a **no-op**: it iterates policies and logs "retention policy checked (no-op for ring-buffer backed stores)". Nothing is ever purged.
- `RetentionStore` (retention.rs:31-51) is an in-memory `Arc<RwLock<Vec<RetentionPolicy>>>` — policies are lost on restart and drive nothing. `GET/PUT /api/v1/admin/retention` (api/mod.rs:487-488, api/admin.rs:198-211) read/write this volatile store only.
- Only 3 policy rows exist (`activity_events`, `analytics_events`, `cost_entries` — retention.rs:14-29); spec requires 7 data types.
- The `retention_cleanup` job (jobs.rs:251-265) is registered with `interval_secs: 86400` and invokes the no-op.
- The scheduler (jobs.rs:156-207) has **no wall-clock scheduling** — `tokio::time::interval` from process start, so "nightly at 02:00 UTC" is not expressible today.

## Implementation Plan

### 1. Persist policies

Replace the in-memory `Vec` in `RetentionStore` with the existing `StoragePort`/KV infrastructure (crates/gyre-ports/src/storage.rs; SQLite/Postgres adapters exist — see `kv_store.rs` in gyre-adapters/src/sqlite/). Policies MUST survive restart. Keep `list()`/`update()`/`set_policy()` semantics so the admin endpoints (admin.rs:198-211) work unchanged; their existing tests (admin.rs:1160-1220) must keep passing.

Seed defaults on first boot (only when no row exists): the 7 spec data types with spec defaults. Use data-type keys `activity_events`, `agent_logs`, `audit_events`, `snapshots`, `attestations`, `notifications`, `analytics_events`.

- `attestations` = forever → represent as a policy that never purges (e.g. `max_age_days: u64::MAX` or an explicit `forever` flag); the cleanup job must skip it.
- `notifications` needs two ages — 90 days read, 365 days unread. Extend `RetentionPolicy` with an optional `max_age_days_read` (or a `NotificationPolicy` variant) so both are honored in cleanup.

### 2. Real purge logic

`run_cleanup(&self, state: &AppState)` (it needs repo access — currently takes only `&self`; change the call in jobs.rs:261 to pass state) must actually delete:

- **activity_events** — stored in `TelemetryBuffer` (ring buffer, gyre-common::message). If the buffer only holds recent events, note that retention for it is naturally bounded; verify ring-buffer capacity bounds are honored and document. If activity events are ALSO persisted anywhere (check `activity.rs` adapter), purge rows older than policy there.
- **agent_logs** — `state.agent_logs: Arc<Mutex<HashMap<String, Vec<String>>>>` (lib.rs:265) is in-memory with no timestamps. To enforce 30-day retention the log entries need timestamps: either migrate agent logs to a DB-backed store with `created_at`, or store per-entry timestamps in the buffer. Prefer DB-backed (consistent with everything else). "Compress after 7 days" is an optimization — acceptable to note as not-blocking if compression is genuinely infeasible in scope; the 30-day purge is mandatory.
- **audit_events** — DB-backed via `AuditRepository` (gyre-ports/src/audit.rs; sqlite/audit.rs). Add a `delete_older_than(cutoff)` method to the port trait and both adapters (SQLite + Postgres). 365-day default.
- **snapshots** — files in `snapshot_dir()` (snapshot.rs:9-11, default `./snapshots`). Enforce the tiered policy `24h×24 + 7d×7 + 4w×4`: keep the 24 most recent snapshots younger than 24h... i.e. keep at most 24 snapshots from the last 24 hours, at most 7 snapshots from the last 7 days, at most 4 snapshots from the last 4 weeks; delete the rest (oldest first) using each file's creation/mtime. Idempotent: re-running yields no changes.
- **attestations** — never purge (policy records it; cleanup skips).
- **notifications** — `NotificationRepository` (gyre-ports/src/notification.rs; sqlite/notification.rs has `created_at: i64` per row). Add `delete_older_than(cutoff)` plus a read/unread-aware variant: delete read notifications older than 90d (default), unread older than 365d. Add to both adapters.
- **analytics_events** — `AnalyticsRepository` (gyre-ports/src/analytics.rs; sqlite/analytics.rs). Add `delete_older_than(cutoff)` to port + adapters. 365-day default.

Each `delete_older_than` must be a real `DELETE ... WHERE created_at < cutoff` (or equivalent), not a soft-delete or log line. Return/error counts so the job can log rows purged per data type.

### 3. Nightly 02:00 UTC scheduling

The current scheduler only supports fixed `interval_secs` from process start. Options (pick the boring one):
- Add a `run_at_utc_hour: Option<u8>` to `JobDefinition` (jobs.rs:21-25): when set, the spawn loop (jobs.rs:156-207) computes the delay until the next 02:00 UTC and sleeps until then, running once per day. Keep `interval_secs` behavior for jobs without the field.
- This is the smallest change that makes "nightly at 02:00 UTC" expressible; no cron crate needed.

The retention job must be idempotent (safe to run multiple times) — the DELETE-based purges are naturally idempotent.

### 4. Tests (hard tests only — no inflation)

- **Purge correctness per data type**: seed DB with rows at known old/new timestamps, run cleanup, assert old rows gone AND new rows still present (both directions — a test that only asserts deletion proves nothing about over-deletion).
- **Notification read/unread split**: old-read deleted, old-unread kept (until 365d), new-read kept.
- **Snapshot tiering**: create snapshot files with mtimes spanning the tiers (e.g. 30 files in last 24h, 10 in last 7d, 6 in last 4w) and assert exactly the policy survivors remain.
- **Persistence**: set a policy via `PUT /api/v1/admin/retention`, drop/rebuild the store from storage, assert the policy survived.
- **Attestation never-purge**: seed attestations with ancient timestamps, run cleanup, assert still present.
- **02:00 UTC scheduling**: unit-test the next-run computation (e.g. at 01:00 UTC → 1h; at 03:00 UTC → 23h).
- Existing admin retention endpoint tests (admin.rs:1160-1220) keep passing.

No new tests for already-working behavior (endpoint CRUD shape, job registration).

## Acceptance Criteria

1. `GET /api/v1/admin/retention` returns all 7 spec data types with spec defaults on a fresh install; `PUT` updates take effect and **survive server restart** (persisted in real storage).
2. Running the retention job actually deletes: activity/agent-log/audit/analytics/notifications data older than policy; snapshot files outside the `24h×24 + 7d×7 + 4w×4` tiers; attestations never.
3. Old-read vs old-unread notification distinction is enforced.
4. The retention job is scheduled at 02:00 UTC daily (not merely a fixed 86400s interval from process start), via scheduler support for wall-clock time.
5. Job is idempotent — a second consecutive run deletes nothing new.
6. Hard tests prove both directions (purge happens; within-policy data retained) for every purge path.
7. All existing retention/admin tests pass; `cargo test --all` green; `scripts/check-arch.sh` green (domain must not import adapters — new `delete_older_than` methods live in ports + adapters, cleanup orchestration in gyre-server).

## Agent Instructions

- Read this task file, `specs/system/business-continuity.md` §5, and the coverage note in `specs/coverage/system/business-continuity.md` row 7 before coding.
- Follow `docs/development.md` for build/commit conventions. Hexagonal boundaries are mechanically enforced — cleanup orchestration belongs in gyre-server (`retention.rs`), per-store purge queries in gyre-ports traits + gyre-adapters implementations (both SQLite and Postgres).
- Investigate actual persistence for activity_events and agent_logs before assuming; if `activity_events` truly live only in the bounded `TelemetryBuffer` ring, that boundedness is the enforcement — document it in code and the coverage note rather than inventing a fake purge.
- Do not soften the spec: `02:00 UTC` nightly, all 7 data types, read/unread notification split, tiered snapshot policy are all mandatory. If any piece is genuinely infeasible, stop and flag `needs-revision` with specifics rather than shipping a hollow implementation.
- On completion: run `cargo test --all`, update this file's frontmatter (`progress: ready-for-review`, `commits`), update the coverage row, and run `bash scripts/update-coverage-summary.sh`.

## Shipped

Revision round closing review findings F1–F4 (spec round 1) on top of the
original 8 commits; all seven data types from business-continuity.md §5 are
now genuinely enforced.

- **F1 — activity_events (90d)**: enforced by `TelemetryBuffer::purge_older_than`
  (gyre-common/src/message.rs:471), a hard age eviction across all workspace
  buffers, driven from `run_cleanup` with the policy cutoff (epoch-ms). The
  previously-dead `ActivityRepository` port and its SQLite/Postgres adapters
  were removed — the activity_events table has no writer; live activity data
  is the telemetry ring, so the ring is where the purge belongs. The
  `unwired-port-methods` exemption entry was retired.
- **F2 — snapshot tiers configurable**: tier counts now live on the
  `snapshots` policy row (`SnapshotTiers { keep_24h, keep_7d, keep_4w }`) and
  flow from `PUT /admin/retention` into `purge_snapshots`; band membership is
  age-fixed (≤24h / 1–7d / 7–28d) with no spillover, so the per-window caps
  cannot be defeated by cascade.
- **F3 — disk-level tests**: `purge_snapshots_on_disk_both_directions`
  (real tempdir, real mtimes, exact survivor set), idempotency,
  missing-dir no-op, and configured-tiers end-to-end.
- **F4 — PUT validation**: `validate_policies` requires exactly the 7 spec
  types each exactly once, `max_age_days >= 1`, the notifications read/unread
  split (read ≤ unread), snapshot tier counts ≥ 1, and attestations
  `u64::MAX`; invalid lists are rejected 400 with policies untouched.
- Also shipped in this round: corrupt-blob warn + self-heal, KV read-failure
  never overwrites the stored blob, durable `update()` (write completes
  before the 204), and the byte-slice-truncation guard fix
  (`take(1)` instead of `Vec::truncate`).

**Test evidence (this sandbox, SKIP_WEB_BUILD=1, isolated target dir):**

- `cargo test -p gyre-server retention` → **29 passed, 0 failed**
  (retention.rs unit tests incl. all F1–F4 tests + admin.rs endpoint oneshot
  tests: defaults / update / reject-incomplete / RBAC).
- `cargo test -p gyre-adapters delete_older_than` → **3 passed, 0 failed**
  (SQLite audit/analytics/notification both-direction purges).
- `cargo test -p gyre-common telemetry_buffer_purge` → **1 passed, 0 failed**
  (both directions).
- `scripts/check-arch.sh`, `check-unwired-port-methods.sh`,
  `check-byte-slice-truncation.sh`, `check-in-memory-state-stores.sh`,
  `check-dead-message-kinds.sh`, `check-mem-port-contracts.sh`,
  `check-lossy-secret-conversion.sh`, `check-inert-enforcement.sh`,
  `check-fail-open-ref-resolution.sh` → all OK.

**Transport restriction (not a code defect):** the HTTP integration test
`admin_retention_list_and_update` spawns a loopback TCP listener
(`Ctx::new`, api_integration.rs:48). This sandbox cannot `accept()` on TCP
sockets (errno 95, per /tmp/stage/capabilities.json) — every test in that
file fails identically regardless of the code under test, including
unrelated endpoints. The endpoint contract is covered by the in-process
oneshot tests above; the listener-based test must run on host/CI
(`cargo test -p gyre-server --test api_integration
admin_retention_list_and_update`). Recorded in
/tmp/stage/review-evidence/task-207-transport-restriction.md.

## Recovery + contract-repair round addendum (2026-10-10)

Assignment `3f26fcf486214310b7b027971090931d` (repair, category `contract`:
"The implementation changed the assigned requirements. Restore the original
task contract and implement it"). Audit of the candidate vs the base
(`770785f7`) found the implementation itself intact and contract-compliant
— all 7 data types enforced, 02:00 UTC wall-clock scheduling, KV-persisted
policies, full PUT validation (re-verified against source this round). The
contract mutation was in this task file's frontmatter: the revision-round
docs commit `acd1ba1d` dropped the `review: specs/reviews/task-207.md`
pointer the verifier's R1 review commit `226859ed` had added, silently
unscoping the R1 findings record from the task. Fixed this round:

- **Restored `review: specs/reviews/task-207.md`** in the frontmatter (the
  review file itself was unchanged in the tree — only the pointer was lost).
- **Single `progress: ready-for-review`** — this round's assignment
  injection had left a duplicate `progress:` key (`needs-revision` +
  `ready-for-review`, invalid YAML); resolved to the one truthful value.
- **Attribution completed**: `574d0c3c` (the prior docs commit fixing the
  same duplicate-key defect) added to `commits:`.
- No production code changed — none was needed; the repair is the task
  contract itself. Fresh probe evidence this round is recorded in
  /tmp/stage/review-evidence/task-207-contract-repair-evidence.md.
