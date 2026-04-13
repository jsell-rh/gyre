---
title: "Derive `attestation_level` from Repo Stack Policy"
spec_ref: "authorization-provenance.md §3.2 (strategy-implied constraint from attestation level policy)"
depends_on: 
  - task-008
  - task-003
progress: complete
review: specs/reviews/task-059.md
coverage_sections: []
commits: 
  - 7ec53ae0
---

## Spec Excerpt

From `authorization-provenance.md` §3.2 (Strategy-Implied Constraints):

> **From attestation level policy:** If the repo's stack policy requires Level 3 attestation, the constraint verifies the agent's attestation level.
>
> ```cel
> // Implied from repo stack policy
> agent.attestation_level >= 3
> ```

## Current State

- **`attestation_level`** is hardcoded to `0` in `build_agent_context()` (`crates/gyre-server/src/constraint_check.rs:982`) with comment: `"Phase 2: attestation level derivation not yet implemented; callers guard constraint generation"`.
- A **Phase 2 guard** at lines 123-125, 360-362, 590-592, 814-816 of `constraint_check.rs` strips attestation-level constraints whenever `agent_ctx.attestation_level == 0`, preventing false violations.
- The `AgentContext` type (`crates/gyre-domain/src/constraint_evaluator.rs`) has the `attestation_level: u32` field ready to receive a real value.
- The existing supply-chain attestation model (`supply-chain.md` §2) defines three attestation levels tied to `gyre-stack.lock` fingerprinting, but this level is not currently propagated into the constraint evaluation context.

## Implementation Plan

1. **Determine the agent's attestation level**: At agent spawn time, the stack attestation check already runs. The result (attested vs. not) should be recorded. Derive the level from:
   - Level 0: No attestation data available
   - Level 1: Agent self-reports stack hash (basic)
   - Level 2: Stack hash matches `gyre-stack.lock` fingerprint (verified)
   - Level 3: Stack hash matches + container image hash verified (full)

2. **Propagate the level into `build_agent_context()`**: In `constraint_check.rs`, replace the hardcoded `0` with the actual level. The agent record or KV store should carry the attestation result from spawn time. Check `stack_attestation` data already stored in KV (the agent report mentions `stack_hash`, `container_id`, `image_hash` are retrieved from KV in `build_agent_context`).

3. **Remove the Phase 2 guard**: Once attestation_level is correctly derived, remove the `if agent_ctx.attestation_level == 0 { strategy_constraints.retain(...) }` guard at all 4 call sites. Constraints should evaluate naturally — if a repo requires Level 3 and the agent is Level 1, the constraint fails as designed.

4. **Update tests**: The existing test `attestation_level_guard_removes_attestation_constraints` (line 2092) tests the current guard behavior. Replace it with a test that verifies:
   - Level 0 agent with Level 3 repo policy → constraint fails
   - Level 3 agent with Level 3 repo policy → constraint passes
   - Level 2 agent with no repo policy → no attestation constraint generated

## Acceptance Criteria

- [ ] `build_agent_context()` derives `attestation_level` from the agent's actual stack attestation data
- [ ] The Phase 2 guard (`attestation_level == 0` → strip constraints) is removed from all 4 call sites
- [ ] Strategy-implied attestation-level constraints evaluate against the real agent level
- [ ] Tests verify correct level derivation and constraint evaluation
- [ ] `cargo test --all` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `crates/gyre-server/src/constraint_check.rs` — focus on `build_agent_context()` (line ~970) and the 4 guard sites
3. Read `crates/gyre-server/src/api/spawn.rs` — understand how stack attestation data is recorded at spawn time
4. Read `crates/gyre-domain/src/constraint_evaluator.rs` — `AgentContext.attestation_level` field and `derive_strategy_constraints()`
5. Read `crates/gyre-common/src/attestation.rs` — check if attestation levels are already defined as an enum or constant
6. Grep for `stack_hash`, `gyre-stack.lock`, `attestation_level` to find all relevant code paths
7. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- `7ec53ae0` feat(provenance): derive attestation_level from workload data and remove Phase 2 guards (TASK-059)
