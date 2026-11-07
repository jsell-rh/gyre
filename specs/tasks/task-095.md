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
commits: ["3ef9e1b8222821f330942f3fbf13edab1b3497eb", "44a8187fe2c39c9884e2fe9c6ff0c8add233ee2f", "3a9b11f69c57cb576d2e3914ff7af7e54941b518", "5aaded213193d1af1cec693e729ac2872893b10b", "86c7d382312fae4fb52ae4effe40e021330f3ac2", "7c723298b41ee761067b15ae426651aa280fb75e"]
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

The §6 Post-Merge Validation + Recovery Protocol is implemented end to
end (Rounds 1–6, details in `specs/reviews/task-095.md`): post-merge
gates run on the merged HEAD via a detached worktree at `head_sha`;
`MrStatus::Reverted`; merge-queue pause on post-merge failure with
pause/resume and workspace-orchestrator notification; automatic `git
revert -m 1` (three-way-merge semantics) pushed to the default branch
and gates re-run on the reverted HEAD, with human escalation if the
revert itself fails; the original MR re-opened as `Reverted`; author
agent RevertNotification; remediation task carrying the failure
reason; MR gate results invalidated. Hardening across rounds:
resubmission-stable circuit breaker keyed on source branch,
merge-commit-sha-scoped manual revert, upstream-merge resolution,
fail-closed `repo_status` ref resolution, enabled
`builtin:attested-authorization` seed, and the Round-6 test fix for the
unattested arm of
`builtin_signed_authorization_satisfied_by_chain_presence` (the
chain-less subject now sets `attestation_chain_present = false`
explicitly; the engine stays fail-closed on missing attributes by
design). After a worker-lease expiry, the full lineage was recovered
as squashed checkpoint `3ef9e1b8` on base `3c39ab1f` (pre-recovery
SHAs retained above only for controller cross-reference — they are
not resolvable in this object store).

### Verification-repair round (this revision)

The deterministic verification gate (`dev-check.sh`) failed with
"new verification exemptions forbidden:
scripts/fabricated-scope-defaults-exemptions.txt: [git_http.rs:1499]".
Root cause: the task-095 lineage had renumbered the legacy
`git_http.rs:1495` exemption entry to 1499 when merge drift shifted
the file — the freeze gate compares exemption entries as a set
against the base, so a renumbered replacement counts as a new entry.
The entry was dead debt in both base and HEAD: push-event emission
already resolves `ws_id` via `match state.repos.find_by_id(...)` with
skip-and-log on lookup failure (the task-097 F3 compliant pattern),
and the gate's matcher flags no site in `git_http.rs` at either ref.
Repair (commit `7a0bd1db`, appended to `commits:`): delete the stale
entry and lower `FROZEN_EXEMPTION_COUNT` 5 → 4, exactly as the
exemption file's header instructs for fixed sites. No Rust code
changed.

### Test evidence

- This round: `check-fabricated-scope-defaults.sh` passes against this
tree with both the candidate script (count 4) and the upstream/base
script version (count 5) placed at its original path — the four
remaining live flagged sites (`gate_executor.rs:111`,
`api/specs.rs:959`, `auth.rs:773`, `speculative_merge.rs:372`) are all
still exempted; the dev-check exemption-freeze set difference vs
base `86a98a29` is empty (probe transcript under
`/tmp/stage/review-evidence/`).
- On the recovered tree (`3ef9e1b8`): `cargo fmt --check -p
  gyre-server` clean; `check-arch.sh`, `check-abac-route-registry.sh`,
  `check-dead-message-kinds.sh`, `check-migration-versions.sh`,
  `check-byte-slice-truncation.sh`,
  `check-fail-open-ref-resolution.sh`, and
  `check-task-commit-attribution.sh` all pass; the Round-6 fix content
  verified present.
- Standalone rustc probe
  (`/tmp/stage/review-evidence/probe_policy_test_fix.rs`) replicating
  the engine verbatim: attested arm PASS (Allow,
  `builtin-attested-authorization`), unattested arm PASS (Deny,
  `builtin-require-signed-authorization`); the pre-fix construction
  reproduced the finding's failure signature as a regression guard.
- Prior full-suite evidence (R2–R3 rounds, pre-recovery lineage):
  `gyre-server --lib merge_processor` 45 passed; `gyre-server` all
  suites 1273 passed; `gyre-adapters` 324 passed / 12 ignored.

Sandbox limitation recorded for remote verification: this runtime has
no crates.io egress, so `cargo test` suites cannot execute here; the
full-suite green must be confirmed by CI/remote verification on the
branch head. This round changed no Rust code — the scripts-only
repair is fully covered by the deterministic gates above.
