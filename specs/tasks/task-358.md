---
title: "Stack-hash attestation unified into the CEL constraint set as one constraint among many"
spec_ref: "Spec gap spec-8493af123949c4d249a7ab8c / stack-hash-constraint-unification / 1d3b1257654b"
depends_on: []
progress: not-started
commits: []
---

## Required behavior

§7.3: the stack-hash equality check becomes a strategy-implied CEL constraint (agent.stack_hash == "<expected fingerprint>") derived from the repo's gyre-stack.lock in the common derive_strategy_constraints path (gyre-domain/src/constraint_evaluator.rs), evaluated by the single verification algorithm at push and merge alongside the explicit, strategy-implied, and gate constraints — the stack attestation is one constraint among many, and the two push-time verification paths (stack attestation + constraint verification) are unified into a single algorithm (today the equality check lives only inside enforce_stack_policy's separate path in git_http.rs, outside the CEL pipeline, while the attestation-level half is already unified). Because the constraint is derived in the common strategy-implied path, every route and boundary that evaluates the constraint set evaluates it — receive-pack and merge today, and the jj export delivery route once boundary-enforcement / the staged source-control plan's jj-export-gate-audit-parity establishes the shared enforcement call point there (no route-specific stack check). Always-on semantics are preserved: the repo-level stack constraint is derived and evaluated on every push to a policy-bearing repo, with or without an attestation chain (a chain-less push's constraint set is the repo-level set rather than empty), so unification never weakens stack enforcement relative to today's separate check. The separate stack-hash-equality branch in enforce_stack_policy is retired in favor of the unified constraint, while supply-chain.md's own observable policy semantics are preserved: Block mode = the constraint is enforcing (violation rejects with refs undone, naming the constraint); Warn mode = the violation emits the ConstraintViolation event and stack-drift audit record without rejecting; min-attestation-level rejection and the lockfile-drift surfaces (GET /repos/:id/stack-drift) keep working. The CEL context's agent.stack_hash continues to come from real attestation state (verified attestations per the supply-chain plan's cli-push-stack-attestation signing mechanism, or platform-observed state) — never an unverified self-report. Coordinate with the staged supply-chain plan (planner spec-bdd98e1a5c6d22f3f997de92, plan b2576f0bb5cd13421ed650b13f3912dad0c8bb4f739145bc108b9e2fc2a0a571, tasks cli-push-stack-attestation and forge-verification-override): land the unification as the evaluation-path change those requirements compose with (always-on evaluation, attributed override grants), not as a removal of the policy semantics they specify.

## Acceptance Criteria

- [ ] For a repo whose gyre-stack.lock pins a fingerprint, the constraint set assembled at push/merge includes the stack-hash equality constraint; a push by an agent whose attested stack_hash differs is rejected in Block mode naming the strategy stack-hash constraint, and in Warn mode emits ConstraintViolation plus the stack-drift audit record without rejecting — the same observable outcomes as today, via the single CEL pipeline.
- [ ] A repo without a lockfile pin derives no stack-hash constraint (no behavior change for unpinned repos).
- [ ] The separate stack-hash equality branch no longer exists outside the constraint pipeline (single verification algorithm); min-attestation-level rejection, drift detection, and the stack-drift audit trail still function.

## Verification intent

At the base: derive_strategy_constraints emits no stack-hash constraint (no 'agent.stack_hash ==' constraint is derived anywhere; the equality check lives only inside enforce_stack_policy's separate path at git_http.rs, outside the CEL pipeline). Probe: collect the evaluated constraint set for a pushing agent whose attested stack drifts from the lockfile — the set contains no stack constraint at the base; after the task the constraint is present and a Block-mode drift push is rejected naming it (with agent.stack_hash taken from real attestation state, so a falsely self-reported hash still fails).

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/authorization-provenance.md`. Source: `973068a86ab3154b6cba448fda9369125326cab7`; plan: `fdf43e24079d62bc303fa18c819671b940fa5414c868ffdc489bfd20a3d957c0`.

[
  {
    "id": "d181208ee20b618ef25389b3",
    "decision": "gap",
    "requirement_quote": "The stack attestation is no longer a separate check — it is one constraint among many, evaluated by the same verification algorithm.",
    "detail": "Partially implemented. The attestation-level half is unified: derive_strategy_constraints emits 'agent.attestation_level >= N' from the repo's real stack policy (KV repo_stack_policies), evaluated by the common CEL pipeline at push and merge. The AIBOM half is done: per-commit chain_attestation replaces the flat stack_attestation field. But the stack-hash half remains a separate check: the CEL constraint set never includes a stack-hash equality from gyre-stack.lock (no 'agent.stack_hash ==' constraint is derived anywhere); stack attestation is still enforced by the separate enforce_stack_policy path with its own Block/Warn modes, min-attestation-level rejection and lockfile-drift detection outside the constraint evaluation algorithm. The two verification paths are therefore not unified into one algorithm as the amendment requires.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "async fn enforce_stack_policy(",
        "detail": "Stack attestation enforcement remains a separate push-time check (min level with Block/Warn modes, gyre-stack.lock drift) outside the CEL constraint pipeline — the unification the amendment mandates is not carried out for the stack-hash constraint."
      },
      {
        "file": "crates/gyre-domain/src/constraint_evaluator.rs",
        "quote": "strategy: attestation level must be >= {level}",
        "detail": "The attestation-level constraint from repo stack policy IS unified into the CEL constraint set (the partially-implemented half)."
      },
      {
        "file": "crates/gyre-server/src/api/aibom.rs",
        "quote": "pub chain_attestation: Option<serde_json::Value>,",
        "detail": "AIBOM carries the full attestation chain per commit, replacing the flat stack_attestation field as specced."
      }
    ]
  }
]
