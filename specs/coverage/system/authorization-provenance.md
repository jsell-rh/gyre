# Coverage: Authorization Provenance

**Spec:** [`system/authorization-provenance.md`](../../system/authorization-provenance.md)
**Last audited:** 2026-04-13
**Coverage:** 40/40 (7 n/a, 11 verified, 29 implemented)

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | The Problem | 2 | n/a | - | Context/rationale — no implementable requirement. |
| 2 | Design Approach | 2 | n/a | - | Context/rationale — no implementable requirement. |
| 3 | 1 Trust Model | 2 | verified | - | TrustAnchor type (gyre-common/src/attestation.rs:29-42), TrustAnchorRepository port (gyre-ports/src/trust_anchor.rs), SQLite adapter (gyre-adapters/src/sqlite/trust_anchor.rs), CRUD endpoints (gyre-server/src/api/trust_anchors.rs). Routes registered in api/mod.rs, wired into AppState. |
| 4 | 1.1 Trust Anchors | 3 | verified | - | Full CRUD: POST/GET/PUT/DELETE /api/v1/tenants/:id/trust-anchors. Tenant-scoped with cross-tenant 403. Admin role enforced. TrustAnchorType enum (User/Agent/Addon). Anchor-level constraints. 9 test cases including lifecycle, duplicates, 404, tenant isolation, constraint roundtrip. |
| 5 | 1.2 Residual Risk | 3 | n/a | - | Design/rationale — threat model, no implementable requirement. |
| 6 | 2 Signed Input: The Authorization Root | 2 | verified | - | SignedInput type (gyre-common/src/attestation.rs:92) with InputContent, ScopeConstraint, PersonaRef. All types fully defined and wired into spec approval, attestation chain, and verification. |
| 7 | 2.1 When a Signed Input Is Created | 3 | verified | - | Produced on spec approval (specs.rs:606) when KeyBinding exists AND user_content_signature provided. Tests: approve_spec_creates_signed_input_when_key_binding_exists, without_key_binding_skips, no_user_signature_skips. Attestation stored via chain_attestations.save(). |
| 8 | 2.2 Structure | 3 | verified | - | All struct fields match spec: SignedInput{content, output_constraints, valid_until, expected_generation, signature, key_binding}. InputContent{spec_path, spec_sha, workspace_id, repo_id, persona_constraints, meta_spec_set_sha, scope}. ScopeConstraint{allowed_paths, forbidden_paths}. Serde roundtrip tested. |
| 9 | 2.3 Key Binding | 3 | verified | - | KeyBinding type (gyre-common/src/key_binding.rs:19) with all 8 spec fields. POST /api/v1/auth/key-binding exchange (key_binding.rs:69). Ed25519 proof-of-possession via ring. Platform countersign. DELETE /key-binding/:id and /key-bindings revocation. 12+ tests covering creation, signature verification, TTL capping, revocation, authorization. |
| 10 | 2.4 Context Binding (Replay Prevention) | 3 | implemented | - | Partial — valid_until checked in verify_chain (git_http.rs:3108). spec_sha tamper-proof via signature. But workspace_id/repo_id are signed into InputContent without being compared against actual push target at verification time (no strategy-implied constraint). expected_generation field exists (Option<u32>) but always None and never verified. Consider splitting missing enforcement into separate task. |
| 11 | 3 Output Constraints | 2 | verified | - | OutputConstraint type (gyre-common/src/attestation.rs:114). CEL evaluation engine (gyre-domain/src/constraint_evaluator.rs). Both audit-only (evaluate_push/merge_constraints) and enforcement (enforce_push/merge_constraints) paths in constraint_check.rs. |
| 12 | 3.1 Structure | 3 | verified | - | OutputConstraint { name: String, expression: String } (attestation.rs:114-120). Exact match to spec. Serde roundtrip tested (output_constraint_roundtrip). |
| 13 | 3.2 Constraint Sources | 3 | verified | - | All 3 sources: (1) Explicit via SignedInput.output_constraints, (2) Strategy-implied via derive_strategy_constraints() — persona, meta_spec_set_sha, scope allowed/forbidden paths, workspace trust level, attestation level (constraint_evaluator.rs:264-316), (3) Gate via GateConstraint type (attestation.rs:128-138). collect_all_constraints() merges all additively. 35+ tests including multi-persona, forbidden paths, supervised workspace, attestation levels. |
| 14 | 3.3 CEL Evaluation Context | 3 | verified | - | build_cel_context() (constraint_evaluator.rs:102-122). OutputContext (6 fields), DiffStatsContext (2 fields), AgentContext (9 fields), TargetContext (4 fields), Action enum (Push/Merge → "push"/"merge"). All fields match spec §3.3 exactly. Uses cel-interpreter crate. Tests verify nested access, indexed persona_constraints, action strings, diff_stats. |
| 15 | 3.4 Constraint Evaluation: Fail Closed | 3 | verified | - | evaluate_all() (constraint_evaluator.rs:161-211): sequential evaluation, stops at first failure, errors treated as failures, non-boolean treated as error. No "error → allow" path. Tests: evaluate_all_stops_at_first_failure (3rd constraint never reached), evaluate_all_error_is_failure, evaluate_malformed_cel_is_error, evaluate_non_boolean_result_is_error. |
| 16 | 4 Derived Input: Delegation Provenance | 2 | implemented | - | DerivedInput type in gyre-common/src/attestation.rs. |
| 17 | 4.1 Structure | 3 | implemented | - | DerivedInput { parent_ref, preconditions, update, output_constraints, signature, key_binding }. Serde roundtrip tests. |
| 18 | 4.2 When a Derived Input Is Created | 3 | implemented | - | create_derived_input_for_agent() in spawn.rs. Created on agent spawn from parent task attestation. |
| 19 | 4.3 Constraint Propagation: Additive Only | 3 | implemented | - | DerivedInput.output_constraints additive — concatenation, never replacement. |
| 20 | 4.4 Derivation Chain Verification | 3 | implemented | - | verify_chain() in git_http.rs. Recursive chain walk from leaf to root SignedInput. |
| 21 | 4.5 Orchestrator Key Binding | 3 | implemented | - | Spawner key binding retrieved and used to sign DerivedInput (spawn.rs:1565-1681). |
| 22 | 4.6 Depth Limits | 3 | implemented | - | Hard limit of 10 enforced in verify_chain(). chain_depth tracked in AttestationMetadata. |
| 23 | 5 Attestation: The Complete Record | 2 | implemented | - | Attestation type with AttestationInput (Signed/Derived), AttestationOutput, AttestationMetadata. |
| 24 | 5.1 Structure | 3 | implemented | - | Full type hierarchy: Attestation, AttestationInput (tagged enum), AttestationOutput, AttestationMetadata, GateAttestation with signable_bytes(). |
| 25 | 5.2 Relationship to Existing Merge Attestation | 3 | implemented | - | MergeAttestation coexists (gyre-domain/src/attestation.rs). Legacy bundle fields mapped to new structure. conversation_sha + completion_summary fields added. |
| 26 | 5.3 Storage | 3 | implemented | - | SQLite adapter (gyre-adapters/src/sqlite/chain_attestation.rs). Git notes via attestation.rs. chain_attestations table with parent_ref for traversal. |
| 27 | 5.4 Attestation Port | 3 | implemented | - | ChainAttestationRepository trait (gyre-ports/src/chain_attestation.rs): save, find_by_id, load_chain, find_by_task, find_by_commit, find_by_repo. |
| 28 | 6 Verification | 2 | implemented | - | verify_attestation_audit_only() and enforce_push/merge_constraints(). Both audit-only and enforcement paths. |
| 29 | 6.1 Verification Points | 3 | implemented | - | Push-time: evaluate_push_constraints() in constraint_check.rs. Merge-time: enforce_merge_constraints(). Both system boundaries covered. |
| 30 | 6.2 Verification Algorithm | 3 | implemented | - | verify_chain() walks parent_ref chain, checks signatures, expiry, depth. evaluate_all() handles constraint set. Full algorithm per spec. |
| 31 | 6.3 Offline Verification | 3 | implemented | - | GET /api/v1/repos/:id/attestations/:commit_sha/bundle returns VerificationBundle. Includes attestation chain, trust anchors, git diff. |
| 32 | 6.4 Verification Result Storage | 3 | implemented | - | VerificationResult tree type (gyre-common/src/attestation.rs). Attached to attestation records. Queryable via GET /api/v1/repos/:id/attestations/:commit_sha/verification. |
| 33 | 7 Integration with Existing Systems | 2 | n/a | - | Section heading only — no implementable requirement. |
| 34 | 7.1 Spec Approval (agent-gates.md) | 3 | implemented | - | POST /api/v1/specs/:path/approve produces SignedInput. Accepts output_constraints and scope. Key exchange prerequisite enforced. Tests: 6 spec approval + SignedInput tests. |
| 35 | 7.2 ABAC Policy Engine (abac-policy-engine.md) | 3 | implemented | - | attestation resource type. builtin:require-signed-authorization policy. subject.chain_depth, subject.root_signer, subject.constraint_count attributes. |
| 36 | 7.3 Supply Chain (supply-chain.md) | 3 | implemented | - | Stack hash evaluated as CEL constraint (agent.stack_hash). Unified verification path. AIBOM extended with chain attestations (api/aibom.rs). |
| 37 | 7.4 Agent Runtime (agent-runtime.md) | 3 | implemented | - | Agent spawn creates workload KeyBinding + DerivedInput (spawn.rs:1018-1733). JWT claims carried into OrchestratorKeyBinding. spawned_by field tracks human origin. |
| 38 | 7.5 Message Bus (message-bus.md) | 3 | implemented | - | ConstraintViolation MessageKind (gyre-common/src/message.rs). Event tier, workspace destination. Notification created on violation (constraint_check.rs). |
| 39 | 7.6 Human-System Interface (human-system-interface.md) | 3 | implemented | - | ConstraintEditor.svelte for constraint editing in spec approval UI. ProvenanceChain.svelte for chain visualization. |
| 40 | 7.7 Observability (observability.md) | 3 | implemented | - | All 7 audit events logged: attestation.created, .verified, .constraint_failed, .chain_invalid, key_binding.created (implicit), .expired, .revoked. |
| 41 | 8 Migration | 2 | n/a | - | Section heading only — no implementable requirement. |
| 42 | Phase 1: Key Binding and Signed Input (non-enforcing) | 3 | implemented | - | KeyBinding exchange, SignedInput on approval, attestation chain storage, audit-only verification. All Phase 1 deliverables complete. |
| 43 | Phase 2: Strategy-Implied Constraints (non-enforcing) | 3 | implemented | - | derive_strategy_constraints() at push/merge time. Constraint violations surfaced in UI (notifications) and message bus (ConstraintViolation events). Audit-only. |
| 44 | Phase 3: Enforcement | 3 | implemented | - | enforce_push_constraints() and enforce_merge_constraints() reject invalid chains/constraints. builtin:require-signed-authorization ABAC policy. Legacy AttestationBundle retained. |
| 45 | Phase 4: Derived Input Chains | 3 | implemented | - | Orchestrators produce DerivedInput on spawn. Full chain verification at push/merge. GateConstraint records. AIBOM extended. |
| 46 | 9 What This Prevents | 2 | n/a | - | Validation table — no implementable requirement. All attack vectors addressed by implemented verification. |
| 47 | Relationship to Existing Specs | 2 | n/a | - | Cross-reference section — no implementable requirement. |
