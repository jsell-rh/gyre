# TASK-005: Authorization Provenance — CEL Constraint Evaluator

**Spec reference:** `authorization-provenance.md` §3  
**Depends on:** TASK-003 (domain types)  
**Progress:** `needs-revision`  
**Review:** [specs/reviews/task-005.md](../reviews/task-005.md)

## Spec Excerpt

Output constraints are CEL predicates evaluated against a context assembled from the actual state of work. The evaluator is fail-closed: evaluation errors are treated as failures. The CEL context (§3.3) includes `input`, `output`, `agent`, `target`, and `action` fields.

Strategy-implied constraints (§3.2) are derived from:
- `persona_constraints` → verify agent persona matches
- `meta_spec_set_sha` → verify agent meta-spec set matches
- `scope` → convert allowed_paths/forbidden_paths to CEL path constraints
- Workspace trust level → additional constraints for Supervised workspaces
- Attestation level policy → verify agent attestation level

## Implementation Plan

1. Add `cel-interpreter` crate dependency (or a suitable Rust CEL library)
2. Create `crates/gyre-domain/src/constraint_evaluator.rs`:
   - `fn build_cel_context(input, output, agent, target, action) -> CelContext`
   - `fn evaluate_constraint(constraint: &OutputConstraint, context: &CelContext) -> Result<bool>`
   - `fn evaluate_all(constraints: &[OutputConstraint], context: &CelContext) -> VerificationResult`
   - `fn derive_strategy_constraints(content: &InputContent, workspace: &Workspace) -> Vec<OutputConstraint>`
3. Implement fail-closed semantics: CEL errors → REJECT
4. Add comprehensive unit tests covering:
   - Path glob constraints (allowed_paths, forbidden_paths)
   - Persona matching
   - Meta-spec SHA matching
   - Commit message constraints
   - Error handling (malformed CEL, missing fields)
   - Sequential evaluation with first-failure-stops behavior

## Acceptance Criteria

- [ ] CEL evaluator compiles and runs basic predicates
- [ ] Strategy-implied constraints are correctly derived from InputContent
- [ ] Fail-closed: evaluation errors are treated as constraint failures
- [ ] Path constraints correctly translate glob patterns to CEL
- [ ] All constraint sources (explicit, strategy-implied, gate) are evaluated
- [ ] Tests cover all constraint types from §3.3
- [ ] `cargo test --all` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `authorization-provenance.md` §3 for constraint specification
3. Evaluate available Rust CEL libraries (cel-interpreter, cel-rust)
4. If no suitable CEL library, implement a minimal evaluator for the subset needed
5. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- `feat(domain): add CEL constraint evaluator for authorization provenance §3`
