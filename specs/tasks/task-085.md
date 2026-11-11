---
title: "HSI Trust Suggestions Background Job"
spec_ref: "human-system-interface.md §2 Trust Suggestions"
depends_on:
  - task-077
progress: ready-for-review
coverage_sections:
  - "human-system-interface.md §2 Trust Suggestions"
commits: ["a1020c2b702664bbf1e25cdebaace4cea097d8b7", "bbcc71c4b7e14a7a3aef220d6eef3f473c378e53", "6d2732817b4c580208a7ccc11b62325ff1186152", "dfdd6cdeb72ce6f8439c9f778f83bc44ef33b853"]
---

## Spec Excerpt

The system can suggest increasing trust based on track record:

```
This workspace has had 0 gate failures and 0 reverted MRs in 30 days.
Consider increasing trust level to Autonomous.
[Increase Trust] [Dismiss]
```

This appears as an Inbox item (priority 8). The human decides.

**Mechanism:** A background job (`trust_suggestion_check`) runs daily per workspace. It queries gate results and MR reverts for the last 30 days. If both counts are 0 and the current trust level is not already Autonomous, and the user has not dismissed this suggestion in the last 30 days, it creates one `Notification` per workspace Admin member. The job is registered in the server's `JobRegistry`.

## Implementation Plan

1. **Register `trust_suggestion_check` job** in the server's `JobRegistry`:
   - Runs daily (24h interval)
   - Iterates over all workspaces

2. **Per-workspace check logic:**
   - Query gate results for last 30 days: count failures (gate status != pass)
   - Query MRs with status `Reverted` for last 30 days
   - Skip if `workspace.trust_level == Autonomous`
   - Skip if any Admin member has dismissed a TrustSuggestion notification in last 30 days (use `NotificationRepository::has_recent_dismissal`)

3. **Notification creation:**
   - If both failure count and revert count are 0:
     - Create one `Notification` per workspace Admin/Developer member:
       - `notification_type: TrustSuggestion`
       - `priority: 8`
       - `title: "Consider increasing trust level"`
       - `body: { "gate_failures": 0, "mr_reverts": 0, "period_days": 30, "current_level": "Guided", "suggested_level": "Autonomous" }`
       - `workspace_id: workspace.id`
       - `repo_id: None` (workspace-level suggestion)
   - Each admin gets their own notification and can dismiss independently

4. **Dismissal tracking:**
   - When an admin dismisses the notification, `dismissed_at` is set
   - The job checks `has_recent_dismissal` per user before creating new notifications
   - 30-day cooldown per user per workspace

5. **Inline actions:**
   - "Increase Trust" → client calls `PUT /api/v1/workspaces/:id` with `trust_level: "Autonomous"` and resolves the notification
   - "Dismiss" → client calls `POST /api/v1/notifications/:id/dismiss`

## Acceptance Criteria

