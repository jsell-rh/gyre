# Review — task-270 (agent-principal default-branch push protection + continuous PR hygiene)

Task: PR collaboration invariants (specs/development/agent-workflow.md §PRs as
the Collaboration Primitive; spec gap spec-4c39ca6ea3e71ce8a53398f3).
Candidate: `3f8fa00a` (base `9f0c709f`; task commits `c2690229`, `d5888ad5`,
`c9faf6a8`, `041bddcd`, `34f7878e`).
Verdict: **needs-revision** — two findings (F1/F2, same root cause: the two
tests that prove the "don't over-block" half of the contract cannot pass).
The production logic itself reviewed clean; no production-code defect found.

## Probes run (evidence: /tmp/stage/review-evidence/, summary.md)

- `SKIP_WEB_BUILD=1 cargo clippy -p gyre-server --lib` → exit 0 (no new
  warnings; 119 pre-existing baseline).
- `cargo check -p gyre-server --lib --tests` → exit 0 — every target
  including the new cfg(test) unit tests type-checks.
- `cargo test -p gyre-common` → 129 passed (NotificationType additions).
- `cd web && npm ci && npm test` → 127 files / 2566 passed — includes the
  UserProfile test that parses the Rust enum and requires exact
  NOTIF_TYPE_IDS parity (the five new types are present).
- `rustfmt --check` on all nine changed .rs files → clean.
- `scripts/check-arch.sh`, `check-dead-message-kinds.sh`,
  `check-mem-port-contracts.sh` → all pass.
- gyre-server test execution is blocked by the sandbox 4 GiB policy
  (wrapper rejects test codegen) — deferred to the verifier; the decisive
  receive-pack probe below was run as a standalone git experiment instead.

## Verified working (no findings)

**Push protection (production logic):**
- The guard lives directly in `git_receive_pack` (git_http.rs:730-812),
  unconditional for the caller class — not an opt-in pre-accept gate — and
  runs BEFORE `run_git_stateless("receive-pack", ...)`, so a rejected push
  can never move the ref.
- Caller-class scoping is correct against every auth path in auth.rs:
  agent tokens authenticate with `user_id: None` (rejected); user API keys,
  Keycloak JWTs (find_or_create user), and impersonation sessions all set
  `user_id: Some(...)` (accepted); the global/system token is excluded via
  `is_system_principal()` (agent_id=="system" && user_id None — not a
  spoofable string match). Scoped gate-agent tokens were already rejected
  earlier in the handler.
- Target ref is `refs/heads/{resolved.default_branch}` from the Repository
  record — never a literal "main" (tested both directions with a
  trunk-default repo).
- The guard matches RAW ref-update commands (`parse_ref_update_commands`,
  deletions included), closing the delete-the-default-branch bypass that a
  deletion-filtering match would leave; `parse_ref_updates` keeps its old
  deletion-filtering semantics for all existing consumers (pure refactor of
  the same parser). pkt-line capability suffixes (`\0caps`) are stripped
  before matching, so real git clients cannot slip past the equality check.
- Rejections are observable: PushRejected bus event + GitPushRejected audit
  event (gate_name `default-branch-protection`, Blocked outcome) — the
  rejection tests assert both.
- Server-side landing is untouched: the merge queue lands via the
  git_ops/jj_ops ports, never through the HTTP handler;
  `merge_queue_auto_merges_mr_and_commit_on_main` (updated to seed main with
  the system token) covers it.
- No new HTTP routes → no ABAC registry/exempt-handler obligations.

**PR hygiene (production logic + tests):**
- `pr_hygiene_check` registered in jobs.rs with `interval_secs` (default
  300, env-configurable) and `run_at_utc_hour: None`, spawned via
  `jobs::spawn_job` in main.rs after `start_job_registry` — a real interval
  loop, not the daily abandoned_branch_check cadence (which is untouched).
  The registration test asserts the interval shape AND that a manual
  trigger runs a real evaluation.
- Stale classes use real persisted state: `MergeRequest.updated_at`,
  reviewer list, review records, `has_conflicts` plus a real read-only
  merge-base check (`GitOpsPort::can_merge` → libgit2 in-memory
  `merge_commits`; infrastructure errors are NOT treated as conflicts).
  Most-severe-first, one action per MR per evaluation.
- Abandoned close: persisted status transition (via `transition_status`,
  valid Open/Approved→Closed), gate-trace deletion mirroring the close
  endpoint, explanatory MR comment, author delivery (agent bus message /
  user notification), Admin/Owner notifications, `pr_hygiene` audit event —
  never silent. Tested end-to-end against the mem state including the bus
  message payload.
