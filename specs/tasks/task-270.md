---
title: "PR collaboration invariants: reject agent-principal pushes to the default branch (human push path preserved per spec-lifecycle.md) and continuously act on stale MRs (nudge, rebase, escalate, close)"
spec_ref: "Spec gap spec-4c39ca6ea3e71ce8a53398f3 / pr-protection-and-hygiene / d30f233daf86"
depends_on: []
progress: complete
commits: ["68564e547fc9677a2ce6b2590ec9a9d1f4aa272a", "5b5ac0852bdcf5b8bde0a960909262e8e175564a", "2f56050a4bed2981b1e84d5659112f5bb5125172", "1b0cbd6c15741fee50202fe06682344f7a91b176", "f48a62f6be9ea6b85b94f5dd1293e137e7b51504", "34f7878e3734df124067c20b225ad7e2a150ec70", "041bddcdd54e58930beb34cfc06ca12a0d16e9fa", "c9faf6a80ba0a216889c2d7efb5c82f71f58deee", "d5888ad56a3369911836d9ccaeb06995dd73de9d", "c269022921fb4faddc818da21fd2d834d00ab63e", "24308e079ba4fe612d27527e5ee1e47f0f4df63a"]
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
Collaboration Primitive are enforced in production code on this branch.
Product commits (oldest first): `c2690229` (push protection), `d5888ad5`
(PR hygiene job), `c9faf6a8` (formatting), `041bddcd` (review repair:
deletion bypass + agent nudge dedup), `34f7878e` (rustfmt/clippy
repair), `f48a62f6` (review repair: packfile-carrying test receive-pack
bodies), `1b0cbd6c` (rustfmt single-line assert), `2f56050a`
(verification repair: nested-path test commits + Fulcio test ref),
`5b5ac085` (verification repair: spec-approval test pushes as system
principal, see below).

### Rebase repair round (f810255a)

The published PR conflicted with current main (task-276 graph-diff work,
cc61956d). The merge was resolved preserving the retained task-270
implementation untouched — the only conflicts were web/dist build
artifacts, taken from main's verified asset set; no task-270 source file
(git_http.rs push protection, pr_hygiene.rs, jobs.rs registration) was
modified by main. Probe evidence for the merged tree (saved under
/tmp/stage/review-evidence): `cargo check -p gyre-server` exit 0 and
`cargo check -p gyre-server --tests` exit 0, both with no warnings.

### Agent-principal default-branch push protection

`git_receive_pack` (crates/gyre-server/src/git_http.rs) rejects, with 403
and an explicit reason directing the agent to the MR flow, any push whose
target ref is `refs/heads/{default_branch}` — resolved from the
Repository record's `default_branch` field, never a hardcoded "main" —
when the caller is an agent principal (`auth.user_id.is_none() &&
!auth.is_system_principal()`). The check is a direct, unconditional
check in the push handler (not an opt-in named pre-accept gate) and runs
BEFORE `git receive-pack` executes, so the ref is never updated
server-side on rejection; it matches the RAW pkt-line ref-update
commands (`parse_ref_update_commands`), so agent-principal deletion of
the default branch is rejected too. The rejection emits a `PushRejected`
message-bus event and a `GitPushRejected` audit event (gate_name
`default-branch-protection`, outcome Blocked). Human principals (user
JWTs / API keys with push permission) and the system principal are
exempt, preserving specs/system/spec-lifecycle.md §Automatic Task
Creation (post-receive task creation, persona changes, ledger sync run
unchanged); server-side landing (merge queue, post-merge reverts) goes
through the git_ops/jj_ops ports, not this handler.

### Continuous PR hygiene job

`pr_hygiene_check` (crates/gyre-server/src/pr_hygiene.rs) is registered
in jobs.rs on a minutes-scale interval (default 5 minutes, configurable
via GYRE_PR_HYGIENE_INTERVAL_SECS; NOT the daily abandoned_branch_check
cadence) and evaluates every open MR per workspace against the stale
classes using real persisted state (MergeRequest.updated_at, reviewer
assignments, review records, a real merge-base conflict check):
unreviewed beyond the nudge threshold nudges assigned reviewers (or
escalates to workspace Developers/Admins when there are none);
conflicted MRs get a real rebase attempt via the jj machinery for
agent-authored branches with live worktrees (rate-limited by a KV
backoff window) and otherwise a rebase-required escalation; long-open
MRs beyond the escalation threshold notify workspace Admins/Owners at
higher priority; abandoned MRs beyond the hard close threshold are
closed with a persisted status transition, an explanatory MR comment,
and delivery to the author — never a silent close. Every action is
recorded (notification rows, message-bus events, `pr_hygiene` audit
events). Thresholds configurable via
GYRE_PR_HYGIENE_{NUDGE,ESCALATE,CLOSE}_SECS (defaults 2d/7d/30d). The
daily abandoned_branch_check job is untouched.

### Verification repair (5b5ac085)

The independent verification run (`cargo test --all`, attempt 83a59ac4)
failed one test: `spec_approval_auto_invalidated_on_spec_change`
(crates/gyre-server/tests/git_integration.rs) — its initial push to
refs/heads/main got HTTP 403 because the test minted an AGENT token via
POST /agents/spawn and pushed its spec commits with it, exactly the
caller class the new protection rejects. The test's subject is
spec-lifecycle post-receive behavior (manifest-driven spec registry
sync, approval revocation on spec change), which spec-lifecycle.md
§Automatic Task Creation scopes to the human/system push path, so the
repair (per the task contract: "system-token test pushes to main keep
succeeding") drives its main pushes with the global dev token — the
system principal, an exempt caller class — and removes the
now-purposeless agent spawn. All assertions are unchanged.

Local probe evidence (saved under /tmp/stage/review-evidence):
`cargo check -p gyre-server --test git_integration` exit 0 (no
warnings), rustfmt clean on the changed file; for the rebase round,
`cargo check -p gyre-server` and `cargo check -p gyre-server --tests`
exit 0 on the merged tree. The test binaries themselves could not be
executed in this sandbox — the build wrapper blocks gyre-server test
compilation (4 GiB budget); that is a sandbox restriction, not a code
defect. Verifier must run:
`cargo test -p gyre-server --test git_integration` (includes the
repaired test plus the task-270 push-protection and PR-hygiene tests),
`cargo test -p gyre-server --lib` (git_http push-protection tests:
agent_push_to_default_branch_rejected_directing_to_mr_flow,
agent_push_to_feature_branch_succeeds,
agent_deletion_of_default_branch_rejected,
protection_resolves_real_default_branch_not_literal_main,
human_push_to_default_branch_accepted_and_spec_lifecycle_runs;
pr_hygiene tests: abandoned/unreviewed/conflicted/escalation/idempotency
suites; jobs.rs pr_hygiene_check_registered_on_interval_not_daily),
`cargo test -p gyre-server --test graph_integration` (merged task-276
surface), and the full `cargo test --all` gate.
