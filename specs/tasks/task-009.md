# TASK-009: Authorization Provenance — Phase 4: Full Chains and AIBOM Integration

**Spec reference:** `authorization-provenance.md` §4, §5.2, §7.3, §8 Phase 4  
**Depends on:** TASK-008  
**Progress:** `ready-for-review`

## Spec Excerpt

Phase 4 completes the authorization provenance system:
- Orchestrators produce `DerivedInput` for sub-task delegation
- Full chain verification at push and merge time
- Gate agents produce `GateConstraint` records
- AIBOM extended with full chain attestations
- Legacy `AttestationBundle` deprecated

## Implementation Plan

1. **Orchestrator-produced DerivedInputs** — When orchestrators decompose tasks:
   - The `DerivedInput` carries the parent attestation reference
   - Constraint propagation is additive-only (§4.3)
   - Chain depth bounded by workspace config (§4.6, hard limit 10)

2. **Gate constraints** — Extend gate execution:
   - Gate agents sign their own results (`GateAttestation`)
   - Gates can attach `GateConstraint` records
   - Merge verification includes gate constraints

3. **AIBOM extension** — Extend AIBOM generation (`supply-chain.md` §5):
   - Include full attestation chain per commit
   - Replace flat `stack_attestation` field with chain attestation
   - Unify stack attestation into constraint set (§7.3)

4. **Deprecate legacy AttestationBundle** — Flag as deprecated:
   - Dual-write period (keep producing both formats)
   - Add deprecation warning in API responses that return `AttestationBundle`

5. **Explorer visualization** — Add provenance chain view (§7.6):
   - Clicking a commit shows attestation chain as directed graph
   - Each node shows signer identity, constraint count, verification status
   - Failed constraints highlighted with expression and failing value

6. **Observability** — New audit event categories (§7.7):
   - `attestation.created`, `attestation.verified`, `attestation.constraint_failed`, `attestation.chain_invalid`
   - `key_binding.created`, `key_binding.expired`, `key_binding.revoked`

## Acceptance Criteria

- [ ] Orchestrators produce DerivedInputs with additive-only constraint propagation
- [ ] Gate agents sign their results and can attach constraints
- [ ] Full chain verification walks from leaf to root SignedInput
- [ ] AIBOM includes full attestation chains
- [ ] Chain depth is bounded (configurable, hard limit 10)
- [ ] Explorer shows provenance chain visualization for commits
- [ ] Audit events for all attestation lifecycle operations
- [ ] Integration tests for full chain (human → orchestrator → agent)
- [ ] `cargo test --all` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `authorization-provenance.md` §4, §5.2, §7.3, §7.6, §7.7, §8 Phase 4
3. Read `crates/gyre-server/src/api/aibom.rs` for AIBOM generation
4. Read `crates/gyre-server/src/gate_executor.rs` for gate execution
5. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- feat(provenance): implement authorization provenance Phase 4 — full chains and AIBOM integration (TASK-009)
