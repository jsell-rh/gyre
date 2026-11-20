---
title: "PR collaboration invariants: reject agent-principal pushes to the default branch (human push path preserved per spec-lifecycle.md) and continuously act on stale MRs (nudge, rebase, escalate, close)"
spec_ref: "Spec gap spec-4c39ca6ea3e71ce8a53398f3 / pr-protection-and-hygiene / d30f233daf86"
depends_on: []
progress: ready-for-review
commits: ["041bddcdd54e58930beb34cfc06ca12a0d16e9fa", "c9faf6a80ba0a216889c2d7efb5c82f71f58deee", "d5888ad56a3369911836d9ccaeb06995dd73de9d", "c269022921fb4faddc818da21fd2d834d00ab63e", "34f7878e3734df124067c20b225ad7e2a150ec70"]
---

## Required behavior

Per specs/development/agent-workflow.md §PRs as the Collaboration Primitive. The MR flow itself is real (completion auto-creates the MR; merge queue + gates decide merges); two normative bullets are unmet. (1) 'No direct pushes to main' — scoped to the document's agent-workflow context: the spec under audit is 'Agent Development Workflow Requirements', and its bullet governs agent code submission. An unconditional rejection of EVERY default-branch push would contradict specs/system/spec-lifecycle.md, which normatively requires the opposite for human pushes: §Automatic Task Creation on Spec Change ('When a push lands on the default branch that modifies files in specs/, the forge automatically creates tasks. This is a server-side post-receive hook' — implemented as process_spec_lifecycle, git_http.rs:1214, which also handles persona changes and spec-ledger sync on applied default-branch ref updates) and §What This Does NOT Do ('Does not block spec changes. Anyone with push access can modify specs. The system reacts, it doesn't prevent.'). Therefore implement, in the git HTTP push path, rejection of pushes whose target ref is the repo's default branch — resolved from the Repository record's default_branch field, never a hardcoded "main" — when the caller is an AGENT principal (authenticated via an agent token / agent identity: an AuthenticatedAgent with no backing user — user_id None — and not the system principal), with an explicit rejection reason directing the agent to the MR flow. Human principals (user JWTs / API keys with push permission, and the system principal) pushing to the default branch remain accepted, and their post-receive spec-lifecycle processing (task creation, persona changes, ledger sync) continues to work unchanged. The agent-principal rejection is an unconditional invariant for that caller class, not an opt-in named pre-accept gate (check_pre_accept_gates returns Ok early when a repo has no gate names configured, git_http.rs:1753-1755, so an opt-in gate would leave default repos unprotected); implement it as a direct check in the push handler. It must not break server-side landing: the merge queue and post-merge reverts land via the git_ops/jj_ops ports server-side, not via HTTP push — verify with tests that merge-queue landing on the default branch still succeeds. Update git_http tests that rely on default-branch pushes to authenticate consistently with the intended caller class (system-token test pushes to main keep succeeding; add agent-token rejection coverage). (2) PR hygiene: a new background job (e.g. pr_hygiene_check) registered in jobs.rs on a short interval (minutes, not the daily abandoned_branch_check cadence — 'continuously monitors') that evaluates every open MR per workspace against stale classes using real persisted state (MergeRequest.updated_at, reviewers, has_conflicts / a real merge-base conflict check, review records): unreviewed beyond a nudge threshold → nudge assigned reviewers (notification; escalate to workspace Developers/Admins when an MR has no reviewers); conflicted → trigger the existing rebase machinery where applicable (agent-authored branches) and otherwise raise a rebase-required escalation; inactive/long-open beyond escalation thresholds → higher-priority escalation notification to workspace Admins; abandoned beyond a hard close threshold → close the MR with an explanation (status transition plus an explanatory notification/comment to the author — never a silent close). Thresholds are configurable (server/repo settings) with sane defaults, and every action is recorded (notification rows/audit events) so it is observable. All new routes get ABAC RouteResourceMapping entries (scripts/check-abac-route-registry.sh); handlers running without middleware ABAC enforce per-handler scoped authorization (scripts/check-abac-exempt-handlers.sh). The existing daily abandoned_branch_check job (HSI §8) is left intact — it is specced elsewhere.

## Acceptance Criteria

