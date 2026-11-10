# Review — task-085 (HSI Trust Suggestions Background Job)

Spec: `specs/system/human-system-interface.md` §2 "Trust Suggestions".
Candidate: `c85e15f5dfec667452bbf5f8dbd5ace69088b11e` (branch tip; assigned base `eb11440d5`).
Scope of the candidate diff (base→candidate): the wire-format repair `bbcc71c4b`
(Inbox.svelte typeNormMap + Inbox.test.js mocks) plus task-file bookkeeping. The
original job implementation (`dfdd6cdeb`, `6d2732817b`) is already an ancestor of
the assigned base (merged upstream via PR #773), so the task contract as a whole
was re-checked against the merged tree.

Verdict: **needs-revision** — one material finding (F1: the registered job is
never scheduled, so it does not run daily). The prior round's durable findings
(wire-format mismatch + stub-backed test) are genuinely fixed and kill-tested.

## Round 2 (this review)

Probes run (all evidence under `/tmp/stage/review-evidence/`):

- `cd web && npm ci && npx vitest run src/__tests__/Inbox.test.js` → **38/38
  passed** with the real wire value `'TrustSuggestion'`
  (`probe-inbox-vitest-candidate.log`).
- Kill test: removed the one-line `TrustSuggestion: 'trust_suggestion'`
  normalization entry from `Inbox.svelte` typeNormMap and re-ran the suite →
  **1 failed** (`findByText('Increase Trust')` times out — the action buttons
  never render), proving the re-pointed test guards the real rendering
  contract (`probe-inbox-killtest.log`). File restored; `git status` clean.
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` → clean (`probe-cargo-check-server.md`).
- `bash scripts/check-task-commit-attribution.sh` → OK.
- Scheduling audit of `trust_suggestion_check` → never spawned
  (`probe-job-not-scheduled.md`, see F1).

Verified working (no findings):

- **Wire-format repair is real and complete.** Server serializes
  `notification_type` via `NotificationType::as_str()` → PascalCase
  `'TrustSuggestion'` (`api/users.rs:244`, `notification.rs:164`).
  `Inbox.svelte:148` now maps `TrustSuggestion → 'trust_suggestion'`; the
  template branch (`:690`) and `TYPE_VARIANTS` badge entry (`:49`) match the
  normalized value, so [Increase Trust]/[Dismiss] render for real
  notifications. All three test mocks re-pointed at the wire value.
- **Inline actions match plan step 5.** `handleIncreaseTrust` (`Inbox.svelte:336`)
  calls `api.updateWorkspace(n.workspace_id, { trust_level: body.suggested_level || 'Autonomous' })`
  then resolves the notification; `handleDismiss` POSTs `/notifications/:id/dismiss`
  (via `api.markNotificationRead`, legacy name, correct endpoint).
  `PUT /api/v1/workspaces/:id` strictly parses `trust_level` (400 on typo, no
  fallback) and authorizes same-tenant callers — a workspace Admin recipient
  can act on the suggestion.
- **Job evaluation logic is spec-exact** (code-read of `trust_suggestion.rs`):
  0 gate failures ∧ 0 MR reverts in 30 days (per-result timestamp decides
  window membership; `reverted_at` for reverts), Autonomous/Custom skipped,
  one notification per Admin/Owner member (Developer/Viewer excluded, tested),
  priority 8, spec title, 5-field body, `repo_id: None`, per-user 30-day
  dismissal cooldown via `NotificationRepository::has_recent_dismissal`
  (genuine SQL in the SQLite and PG adapters, `sqlite/notification.rs:309-341`),
  active-suggestion dedup on re-runs. 9 unit tests cover the acceptance
  criteria (could not be executed here — see verifier list below).
- Registration itself is correct: `trust_suggestion_check` in
  `start_job_registry` (`jobs.rs:494-511`), `interval_secs: 86400`, enabled,
  handler runs `trust_suggestion::run_once` over all workspaces.

Findings:

- [-] **F1 (major): the job is registered but never scheduled — it does not
  "run daily per workspace", so the HSI §2 mechanism is inert in production.**
  No `spawn_job("trust_suggestion_check")` call exists in `main.rs` (or
  anywhere), and `trust_suggestion::run_once` has no production caller besides
  the registry handler. `interval_secs: 86400` is display metadata only — the
  executing scheduler loop is `jobs::spawn_job` (`jobs.rs:280`), started in
  `main.rs` for six other jobs (retention_cleanup, meta_spec_stale_pin_check,
  secret_rotation_check, meta_spec_conformance_sweep, impersonation_expiry,
  log_compression) but not this one. The registry's own liveness model reports
  the job `NotScheduled` (`jobs.rs:237-240`), it is absent from `healthz`
  `REQUIRED_JOBS`, `git log --all -S` shows it was never spawned in any
  commit, and no external cron/operator trigger is documented in `docs/` or
  `scripts/`. Consequence: in a default deployment the job never runs and no
  TrustSuggestion notification is ever created automatically, while the 9 unit
  tests stay green because they invoke `run_once` directly. This violates HSI
  §2 ("A background job (`trust_suggestion_check`) runs daily per workspace")
  and the task AC "Job runs daily per workspace" (a distinct criterion from
  "registered in JobRegistry" — registration alone would make the two
  redundant). `specs/reviews/task-151.md:32` already recorded this
  register-only family as "NOT in the scheduled set … If those jobs are meant
  to run, that is a separate gap" — for task-085 it is in-scope. Note the gap
  pre-dates this candidate's diff (it ships from `dfdd6cdeb`/`6d2732817b`,
  already in base), but it is task-085's own attributed work and its coverage
  row 15 claims §2 `implemented`/REAL, which is hollow for the mechanism
  clause (goal rule 3).

  **Repair path (one line + bookkeeping):** in `main.rs`, alongside the other
  daily jobs, add
  `jobs::spawn_job(Arc::clone(&state.job_registry), "trust_suggestion_check".to_string(), Arc::clone(&state));`
  — the fixed-interval path (`run_at_utc_hour: None`) runs it every 86400s
  from process start. Then record the repair commit in `task-085.md`
  `commits:` frontmatter. A regression guard is welcome but optional (e.g.
  assert the job appears in `scheduled_jobs()` after server startup, or a
  main.rs wiring test mirroring the existing spawn coverage).

Minor (non-blocking, recorded for completeness):

- `WorkspaceHome.svelte:107` `NOTIF_TYPE_NORM` lacks a `TrustSuggestion`
  entry, so a trust suggestion in the Workspace Home decisions panel renders
  with the raw `'TrustSuggestion'` type label and fallback `?` icon. Cosmetic;
  §2's specced surface is the Inbox, which is fixed. `CrossWorkspaceHome`
  has the same fallback-display pattern for all wire types.
- The hand-maintained coverage header in
  `specs/coverage/system/human-system-interface.md` (11 implemented /
  17 task-assigned) is stale vs actual rows (13/15) — pre-existing at the
  assigned base, untouched by this candidate.
- The working tree's `specs/tasks/task-085.md` (executor-staged assignment
  body) lists `commits: [dfdd6cdeb, 6d2732817b]` and omits the repair commit
  `bbcc71c4b`; the candidate commit `c85e15f5` itself records all three.
  Attribution check passes at HEAD.

Test-inflation check: none found in the repaired tests — the re-pointed mocks
use the real wire value and the kill test proves the Increase Trust test fails
when the normalization is removed. The pre-existing `trust_suggestion.rs`
suite exercises real state through the mem adapters (including the cooldown
via a real `dismiss` call), but see F1: nothing covers the scheduling link.

Verifier must run (blocked here by the 4 GiB build budget — server test
compilation is rejected by the executor wrapper):

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib trust_suggestion` (9 tests)
- `SKIP_WEB_BUILD=1 cargo test --all` (task AC)
- Optionally the full web suite: `cd web && npm ci && npx vitest run`
  (Inbox.test.js alone run here: 38/38).
