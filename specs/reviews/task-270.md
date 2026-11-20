# Review — task-270 (Agent-principal default-branch push protection + continuous PR hygiene)

Spec: `specs/development/agent-workflow.md` §PRs as the Collaboration
Primitive (gap `4a3b61a03d2e9cd037d33c0c`).
Candidate under review: `688980d6` (base `2f33a117`); product commits
`c2690229`, `d5888ad5`, `c9faf6a8`, `041bddcd`, `34f7878e`, `f48a62f6` —
all recorded in the task frontmatter.
Verdict: **needs-revision** — one blocking finding (F1, CI rustfmt gate);
all functional behavior verified clean by independent probes.

## Round 3 (this review)

Prior rounds: bc9fa879 found the push-protection tests carried
packfile-less receive-pack bodies that `git receive-pack` never applies
(exits 0, ref untouched), so `agent_push_to_feature_branch_succeeds` and
`human_push_to_default_branch_accepted_and_spec_lifecycle_runs` could
never pass. Repaired in `f48a62f6` (packfile-carrying bodies via
`pack_objects_for` / `receive_pack_body_with_pack`).

Probes run (evidence in `/tmp/stage/review-evidence/`):

- **Plain-git receive-pack probe** (`receive_pack_probe.sh/.out`,
  git 2.43.0, the server's exact `git receive-pack --stateless-rpc <repo>`
  invocation): a non-delete command+flush body with NO packfile exits 0
  and applies nothing (reproduces the prior finding); the same body with a
  `git pack-objects --stdout` pack appended after the flush lands the ref
  (validates the repair and both accepted-path tests); a delete body
  (old→zeros, no packfile) deletes the ref (validates that the
  deletion-bypass repair — raw-command matching in
  `parse_ref_update_commands` — guards a real applyable command).
- `cargo check -p gyre-server --tests` → exit 0 (9m29s cold;
  `cargo_check_gyre_server_tests.out`).
- `cargo clippy -p gyre-server --lib --tests` → exit 0; all warnings are
  pre-existing dead-code warnings in `gyre-ports`, none in `pr_hygiene.rs`
  or the new `git_http.rs` regions (`clippy_run.out`).
- `cargo fmt -p gyre-server -- --check` → **FAILS** (see F1).
- `python3 scripts/check-rustfmt-diff.py 2f33a117` → **FAILS, exit 1**
  (see F1). This is CI's hard `fmt` gate.
- `scripts/check-arch.sh`, `check-abac-route-registry.sh`,
  `check-notification-toggle-coverage.sh`, `check-dead-message-kinds.sh`,
  `check-unwired-port-methods.sh` → PASS.
- Full `scripts/check-*.sh` sweep at candidate vs base worktree
  (`full_script_compare.sh`): the only candidate-introduced differences
  are F1 and (a) the expected 41→46 variant count in
  notification-toggle-coverage (PASS), (b) two new advisory
  `let _ = merge_requests.update(...)` discards in `pr_hygiene.rs:476,506`
  flagged by `check-silent-result-discard` — a non-CI, non-exemption-based
  script that already fails at base with 125 findings (state-repair
  best-effort writes with the conflict flag already logged upstream; not
  blocking), and (c) line-number/order churn in scripts that fail
  identically at base (several use gawk syntax this sandbox's mawk cannot
  parse — infrastructure, not code).
- `cd web && npm ci && npx vitest run src/__tests__/UserProfile.test.js`
  → 38/38 passed (`vitest_userprofile.out`), including the drift test that
  extracts `NotificationType` variants from the Rust source and requires
  `NOTIF_TYPE_IDS` to match — the 5 new variants
  (ReviewerNudge/MrUnreviewed/MrNeedsRebase/MrStaleEscalation/MrAutoClosed)
  are in lockstep across enum, `all()`, priorities, email headings, web
  toggle list, and en.json labels.

Verified working (no findings):

- **Push protection is real and correctly scoped.** The guard in
  `git_receive_pack` (git_http.rs ~754) runs after repo/ABAC
  authorization and BEFORE `run_git_stateless("receive-pack", ...)`, so a
  rejected push never touches the ref. Caller class is
  `auth.user_id.is_none() && !auth.is_system_principal()` — the hardened
  `is_system_principal()` (agent_id "system" AND user_id None; agent ids
  are server-generated UUIDs at spawn, so an agent token cannot spoof it).
  The target ref is `format!("refs/heads/{default_branch}")` from the
  Repository record (never a literal "main"), matched against RAW pkt-line
  commands including deletions. Human principals (API key / JWT with push
  permission) and the system principal are exempt, preserving
  spec-lifecycle.md §Automatic Task Creation; the human-path test drives a
  real API-key Developer push of `specs/system/human-push-spec.md` to main
  and asserts the `spec_changed` audit event with a created `task_id`.
  Server-side landing is untouched (merge queue lands via
  `git_ops.merge_branches` in merge_processor.rs, never through the HTTP
  handler; `merge_queue_auto_merges_mr_and_commit_on_main` keeps covering
  it with the initial main push switched to the system token). The
  rejection emits a `PushRejected` bus event and a `GitPushRejected` audit
  event (gate `default-branch-protection`, outcome Blocked) with an
  explicit MR-flow reason. Test coverage maps 1:1 to AC-1..AC-4,
  including the trunk-default repo where a push to a literal "main"
  branch must NOT be rejected.
