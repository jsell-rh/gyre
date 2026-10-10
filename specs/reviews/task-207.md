# Review — task-207 (Business Continuity §5 — Data Retention: Real Enforcement for All 7 Data Types)

Spec: `specs/system/business-continuity.md` §5 "Data Retention Policies" (lines 131-145), plus §1 line 38 ("Retention: configurable via `PUT /api/v1/admin/retention`. Default policy: keep 24 hourly + 7 daily + 4 weekly snapshots.").
Commit under review: `6a908460` (marked ready-for-review by the implementer's docs commit `ff3fbb87`). An independent prior audit (`68bf8d09`) already demoted the coverage row to Partial on the activity-events gap; this review corroborates it and adds three more findings.
Verdict: **needs-revision**.

## Round 1

Test runs (all with `CARGO_TARGET_DIR=/tmp/t207-review/target`; the shared `/home/jsell/code/gyre/target` is poisoned by sibling-agent worktrees — same artifact documented in task-095 R1 and task-097 R1, cross-worktree contamination, not a defect of the tree under review):

- Isolated extract of `6a908460` (`git archive` to `/tmp/t207-review`): `cargo test -p gyre-adapters delete_older_than` → **4 passed, 0 failed** (audit/analytics/activity/notification SQLite tests).
- Main checkout at HEAD (`6a908460` is an ancestor): `cargo test -p gyre-server retention` → **14 passed, 0 failed** (10 retention.rs unit tests, 3 admin.rs retention endpoint tests, 1 integration `retention_job_purges_across_restart`).
- Clean-target `cargo build -p gyre-adapters` at HEAD → clean (confirms the earlier shared-target compile failure was contamination, not a code defect).
- `bash scripts/check-arch.sh` → OK (hexagonal boundary clean; `gyre-domain` imports no infrastructure).

Verified working (no findings):

- **Policy store**: 7 default policy rows with the spec's exact default values — activity_events 90, agent_logs 30, audit_events 365, snapshots (tier set), attestations `u64::MAX` ("forever"), notifications read 90 / unread 365 split, analytics_events 365 (`retention.rs:399-400` asserts exactly these 7 types, no `cost_entries`). Persisted via `KvJsonStore` with `policies_persist_across_store_reinit` proving restart survival.
- **Real DELETE purges** for audit, analytics, and notifications: `AuditRepository::delete_older_than`, `AnalyticsEventRepository::delete_older_than`, `NotificationRepository::delete_older_than` implemented in SQLite, Postgres, and mem, with both-direction adapter tests (old rows deleted, new rows kept) — 4/4 green in the isolated run. The mem notification adapter correctly treats "read" as `resolved_at OR dismissed_at` (the only read markers on the `Notification` struct), applying the 90/365 read/unread split.
- **Agent-log purge**: `agent_log_line_is_old` parses the `[unix_ts]` prefix written by the actual ingest path (`api/agent_logs.rs:41` writes `format!("[{}] {}", ts, req.message)`; the `tty.rs` log path uses the same format), so old lines are genuinely matched and dropped. Both keep and purge directions tested.
- **Scheduling**: nightly 02:00 UTC via `run_at_utc_hour` / `next_daily_run_secs` with unit tests (boundary at hour rollover, DST-free UTC math); `spawn_job` wires it into the background job scheduler; idempotency covered by `cleanup_is_idempotent`.
- **Attestations never purged**: `u64::MAX` sentinel means the cutoff arithmetic can never select an attestation row; verified the arm passes the sentinel through rather than special-casing.

Findings:

- [-] [process-revision-complete] **F1 (major): activity_events 90-day retention is not enforced — the purge path is hollow and the new port method is dead code.** Spec §5 mandates 90 days default retention for Activity events, configurable via `GET/PUT /api/v1/admin/retention`. In the commit, `run_cleanup`'s `activity_events` arm (`crates/gyre-server/src/retention.rs:241-250`) computes the cutoff and logs, then does nothing; the module doc (`retention.rs:8-12`) admits no purge path exists. `ActivityRepository::delete_older_than` was added to the port and implemented in SQLite, Postgres, and mem with tests — but `grep -rn ActivityRepository crates/gyre-server/src` shows it referenced only in a doc comment: it is not in `AppState`, so no production code can call it. The two surfaces that actually serve activity data do not read the DB table either: `api/activity.rs:22` synthesizes activity from notifications, and the admin export (`api/admin.rs:186-192`) reads `telemetry_buffer.list_all_since(0, 10_000)`. `TelemetryBuffer` (`gyre-common/src/message.rs:361-384`) is count-bounded (10k per workspace, 100 workspaces), not age-bounded — a 90-day-old event survives indefinitely in a quiet workspace, and a busy workspace's events never reach 90 days. The `activity_events` policy row therefore drives nothing; the spec's retention guarantee for this data type is absent. Corroborated by the independent audit in `68bf8d09` (coverage row 7 demoted to Partial on exactly this). Remedy: wire `ActivityRepository` into `AppState` and have `run_cleanup`'s `activity_events` arm call `delete_older_than(cutoff)` (and point the activity surfaces at the table), or add age-based eviction to `TelemetryBuffer` driven by the policy row.

  **Process fixes:** implementation.md item 189 (adapter-implemented, adapter-tested port must be wired into AppState or called by production code before ready-for-review) + scripts/check-unwired-port-methods.sh (pre-commit + CI, advisory; ActivityRepository itself seeded in scripts/unwired-port-methods-exemptions.txt pending the product fix) + verifier bullet. Product fix owned by task-207's revision round.
- [-] [process-revision-complete] **F2 (major): snapshot retention is not configurable — the `snapshots` policy row is ignored.** Spec §5 header: all 7 types "Configurable via `GET/PUT /api/v1/admin/retention`"; spec §1 line 38: snapshot retention "configurable via `PUT /api/v1/admin/retention`". The `snapshots` arm in `run_cleanup` (`retention.rs:231-234`) calls `purge_snapshots()` with no policy argument; `purge_snapshots` (`retention.rs:300-336`) uses the hardcoded constants `SNAPSHOT_TIER_24H_KEEP/7D/4W` (`retention.rs:43-45`). `PUT /admin/retention` can update the `snapshots` row and `GET` will report the new value, but the nightly purge keeps applying the compile-time defaults — the configuration surface exists for this type but is dead, which is worse than not exposing it. Remedy: drive the tier counts from the `snapshots` policy row (store the three tier counts as structured fields of the row, or as three policy types) and thread them into `purge_snapshots`.

  **Process fixes:** implementation.md item 190 (every exposed configuration value must be traced from its persisted row into the enforcement path; a const of the same name at the enforcement site means the knob is dead) + verifier bullet. Product fix owned by task-207's revision round.
- [-] [process-revision-complete] **F3 (major): `purge_snapshots` — the only file-deleting purge path — has no disk-level test.** The task plan (§4, `specs/tasks/task-207.md` line 72) explicitly required "create snapshot files with mtimes spanning the tiers"; AC6 requires hard tests "both directions for every purge path". The only snapshot tests are the pure function `snapshot_tier_deletions` (`retention.rs:673-721`), which feeds it constructed `SnapshotMeta` vectors. The untested surface is everything else in `purge_snapshots`: real directory reads, `*.json` filename filtering against files `create_snapshot` actually writes (`<unix_secs>.json`, `snapshot.rs:46-48`), mtime→tier classification of real files, unlinking, and the missing-directory path. A regression in any of these ships green. Remedy: add a test that creates real files in a tempdir with spanning mtimes (including non-`.json` files that must survive) and asserts both directions on disk.

  **Process fixes:** implementation.md item 191 (filesystem-mutating functions need a real-tempdir test asserting both directions on disk; a pure helper they call is not coverage for the I/O wrapper) + verifier bullet. Product fix owned by task-207's revision round.
- [-] [process-revision-complete] **F4 (moderate): `PUT /api/v1/admin/retention` accepts arbitrary policy lists with zero validation.** `admin_update_retention` (`api/admin.rs:206-212`) passes the request body straight to `store.update(policies)`. The store's `update` comment says "Caller (admin PUT) validates shape" (`retention.rs:156-160`) — but the caller validates nothing. Three concrete failure modes: (a) a PUT body listing fewer than 7 types silently drops enforcement for the omitted types (the commit's own test `admin_update_retention_replaces_policies`, `admin.rs:1183-1208`, replaces the 7 defaults with a single `activity_events` policy — after which audit events, notifications, etc. are never purged and no error is raised); (b) `max_age_days: 0` makes the cutoff `now`, purging everything on the next nightly run; (c) unknown data types are accepted and then silently no-op'd by the `_ => 0` fallback in `run_cleanup` (`retention.rs:251`). Spec §5's "configurable" presumes the stored configuration remains a valid, complete retention policy. Remedy: validate in the PUT handler — all 7 required types present, `max_age_days >= 1` (or a documented floor), reject unknown types with 400.

  **Process fixes:** implementation.md item 192 (a "caller validates" comment is a contract that must be verified on both sides; grep the named caller for the deferred validation and test the rejection) + verifier bullet. Product fix owned by task-207's revision round.

