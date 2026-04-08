# Review: TASK-007 — Authorization Provenance Phase 2: Strategy-Implied Constraints

**Reviewer:** Verifier  
**Commit:** `af37a23a`  
**Round:** R1  
**Date:** 2026-04-08

## Findings

- [ ] [process-revision-complete] **F1 — `ConstraintViolation` event payload missing spec-required `attestation_id` field**
  `crates/gyre-server/src/constraint_check.rs` lines 720–729: The `emit_constraint_violations` function builds the ConstraintViolation event payload with fields `constraint_name`, `expression`, `action`, `agent_id`, `repo_id`, `task_id`, `timestamp`, `message`. The spec §7.5 defines the `ConstraintViolation` message kind with `attestation_id: string` as a required field — it identifies which specific attestation chain was being verified when the constraint failed. The implementation omits `attestation_id` and substitutes `task_id` (not in the spec schema). Even in audit-only mode, `attestation_id` is needed for forensic tracing: if a task has multiple attestation nodes, `task_id` alone does not identify which node failed. The spec also requires `context_snapshot: object` (the CEL context at evaluation time), which is omitted. **Fix:** Pass the attestation ID through to `emit_constraint_violations` and include it in the payload. Add `context_snapshot` with the serialized CEL context (or a representative subset).

- [ ] [process-revision-complete] **F2 — `ConstraintViolation` notification priority is 3 but spec requires 2**
  `crates/gyre-common/src/notification.rs` line 110: `NotificationType::ConstraintViolation` returns priority 3. The spec §7.5 states: "Human operator's inbox receives notification (priority 2 — high but not critical, since push/merge already rejected)." The task file (task-007.md line 33) erroneously transcribed this as "priority-3" and the implementation followed the task description rather than the spec. The spec is the source of truth. **Fix:** Change `Self::ConstraintViolation => 3` to `Self::ConstraintViolation => 2` in `notification.rs`.

- [ ] [process-revision-complete] **F3 — Push-time constraint evaluation hardcodes `default_branch` to `"main"`**
  `crates/gyre-server/src/constraint_check.rs` line 145: The `evaluate_push_constraints` function sets `default_branch: "main".to_string()` in the `TargetContext`. The repository's actual `default_branch` field is available in the calling context (`git_http.rs` line 400: `let default_branch_clone = default_branch;`) but is not passed to `evaluate_push_constraints`. Any CEL constraint referencing `target.default_branch` (e.g., `target.branch == target.default_branch` to check if the push targets the default branch) will silently return wrong results for repos with a non-"main" default branch (e.g., "master", "develop"). **Fix:** Add a `default_branch: &str` parameter to `evaluate_push_constraints` and pass it from `git_http.rs` where it is already resolved from the repository record.
