# Review — task-085 (HSI Trust Suggestions Background Job)

Spec: `specs/system/human-system-interface.md` §2 Trust Suggestions (lines 264–276).
Candidate: `a820e43d35f49b528b8376648320b3b37df91912` (base `a884329d7`). Product commit per
`commits:` frontmatter: `dfdd6cdeb` (plus two task-bookkeeping commits `0e023a527`/`a820e43d3`
touching only `specs/tasks/task-085.md`).
Verdict: **complete** — approved.

## Probes (this checkout, HEAD = candidate, working tree clean)

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib trust_suggestion` → **9 passed, 0 failed**
  (`/tmp/stage/review-evidence/probe-trust-suggestion-tests.log`). Covers: clean record generates
  suggestion (asserts type, priority 8, title, all 5 body fields, `repo_id: None`); 0-activity
  workspace (0/0 counts) generates suggestion; gate failure in window suppresses; MR revert in
  window suppresses; failure older than 30 days ignored; Autonomous workspace skipped; one
  notification per Admin member (Developer/Viewer excluded); dismissal < 30 days ago → no new
  notification; active suggestion not duplicated on re-run.
- `npm ci` + `npx vitest run src/__tests__/Inbox.test.js` (web) → **35 passed, 0 failed**
  (`/tmp/stage/review-evidence/probe-inbox-vitest.log`), including the new test asserting
  "Increase Trust" calls `api.updateWorkspace('ws-1', { trust_level: 'Autonomous' })` and
  `api.resolveNotification('notif-trust')`.
- `bash scripts/check-arch.sh` → OK. `bash scripts/check-task-commit-attribution.sh` → OK.
- `cargo fmt --all -- --check` could not run here (executor wrapper blocks workspace-wide
  invocations) — deferred to verification; diff visually conforms.

## Verified against the contract

- **Job registration**: `trust_suggestion_check` in `JobRegistry` (jobs.rs:475-490), daily
  (86400s), enabled, handler `crate::trust_suggestion::run_once` iterating all workspaces via
  `state.workspaces.list()`; per-workspace errors are warned-and-continued, not fatal.
- **Criteria are the spec's zero-count contract, not the base's percentage thresholds**: the
  base's recovered-checkpoint version (>80%/95% pass rate, <5%/2% revert rate, terminal-MR
  requirement, different title/body schema) is fully replaced. Gate failures = results whose own
  timestamp (`finished_at` else `started_at`) is within 30 days and status != `Passed`
  (Pending/Running counting as failure is conservative and matches the spec's literal
  "status != pass"); MR reverts = status `Reverted` with `reverted_at` (else `updated_at`) in
  window. Both counts 0 → suggestion.
- **Trust ladder**: Supervised → Guided, Guided → Autonomous; Autonomous skipped (acceptance
  criterion); Custom also skipped (no next preset rung — defensible, documented in code).
- **Notification shape**: `Notification::new` derives priority 8 from
  `NotificationType::TrustSuggestion::default_priority()` (gyre-common notification.rs:134);
  title "Consider increasing trust level"; body `{gate_failures, mr_reverts, period_days: 30,
  current_level, suggested_level}`; `repo_id: None`; one per Admin/Owner member, each dismissing
  independently. Owner inclusion is a reasonable superset of "Admin member" (Owner holds admin
  rights); Developer/Viewer exclusion is test-pinned.
- **Cooldowns are real and adapter-consistent**: per-user 30-day dismissal cooldown via
  `NotificationRepository::has_recent_dismissal` — implemented in mem (mem.rs:2160, filters
  workspace+user+type+`dismissed_at >= cutoff`), SQLite (Diesel `dismissed_at.ge(cutoff)`, NULL
  never matches), and Postgres identically. Active-suggestion dedup query filters priority 8..8,
  and TrustSuggestion is the only priority-8 kind, so the limit-20 window can only contain
  TrustSuggestions. `unwrap_or(false)` on the cooldown lookup fails open only to creating an
  advisory notification on a repo error — benign, no scope/security impact.
- **Inline actions (plan step 5)**: Inbox "Increase Trust" (Inbox.svelte `handleIncreaseTrust`)
  calls `PUT /api/v1/workspaces/:id` with `trust_level` from the body's `suggested_level`
  (spec-example fallback "Autonomous"), then resolves the notification; "Dismiss" calls
  `POST /api/v1/notifications/:id/dismiss` (route + handler confirmed in api/mod.rs:821,
  api/users.rs:342). The PUT handler strictly parses `trust_level` (400 on typo, no
  from_db_str Supervised fallback) — pre-existing endpoint, unchanged by this diff.
- **Tests are hard, not self-confirming**: they drive the real `run_once` against mem
  repositories (no logic mocking), assert on stored notification state (counts, fields,
  `dismissed_at`, `is_active()`), and the suppression tests assert emptiness on the negative
  paths. The dismissal-cooldown test dismisses through the real repository and re-runs the job.
- **Coverage matrix**: HSI row 15 task-assigned → implemented with an accurate, evidence-backed
  note (counts match the code and tests). Scope respected: diff touches only trust_suggestion.rs,
  jobs.rs description, web Inbox + test + locales, and task/spec bookkeeping. No scripts,
  exemptions, or unrelated specs modified.

Findings:

- (none)

Deferred to verification (per assignment): full `cargo test --all`, all-target Clippy,
workspace-wide `cargo fmt --check`, GitHub CI.
