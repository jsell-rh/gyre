---
title: "Complete chain verification: immutable trust-anchor policy, anchor constraints, binding verification, preconditions, content-addressed node ids, workload claims, configurable depth limits"
spec_ref: "Spec gap spec-8493af123949c4d249a7ab8c / chain-verification-trust / 1d3b1257654b"
depends_on: [task-352]
progress: not-started
commits: []
---

## Required behavior

§1.1: trust-anchor management (POST/PUT/DELETE /api/v1/tenants/:id/trust-anchors) is guarded by an immutable Deny ABAC policy (builtin, same pattern as builtin:require-human-spec-approval, not overridable by an Allow policy) that denies non-human subjects — an agent principal holding the Admin role can no longer add, remove, or rotate anchors (reads keep the Admin role gate; today protection is handler RBAC only, trust_anchors.rs require_admin). Anchor-level constraints (TrustAnchor.constraints, §1.1/§3.2) are loaded into the evaluated constraint set at verification time for the anchors that authenticated the chain's key bindings — additive with explicit, strategy-implied, and gate constraints (today they are stored and returned by the CRUD API but never evaluated). §4.4 step 2: chain verification resolves each node's key_binding.trust_anchor_id against the tenant-scoped trust anchor registry (a workspace inherits its tenant's anchors) and fails closed on unknown/removed anchors; the platform countersignature on bindings is verified as the registration witness, and — per the §2.3 review correction delivered by approval-signed-input-root — the user's document signature over the binding document is verified so the binding's identity claims are user-attested, not platform-attested; binding expiry is checked (already present). §4.4 steps 4–6: preconditions are evaluated against the parent's verified state (CEL, evaluation errors reject — fail closed; today spawn creates them empty and no verifier executes them); the update expression is evaluated to derive content and the derived content hash must match the claimed content; parent linkage is verified by content hash — parent_ref must equal the parent attestation's stored id, spawn-created parent_ref values must be the parent's stored id (fixing the SHA-of-JSON vs stored-id mismatch; today verify_chain checks only 'has_parent_ref && has_parent_at_lower_depth', git_http.rs), and load_chain must traverse production-created chains root→leaf with no gaps. §5.1 id field (review correction): spawn-created derived attestation node ids are content-addressed — SHA-256 over the node's canonical content, matching the root's SHA-256-of-SignedInput construction — replacing the random uuid::Uuid::new_v4() id (spawn.rs:4470), so identical derivations yield identical ids and any verifier (including offline §6.3 verification) can recompute and validate an id. §4.5: workload key bindings carry the JWT claims (task_id, spawned_by, stack_hash, persona) populated at spawn (today the KeyBinding type has none of these fields, gyre-common/src/key_binding.rs); verification checks them against the attestation metadata/agent context; the synthesized empty-signature fallback bindings are replaced with real signed bindings so the binding attests the workload identity, not just key possession; spawned_by links the orchestrator back to the human who approved the chain root. §4.6: the chain depth limit is workspace-configurable (a strategy-implied chain.depth <= N derived from workspace configuration; Supervised workspaces default to shallow limits such as 2, Autonomous may allow deeper) with the hard safety-net cap of 10 retained; the verification endpoint no longer hardcodes max_depth = 10 (provenance.rs:182 'Will be configurable per workspace in future').

## Acceptance Criteria

- [ ] An agent principal with the Admin role cannot create/update/delete trust anchors (403 from the immutable Deny policy; an Allow policy does not override); a human admin still can.
- [ ] A chain node whose key binding references an unknown or removed trust anchor fails verification with a named reason; a binding with an invalid countersignature fails; a binding whose user document signature does not verify over its identity fields fails (identity is user-attested).
- [ ] A DerivedInput whose precondition evaluates false (or errors) against the parent state rejects; one whose update expression yields content whose hash differs from the claimed content rejects.
- [ ] load_chain walks a production-created chain (spawn-derived nodes) root→leaf: each parent_ref equals the parent's stored attestation id.
- [ ] A spawn-created derived node's id is the SHA-256 of its canonical content: re-hashing the node's content yields the stored id, and two identical derivations produce the same id (recomputable by an offline verifier, unlike uuid::Uuid::new_v4()).
- [ ] Derived-node key bindings carry task_id/spawned_by/stack_hash/persona, and a mismatch with the acting agent or attestation metadata rejects at verification.
- [ ] A workspace-configured max chain depth N rejects chains deeper than N; the hard cap 10 still applies; the verification endpoint uses the configured limit.
- [ ] TrustAnchor.constraints appear in the evaluated constraint set (a violating anchor constraint is reported and, on the enforcing path, rejects).

## Verification intent

At the base: (a) an agent principal granted UserRole::Admin can PUT /api/v1/tenants/:id/trust-anchors/:aid successfully (protection is handler RBAC only — trust_anchors.rs require_admin — the specced immutable Deny policy does not exist); (b) verify a chain whose leaf key binding has trust_anchor_id 'nonexistent-anchor' — verification accepts it (no anchor resolution exists in any verification call path); (c) craft a DerivedInput with precondition 'false' — verification ignores preconditions entirely (spawn creates them empty and no verifier executes them; git_http.rs checks only 'has_parent_ref && has_parent_at_lower_depth'); (d) load_chain on a spawn-created leaf returns only the leaf (parent_ref is SHA-256 of the parent attestation JSON, never equal to the parent's stored id); (e) a spawn-created node's id is a random UUID (spawn.rs:4470) — re-derive the node's content and hash it: the result does not match the stored id and two identical derivations get different ids, so no offline verifier can validate ids; (f) provenance.rs:182 hardcodes 'let max_depth = 10u32; // Will be configurable per workspace in future'; (g) no verification path checks a user document signature over binding identity fields (the only user signature ever checked is the SignedInput content signature). Each probe must fail after the task (403, named rejection, named rejection, full root→leaf traversal, recomputable content-addressed id, configured N enforced, named binding-signature rejection).

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/authorization-provenance.md`. Source: `973068a86ab3154b6cba448fda9369125326cab7`; plan: `fdf43e24079d62bc303fa18c819671b940fa5414c868ffdc489bfd20a3d957c0`.

[
  {
    "id": "80fa5d551228a894b5a85132",
    "decision": "gap",
    "requirement_quote": "The platform cannot add, remove, or modify trust anchors without a human admin action — this is enforced by an immutable Deny ABAC policy (same pattern as `builtin:require-human-spec-approval`).",
    "detail": "TrustAnchor types, a tenant-scoped CRUD repository (SQLite + mem), and /api/v1/tenants/:id/trust-anchors endpoints exist, and modification is gated on the Admin role plus a tenant match. However the spec's enforcement mechanism — an immutable Deny ABAC policy that (like builtin:require-human-spec-approval) denies non-human subjects — is not implemented: protection is an RBAC role check in the handler, so any principal holding the Admin role (including an agent principal) can add, remove or rotate anchors, and no builtin policy guards the trust-anchor resource. Additionally, anchor-level `constraints` (§3.2 reference in the TrustAnchor struct) are stored and returned by the CRUD API but never evaluated: derive_strategy_constraints/collect_all_constraints take only InputContent, workspace trust level and gate constraints — no code path loads a TrustAnchor's constraints into the evaluated constraint set. 'A workspace inherits its tenant's anchors' is only exercised by the §6.3 bundle export (list_by_tenant via the workspace's tenant); verification never resolves key_binding.trust_anchor_id against the anchor registry.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/trust_anchors.rs",
        "quote": "\"trust anchor management requires Admin role\".to_string(),",
        "detail": "All five trust-anchor endpoints gate on require_admin (UserRole::Admin) plus a tenant_id equality check. This is handler-level RBAC, not the specced immutable Deny ABAC policy; grep of m34_builtin_policies and the policy engine shows no builtin policy for trust-anchor resources."
      },
      {
        "file": "crates/gyre-domain/src/constraint_evaluator.rs",
        "quote": "pub fn collect_all_constraints(",
        "detail": "The evaluated constraint set is the union of explicit, strategy-implied and gate constraints only. TrustAnchor.constraints are accepted and stored by the CRUD API but no production call path adds them to the evaluated set — the anchor-level constraints of §1.1/§3.2 are dead data."
      },
      {
        "file": "crates/gyre-adapters/src/sqlite/trust_anchor.rs",
        "quote": "impl TrustAnchorRepository for SqliteStorage {",
        "detail": "Real tenant-scoped SQLite persistence for trust anchors exists (create/find/list/update/delete), so the registry itself is real storage, not a stub — the gap is the enforcement mechanism and the unevaluated anchor constraints."
      }
    ]
  },
  {
    "id": "74593d79542d482cc5567984",
    "decision": "gap",
    "requirement_quote": "// 4. Evaluate preconditions against parent's verified state",
    "detail": "The implemented chain verification covers part of the algorithm: per-node Ed25519 signature verification over the signed/derivation content hash, key-binding expiry, valid_until, depth limit, presence of a root SignedInput, and structural parent linkage (non-empty parent_ref plus a lower-depth node in the chain). Missing steps: (2) verify_key_binding against the trust anchor — the key binding's trust_anchor_id/issuer are never resolved against the TrustAnchor registry or JWKS, and the platform countersignature is never verified; (4) preconditions are never evaluated against the parent's state (spawn creates them empty and no verifier executes them); (5) the update expression is never evaluated; (6) derived content is never recomputed and compared to claimed content. Parent linkage is only structural: verify_chain checks that some lower-depth node exists, explicitly not matching parent_ref content hashes to parent IDs — and the production parent_ref (SHA-256 of the parent attestation JSON) does not equal the parent's stored id (uuid for derived nodes, SHA-256-of-SignedInput for roots), so load_chain's id-based walk cannot traverse production-created links at all.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "let linkage_valid = has_parent_ref && has_parent_at_lower_depth;",
        "detail": "Linkage verification is structural only — the adjacent comment states 'we verify structural consistency (non-empty, depth ordering) rather than trying to match content hashes to IDs'; steps 4–6 of the specced algorithm (preconditions, update expression, derived-content hash match) have no implementation anywhere in the verifier."
      },
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "UnparsedPublicKey::new(&signature::ED25519, &signed.key_binding.public_key);",
        "detail": "Signature verification uses the key embedded in the attestation; step 2 (verify_key_binding — resolving trust_anchor_id against the trust-anchor registry / JWKS, checking the countersignature) is absent from all inspected verification call paths."
      },
      {
        "file": "crates/gyre-server/src/api/spawn.rs",
        "quote": "preconditions: vec![],",
        "detail": "Production derived inputs carry no preconditions and no verifier evaluates the preconditions or update fields, so those CEL programs are dead spec surface."
      }
    ]
  },
  {
    "id": "7910f63c3be5b0641f51c47b",
    "decision": "gap",
    "requirement_quote": "The `spawned_by` field is critical — it links the orchestrator's key binding back to the human who approved the original spec, even though the orchestrator (not the human) is the immediate signer.",
    "detail": "Orchestrators do sign derived inputs with a workload-bound key (spawn stores a per-agent Ed25519 keypair and a KeyBinding with user_identity 'agent:<id>', issuer = platform base URL, trust_anchor_id 'gyre-oidc', expiry tied to the agent JWT TTL). But the KeyBinding type has no workload claims: no task_id, spawned_by, stack_hash or persona fields exist on it (the spec's OrchestratorKeyBinding extension), so the derivation signature does not cryptographically bind the orchestrator's task, spawning human, stack or persona. Those attributes live only in the unverified AttestationMetadata/AgentContext. The spawner's KeyBinding is also frequently synthesized at spawn time from the spawner's public key with empty signature/countersignature fields when no stored binding exists.",
    "evidence": [
      {
        "file": "crates/gyre-common/src/key_binding.rs",
        "quote": "pub struct KeyBinding {",
        "detail": "The KeyBinding struct carries only public_key, user_identity, issuer, trust_anchor_id, issued_at, expires_at, user_signature, platform_countersign — none of the specced workload claims (task_id, spawned_by, stack_hash, persona) exist on the type."
      },
      {
        "file": "crates/gyre-server/src/api/spawn.rs",
        "quote": "user_signature: vec![],       // workload-bound — no user signature",
        "detail": "The child agent's workload KeyBinding (and the synthesized root-agent fallback binding) are constructed with empty signature and countersignature fields — the binding attests nothing beyond key possession."
      }
    ]
  },
  {
    "id": "827a7cc2ad6eaf0f8e262900",
    "decision": "gap",
    "requirement_quote": "The strategy-implied constraint `chain.depth <= N` is derived from workspace configuration.",
    "detail": "The hard safety-net limit is implemented: verify_chain clamps max_depth to 10, per-node verification rejects depth > 10, and spawn refuses to create derived inputs at depth >= 10. But no workspace-configuration-derived depth limit exists: the verification endpoint hardcodes 'let max_depth = 10u32; // Will be configurable per workspace in future', no strategy-implied chain.depth CEL constraint is derived anywhere in derive_strategy_constraints, and workspace trust level (Supervised vs Autonomous) does not influence chain depth.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "let effective_max = max_depth.min(10);",
        "detail": "The hard limit of 10 is enforced regardless of the configurable parameter (the safety net of the third bullet)."
      },
      {
        "file": "crates/gyre-server/src/api/provenance.rs",
        "quote": "let max_depth = 10u32; // Will be configurable per workspace in future",
        "detail": "The workspace-configurable depth limit and the trust-level-derived chain.depth constraint are explicitly deferred — no production code derives a per-workspace N or a chain.depth strategy constraint."
      }
    ]
  },
  {
    "id": "7298cfa0831c074e5e21aa9e",
    "decision": "gap",
    "requirement_quote": "  id:               string          -- content-addressable hash of the attestation",
    "detail": "The Attestation record structure is real and produced in production: input (tagged Signed/Derived), AttestationOutput (content_hash/commit_sha/agent_signature/gate_results), AttestationMetadata (created_at/workspace_id/repo_id/task_id/agent_id/chain_depth) all exist as production types persisted through the chain store, and GateAttestations are genuinely produced and signed at gate execution (signed over canonical signable_bytes with the platform key, appended to the chain leaf, re-persisted, written as a git note). agent_signature is optional ('if capable') and always None in production — accounted under §7.4. The gap is the id field's own requirement: 'content-addressable hash of the attestation'. Approval-time roots satisfy it (SHA-256 over the serialized SignedInput), but spawn-created derived nodes set id to a random UUID (uuid::Uuid::new_v4()) — a random UUID is not content-addressable: identical derivations get different ids, and no verifier can recompute or validate a derived node's id offline. This also breaks chain traversal semantics (§4.4): parent_ref (SHA-256 of the parent attestation) does not equal the parent's stored UUID id, so the id-based load_chain walk cannot traverse production-created links (audited under §4.4). The id-field requirement is split out of the otherwise-satisfied structure as this section's gap.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/spawn.rs",
        "quote": "id: uuid::Uuid::new_v4().to_string(),",
        "detail": "Spawn-created derived attestation nodes use a random UUID as the id — not a content-addressable hash. Identical derivations produce different ids and no offline verifier can recompute the id, contradicting the §5.1 field spec."
      },
      {
        "file": "crates/gyre-server/src/api/specs.rs",
        "quote": "let hash = ring::digest::digest(&ring::digest::SHA256, &att_bytes);",
        "detail": "Approval-time roots ARE content-addressed (SHA-256 over the serialized SignedInput), showing the deviation is specific to spawn-created derived nodes, not a uniform id scheme."
      },
      {
        "file": "crates/gyre-server/src/gate_executor.rs",
        "quote": "let mut gate_attestation = GateAttestation {",
        "detail": "The remainder of §5.1's structure is real production code: per-gate signed GateAttestation records are constructed at gate execution and signed via the shared signable_bytes() helper, so the gap is confined to the id field's content-addressing requirement."
      }
    ]
  }
]