- Conflicted: real `jj_ops.jj_rebase` attempt for active-agent worktrees
  (same safeguards + shared 60s KV backoff window as the merge processor),
  jj_rebase audit events on every outcome, persisted conflict flag on
  Conflict, and a MrNeedsRebase escalation whenever the rebase is
  inapplicable or fails — never silence. Both paths tested with the
  ConfigurableGitOps/ConfigurableJjOps doubles.
- Unreviewed: reviewer nudge (ReviewerNudge) or, with no reviewers,
  MrUnreviewed escalation to Developers/Admins; fresh-MR no-action and
  reviewed-MR-not-nudged both tested.
- Idempotence: user deliveries dedupe via `has_active_notification`; agent
  reviewer nudges dedupe via a persisted KV backoff keyed (MR, agent) —
  added in review repair `041bddcd` and tested on both paths; closed MRs
  leave the open set so close can't repeat.
- Notification model plumbing is complete on all surfaces (Rust enum +
  priority ranks + parse/from_str/all(), email headings EN/DE,
  NOTIF_TYPE_IDS, en.json labels) — the parity-sensitive web test passes.

## Findings

- [-] **F1 (major, test): `agent_push_to_feature_branch_succeeds`
  (git_http.rs:8527) cannot pass.** It pushes through
  `receive_pack_body` (git_http.rs:8378), which builds command + flush with
  NO packfile. Verified empirically against the server's exact invocation
  (`git receive-pack --stateless-rpc <bare repo>`, target objects already
  present, git 2.43.0; see /tmp/stage/review-evidence/receive-pack-probe2.out):
  a packfile-less non-delete receive-pack request exits 0 while applying
  NOTHING. The handler therefore returns 200 with the ref never created, and
  the test's `ref_exists(...refs/heads/feat/dbp)` assertion fails. This is
  the task's acceptance evidence for "the same agent token pushing a feature
  branch succeeds", and it was never executed in any sandbox (task-270.md
  defers all gyre-server test runs to the verifier). Repair: append a real
  packfile to the body — the in-repo pattern is the `git_push_accepted`
  test (git_http.rs ~9921: `git pack-objects --stdout` fed the new SHA,
  output appended after the flush pkt). Extend `push_ref_as` /
  `receive_pack_body` accordingly.
- [-] **F2 (major, test): `human_push_to_default_branch_accepted_and_
  spec_lifecycle_runs` (git_http.rs:8664) cannot pass — same root cause.**
  The packfile-less push never lands refs/heads/main, so
  `verify_applied_ref_updates` (git_http.rs:2664) drops the update and
  `process_spec_lifecycle` receives an empty applied list: no spec-change
  task, no `spec_changed` audit event. The test's
  `ref_exists(...refs/heads/main)` assertion fails, and
  `wait_for_audit_event(&state, "spec_changed")` times out after 30s and
  panics. This is the only acceptance evidence for the over-blocking
  companion criterion (human push-capable token + specs/ change to the
  default branch still accepted AND spec-lifecycle task creation still
  runs — the spec-lifecycle.md §Automatic Task Creation invariant). Repair:
  same as F1; with a real packfile the spec_changed/task_id assertions
  become meaningful.

Corroborating consistency check: no pre-existing green test asserts that a
ref lands from a packfile-less `receive_pack_body` (the attestation tests
only assert status/audit/agent-state, and the l1 rejection test asserts the
ref does NOT exist), so the baseline being green does not contradict the
probe.

## Minor (non-blocking, recorded for completeness)

- `deliver_author_message_or_notification` returns early when
  `author_agent_id` is None, so a user-authored MR's author learns of an
  auto-close only via the MR comment and Admin notifications, not a direct
  inbox row. The task wording ("an explanatory notification/comment to the
  author") is satisfied by the comment; tightening this in a later pass
  would be reasonable.
- In the shared 60s jj-rebase backoff window a conflicted-MR evaluation
  returns without a new recorded action; the window is shorter than the
  job's cadence and the previous action was recorded, so this is a
  rate-limit, not silence in practice.

## Test-inflation check

None found beyond F1/F2 (which are failing, not hollow): the rejection
tests assert the 403, the response text, the ref state, and the audit
event; the hygiene tests assert persisted state transitions, notification
rows, bus messages, KV backoff persistence, and audit events, and each
would fail if its production behavior were disabled (e.g. the close test
fails if `close_abandoned` doesn't transition status; the agent-nudge
idempotence test fails without the KV backoff).

## Verifier instructions

Run the server test list in /tmp/stage/review-evidence/summary.md after the
F1/F2 repair; F1 and F2 must be re-reviewed once the packfile is included —
the human-push test is the load-bearing over-blocking guard for
spec-lifecycle.md §Automatic Task Creation.
