# Review: TASK-059 — Derive `attestation_level` from Repo Stack Policy

**Reviewer:** Verifier  
**Round:** R1  
**Verdict:** needs-revision

---

## Summary

The implementation correctly:
- Derives `attestation_level` from `WorkloadAttestation` data in `derive_attestation_level()`
- Propagates the derived level into `build_agent_context()` (replacing hardcoded `0`)
- Removes Phase 2 guards from all 5 call sites (4 in `constraint_check.rs` + 1 in `provenance.rs`)
- Passes `required_attestation_level` to `derive_strategy_constraints()` at all 5 call sites
- Adds unit tests for `derive_attestation_level()` covering all 4 levels (0-3)
- Adds integration tests for `build_agent_context` level derivation and `get_repo_required_attestation_level`

## Findings

- [ ] **F1: Aspirational test name — `attestation_level_constraint_evaluates_against_real_level` tests generation, not evaluation.** The test name and leading comments claim to verify constraint evaluation outcomes ("Level 0 agent with Level 3 repo policy → constraint fails", "Level 3 agent with Level 3 repo policy → constraint passes"). But the test body only calls `derive_strategy_constraints()` and asserts on constraint TEXT via `.contains("agent.attestation_level >= 3")`. No `evaluate_all()` call, no `build_cel_context()` call, no `AgentContext` with a specific level — the test never evaluates a constraint against an agent. The task acceptance criteria say "Tests verify correct level derivation **and constraint evaluation**" — the evaluation half is not tested here. Domain-layer tests in `constraint_evaluator.rs` DO cover actual CEL evaluation (e.g., `strategy_attestation_level_fails_for_low_level` evaluates `>= 3` against level 1), but this task-specific test's name is misleading and the acceptance criteria's evaluation test cases (Level 0 vs Level 3 policy, Level 3 vs Level 3 policy) are not exercised. **Fix:** Either rename the test to `attestation_level_constraints_generated_correctly` to match what it actually tests, or extend it to build an `AgentContext` with specific levels and call `evaluate_all()` to verify the constraints pass/fail as claimed in the comments. The latter is preferred since it fulfills the task plan's test case #4.

- [ ] **F2: `get_repo_required_attestation_level` hardcodes `Some(2)` — spec says repos can require Level 3.** The function at `constraint_check.rs:929` always returns `Some(2)` when any `repo_stack_policies` KV entry exists. The spec (supply-chain.md §2 "Policy per Level") says "Production repos may require Level 3 (Gyre-managed only)" and the task's own spec excerpt shows the constraint `agent.attestation_level >= 3`. With hardcoded `Some(2)`, this code path can never produce a `>= 3` constraint — production repos that should require Level 3 attestation only get Level 2 enforcement. The underlying KV store (`repo_stack_policies`) stores only a fingerprint string, so the required level is not currently persisted. **Fix:** Either (a) store a structured policy value (e.g., `{"fingerprint": "sha256:...", "required_level": 3}`) in the KV store and parse the level from it, or (b) document the limitation with a code comment and open a follow-up task for Level 3 policy support. Option (a) is preferred since it aligns with the spec's explicit multi-level policy model.
