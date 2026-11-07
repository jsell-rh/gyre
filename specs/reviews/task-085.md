# Review: TASK-085 — HSI Trust Suggestions Background Job

**Reviewer:** Independent review (pi runtime)
**Date:** 2026-11-07
**Comparison base:** 4d6759a0bfd637e899bbf65e725f91b07f745c69
**Candidate (HEAD):** 2c138172fabab5c0a4900e9d1b58f2928bcf7b85 (merged tree includes rebase of branch onto base)
**Verdict:** approved — task contract satisfied with production code

---

## Scope of diff (base → candidate)

`crates/gyre-server/src/trust_suggestion.rs` (rewrite of the recovered-checkpoint
percentage-threshold version to the spec's zero-count criteria),
`crates/gyre-server/src/jobs.rs` (job description text only — the
`trust_suggestion_check` registration, daily 86400s, enabled, already existed at
base and is carried through the merge), `web/src/components/Inbox.svelte`
(`handleIncreaseTrust` now calls `PUT /api/v1/workspaces/:id` with
`body.suggested_level` and resolves the notification instead of navigating
away), `web/src/__tests__/Inbox.test.js` (+1 behavioral test),
`web/src/locales/en.json` (+2 i18n keys), `specs/coverage/system/human-system-interface.md`
(row 15 → implemented; header counts recomputed), `specs/tasks/task-085.md`
(shipped notes + commit attribution).

## Acceptance criteria — all verified against the merged tree

- [x] **Job registered in JobRegistry** — jobs.rs:475-490, `trust_suggestion_check`,
  `interval_secs: 86400`, `enabled: true`, runs `trust_suggestion::run_once`.
- [x] **Runs daily per workspace** — `run_once` lists all workspaces and evaluates
  each; per-workspace errors are logged and don't abort the pass.
- [x] **Skips Autonomous** — match on `ws.trust_level`: Supervised→Guided,
  Guided→Autonomous, Autonomous/Custom return early. Mutation-verified (see below).
  Custom skip is a documented interpretation: the spec's ladder tops out at
  Autonomous and defines no suggestion target for Custom (noted in shipped notes
  and coverage row 15).
- [x] **Queries 30-day gate failures and MR reverts** —
  `count_gate_failures` walks the workspace's MRs, scopes per-MR gate results,
  window membership by each result's own timestamp (`finished_at` else
  `started_at`), `status != Passed` counts (Pending/Running count conservatively,
  matching the plan's literal "gate status != pass").
  `count_mr_reverts` counts `MrStatus::Reverted` by `reverted_at` else
  `updated_at` within the window.
- [x] **Creates notifications only when both counts are 0** — single
  `if gate_failures > 0 || mr_reverts > 0 { return }`. Mutation-verified.
- [x] **One per Admin member (not per workspace)** — per-member loop gated on
  `WorkspaceRole::Admin | WorkspaceRole::Owner`; Developer/Viewer excluded
  (asserted in test). Owner inclusion is a documented interpretation of the
  spec's "Admin member" (Owner holds admin rights; excluding it would leave
  Owner-only workspaces unnotified). Mutation-verified.
- [x] **30-day dismissal cooldown per user** —
  `NotificationRepository::has_recent_dismissal(ws, user, "TrustSuggestion", 30)`
  checked per member before creation; plus active-suggestion dedup on daily
  re-runs. Mutation-verified. Mem/SQLite/Postgres adapters implement
  `has_recent_dismissal` with identical semantics (check-mem-port-contracts passes).
- [x] **Type/priority/body correct** — `NotificationType::TrustSuggestion`
  (default priority 8), title "Consider increasing trust level", body
  `{gate_failures, mr_reverts, period_days: 30, current_level, suggested_level}`
  (actual current level, not hardcoded), `repo_id: None`, `tenant_id` derived
  from the workspace (no fabricated "default"). Asserted field-by-field in
  `suggestion_created_when_record_clean`.
- [x] **Unit tests** — clean record generates suggestion (incl. 0-activity
  workspace, which legitimately qualifies at 0/0); gate failure in window
  suppresses; MR revert in window suppresses; out-of-window failure ignored;
  Autonomous skip; per-admin delivery; dismissal cooldown; rerun dedup.
- [x] **`cargo test --all`** — workspace-wide suites belong to verification per
  the assignment's own constraints. Focused proxies on the exact merged tree:
  `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib` → **1354 passed, 0 failed**;
  `npx vitest run src/__tests__/Inbox.test.js` (after `npm ci`) → **35 passed**.
  Deferred checks recorded in `/tmp/stage/review-evidence/deferred-checks.md`.

## Inline actions (plan step 5)

- "Increase Trust" → `api.updateWorkspace(n.workspace_id, { trust_level: body.suggested_level })`
  then `resolveNotification` (Inbox.svelte `handleIncreaseTrust`); the server
  `PUT /api/v1/workspaces/:id` handler parses/validates `trust_level` for real.
- "Dismiss" → `POST /api/v1/notifications/:id/dismiss`, whose handler enforces
  per-user ownership + cross-tenant guard (users.rs:339-363).
- Frontend test asserts both calls with exact arguments.

## Mutation evidence (each killed by the suite; source restored after each)

Working tree mutations on `trust_suggestion.rs` / `Inbox.svelte`, restored to
candidate tree afterwards (final `git status` clean, HEAD == 2c138172f):

1. `if gate_failures > 0 || mr_reverts > 0` → `if false` (never suppress):
   `no_suggestion_when_gate_failure_in_window` + `no_suggestion_when_mr_reverted_in_window` FAILED.
2. Remove Autonomous/Custom early-return (suggest at top of ladder):
   `no_suggestion_for_autonomous_workspace` FAILED.
3. `has_recent_dismissal` check stubbed to `false`:
   `dismissal_cooldown_suppresses_new_suggestion` FAILED.
4. Drop the Admin/Owner role gate (notify all members):
   `one_notification_per_admin_member` FAILED.
5. Revert `handleIncreaseTrust` to the base behavior (navigate+dismiss, no API
   call): new Inbox vitest `trust_suggestion: Increase Trust updates the
   workspace trust level and resolves` FAILED.

Logs: `/tmp/stage/review-evidence/mutation{1..5}-*.log`.

## Invariant gates

- `scripts/check-arch.sh` — pass.
- `scripts/check-mem-port-contracts.sh` — pass.
- `scripts/check-task-commit-attribution.sh` — pass (all task-085-labeled
  commits in `4d6759a0..2c138172f` attributed).
- `rustfmt --check` on the two touched Rust files — pass.
- Coverage matrix: row 15 → `implemented`; header recomputed and arithmetically
  consistent (55 rows = 20 n/a + 17 task-assigned + 11 implemented + 7 verified;
  18/35 = closed/implementable; `| not-started |` count = 0).

## Non-blocking observations (pre-existing at base, unchanged by this candidate)

- `has_recent_dismissal(...).unwrap_or(false)` and the active-dedup
  `list_for_user(...).unwrap_or_default()` fail open on repository errors — a
  transient DB error could produce one duplicate inbox item. Identical code
  existed at the comparison base; not introduced here.
- The active-dedup query limits to 20 priority-8 rows; >20 concurrent priority-8
  notifications for one user in one workspace could push an active
  TrustSuggestion out of the page. Also pre-existing at base, marginal in practice.
