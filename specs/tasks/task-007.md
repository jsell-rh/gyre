# TASK-007: Authorization Provenance — Phase 2: Strategy-Implied Constraints (non-enforcing)

**Spec reference:** `authorization-provenance.md` §3.2, §8 Phase 2  
**Depends on:** TASK-006  
**Progress:** `needs-revision`  
**Review:** [`specs/reviews/task-007.md`](../reviews/task-007.md)  
**Note:** R1 found 3 findings: F1 (missing attestation_id in ConstraintViolation payload), F2 (notification priority 3 vs spec-required 2), F3 (default_branch hardcoded to "main").

## Spec Excerpt

Phase 2 derives and logs strategy-implied constraints at push and merge time:
- Derive strategy-implied constraints at push and merge time
- Surface constraint violations in the UI and message bus
- Humans can add explicit constraints at approval time
- Still audit-only: log, do not reject

Strategy-implied constraints come from persona_constraints, meta_spec_set_sha, scope, workspace trust level, and attestation level policy.

## Implementation Plan

1. **Push-time constraint evaluation** — In the git push handler:
   - Look up the task's attestation chain
   - Derive strategy-implied constraints from the `InputContent`
   - Evaluate all constraints against the actual diff
   - Log results and emit `ConstraintViolation` Event-tier messages on failures
   - Do NOT reject the push (audit-only)

2. **Merge-time constraint evaluation** — In the merge processor:
   - Collect all constraints (explicit + strategy-implied + gate)
   - Evaluate against the final merged diff
   - Log results and emit violations
   - Do NOT block the merge (audit-only)

3. **Constraint violation notifications** — Create priority-3 Inbox notifications for constraint violations (alongside gate failures)

4. **UI: constraint editing at approval time** — Extend the spec approval dialog:
   - Show strategy-implied constraints (read-only)
   - Allow explicit constraint entry (CEL expression editor)
   - Allow scope definition (glob pattern input for allowed/forbidden paths)
   - Dry-run button to evaluate current constraints against repo state

## Acceptance Criteria

- [ ] Strategy-implied constraints correctly derived at push time
- [ ] Constraint violations emitted as Event-tier messages
- [ ] Constraint violation notifications appear in Inbox
- [ ] Spec approval UI shows constraint editing (explicit + scope)
- [ ] Dry-run evaluation works from approval dialog
- [ ] All evaluations are audit-only (no rejections)
- [ ] Integration tests for push-time and merge-time evaluation
- [ ] `cargo test --all` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `authorization-provenance.md` §3.2, §7.5, §7.6, §8 Phase 2
3. Read `crates/gyre-server/src/git_http.rs` for push handler
4. Read `crates/gyre-server/src/api/merge_queue.rs` for merge processor
5. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- feat(provenance): implement authorization provenance Phase 2 — strategy-implied constraints (TASK-007)
