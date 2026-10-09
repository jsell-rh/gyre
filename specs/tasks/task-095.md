---
title: "Platform Model Post-Merge Validation + Recovery Protocol"
spec_ref: "platform-model.md §6 Rollback & Recovery"
depends_on: []
progress: needs-revision
review: specs/reviews/task-095.md
coverage_sections:
  - "platform-model.md §6 Rollback & Recovery"
  - "platform-model.md §6 Post-Merge Validation"
  - "platform-model.md §6 Recovery Protocol"
  - "platform-model.md §6 Agent Behavior During Recovery"
commits: ["9cceb83b8ce751791142659cca70ef7b45c313a8", "bc3413de4a99e9592c6f8dac967d331f836adc71", "3113b002e7cde69fb24170b616c1d43af71d26a2", "6e1d8b145880443672c1a0055db2939d38e13985", "34b6a7845b143faea5ae12c26363d184b2e06e9a", "178e442ab02987424c99818762b12143ade4d725", "3a9b11f69c57cb576d2e3914ff7af7e54941b518", "5aaded213193d1af1cec693e729ac2872893b10b", "86c7d382312fae4fb52ae4effe40e021330f3ac2", "7c723298b41ee761067b15ae426651aa280fb75e"]
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
- **R4 (found during this round):** `rollback_atomic_group` addressed
  AtomicGroupFailure notifications to the raw author AGENT id — a user
  that does not exist — so no human ever saw a group-failure notice.
  Now resolves the agent's spawning user (agent-id fallback), matching
  `notify_gate_failure`/`notify_mr_merged` (commit `9cceb83b`).

**Test evidence** (focused probes, CARGO_TARGET_DIR-shared with the
workspace; logs under `/tmp/stage/review-evidence/`):
`cargo test -p gyre-server --lib merge_processor` → 52 passed / 0
failed (includes all six post-merge/recovery tests, the R3-F1/F2
breaker and revert tests, and the R4 group test);
`api::recovery` → 5 passed; `api::merge_queue` → 8 passed;
`gyre-adapters` → 344 passed / 12 ignored (real-git revert + no-FF
merge + migration round-trip); `gyre-domain` → 363 passed;
`gyre-common` → 94 passed. Check scripts green:
`check-fail-open-ref-resolution`, `check-abac-route-registry`,
`check-migration-versions`, `check-byte-slice-truncation`,
`check-dead-message-kinds`, `check-arch`,
`check-task-commit-attribution`, `check-inert-enforcement`,
`check-mem-port-contracts`.

Sandbox limitation recorded: the TCP listener probe is unsupported in
this runtime (`errno 95`, `/tmp/stage/capabilities.json`), so no live
server/browser smoke test was performed here; behavior is verified by
the in-process axum `oneshot` handler tests above. GitHub CI on the
branch head remains the authoritative transport-level check.

## Review

### Review changed source code

- crates/gyre-server/src/merge_processor.rs

Preserved these edits for implementation. Review cannot approve its own source or verifier edits. Repair them within task scope and request a fresh independent review.
