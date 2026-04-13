# Review: TASK-059 — Derive `attestation_level` from Repo Stack Policy

**Reviewer:** Verifier  
**Round:** R2  
**Verdict:** pass

---

## Summary

The implementation correctly:
- Derives `attestation_level` from `WorkloadAttestation` data in `derive_attestation_level()`
- Propagates the derived level into `build_agent_context()` (replacing hardcoded `0`)
- Removes Phase 2 guards from all 5 call sites (4 in `constraint_check.rs` + 1 in `provenance.rs`)
- Passes `required_attestation_level` to `derive_strategy_constraints()` at all 5 call sites
- Adds unit tests for `derive_attestation_level()` covering all 4 levels (0-3) plus edge case
- Adds integration tests for `build_agent_context` level derivation and `get_repo_required_attestation_level`
- Stores structured `RepoStackPolicy` with `required_level` field in KV (fixing F2)
- Renames aspirational test to accurately describe what it tests (fixing F1)
- Backward-compatible `parse_stack_policy()` handles both structured JSON and legacy plain-string entries
- Updates `git_http.rs` `check_pre_accept_gates` to extract fingerprint from structured policy (preventing regression)
- Adds Level 3 API tests for `set_stack_policy`/`get_stack_policy` round-trip

All 1,625 tests pass (367 domain + 1,109 server + integration suites).

## R1 Findings — Resolution

- [-] [process-revision-complete] **F1 (resolved R2): Aspirational test name — `attestation_level_constraint_evaluates_against_real_level` tests generation, not evaluation.** Test renamed to `attestation_level_constraints_generated_correctly` with updated comments clarifying that actual CEL evaluation is tested in `gyre-domain` `constraint_evaluator` tests (`strategy_attestation_level_fails_for_low_level`). The test now accurately describes its scope.

- [-] [process-revision-complete] **F2 (resolved R2): `get_repo_required_attestation_level` hardcodes `Some(2)` — spec says repos can require Level 3.** Fixed via option (a): introduced `RepoStackPolicy` struct with `fingerprint` and `required_level` fields. `set_stack_policy` stores structured JSON in KV. `get_repo_required_attestation_level` now parses the stored value via `parse_stack_policy()` and returns the actual `required_level`. Legacy plain-string entries default to Level 2 for backward compatibility. New tests cover Level 2 policy, Level 3 policy, legacy format, and API round-trip at both levels.

## R2 Findings

No new findings.