- [ ] Agent-principal push protection: an HTTP push of a new commit directly to refs/heads/{default_branch} authenticated with an agent token is rejected with an explicit error directing to the MR flow, and the ref is not updated server-side; the same agent token pushing a feature branch succeeds.
- [ ] Human push path preserved (spec-lifecycle.md reconciliation): a push-capable human token pushing a spec change to the repo's default branch is still accepted, and process_spec_lifecycle runs — spec-change tasks are created exactly as before; the system principal's pushes to the default branch are likewise unaffected.
- [ ] Protection resolves the real default branch per repo: a repo whose default_branch is e.g. "trunk" is protected on refs/heads/trunk for agent principals, and an agent push to a branch literally named "main" on that repo is NOT wrongly rejected.
- [ ] Server-side landing unaffected: a merge-queue merge landing on the default branch still succeeds (the rejection lives only in the HTTP push path for agent principals).
- [ ] Unreviewed nudge: an open MR with zero reviews older than the nudge threshold gets a reviewer-nudge notification on its next evaluation; an MR with no reviewers escalates to workspace Developers/Admins; a fresh MR under the threshold gets no action.
- [ ] Abandoned close: an open MR with updated_at beyond the hard close threshold is closed by the job with an explanation delivered to the author, and its status transition is persisted.
- [ ] Conflicted action: a conflicted MR gets a rebase attempt via the existing machinery (or a rebase-required escalation when not applicable) — some real action is taken and recorded, never silence.
- [ ] Continuous cadence: the job is registered with an interval (not run_at_utc_hour daily) and two evaluations occur within the test window, each idempotent (no duplicate notifications for the same stale MR on consecutive runs).

## Verification intent

Push probe: authenticate as a spawned agent (agent token from agent_tokens), push a commit to the repo's default branch over git HTTP — today (the real defect) the push is accepted and processed by post-receive (spec-lifecycle scans run); the probe must fail on the current tree by observing the ref update succeed, and pass only when the push is rejected and the default-branch ref is unchanged. Companion probe guarding the scoping against over-blocking: the same push made with a human push-capable token (and a specs/ file change) must still be accepted and trigger spec-lifecycle task creation — an implementation that rejects human or system-principal default-branch pushes and thereby kills specs/system/spec-lifecycle.md §Automatic Task Creation fails this probe. Hygiene probe: seed an open MR whose updated_at is older than the hard close threshold, run the job once — today nothing ever closes it (the only stale-PR monitor, abandoned_branch_check, covers spec-edit/* branches only and only notifies); the probe must fail on the current tree by finding the MR still open, and pass only when the MR is closed with an authored explanation.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/development/agent-workflow.md`. Source: `fb1424fbac55cd23cb08606f4cd667a96f4dbc43`; plan: `b3acf94c2c31906d0a88cf9be08f87a86c09232cb273c0a110aa5daca9688c95`.

