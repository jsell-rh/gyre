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
commits: ["c8595a409f15c267ab53b460532f409917124e2b", "ee2bcf8eff79c9849b8576ae12ba2a173108cd80", "c2e1ee93344be8f6989615ba84e315b590450273", "e6a6db47dbd8b988c8367a1965db50fd77d5abe7", "3a9b11f69c57cb576d2e3914ff7af7e54941b518", "5aaded213193d1af1cec693e729ac2872893b10b", "86c7d382312fae4fb52ae4effe40e021330f3ac2", "7c723298b41ee761067b15ae426651aa280fb75e", "54a7feeb0a86b24162a9603695b8662f6b236b6a", "3ef9e1b8222821f330942f3fbf13edab1b3497eb"]
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

Round-6 revision, plus checkpoint recovery, on top of the §6
Post-Merge Validation + Recovery Protocol work (Rounds 1–5: post-merge
gates on merged HEAD via detached worktree at `head_sha`,
`MrStatus::Reverted`, queue pause on failure, `git revert -m 1`
three-way-merge semantics, remediation tasks + RevertNotification,
resubmission-stable circuit breaker keyed on source branch,
merge-commit-sha-scoped manual revert, upstream-merge resolution,
fail-closed `repo_status` ref resolution, enabled
`builtin:attested-authorization` seed — details in
`specs/reviews/task-095.md`).

### Round-6 fix (unattested test arm)

The unattested arm of
`policy_engine::builtin_signed_authorization_satisfied_by_chain_presence`
built its context without `subject.attestation_chain_present`, so under
the engine's fail-closed missing-attribute semantics (`eval_condition`
returns `false` when the attribute is absent) the immutable deny's
`!= true` condition could not match and `evaluate` fell through to the
anonymous default deny — `matched_policy: None` instead of
`Some("builtin-require-signed-authorization")`, panicking the
assertion. Fix is test-side: the chain-less subject now sets
`attestation_chain_present = false` explicitly (with an in-test comment
explaining why), matching the neighboring
`not_equals_bool_condition_compares_booleans` test. The engine was
deliberately left unchanged: production always sets the attribute
explicitly whenever it runs the attestation-scoped evaluation
(`git_http.rs` / `merge_processor.rs`), and loosening `NotEquals` for
missing attributes would let any Allow policy with a `!=` condition
match subjects lacking the attribute — fail-open. The production
decision is Deny either way; only the matched-policy attribution was
wrong.

### Checkpoint recovery (this revision)

The worker lease expired after Round 6; the source was recovered as a
single squashed checkpoint commit `3ef9e1b8` on base `3c39ab1f`. The
nine pre-recovery lineage SHAs previously listed in `commits:` are not
resolvable in this repository's object store — they are retained above
for cross-reference with `specs/reviews/task-095.md` and controller
history. The full task work, including the Round-6 fix, now lives in
`3ef9e1b8`, which has been appended to the `commits:` frontmatter so
review scoping covers the recovered source (R3-F4 precedent: an
unlisted task commit is invisible to review).

### Test evidence

- On the recovered tree (`3ef9e1b8`): `cargo fmt --check -p
  gyre-server` clean; `check-arch.sh`, `check-abac-route-registry.sh`,
  `check-dead-message-kinds.sh`, `check-migration-versions.sh`,
  `check-byte-slice-truncation.sh`,
  `check-fail-open-ref-resolution.sh`, and
  `check-task-commit-attribution.sh` all pass; the Round-6 fix content
  verified present (`policy_engine.rs` unattested arm sets
  `attestation_chain_present = false` with the rationale comment).
- Standalone rustc probe
  (`/tmp/stage/review-evidence/probe_policy_test_fix.rs`) replicating
  the engine verbatim, running the fixed test's exact assertions:
  attested arm PASS (Allow, attributed to
  `builtin-attested-authorization`), unattested arm PASS (Deny,
  attributed to `builtin-require-signed-authorization`); the pre-fix
  absent-attribute construction reproduced the finding's failure
  signature (Deny, `matched_policy: None`) as a regression guard.
- Prior full-suite evidence (R2–R3 rounds, on the pre-recovery
  lineage): `gyre-server --lib merge_processor` 45 passed;
  `gyre-server` all suites 1273 passed; `gyre-adapters` 324 passed /
  12 ignored.

Sandbox limitation recorded for remote verification: this runtime has
no crates.io egress (connection refused to crates.io and
static.crates.io; no registry cache, no target dir), so `cargo test -p
gyre-server --lib policy_engine` and the full suite cannot execute
here. The deterministic repro and full-suite green must be confirmed
by CI/remote verification on the branch head.
