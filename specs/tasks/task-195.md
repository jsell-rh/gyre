---
title: "Per-spec manifest gate selection for MR gate chain"
spec_ref: "spec-registry.md §16 Agent Gates Integration"
depends_on: []
progress: not-started
coverage_sections:
  - "spec-registry.md §16"
commits: []
---

## Spec Excerpt

From `specs/system/spec-registry.md` §16 (Integration → Agent Gates):

> The `gates:` field in the manifest feeds the gate chain:
> - MR references `system/identity-security.md` -> manifest says gates = [security, accountability]
> - Forge spawns security gate agent and accountability gate agent for this MR
> - Different specs can require different gate agents

§5 (shared schema) — each gate declares `persona`, `min_attestation_level` (default 1), and optional `stack_hash`.

## Problem (current state)

`trigger_gates_for_mr` in `crates/gyre-server/src/gate_executor.rs:23` sources gates **only** from repo-level `quality_gates.list_by_repo_id(repo_id)`. The manifest's per-spec `gates: Vec<GateConfig>` (parsed onto `SpecEntry` in `crates/gyre-server/src/spec_registry.rs`) is never consumed anywhere. As a result:
- An MR whose `spec_ref` points at a spec declaring `gates: [security, accountability]` does NOT get those persona gate agents spawned.
- Different specs cannot require different gate agents — the coverage note (§16) confirms this is unimplemented.

The MR carries `spec_ref` in `path@sha` form (`crates/gyre-server/src/api/merge_requests.rs:255`). `trigger_gates_for_mr` is invoked from `crates/gyre-server/src/api/merge_queue.rs:77` with `mr_id` + `repo_id`.

## Implementation Plan

1. **Resolve the MR's spec_ref and manifest gates.** In `trigger_gates_for_mr` (or a helper it calls):
   - Load the MR by `mr_id`; if it has a `spec_ref`, split off the `path` portion.
   - Load the repo by `repo_id` to get its on-disk path; read `specs/manifest.yaml` at HEAD via `crate::spec_registry::read_git_file` + `parse_manifest`.
   - Find the `SpecEntry` for the referenced path and read its `gates: Vec<GateConfig>` (`persona`, `min_attestation_level`, `stack_hash`).

2. **Synthesize per-spec gates into the chain.** For each manifest `GateConfig`, construct an in-memory `gyre_domain::QualityGate` with `gate_type = GateType::AgentReview` (the persona-driven gate type), `persona = <resolved persona file path for the named persona>`, `required = true`, and a stable name (e.g. `spec-gate:<persona>`). Append these to the repo-level gates before creating `GateResult`s. Dedup so the same persona isn't run twice if already present as a repo gate.

3. **Enforce attestation constraints on the spawned gate agent.** When the manifest gate declares `min_attestation_level` / `stack_hash`, thread those into the gate agent spawn / validation path (`run_agent_review_gate`, gate_executor.rs:331) so a gate agent that does not meet the attestation floor or stack_hash cannot satisfy the gate. Reuse the constraint machinery already present (`gyre_domain::constraint_evaluator` supports `agent.attestation_level >= N`) rather than inventing a parallel check.

4. **Behavior when no spec_ref / no manifest entry:** fall back to today's repo-level-only gate set (no regression).

## Acceptance Criteria

- An MR whose `spec_ref` path resolves to a manifest `SpecEntry` with `gates: [security, accountability]` produces `GateResult`s for those two persona gates (in addition to any repo-level gates), and the corresponding gate agents are spawned.
- Two MRs referencing two different specs with different manifest `gates:` lists get different gate sets.
- A manifest gate with `min_attestation_level`/`stack_hash` set is not satisfiable by a gate agent below that attestation floor or with a mismatched stack_hash.
- MRs with no `spec_ref` (or a path absent from the manifest) behave exactly as before (repo-level gates only).
- `cargo build --all` and `bash scripts/check-arch.sh` pass.

## Agent Instructions

- Real work only. The manifest `gates:` field MUST actually drive which gate agents run for the MR — no audit-only, no logging-without-spawning, no hardcoded persona list.
- Write a hard test that fails if per-spec gate selection regresses: factor the "compute effective gate set for an MR" logic into a pure function taking (repo gates, matched SpecEntry) → gate list, and assert that a spec with `gates:[security,accountability]` yields exactly those persona gates merged with repo gates (deduped), while a `None` spec_ref yields only repo gates. Add a test proving a below-`min_attestation_level` agent cannot satisfy a manifest gate. Do NOT write assertionless or mirrored-logic tests.
- Reuse the existing `GateType::AgentReview` path and `constraint_evaluator`; do not add a second gate-execution mechanism.
- Skip project-wide formatters/linters and the full test suite; run only targeted tests plus `cargo build --all` and `scripts/check-arch.sh`.
- Respect hexagonal boundaries; wiring lives in `gyre-server`, reusing existing domain gate types and ports.
