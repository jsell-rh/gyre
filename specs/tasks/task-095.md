---
title: "Platform Model Post-Merge Validation + Recovery Protocol"
spec_ref: "platform-model.md §6 Rollback & Recovery"
depends_on: []
progress: ready-for-review
review: specs/reviews/task-095.md
coverage_sections:
  - "platform-model.md §6 Rollback & Recovery"
  - "platform-model.md §6 Post-Merge Validation"
  - "platform-model.md §6 Recovery Protocol"
  - "platform-model.md §6 Agent Behavior During Recovery"
commits: ["c2e1ee93344be8f6989615ba84e315b590450273", "e6a6db47dbd8b988c8367a1965db50fd77d5abe7", "3a9b11f69c57cb576d2e3914ff7af7e54941b518", "5aaded213193d1af1cec693e729ac2872893b10b", "86c7d382312fae4fb52ae4effe40e021330f3ac2", "7c723298b41ee761067b15ae426651aa280fb75e"]
---

## Spec Excerpt

### Post-Merge Validation

After the merge processor merges an MR, a post-merge validation gate runs against the new HEAD of the default branch.

### Recovery Protocol

```
MR merged to main → Post-merge gate runs
  ├── PASS: continue. Merge queue processes next entry.
  └── FAIL:
        1. Merge queue PAUSES
        2. Forge creates a REVERT commit
        3. Post-merge gate re-runs on reverted HEAD
        4. Original MR re-opened with status `Reverted`
        5. Author agent receives RevertNotification via MCP
        6. Task created: "MR #{id} reverted: {failure reason}"
        7. MR's gate results invalidated
```

**MR Status Enum** gains `Reverted` variant.

### Agent Behavior During Recovery

Agents keep working — their branches need rebasing after main is fixed (jj handles automatically). Merge queue paused, workspace orchestrator notified.

## Implementation Plan

1. **Post-merge gate execution:**
   - After `merge_processor.rs` merges an MR, run configured post-merge gates
   - Add `post_merge_gates` to repo gate configuration (same schema as pre-merge gates)
   - Execute against HEAD of default branch after merge commit

2. **MR `Reverted` status:**
   - Add `Reverted` variant to `MrStatus` enum in domain
   - Migration to add "Reverted" to the status constraint (if using CHECK)

3. **Recovery protocol implementation:**
   - On post-merge gate failure:
     a. Pause the merge queue for the repo (`merge_queue_paused: bool` on repo state)
     b. Create revert commit using `git revert --no-edit <merge_sha>`
     c. Push revert to default branch
     d. Re-run post-merge gates on reverted HEAD
     e. If revert passes: resume merge queue, re-open original MR as `Reverted`
     f. If revert fails: escalate to human (critical notification)
   - Send `RevertNotification` message to author agent
   - Create remediation task referencing the failure reason

4. **Merge queue pause/resume:**
   - Add `paused: bool` and `pause_reason: Option<String>` to merge queue state
   - When paused, merge processor skips processing new entries
   - Resume clears the flag and triggers immediate queue processing

5. **Workspace orchestrator notification:**
   - Emit Event-tier message when merge queue pauses/resumes
   - Workspace orchestrator can reprioritize work accordingly

## Acceptance Criteria

- [x] Post-merge gates run after merge against new HEAD
- [x] `Reverted` variant added to MrStatus
- [x] Merge queue pauses on post-merge gate failure
- [x] Revert commit created and pushed automatically
- [x] Original MR re-opened as Reverted
- [x] Author agent receives RevertNotification
- [x] Remediation task created with failure reason
- [x] Merge queue resume works after main is green
- [x] `cargo test --all` passes

## Agent Instructions

Read `specs/system/platform-model.md` §6 "Rollback & Recovery". The merge processor is in `gyre-server/src/merge_processor.rs`. Gate execution is in `gyre-server/src/gate_executor.rs`. The MrStatus enum is in `gyre-domain/src/lib.rs` (or similar). For git revert operations, check how the forge executes git commands (likely via `gyre-server/src/git_http.rs` or a git helper). The merge queue state is managed in the merge processor — look for how queue entries are processed sequentially.

## Shipped

Revision round for the Round-3 review findings (R3-F1 … R3-F4), all
closed in product code on this branch:

- **R3-F1 (manual revert of the wrong commit):** `MergeRequest` now
  persists `merge_commit_sha`, recorded at merge time on both the
  single-entry and atomic-group paths (migration `2026-09-30-000056`,
  sqlite + postgres adapters, schema.rs). `POST /repos/:id/revert/:mr_id`
  reverts the MR's OWN recorded merge commit — never the current branch
  HEAD — rejects MRs with no recorded sha (409) and MRs belonging to a
  different repo (403), and routes manual reverts through
  `increment_revert_count`/`trip_circuit_breaker` so they count toward
  the breaker. Tests: `manual_revert_reverts_recorded_merge_commit`
  (observes the exact SHA passed to `revert_commit` through the
  `ConfigurableGitOps.revert_calls` double),
  `manual_revert_without_recorded_merge_commit_is_conflict`,
  `manual_revert_rejects_foreign_repo_mr`,
  `manual_revert_counts_toward_circuit_breaker`.
- **R3-F2 (breaker unreachable through the prescribed workflow):** the
  breaker counter is keyed on the resubmission-stable identity
  (`revert_breaker_key`: `spec:<spec_ref>` when bound, else `mr:<id>`);
  a trip cancels the queue entries of EVERY MR sharing the key; the
  remediation task now tells the author to bind the resubmitted MR to
  the same spec; and `enqueue` plus the processor's selection loop
  (single-entry 4b'' and atomic-group fetch-time re-validation) reject
  entries whose MR is not Open/Approved, so a Reverted MR can no longer
  be silently re-merged. Tests:
  `revert_breaker_accumulates_across_resubmitted_mrs`,
  `process_next_fails_entry_for_reverted_mr`,
  `atomic_group_skips_non_mergeable_member_and_rolls_back`,
  `enqueue` conflict test in `api/merge_queue.rs`.
