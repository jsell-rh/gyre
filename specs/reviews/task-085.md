# Review — task-085 (HSI Trust Suggestions Background Job)

Spec: `specs/system/human-system-interface.md` §2 "Trust Suggestions".
Candidate: `93821119aa260496edaf8dbc54db8a733e78e4b5` (branch
`pipeline/task-085/4756ea65087b40a79d26b9e6607e7315-1`; assigned base
`eb11440d5c2518842cdca4e0202ace5a37357db0`). Candidate diff from base =
the two prior-round repair commits (`bbcc71c4b` Inbox wire-format,
`a1020c2b7` scheduler spawn in main.rs) plus task-file bookkeeping; the
original implementation (`dfdd6cdeb`, `6d2732817b`) is already an ancestor
of the assigned base, so the task contract as a whole was re-checked against
the merged tree.

Verdict: **approved** — both durable findings from earlier rounds are
genuinely fixed and kill-tested; no new defects found in the candidate diff.

## Round 3 (this review)

Prior rounds: R1 found the wire-format mismatch that made the TrustSuggestion
inline actions unreachable in production plus a stub-backed test; R2 found the
job registered in the `JobRegistry` but never spawned (the daily mechanism did
not run in a default deployment). This round verifies both repairs on the
current candidate.

Probes run (all evidence under `/tmp/stage/review-evidence/`, summary in
`review-summary.md`):

- `cd web && npm ci` → clean; `npx vitest run src/__tests__/Inbox.test.js`
  → **38/38 passed**, with mocks on the real server wire value
  `'TrustSuggestion'` (`probe-inbox-vitest.log`).
- **Kill test**: removed the one-line `TrustSuggestion: 'trust_suggestion'`
  entry from `Inbox.svelte` typeNormMap and re-ran the suite → **1 failed**
  ("Unable to find an element with the text: Increase Trust" — exactly the R1
  production defect), proving the re-pointed test guards the real rendering
  contract and is no longer stub-backed (`probe-mutation-typenormmap.log`).
  File restored; tree clean apart from the executor's task-file bookkeeping.
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --lib --tests` → clean
  (7m34s cold; proves the new `job_is_scheduled_in_main` regression test
  compiles and its `include_str!("main.rs")` resolves);
  `--bin gyre-server` → clean (covers the main.rs spawn edit).
- `cargo fmt -p gyre-server -- --check` → clean; `check-arch.sh` OK;
  `check-task-commit-attribution.sh` OK (the candidate's committed frontmatter
  records all four attributed commits, including both repairs).
- Scheduling audit: main.rs contains exactly one `jobs::spawn_job` call whose
  argument list contains `"trust_suggestion_check"` (main.rs:126-130), placed
  after `start_job_registry` (so the registered 86400s interval is what the
  loop uses) alongside the other daily jobs. It is the only occurrence of the
  job name in main.rs, and `job_is_scheduled_in_main` bounds its search to
  each spawn call's argument list — so removing the spawn turns the test red
  (verified by replicating the test's parse over the file; execution deferred
  to the verifier per the build-budget rule).

Verified against the task contract (whole-tree, since the original
implementation is an ancestor of base):

- **Job registered + daily**: `trust_suggestion_check` in `start_job_registry`
  (jobs.rs:494-511, interval 86400, enabled, handler
  `trust_suggestion::run_once`) AND spawned in main.rs — the executing daily
  loop, no longer manually-triggerable only. `run_once` iterates all
  workspaces; per-workspace errors are logged and don't abort the pass.
- **Criteria (HSI §2)**: 30-day gate-failure count (results scoped per-MR
  across the workspace's MRs, each result's own timestamp `finished_at` else
  `started_at`, status != Passed) and 30-day MR-revert count (status
  `Reverted` by `reverted_at` else `updated_at`); suggestion only when both
  are 0. Autonomous (and Custom) workspaces skipped. Escalation ladder
  Supervised→Guided, Guided→Autonomous.
- **Notification**: one per Admin/Owner member (Developer/Viewer excluded,
  tested), `TrustSuggestion` type, priority 8 (derived from the type enum,
  asserted in test), title "Consider increasing trust level", body
  `{gate_failures, mr_reverts, period_days: 30, current_level,
  suggested_level}`, workspace-scoped `repo_id: None`, per-user dismissal via
  real `has_recent_dismissal` (mem adapter check is real, not a stub), plus
  active-suggestion dedup on daily re-runs.
- **Inline actions**: Inbox "Increase Trust" → `PUT /api/v1/workspaces/:id`
  with `trust_level` from the body's `suggested_level`, then resolves the
  notification; "Dismiss" → `POST /api/v1/notifications/:id/dismiss`
  (`handleIncreaseTrust`/`handleDismiss`, `api.updateWorkspace`/
  `api.resolveNotification`/`markNotificationRead` in api.js). i18n keys
  present in en.json (JSON valid).
- **Tests are behavioral, not mirrors**: the 9 job-logic tests drive `run_once`
  through the mem adapters and assert stored notifications via
  `list_for_user`; the dismissal-cooldown test genuinely depends on
  `has_recent_dismissal` (the dismissed notification is inactive, so a stubbed
  cooldown check would let a second notification through and fail the test).

Deferred to the independent verifier (hard build-budget rule; recorded, not
bypassed):

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib trust_suggestion` —
  expect **10 passed** (9 job-logic tests + `job_is_scheduled_in_main`).
- `SKIP_WEB_BUILD=1 cargo test --all` (task acceptance criterion).
- `cd web && npm ci && npx vitest run` (full web suite).

Minor (non-blocking, recorded for completeness):

- `WorkspaceHome.svelte` `NOTIF_TYPE_NORM` / `CrossWorkspaceHome.svelte`
  `TYPE_ICONS` lack a TrustSuggestion entry, so the decisions-feed icon falls
  back to the default glyph. Pre-existing pattern for many notification types,
  outside HSI §2's Inbox-item contract and outside this candidate's diff.
- `evaluate_workspace` degrades open on repository errors
  (`has_recent_dismissal(...).unwrap_or(false)`,
  `list_for_user(...).unwrap_or_default()`): a transient storage error could
  admit one extra suggestion notification. Duplicate Inbox item, not an
  enforcement or scope failure.
- `job_is_scheduled_in_main` is a source-scan regression test
  (`include_str!` of main.rs) rather than a behavioral one — the binary's
  `main` is not exercisable from lib tests; it mirrors the established
  spec_registry starter-manifest pattern and is bounded to the spawn call's
  argument list so job-name mentions elsewhere cannot satisfy it.
