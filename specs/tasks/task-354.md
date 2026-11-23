---
title: "Gate agents produce signed GateConstraint records; legacy AttestationBundle deprecated only for fully-mapped chained merges"
spec_ref: "Spec gap spec-8493af123949c4d249a7ab8c / gate-constraint-emission / 1d3b1257654b"
depends_on: [task-352]
progress: not-started
commits: []
---

## Required behavior

§3.2 Source 3 (emission half): review/validation gate agents attach signed GateConstraint records — a gate whose structured verdict output carries restrictions for subsequent work (e.g. a review gate scoping remaining changes to specific paths) constructs GateAttestation.constraint = Some(GateConstraint { gate_id, gate_name, constraint, signed_by }) signed over the shared signable_bytes with the gate signer key, instead of the unconditional constraint: None (gate_executor.rs:2028). The emitter must be a real production gate path (the gate executor's review/validation gates), driven by the gate's actual verdict output — the platform never fabricates constraints the gate did not produce; constraints are additive only (a gate can tighten, never loosen — the existing accumulate path already enforces concatenation). §8 Phase 4 remaining bullets, with reachability delivered by the dependency: orchestrator→agent DerivedInput production fires in the production flow (chain-task-commit-linkage), full chain verification at push/merge (chain-verification-trust and boundary-enforcement), AIBOM chain attestations per commit (already implemented, reachable via the dependency). Legacy deprecation (review correction — conditional, not unconditional): when a chain attestation is available for the merged work AND the chain's merge-time node carries the §5.2 mapping-table fields (conversation_sha/completion_summary, delivered by chain-task-commit-linkage), the merge processor no longer generates/stores the legacy AttestationBundle (the chain note is the attestation of record and no mapped data is lost); the legacy bundle/type/port remain as the fallback for chain-less legacy work during migration, per §5.2's backward-compatibility retention — do not deprecate the legacy bundle while the chain lacks the mapped fields.

## Acceptance Criteria

- [ ] A review/validation gate that produces a restriction in its verdict yields a persisted GateAttestation with constraint: Some(GateConstraint) whose signature verifies over signable_bytes; a production (non-test) GateConstraint constructor exists.
- [ ] The gate constraint enters the evaluated set at merge: a merge whose final diff violates a gate-attached constraint is rejected naming that constraint; gates cannot loosen the set (no replace/subtract path).
- [ ] A merge with a chain available produces no new legacy AttestationBundle record or bundle note (chain note only) while the chain's merge node carries conversation_sha/completion_summary — a chained merge loses no data the legacy bundle carried; chain-less merges still write the legacy fallback.

## Verification intent

At the base: run any production review gate and read its GateAttestation — constraint is None (gate_executor.rs:2028 'constraint: None, // Gate constraints are attached by review/validation agents') and the only non-test constructions of GateConstraint are in test modules, so no gate can ever tighten the constraint set; then merge a diff that violates what the review gate required — the merge succeeds because no gate constraint is evaluated. After the task the gate's constraint is present and signature-verified, and the violating merge is rejected naming it; a chained merge whose chain node carries the mapped §5.2 fields leaves no legacy bundle record (at the base one is always generated), while the mapped data (conversation_sha/completion_summary) remains readable from the chain — at the base it exists only on the legacy bundle.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/authorization-provenance.md`. Source: `973068a86ab3154b6cba448fda9369125326cab7`; plan: `fdf43e24079d62bc303fa18c819671b940fa5414c868ffdc489bfd20a3d957c0`.

[
  {
    "id": "3a1de6328a8b4ff8639ece89",
    "decision": "gap",
    "requirement_quote": "A gate agent that reviews code can attach constraints that subsequent gates or the merge processor must verify:",
    "detail": "The GateConstraint type, its inclusion in GateAttestation.constraint, and its consumption are real: gate constraints are collected from chain nodes and evaluated at push/merge time and in the verification endpoint, additively with the other sources. But no production gate ever produces one. The only production site that builds GateAttestation records (gate_executor's attach path) sets constraint: None with the comment that gate constraints are 'attached by review/validation agents', and no review/validation agent path constructs a GateConstraint — grep shows the only non-type constructions of GateConstraint are in test modules (constraint_evaluator, merge_processor tests). The signed-by-gate-agent emission half of Source 3 is therefore unimplemented; only the evaluation half exists.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/gate_executor.rs",
        "quote": "constraint: None, // Gate constraints are attached by review/validation agents",
        "detail": "The sole production GateAttestation constructor leaves the gate constraint empty; no other production code path creates a GateConstraint (all other constructions are in #[cfg(test)] modules), so gates can never tighten the constraint set as §3.2 Source 3 requires."
      },
      {
        "file": "crates/gyre-server/src/merge_processor.rs",
        "quote": "if !chain_found && denial.is_none() {",
        "detail": "Merge-time consumption of gate constraints exists (accumulate_chain_constraints collects GateAttestation.constraint into the evaluated set), confirming the gap is on the production emission side only."
      }
    ]
  },
  {
    "id": "1f7628ebe440a75c8f32588b",
    "decision": "gap",
    "requirement_quote": "- Gate agents produce `GateConstraint` records.",
    "detail": "Mixed: chain verification at push and merge time exists (verify_chain + constraint evaluation in both enforce paths); the AIBOM is extended with full chain attestations per commit; and orchestrator DerivedInput production code exists — but it never fires in the production flow (find_by_task cannot see the unlinked root, §2.1/§4.2). 'Gate agents produce GateConstraint records' is not implemented at all: the only production GateAttestation constructor sets constraint: None and no review/validation agent path creates a GateConstraint (all other constructions are tests). 'Legacy AttestationBundle deprecated' is not done either — the legacy bundle is still generated at every merge and written as the git-note fallback whenever no chain is found (which is always, in production).",
    "evidence": [
      {
        "file": "crates/gyre-server/src/gate_executor.rs",
        "quote": "constraint: None, // Gate constraints are attached by review/validation agents",
        "detail": "No production gate emits a GateConstraint — the phase's explicit bullet is unimplemented (only the consumption side exists)."
      },
      {
        "file": "crates/gyre-server/src/merge_processor.rs",
        "quote": "// During the dual-write period, we write EITHER the chain attestation",
        "detail": "The legacy AttestationBundle is still produced at merge and used as the git-note fallback — not deprecated, though this is consistent with the chain never being findable by commit in production."
      },
      {
        "file": "crates/gyre-server/src/api/spawn.rs",
        "quote": "no attestation chain for task — skipping DerivedInput creation",
        "detail": "Orchestrator → agent DerivedInput production never fires in the production flow because the parent chain is unreachable by task lookup."
      }
    ]
  }
]