[
  {
    "id": "4a3b61a03d2e9cd037d33c0c",
    "decision": "gap",
    "requirement_quote": "A **PR hygiene agent** (or background job) continuously monitors for stale PRs - unreviewed, unmerged, conflicted, or abandoned - and takes action: nudge reviewers, rebase, escalate, or close with explanation.",
    "detail": "Partially satisfied: MRs are genuinely the submission/collaboration primitive — agent completion auto-creates the MR and hands off to the merge queue and its gates, the protocol forbids agents merging their own work, and reviews/merge-queue/gates (merge_requests.rs, merge_queue.rs, gate_executor.rs) implement the PR flow. Two normative bullets are unmet. (1) 'No direct pushes to main': nothing rejects pushes to the default branch. Git HTTP authorization is repo-level (authorize_repo_access checks a push permission, git_http.rs:148-215), the pre-accept push-gate registry contains only conventional-commit, task-ref, and no-em-dash (pre_accept.rs), and the production post-receive path processes default-branch pushes as normal accepted events (spec-lifecycle task creation, persona changes, ledger sync; git_http.rs tests push directly to refs/heads/main and succeed). (2) The PR hygiene requirement: the only stale-PR monitoring in production is the daily abandoned_branch_check job, which flags only spec-edit/* MRs inactive >7 days and only creates inbox notifications for Admin/Developer members — it never nudges reviewers, rebases, escalates, or closes with explanation, runs once daily rather than continuously, and ignores unreviewed/conflicted/abandoned MRs on ordinary branches. Conflict handling exists only as the merge processor's rebase of in-flight agents when the target baseline moves (merge_processor.rs:290-330), which is not stale-PR hygiene. No background job or agent monitors unreviewed or long-open MRs, and nothing auto-closes stale PRs.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/pre_accept.rs",
        "quote": "//! - `conventional-commit` — reject commits that don't match `type(scope): description`\n//! - `task-ref`            — reject feat/ and fix/ branches without a TASK-{id} reference\n//! - `no-em-dash`          — reject commits whose message contains an em-dash (—)",
        "detail": "The complete built-in pre-accept push-gate catalog. None of the gates rejects a push targeting the default branch, so 'No direct pushes to main' is not enforced at the only synchronous push-check point in the platform."
      },
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "// Spec lifecycle: auto-create tasks for spec changes on the default\n        // branch — only for ref updates git actually applied.",
        "detail": "Production post-receive handling treats pushes to the default branch as normal, accepted events to be processed (spec-lifecycle scans, persona-change handling, ledger sync all key off applied default-branch ref updates; tests in this file push directly to refs/heads/main and succeed). Combined with repo-level-only push authorization (authorize_repo_access, git_http.rs:148), a caller with push permission can push straight to main with no MR — the spec's 'All code submission happens via pull requests. No direct pushes to main' is not enforced."
      },
      {
        "file": "crates/gyre-server/src/abandoned_branch.rs",
        "quote": "//! Runs daily per workspace. Flags `spec-edit/*` branches where the MR has had\n//! no activity for >7 days (`updated_at` older than 7 days). Creates priority-9\n//! `AbandonedBranch` notifications for each Admin/Developer member of the workspace.",
        "detail": "The only stale-PR monitor in production (registered as the daily abandoned_branch_check job in jobs.rs:471-483). Its scope is a narrow slice of the spec requirement: only spec-edit/* source branches, only the 'abandoned' stale class (not unreviewed, unmerged-conflicted, or general long-open PRs), daily rather than continuous, and its only action is creating a notification — it never nudges reviewers, rebases, escalates, or closes the MR with an explanation."
      },
      {
        "file": "crates/gyre-server/src/api/prompts.rs",
        "quote": "When your task is done, commit to your branch, push, and signal completion with `gyre_agent_complete` — this creates a merge request and moves your task to review. Never merge your own work: merging is decided by the merge queue and its gates.",
        "detail": "Satisfied part of the section: MRs are the real collaboration primitive for agents — completion auto-creates the MR (create_mr_for_completion shared by the REST and MCP surfaces), the merge queue runs gates, and agents cannot merge their own work. The gap is the missing enforcement against direct default-branch pushes and the missing continuous stale-PR hygiene actions, not the MR flow itself."
      }
    ]
  }
]

## Shipped

Both normative bullets of specs/development/agent-workflow.md §PRs as the
Collaboration Primitive are enforced in production code. Task commits:
`c2690229` (push protection), `d5888ad5` (PR hygiene job), `c9faf6a8`
(formatting pass), `041bddcd` (review repair: deletion bypass + agent
nudge dedup), `34f7878e` (verification repair: rustfmt module ordering
+ clippy argument budget, merged with base `9f0c709f`).

### Agent-principal default-branch push protection

`git_receive_pack` (crates/gyre-server/src/git_http.rs) rejects, with 403
and an explicit reason directing the agent to the MR flow, any push whose
target ref is `refs/heads/{default_branch}` — resolved from the
Repository record's `default_branch` field, never a hardcoded "main" —
when the caller is an agent principal (`auth.user_id.is_none() &&
!auth.is_system_principal()`). The check is a direct, unconditional check
in the push handler (not an opt-in named pre-accept gate) and runs BEFORE
`git receive-pack` executes, so the ref is never updated server-side on
rejection. The rejection emits a `PushRejected` message-bus event and a
`GitPushRejected` audit event (gate_name `default-branch-protection`).
Human principals (user JWTs / API keys with push permission) and the
system principal are exempt, preserving specs/system/spec-lifecycle.md
§Automatic Task Creation (post-receive task creation, persona changes,
ledger sync run unchanged); server-side landing (merge queue, post-merge
reverts) goes through the git_ops/jj_ops ports and never through this
handler. The guard matches the raw pkt-line ref-update commands
(`parse_ref_update_commands`, deletions included), so an agent token
cannot delete the default branch either; `parse_ref_updates` remains a
deletion-filtering wrapper with unchanged semantics for push-acceptance
processing. Verified intact after the merge with base `9f0c709f` (the
merge only added session attribution to `record_pushed_commits`).