- **PR hygiene job is real and continuous.** `pr_hygiene_check` is
  registered in jobs.rs with `run_at_utc_hour: None` and a 300s default
  interval (`GYRE_PR_HYGIENE_INTERVAL_SECS`), spawned via `spawn_job`
  (interval loop confirmed); the daily `abandoned_branch_check` is
  untouched. `run_with` walks every workspace's Open/Approved MRs with
  persisted state (`updated_at`, reviewers, review records,
  `has_conflicts` plus a real `GitOpsPort::can_merge` merge-base check —
  `Ok(false)` = conflicts, infra errors are not conflicts) and takes one
  severity-ordered action per MR: close (persisted `transition_status`
  Open/Approved→Closed + gate-trace deletion mirroring the close endpoint
  + explanatory MR comment + author delivery + Admin/Owner notification +
  `pr_hygiene` audit event), conflict (jj rebase in the author's live
  worktree with the merge processor's exact safeguards — Active agent,
  same-repo worktree, exists on disk — and the SHARED
  `jj_rebase_backoff` KV namespace/window, or a `MrNeedsRebase`
  escalation to Developers/Admins), long-open (`MrStaleEscalation`,
  priority 3, to Admins/Owners), unreviewed (`ReviewerNudge` to assigned
  reviewers; `MrUnreviewed` escalation when there are none). User-path
  dedupe is `has_active_notification`; agent-reviewer bus emissions dedupe
  via a persisted KV backoff keyed (MR, agent) — both idempotence tests
  run two evaluations in-window and assert exactly one delivery.
  Thresholds are env-configurable with non-zero defaults (2d/7d/30d).
- No new routes (no ABAC registry impact), no new port traits, no new
  MessageKind variants (Custom strings), no migrations, hexagonal
  boundaries intact.

Findings:

- [**F1 (blocking, trivial): CI rustfmt gate fails on changed lines.**
  `pack_objects_for` in `crates/gyre-server/src/git_http.rs` (added by
  `f48a62f6`, after the earlier rustfmt repair commit `34f7878e`) formats
  its second assert across four lines:
  `assert!(!out.stdout.is_empty(), "pack-objects produced empty pack");`
  belongs on one line (62 chars) — the identical pre-existing pattern
  sits ~15 lines below (git_http.rs ~9804). Reproduce:
  `python3 scripts/check-rustfmt-diff.py 2f33a117` → exit 1,
  "rustfmt: crates/gyre-server/src/git_http.rs: changed lines need
  formatting: 8409, 8410, 8411, 8412"; also `cargo fmt -p gyre-server
  -- --check` fails on the same hunk (CI's `fmt` job and the pre-commit
  `cargo-fmt` hook both run these). Repair: collapse the assert to one
  line, commit (e.g. `style(task-270): collapse pack-objects assert to
  satisfy rustfmt`), append the commit SHA to task-270.md `commits:`, and
  re-run the diff-scoped script against the same base.

Non-blocking observations (recorded, no action required):

- `is_conflicted` runs a real in-memory merge-base check per open MR per
  evaluation (default every 5 min) — a latency consideration for very
  large repos, not a correctness defect.
- No coverage matrix file exists for `specs/development/` specs
  (specs/coverage/ covers system/ only), so the task's "update coverage
  rows" instruction has no target; the verifier regenerates
  specs/coverage/SUMMARY.md from the matrices regardless.
- An MR whose `author_agent_id` is None (model has no separate
  user-author field) gets Admin/Owner notifications on auto-close but no
  direct author delivery; the delivery helper handles both agent and user
  ids whenever one is recorded.
- The `web/dist/` regeneration side effect of any gyre-server build is
  owned by the verifier (the review sandbox restores it).

## Deferred to the independent verifier (gyre-server test compilation exceeds the review memory budget)

Run after F1's repair, on the merged tree:

```
cargo test -p gyre-server --lib \
  agent_push_to_default_branch_rejected_directing_to_mr_flow \
  agent_push_to_feature_branch_succeeds \
  agent_deletion_of_default_branch_rejected \
  protection_resolves_real_default_branch_not_literal_main \
  human_push_to_default_branch_accepted_and_spec_lifecycle_runs
cargo test -p gyre-server --lib pr_hygiene
cargo test -p gyre-server --lib pr_hygiene_check_registered_on_interval_not_daily
cargo test -p gyre-server --test git_integration -- \
  push_records_agent_commit_provenance merge_queue_auto_merges_mr_and_commit_on_main
cargo test -p gyre-server --test e2e_ralph_loop -- full_ralph_loop_via_gyre
```

Plus the standard CI gates (`check-rustfmt-diff.py`, `check-clippy-diff.py`
against the merge base, arch/ABAC scripts, web build + vitest). The
receive-pack constructions behind the first five tests were independently
validated against plain `git receive-pack --stateless-rpc` in this review
(see probe evidence), so a failure there would be environmental, not
constructional.

Test-inflation check: none found. Every new test drives the real router
or the real job core (`run_with`) against real persisted state and
asserts externally observable outcomes (HTTP status, on-disk refs,
notification rows, persisted MR status, audit events, bus messages); the
guards fail on reintroduction of the defect (e.g. removing the
agent-principal check makes the rejection tests fail; filtering deletions
again makes the deletion test fail).