Minor (not blocking, recorded for completeness):

- Spec §5 lists "compress after 7 days" for agent logs; compression is unimplemented and disclosed only in code comments (`retention.rs:15-16`). The task plan pre-authorized deferring it, and the 30-day purge bound still applies, so this is a spec-text gap rather than a silent failure — flagging so it is tracked rather than lost.
- `RetentionStore::init` (`retention.rs:124-139`) resets to defaults without a warn log when the persisted KV blob fails to parse, and `persist_best_effort` is fire-and-forget after the PUT response returns (a crash in that window loses the change silently). Both are low-severity robustness gaps; spec's "survive server restart" holds in normal operation.

Scope note: task-207's `commits:` frontmatter lists only `6a908460`; docs commit `ff3fbb87` is attributed to the task lifecycle, not the product surface, so no commit-attribution finding. Postgres adapter tests are absent for the new `delete_older_than` methods — consistent with the repo-wide pattern (Postgres is never integration-tested; `scripts/check-migration-sql-portability.sh` covers SQL portability), not a finding against this task.

## Round 2 (2026-10-10) — candidate `6d0c53ff` vs base `18c44f1a`

Verdict: **approved**.

Scope: the full diff base→candidate (41 commits; the 9 task-attributed product
commits plus pipeline merge/checkpoint noise). Independent probes run in this
sandbox on the candidate working tree (`SKIP_WEB_BUILD=1`, isolated target):
evidence in `/tmp/stage/review-evidence/task-207-review-evidence.md`.

