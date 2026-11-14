# Review: TASK-056 — Abandoned Branch Detection Job

**Reviewer:** Verifier
**Round:** R1
**Verdict:** complete (0 findings)

## Summary

Implementation correctly follows the `trust_suggestion.rs` job pattern and satisfies all spec requirements from `ui-layout.md` §3 and `human-system-interface.md` §8.

### Verified

- [x] Job stub replaced with real logic in `jobs.rs`
- [x] `abandoned_branch.rs` module created and wired via `lib.rs`
- [x] Filters: `spec-edit/*` prefix, `MrStatus::Open`, `updated_at < now - 7d`
- [x] Notification: `AbandonedBranch` type, priority 9 (via `default_priority()`), title matches task spec
- [x] Notification fields: `entity_ref` (MR id), `repo_id` (repository_id), `body` JSON (branch, title, mr_id, repository_id, updated_at)
- [x] Recipients: workspace Admin and Developer members only (Viewer excluded)
- [x] Suppression: `has_recent_dismissal` (30-day, workspace+user+type scope, same pattern as trust_suggestion)
- [x] Deduplication: `list_for_user` + per-MR `entity_ref` check on active notifications
- [x] 7 test functions covering: happy path, role filtering (Admin/Dev/Viewer), non-spec-edit exclusion, merged exclusion, closed exclusion, recency exclusion, dismissal suppression, active deduplication
- [x] `cargo test --all` passes (0 failures)

---

**Reviewer:** Independent review agent
**Round:** R2 (candidate `8bc504b04bc8f8683a0f1dad559fdc2a1a066349`, base `6f5a631c9fc09664a3e766cd10536f8b56d9ce2f`)
**Verdict:** approved (0 blocking findings)

## Scope

Note on delta: base `6f5a631c` already carries the task-056 implementation from
earlier merged pipeline rounds (779-line `abandoned_branch.rs` + `jobs.rs`
wiring). The candidate delta over base touches only
`crates/gyre-server/src/abandoned_branch.rs` (dedup mechanism + one new test)
and `specs/tasks/task-056.md` (docs/frontmatter). The review assessed the whole
candidate tree against the task contract, with the delta examined line-by-line.

## Delta assessment

The candidate replaces the previous page-bounded dedup — a `list_for_user`
scan capped at 100 rows, which silently misses an existing active
`AbandonedBranch` notification past the first page and stacks a duplicate on
every daily run — with an exact storage lookup:
`NotificationRepository::has_active_notification(workspace, user, type,
entity_ref, since=0)`. This is a real correctness fix, not cosmetic.

Verified `has_active_notification` is implemented consistently in **all four**
adapters (hexagonal port-contract invariant):
- `gyre-ports/src/notification.rs:99` (trait: unresolved ∧ undismissed ∧ entity_ref match ∧ created ≥ since)
- `gyre-adapters/src/sqlite/notification.rs:427` (filters `entity_ref` column; column is persisted from `source.entity_id` at notification.rs:111)
- `gyre-adapters/src/postgres/notification.rs:427`
- `gyre-server/src/mem.rs:2829` (matches `source.entity_id`)
- `gyre-server/src/notifications.rs:678` (broadcasting wrapper delegates)

So `entity_ref`-based dedup semantics match across SQLite, PostgreSQL, mem, and
the broadcasting wrapper used by `AppState::create_notification`.

## Contract verification (candidate tree)

- `jobs.rs:465-477`: `abandoned_branch_check` registered daily (86400 s), real
  closure `crate::abandoned_branch::run_once`; `lib.rs:5` exports the module.
- Filter: `source_branch.starts_with("spec-edit/") && status == MrStatus::Open
  && updated_at < now − 604800` — matches ui-layout.md §3 ("MR `updated_at`
  older than 7 days and MR still open", "uses the MR's `updated_at` timestamp
  (updated on every commit push), not the git branch directly").
- Recipients: Admin + Developer only; Viewer excluded (test asserts all three).
- Notification: priority rank 9 via `default_priority_rank()`
  (gyre-common notification.rs:291); title matches the task spec verbatim;
  `entity_ref` = MR id, `repo_id` = MR's `repository_id`, workspace/tenant from
  the workspace; JSON body carries branch/title/mr_id/repository_id/updated_at.
- Creation routes through `AppState::create_notification` →
  `BroadcastingNotificationRepo`, so live `/notifications/stream` clients see it.
- Dismissal suppression: `has_recent_dismissal(ws, user, type, 30)` — exactly
  the mechanism the task plan prescribes ("same pattern as trust_suggestion").
- Fail-safety: per-workspace errors are warned and skipped (one bad workspace
  cannot abort the pass); suppression-check storage errors default to
  "suppress creation" (no inbox spam).

## Test assessment (9 tests)

All six plan-required cases are covered (happy path incl. entity_ref + repo_id
assertions, role filtering, non-spec-edit, merged, closed, recency, dismissal
suppression, active dedup), plus `dedup_is_not_page_bounded`. The new test is
mutation-distinguishing: 150 newer active `AbandonedBranch` notifications push
the target's existing notification past any newest-first 100-item page (mem
`list_for_user` sorts priority_rank then `created_at` desc), so restoring the
old paginated dedup makes it fail (count 2) while the exact lookup passes
(count 1). Traced the test's data through the mem adapter semantics manually;
the expected outcome holds. Negative tests are not vacuous: the query filters
match the created notifications' shape (priority_rank 9 band).

## Probes run (evidence in /tmp/stage/review-evidence)

- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo check -p gyre-server --lib --tests`
  → **exit 0** (8 m 35 s cold). Test call sites type-check against the current
  9-arg `list_for_user` and 5-arg `has_active_notification` port signatures.
- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo test -p gyre-server --lib
  abandoned_branch` → **blocked by the executor's 4 GiB build wrapper** (exit
  64, by design for this stage). Not a code defect.

## Verifier must run

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib abandoned_branch` (9 tests)
- Standard workspace suite (`cargo test --all`) per verification policy.

## Non-blocking observations

1. `has_recent_dismissal` is scoped (workspace, user, type), not per-MR — the
   port has no entity parameter and the task plan explicitly prescribes the
   trust-suggestion pattern. Consequence: dismissing one `AbandonedBranch`
   notification suppresses re-notification for all abandoned branches for that
   user for 30 days. Matches the specified mechanism; noting for spec fidelity.
2. `Approved`/`Reverted` MR statuses are not flagged (only `Open` is) — exactly
   what the task plan specifies ("status is Open (not merged, not closed)").
