---
title: "HSI Trust Suggestions Background Job"
spec_ref: "human-system-interface.md §2 Trust Suggestions"
depends_on:
  - task-077
progress: complete
coverage_sections:
  - "human-system-interface.md §2 Trust Suggestions"
commits: ["dfdd6cdeb72ce6f8439c9f778f83bc44ef33b853", "6d2732817b4c580208a7ccc11b62325ff1186152"]
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

The `trust_suggestion_check` background job implements HSI §2 exactly as specified. A prior recovered-checkpoint version of `trust_suggestion.rs` existed at the assignment base but used percentage-based escalation thresholds (>80%/95% gate pass rate, <5%/2% revert rate), required terminal MRs in the window, and emitted a different title/body schema — all diverging from the spec's "0 gate failures and 0 reverted MRs in 30 days" contract. This assignment rewrote the evaluation to the spec, then resolved the rebase of the branch onto base `4d6759a0` (upstream task-084/task-087 changes).

### Rebase resolution

Merging base `4d6759a0bfd637e899bbf65e725f91b07f745c69` into the branch conflicted only in `specs/coverage/system/human-system-interface.md` (rows 14–15). Resolution keeps upstream's row 14 (task-084 Policies ↔ Trust Level Integration, `implemented` upstream) plus this task's row 15 (Trust Suggestions, `implemented`), and updates the file's hand-maintained coverage header to the recomputed counts (18/35, 11 implemented, 17 task-assigned). `web/src/locales/en.json` auto-merged cleanly (JSON validated; the task's `decisions.trust_increased`/`trust_increase_failed` keys intact). The task's source files (`trust_suggestion.rs`, `jobs.rs` registration, `Inbox.svelte` inline actions) carried through the merge unchanged.

### Behavior

- **Job registration**: `trust_suggestion_check` in the server's `JobRegistry` (jobs.rs) — daily (86400s), enabled, iterates all workspaces via `trust_suggestion::run_once`.
- **Per-workspace criteria (HSI §2)**: counts gate failures in the last 30 days (gate results scoped per-MR across the workspace's MRs, each result's own timestamp — `finished_at` else `started_at` — decides window membership, status != Passed counts as a failure) and MR reverts in the last 30 days (status `Reverted`, by `reverted_at` else `updated_at`). Both counts 0 → suggestion warranted.
- **Escalation ladder**: Supervised → Guided, Guided → Autonomous. Workspaces already at `Autonomous` (top of ladder) are skipped, as are `Custom` workspaces (off the ladder — no next preset level exists to suggest).
- **Notification**: one per Admin/Owner member (each dismisses independently — spec says "Admin member"; Owner is included as the role holding admin rights over the workspace; Developer/Viewer members are excluded, covered by test). `notification_type: TrustSuggestion`, priority 8, title "Consider increasing trust level", body `{gate_failures, mr_reverts, period_days: 30, current_level, suggested_level}`, `repo_id: None` (workspace-level).
- **Cooldowns**: per-user 30-day dismissal cooldown via `NotificationRepository::has_recent_dismissal`; active (unresolved, undismissed) suggestions are not duplicated on daily re-runs.
- **Inline actions (plan step 5)**: Inbox "Increase Trust" calls `PUT /api/v1/workspaces/:id` with `trust_level` from the notification body's `suggested_level` and resolves the notification (previously it only navigated to settings and dismissed); "Dismiss" calls `POST /api/v1/notifications/:id/dismiss`. i18n keys `decisions.trust_increased` / `decisions.trust_increase_failed`.

### Test evidence

- Post-rebase (merged tree, this assignment): `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib trust_suggestion` — 9 passed, 0 failed (log: `/tmp/stage/review-evidence/probe-trust-suggestion-tests-rebase.log`). The suite covers: clean record generates suggestion (asserts type, priority 8, title, all 5 body fields, repo_id None); 0-activity workspace (0/0 counts) generates suggestion; gate failure in window suppresses; MR revert in window suppresses; failure older than 30 days ignored; Autonomous workspace skipped; one notification per Admin member (Developer/Viewer excluded); dismissal < 30 days ago → no new notification; active suggestion not duplicated on re-run.
- Pre-rebase (original assignment): `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib` — 1344 passed, 0 failed; `npx vitest run src/__tests__/Inbox.test.js` (web, after `npm ci`) — 35 passed, including a test asserting "Increase Trust" calls `api.updateWorkspace('ws-1', { trust_level: 'Autonomous' })` and resolves the notification; `cargo fmt --all -- --check` clean. Logs: `/tmp/stage/review-evidence/probe-trust-suggestion-tests.log`, `probe-gyre-server-lib.log`, `probe-inbox-vitest.log`.

Full-workspace suites, all-target Clippy, and GitHub CI remain owned by verification/publication per the assignment.