- **R3-F3 (fail-open `main_green`):** `repo_status` resolves the default
  branch HEAD without a default; unresolvable HEAD reports
  `main_green: false` (fail closed). Test:
  `repo_status_main_green_fails_closed_when_head_unresolvable` (plain
  directory fixture + always-pass gate — proves the gate never ran).
  Both recovery.rs exemption entries removed from
  `fail-open-ref-resolution-exemptions.txt` (frozen at 0).
- **R3-F4 (review-record drift / dead ABAC duplicates):** the five
  duplicated resolver entries removed (single first-match block remains;
  `post-merge-gates` maps to resource type `gate`, matching sibling
  gate routes); `check-abac-route-registry.sh` duplicate baseline
  lowered from 22 to 17; task-096 rescoped to the remaining §6 UI work
  only (Circuit Breaker + CLI/REST recorded as delivered under
  task-095); all task-labeled commits recorded in this file's
  `commits:` frontmatter.
- **R4 (found during the Round-3 revision):** `rollback_atomic_group`
  addressed AtomicGroupFailure notifications to the raw author AGENT
  id — a user that does not exist — so no human ever saw a
  group-failure notice. Now resolves the agent's spawning user
  (agent-id fallback), matching `notify_gate_failure`/`notify_mr_merged`
  (commit `9cceb83b`).
- **R4-F1 (non-tip revert destroyed later merges — Round-4 major):**
  `Git2OpsAdapter::revert_commit` now implements true `git revert -m 1`
  semantics instead of snapshot semantics: a three-way merge of
  `tree(M^1)` against the current branch tip with `tree(M)` as the
  merge base, so only the reverted merge's own changes are undone and
  every later merge survives (the pre-fix code restored M's parent
  tree wholesale, wiping all later merges while reporting success).
  When the inverse patch collides with later changes on the same file,
  the port returns `RevertResult::Conflict` and leaves the branch
  untouched; all three callers handle it without falling back to
  anything destructive — the manual REST endpoint surfaces 409 with no
  side effects, and both automatic recovery paths (single MR and
  atomic group) stay paused and escalate to a human instead of
  recording a revert that never happened. The port contract
  (`gyre-ports/src/git_ops.rs`) documents the semantics. Real-git
  regression tests: `test_revert_commit_non_tip_preserves_later_merges`
  (two sequential merges; reverting A leaves B's file intact),
  `test_revert_commit_conflict_leaves_branch_untouched` (same-file
  collision → Conflict, branch unchanged). Automatic-path conflict
  handling is covered by
  `post_merge_recovery_revert_conflict_stays_paused_without_side_effects`
  and
  `atomic_group_recovery_revert_conflict_stops_group_reverts`
  (pause holds, MR(s) stay Merged with no revert recorded, escalation
  to the author's spawner, no remediation task), plus the REST-level
  `manual_revert_conflict_is_409_without_side_effects`.

**Test evidence** (focused probes on this branch head; logs under
`/tmp/stage/review-evidence/`):
`cargo test -p gyre-adapters --lib git2_ops` → 31 passed / 0 failed
(real-git three-way-merge revert, non-tip preservation, conflict
untouched-branch); `cargo test -p gyre-server --lib merge_processor` →
54 passed (includes the six post-merge/recovery protocol tests, the
R3-F1/F2 breaker and revert tests, and the two new R4-F1
revert-conflict tests); `cargo test -p gyre-server --lib api::recovery`
→ 6 passed; `cargo test -p gyre-server --lib api::merge_queue` → 8
passed. Check scripts green at head:
`check-task-commit-attribution` (after recording `a11ba8d3` in
task-068's frontmatter — the base commit that entered this branch via
the recovery merge, same R3-F4 drift class),
`check-fail-open-ref-resolution`, `check-abac-route-registry`,
`check-migration-versions`, `check-byte-slice-truncation`,
`check-dead-message-kinds`, `check-arch`, `check-inert-enforcement`,
`check-mem-port-contracts`.

## Rebase Repair (Round 5)

Rebased the branch onto the current pipeline base
`7c6ac232ad1e43c977540034381e41c47548aa81` (merge commit
`393e96a9`). One content conflict in `specs/tasks/task-068.md`
`commits:` frontmatter — both sides held the identical 10-commit set
differing only in ordering; resolved by keeping the HEAD ordering
previously verified green by `check-task-commit-attribution`.

The merge brought the base branch's web sources (task-196
Briefing/InlineChat) while `web/dist` was stale from the task-095
side; regenerated dist from the merged sources (commit `3668aef6`,
same convention as `44a8187f`), after `npm ci` with the locked
versions and a passing `npx vitest run
src/__tests__/Briefing.test.js` (24/24) on the merged sources.

All focused probes re-run green at final head `3668aef6`:
`merge_processor` 54/54, `git2_ops` 31/31, `api::recovery` 6/6,
`api::merge_queue` 8/8, `check-task-commit-attribution`,
`check-abac-route-registry`, `check-fail-open-ref-resolution`.
Evidence: `/tmp/stage/review-evidence/rebase-repair-393e96a9.txt`.
Tree clean at `3668aef6`.

Sandbox limitation recorded: the TCP listener probe is unsupported in
this runtime (`errno 95`, `/tmp/stage/capabilities.json`), so no live
server/browser smoke test was performed here; behavior is verified by
the in-process axum `oneshot` handler tests above. GitHub CI on the
branch head remains the authoritative transport-level check.