Round-1 findings F1–F4 verified closed in code and by test:

- **F1 (activity_events hollow)** — closed for real: the dead `ActivityRepository`
  port + SQLite/Postgres adapters were *removed* (no writer existed for the
  activity_events table; live activity data is the telemetry ring), and the
  90-day policy is now enforced by `TelemetryBuffer::purge_older_than`
  (message.rs:847, hard age eviction, epoch-ms matching the buffer's real
  `created_at` units) driven from `run_cleanup`'s activity arm with
  `cutoff * 1000`. Both-direction + idempotency + configured-policy tests pass
  (gyre-common 1/1, gyre-server retention 29/29). The unwired-port-methods
  exemption entry was retired by deletion (check green, no growth).
- **F2 (snapshot tiers not configurable)** — closed: `SnapshotTiers` lives on
  the `snapshots` policy row, flows from `PUT /admin/retention` through
  `run_cleanup` into `purge_snapshots`; bands are age-fixed with no spillover
  (the `snapshot_tiering_window_cap_not_defeated_by_cascade` test guards
  exactly the old cascade defect). Configured-tiers test proves a non-default
  row changes the decision, pure and on disk.
- **F3 (no disk-level snapshot test)** — closed: real-tempdir/real-mtime tests
  assert the exact survivor set both directions, idempotency, missing-dir
  no-op, and non-`.json` survival; filenames match `create_snapshot`'s
  `<unix_secs>.json` output.
- **F4 (unvalidated PUT)** — closed: `validate_policies` requires exactly the
  7 spec types each once, `max_age_days >= 1`, mandatory read≤unread
  notification split, tier counts ≥ 1, attestations pinned to `u64::MAX`;
  rejection tests per class plus an endpoint-level 400-with-policies-untouched
  test, and the integration test's old single-type PUT body (which previously
  *passed* and silently disabled 6 types) was rewritten to the valid 7-type
  body with a negative case.

Additional verification this round (no findings):

- All seven data types purge through real code paths; DELETEs are hard
  diesel deletes with row counts in SQLite + Postgres + mem adapters. Unit
  consistency verified against production writers on every path (audit secs,
  analytics secs, notifications secs, agent-log `[secs]` prefix, telemetry
  ms) — no ms/s cutoff mismatch anywhere.
- 02:00 UTC wall-clock scheduling is real: `run_at_utc_hour` +
  `next_daily_run_secs` (unit-tested boundaries), spawn loop sleeps to the
  next h:00 UTC, `retention_cleanup` registered `Some(2)` and spawned from
  main after `RetentionStore::init` loads policies from the KV store (which
  is SQLite/PG-backed in DB mode). Round-1 minor (fire-and-forget PUT
  persist) also fixed: `update()` persists before acking; corrupt-blob
  self-heal and read-failure never-overwrite are both tested.
- Mechanical checks green: arch, unwired-port-methods, mem-port-contracts,
  in-memory-state-stores, byte-slice-truncation, dead-message-kinds,
  inert-enforcement, fail-open-ref-resolution, lossy-secret-conversion,
  migration-sql-portability, relative-path-defaults, abac-route-registry,
  task-commit-attribution (all 9 attributed commits reachable from the
  candidate; `6a908460` is an ancestor via main).
- `scripts/update-coverage-summary.sh` reproduces the candidate SUMMARY.md
  byte-identically. The HSI row delta (19/20 → 20/19 n/a/assigned) is a
  regeneration correcting a stale base row: the HSI coverage file itself
  (unchanged in this range since `acd20917`, an ancestor of the base) already
  read 20 n/a / 19 assigned — verified by recounting the base file.
- web/dist hashed-bundle churn is unexplained-by-diff rebuild noise from a
  checkpoint commit with no web/src changes in range — repo-recognized churn
  class (task-205 reverted identical churn on its branch); not a product
  defect. No production code under `crates/` is affected.
- Transport restriction (not a code defect): `admin_retention_list_and_update`
  and every listener-based test in api_integration.rs fail identically at the
  first request in this sandbox (no TCP accept, errno 95 per
  /tmp/stage/capabilities.json — control test `health_returns_ok` fails the
  same way). The endpoint contract is covered in-process by the green oneshot
  tests. Host/CI must run:
  `cargo test -p gyre-server --test api_integration admin_retention_list_and_update`.

All acceptance criteria (1–7) are satisfied by shipped production code and
hard tests; both directions (purge happens / within-policy data retained) are
asserted for every purge path.
