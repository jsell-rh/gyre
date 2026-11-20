# Review — task-270 (PR collaboration invariants)

Candidate: `fae0c251c2b59feae5ff86ffb0546220de74e6a6` (base `cae04d59`)
Verdict: **approved** — independent evidence supports the task contract.
Evidence: `/tmp/stage/review-evidence/` (probes, logs, source-identity
analysis).

## What was reviewed

Diff `cae04d59..fae0c251` (12 files, ~2.4k insertions): agent-principal
default-branch push protection in `git_http.rs`, the `pr_hygiene.rs`
background job + `jobs.rs`/`main.rs` registration, five new
`NotificationType` variants wired through gyre-common/email/web, and the
test updates in `git_http.rs` (lib), `git_integration.rs`,
`e2e_ralph_loop.rs`.

## Push protection — verified

- Direct, unconditional check in `git_receive_pack` before
  `run_git_stateless("receive-pack", …)`: agent principals
  (`auth.user_id.is_none() && !auth.is_system_principal()`) pushing (or
  deleting, via raw command matching) `refs/heads/{default_branch}` get a
  403 whose body directs to the MR flow; the ref is never updated.
  Humans (user JWT / API key → `user_id` Some) and the genuine system
  principal stay exempt, preserving spec-lifecycle.md §Automatic Task
  Creation — proven by the `human_push_to_default_branch_accepted_and_
  spec_lifecycle_runs` test (spec-change task created) and the 17 passing
  git_integration tests that push main with the dev token.
- `default_branch` is resolved from the Repository record (tested with a
  "trunk"-default repo; a literal "main" branch there is not blocked).
- No pkt-line bypass: capabilities are NUL-stripped, SHAs validated,
  deletions included in the raw command list.
- Rejection is observable: `PushRejected` bus event + `GitPushRejected`
  audit (gate `default-branch-protection`, outcome Blocked).
- Server-side landing unaffected: `merge_queue_auto_merges_mr_and_
  commit_on_main` (merge queue lands via the git_ops port) passed in the
  retained verification run.

## PR hygiene job — verified

`pr_hygiene_check` registered in `jobs.rs` on an interval (default 300 s,
`run_at_utc_hour: None`) and spawned in `main.rs` via the same
`spawn_job` pattern as sibling interval jobs. Each pass evaluates every
workspace's open MRs against the stale classes with real persisted state
(`updated_at`, reviewers, review records, `has_conflicts` + a real
`GitOpsPort::can_merge` merge-base check), one most-severe action per MR:
abandoned → close via domain `transition_status` with persisted
transition, explanatory MR comment, author delivery, Admin/Owner
notifications, audit; conflicted → jj rebase for active agent worktrees
(shared KV backoff with the merge processor) else `MrNeedsRebase`
escalation + audit; long-open → priority-3 `MrStaleEscalation` to
Admins/Owners; unreviewed → `ReviewerNudge` to reviewers, or
`MrUnreviewed` escalation to Developers/Admins when none are assigned.
Idempotence: user notifications dedupe through
`has_active_notification` (entity-ref scoped), agent bus nudges through
a persisted KV backoff keyed (MR, agent); both paths have
two-consecutive-evaluation tests. Thresholds env-configurable with sane
defaults. The daily `abandoned_branch_check` job is untouched.

## Independent probe evidence

- `cargo check -p gyre-server` — exit 0 (lib).
- `cargo check -p gyre-server --tests` — exit 0 (all new cfg(test) code
  and the modified integration tests type-check).
- `cargo test -p gyre-common` — 129 passed, 0 failed (notification
  taxonomy/priority/band tests cover the five new variants).
- Retained verification run (`/tmp/stage/findings.log`): full
  `cargo test --all` on tree `2f56050a`, whose production source is
  byte-identical to the candidate (`git diff 2f56050a fae0c251 -- …/src`
  is empty; only the spec-approval test's token and task docs changed
  afterwards): gyre-server lib 2564/2564 passed, git_integration 17/18 —
  the single failure was the pre-repair test pushing main with an AGENT
  token and getting 403, which is itself runtime proof the protection
  rejects real agent pushes; fixed in `5b5ac085` by driving that test's
  pushes with the system token (an exempt caller class exercised by the
  passing tests).

## Deferred to the verifier (gyre-server codegen blocked in review stage)

`cargo test -p gyre-server --lib`, `--test git_integration`,
`--test e2e_ralph_loop`, full `cargo test --all`, and `cd web && npm ci
&& npm test` (the UserProfile test pins `NOTIF_TYPE_IDS` against the
gyre-common enum source, so the frontend wiring is enforced).

## Non-blocking observations

1. Thresholds are env-configurable server settings (matching the
   `log_compression` job convention); per-repo overrides were not built.
2. A persistently-conflicted agent MR is rebase-attempted once per
   interval (60 s backoff < 300 s interval); bounded, audited churn of
   the specced "rebase" action.
3. Auto-close explanation reaches the author through `author_agent_id`
   resolution; an MR with no author id still gets the MR comment and
   Admin/Owner notifications (never a silent close).
4. The 403 body is not rendered by the git CLI (same as sibling 403
   paths in this handler); the reason is in logs, the bus event, and the
   audit event.