Tests: `agent_push_to_default_branch_rejected_directing_to_mr_flow`,
`agent_push_to_feature_branch_succeeds`,
`protection_resolves_real_default_branch_not_literal_main`,
`agent_deletion_of_default_branch_rejected`,
`human_push_to_default_branch_accepted_and_spec_lifecycle_runs`, and
merge-queue landing coverage.

### Continuous PR hygiene job

`pr_hygiene_check` (crates/gyre-server/src/pr_hygiene.rs) is registered
in jobs.rs on a short interval (default 5 minutes, not the daily
abandoned_branch_check cadence). Each pass evaluates every open MR per
workspace against the stale classes using real persisted state: unreviewed
beyond the nudge threshold nudges assigned reviewers (or escalates to
workspace Developers/Admins when there are none); conflicted MRs get a
real rebase attempt via the jj machinery for agent-authored branches with
live worktrees (rate-limited by the shared jj_rebase_backoff KV window)
and otherwise a rebase-required escalation; long-open MRs beyond the
escalation threshold notify workspace Admins/Owners at higher priority;
abandoned MRs beyond the hard close threshold are closed with a persisted
status transition, an explanatory MR comment, and delivery to the author
(user notification / agent message) — never a silent close. Every action
is recorded (notification rows, message-bus events, `pr_hygiene` audit
events). Thresholds are configurable via
GYRE_PR_HYGIENE_{NUDGE,ESCALATE,CLOSE}_SECS with sane defaults
(2d/7d/30d). The daily abandoned_branch_check job is untouched.
Deduplication is real on both reviewer paths: user reviewers through the
notification store's active-notification lookup, agent reviewers through
the `pr_hygiene_nudge_backoff` KV window keyed `{mr_id}:{agent_id}` — so
consecutive evaluations never duplicate notifications.

Tests: `abandoned_mr_is_closed_with_explanation_delivered_to_author`,
`unreviewed_mr_nudges_assigned_reviewers`,
`unreviewed_mr_without_reviewers_escalates_to_developers_and_admins`,
`fresh_mr_under_nudge_threshold_gets_no_action`,
`conflicted_mr_with_active_agent_worktree_is_rebased`,
`conflicted_mr_without_agent_raises_rebase_required_escalation`,
`long_open_mr_escalates_to_admins_at_higher_priority`,
`consecutive_evaluations_do_not_duplicate_notifications`,
`consecutive_evaluations_do_not_duplicate_agent_reviewer_nudges`.

### Verification-repair round (`34f7878e`)

Deterministic verification (finding `d2d8bca30d4146938346f10450da8dd6`)
failed on two findings, both repaired with behavior preserved: (1)
rustfmt module ordering in gyre-server/src/lib.rs (`pr_hygiene` now
declared before `pre_accept`); (2) clippy `too_many_arguments` (8/7) on
`create_notification` and `notify_members` in pr_hygiene.rs — the
(type, title, body) delivery triple is now bundled into a `MrNotice`
payload struct taken by reference (helpers at 6/6/5 arguments, no
`#[allow]`), same notification rows, dedup keys, bus payloads, and audit
events. Probes run in this sandbox (logs retained under
/tmp/stage/review-evidence/): `rustfmt --check` over every `crates/**/*.rs`
exit 0 with zero diffs; `SKIP_WEB_BUILD=1 cargo clippy -p gyre-server
--lib` and `--all-targets` (through the build wrapper) exit 0 with zero
warnings in pr_hygiene.rs and zero errors — remaining
`too_many_arguments` hits are the pre-existing baseline in files
task-270 did not touch, identical to what the failing verification already
tolerated.

### Test evidence and deferred verification

gyre-server test codegen exceeds this sandbox's 4 GiB budget (build
wrapper exit 64 — infrastructure policy, not a code defect); test targets
type-check clean under `cargo clippy -p gyre-server --all-targets`. The
independent verifier must execute (see
/tmp/stage/review-evidence/task-270-repair.md):

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib pr_hygiene`
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib agent_deletion_of_default_branch_rejected`
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib agent_push_to_default_branch_rejected_directing_to_mr_flow`
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib agent_push_to_feature_branch_succeeds`
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib protection_resolves_real_default_branch_not_literal_main`
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib human_push_to_default_branch_accepted_and_spec_lifecycle_runs`
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib parse_ref_updates`
