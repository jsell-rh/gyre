---
title: "Business Continuity §5 — Data Retention: Real Enforcement for All 7 Data Types"
spec_ref: "business-continuity.md §5. Data Retention Policies"
depends_on: []
progress: needs-revision
coverage_sections:
  - "business-continuity.md §5. Data Retention Policies"
commits: ["5f58013e398a726ecb2d583e1e3294f042c353bf", "2b1fa2ae823dfc912d85cbd41d4e9069a9cc02b4", "3a3c727b1df1480c95b3c0929cc3297d2e5ae161", "d69ef5baf6ccae0700b4aaf74499189080333f17", "dbc06219e0500d33c08b0c578f6c9e3679f7bc88", "6a908460b4d37971938a6f9cc4bfca182fefd592"]
review: specs/reviews/task-207.md
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