- [ ] `trust_suggestion_check` job registered in JobRegistry
- [ ] Job runs daily per workspace
- [ ] Skips workspaces already at Autonomous trust level
- [ ] Queries gate failures and MR reverts for last 30 days
- [ ] Creates notifications only when both counts are 0
- [ ] Creates one notification per Admin member (not one per workspace)
- [ ] Respects 30-day dismissal cooldown per user
- [ ] Notification has correct type (TrustSuggestion), priority (8), and body fields
- [ ] Unit test for job logic: workspace with clean 30-day record generates suggestion
- [ ] Unit test for job logic: workspace with failures does NOT generate suggestion
- [ ] Unit test for dismissal cooldown: dismissed < 30 days ago → no new notification
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/human-system-interface.md` §2 "Trust Suggestions" for the full requirement. The `JobRegistry` exists in the server — check how existing background jobs are registered (look for `admin_run_job` or similar patterns in `crates/gyre-server/src/`). The `NotificationRepository` port already exists in `crates/gyre-ports/src/notification.rs` — use its `has_recent_dismissal` method for the 30-day cooldown check. The `Notification` struct is in `crates/gyre-common/src/notification.rs`. You need to determine the workspace's trust level from the workspace entity. Gate results should be queryable from the gate evaluation records. MR reverts require checking MR status for `Reverted` — verify this status variant exists in the MR model (the spec says to add `Reverted` to the MR status enum if not present).


## Shipped

The `trust_suggestion_check` background job implements HSI §2. Two review rounds repaired real defects: a wire-format mismatch that made the Inbox inline actions unreachable in production, and a missing scheduler spawn that left the daily mechanism dead in default deployments.

### Server job (HSI §2 mechanism)

- **Job registration + scheduling**: `trust_suggestion_check` in the server's `JobRegistry` (jobs.rs) — daily (86400s), enabled, iterates all workspaces via `trust_suggestion::run_once` — and spawned in `main.rs` via `jobs::spawn_job` alongside the other daily jobs, so the handler actually executes daily in a default deployment (registration alone is manually-triggerable only; `interval_secs` is display metadata, `spawn_job` is the executing loop). With the spawn, `/healthz` liveness covers the job via `scheduled_jobs()` (Pending until its window elapses, then Ok).
- **Per-workspace criteria**: counts gate failures in the last 30 days (gate results scoped per-MR across the workspace's MRs, each result's own timestamp — `finished_at` else `started_at` — decides window membership, status != Passed counts as a failure) and MR reverts in the last 30 days (status `Reverted`, by `reverted_at` else `updated_at`). Both counts 0 → suggestion warranted.
- **Escalation ladder**: Supervised → Guided, Guided → Autonomous. Workspaces already at `Autonomous` (top of ladder) are skipped, as are `Custom` workspaces (off the ladder — no next preset level exists to suggest).
- **Notification**: one per Admin/Owner member (each dismisses independently — spec says "Admin member"; Owner is included as the role holding admin rights over the workspace; Developer/Viewer members are excluded, covered by test). `notification_type: TrustSuggestion`, priority 8, title "Consider increasing trust level", body `{gate_failures, mr_reverts, period_days: 30, current_level, suggested_level}`, `repo_id: None` (workspace-level).
- **Cooldowns**: per-user 30-day dismissal cooldown via `NotificationRepository::has_recent_dismissal`; active (unresolved, undismissed) suggestions are not duplicated on daily re-runs.

### Inline actions and wire-format repair (plan step 5)

Inbox "Increase Trust" calls `PUT /api/v1/workspaces/:id` with `trust_level` from the notification body's `suggested_level` and resolves the notification; "Dismiss" calls `POST /api/v1/notifications/:id/dismiss` (i18n keys `decisions.trust_increased` / `trust_increase_failed`).

Review found the server serializes `notification_type` via `NotificationType::as_str()` — PascalCase `'TrustSuggestion'` — while `Inbox.svelte`'s `typeNormMap` had no entry for it, so a real TrustSuggestion notification matched no template branch and rendered with no action buttons at all. Fixed by adding `TrustSuggestion: 'trust_suggestion'` to `typeNormMap` (the template branch and `TYPE_VARIANTS` badge entry already used snake_case), and re-pointed all three Inbox test mocks (`dismissedNotif` fixture, the Increase Trust test, the repo-scope filter test) at the real wire value `'TrustSuggestion'`, matching the existing `SpecAssertionFailure` PascalCase precedent — previously they used a snake_case value the server never emits.

### Scheduling repair (commit a1020c2b7)

Review found the job registered in the `JobRegistry` but never spawned: no `jobs::spawn_job("trust_suggestion_check")` call existed in `main.rs`, so in a default deployment the handler never ran and no TrustSuggestion notification was ever created automatically, while the unit tests stayed green because they call `run_once` directly (audit-only registration standing in for the specced daily mechanism). Fixed by adding the spawn in `main.rs` (after `log_compression`, fixed 86400s interval from process start), plus a regression test `job_is_scheduled_in_main` that reads the shipped `main.rs` bytes via `include_str!` (the binary's `main` is not exercisable from lib tests) and fails when the spawn call is removed — bounded to the call's argument list so a job name mentioned elsewhere in `main.rs` cannot satisfy it.

### Test evidence

- Server job logic (verified at the rebase checkpoint): `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib trust_suggestion` — 9 passed, 0 failed. Covers: clean record generates suggestion (type, priority 8, title, all 5 body fields, repo_id None); 0-activity workspace generates suggestion; gate failure in window suppresses; MR revert in window suppresses; failure older than 30 days ignored; Autonomous workspace skipped; one notification per Admin member; dismissal < 30 days ago → no new notification; active suggestion not duplicated on re-run. Earlier full probes: `cargo test -p gyre-server --lib` 1344 passed; `cargo fmt --all -- --check` clean.
- Wire-format repair (after `npm ci` with locked deps): `npx vitest run src/__tests__/Inbox.test.js` — 38 passed, 0 failed (log: `/tmp/stage/review-evidence/probe-inbox-wire-format.log`). **Mutation check**: with the `typeNormMap` entry removed, the Increase Trust test fails with "Unable to find an element with the text: Increase Trust" — exactly the production defect — proving the test is no longer stub-backed and kills the rendering-contract regression (evidence: `/tmp/stage/review-evidence/repair-summary.md`). Full web unit suite: `npx vitest run` — 99 files, 2010 passed, 41 skipped, 0 failed.
- Scheduling repair: `SKIP_WEB_BUILD=1 cargo check -p gyre-server --bin gyre-server` — clean (5m 52s); `SKIP_WEB_BUILD=1 cargo check -p gyre-server --lib --tests` — clean (54.64s; also proves the `include_str!("main.rs")` path resolves). The sandbox build wrapper blocks gyre-server lib test compilation (4 GiB budget), so execution is recorded for the independent verifier: `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib trust_suggestion` — 10 tests expected (the prior 9 + `job_is_scheduled_in_main`). **Mutation check** (logic mirrored, test binary not runnable here): on the pre-repair tree the test fails, on the repaired tree it passes — removing the spawn turns it red (evidence: `/tmp/stage/review-evidence/probe-job-scheduled.md`).

Full-workspace cargo suites, all-target Clippy, and GitHub CI remain owned by verification/publication per the assignment.
