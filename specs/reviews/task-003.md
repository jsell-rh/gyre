# Review: TASK-003 — Authorization Provenance Domain Types and Port Traits

**Reviewer:** Verifier  
**Date:** 2026-04-07 (R1)  
**Verdict:** needs-revision

---

## R1 Findings

- [ ] **F1 — `persona_constraints` uses `Vec<String>` but spec defines `PersonaRef[]` (a structured type).**  
  The spec (authorization-provenance.md §2.2, line 107) defines `persona_constraints: PersonaRef[]`. The CEL evaluation context (§3.3, line 263) shows persona_constraints as objects: `"persona_constraints": [{"name": "security"}]`. The strategy-implied constraint derivation (§3.2, line 206) evaluates `agent.persona == input.persona_constraints[0].name` — which requires `.name` field access on each element. The implementation (attestation.rs:68-69) uses `Vec<String>`, which serializes as `["security"]` — a flat string array, not an array of objects. Downstream consequences: TASK-005 (CEL evaluator) will be unable to evaluate `input.persona_constraints[0].name` because a string has no `.name` property. TASK-007 (strategy-implied constraints) will produce incorrect CEL expressions. Fix: define a `PersonaRef` struct with at minimum `name: String`, and change `persona_constraints` to `Vec<PersonaRef>`.  
  **Files:** `crates/gyre-common/src/attestation.rs:68-69` (type definition), spec §2.2 line 107 and §3.3 line 263 (required structure).

- [ ] **F2 — `key_binding` field uses `KeyBindingRef` (2 fields) instead of spec's `KeyBinding` (8 fields) in SignedInput, DerivedInput, and GateAttestation.**  
  The spec defines `key_binding: KeyBinding` on `SignedInput` (§2.2), `DerivedInput` (§4.1), and `GateAttestation` (§5.1), where `KeyBinding` is the full 8-field struct (public_key, user_identity, issuer, trust_anchor_id, issued_at, expires_at, user_signature, platform_countersign — §2.3, lines 129-138). The implementation uses `KeyBindingRef` (attestation.rs:99-105), a 2-field struct (public_key, trust_anchor_id). The spec's verification algorithm (§4.4, line 375) calls `verify_key_binding(attestation.input.key_binding)` — which requires the full `KeyBinding` to verify user_identity, issuer, timestamps, user_signature, and platform_countersign. With only `KeyBindingRef`, none of this is verifiable from the attestation alone. The offline verification bundle (§6.3, lines 631-639) is designed to be self-contained — but with `KeyBindingRef`, the verifier must perform a separate lookup to the `KeyBindingRepository` to reconstruct the full binding, defeating the self-containment property. Fix: embed the full `KeyBinding` struct (from `key_binding.rs`) instead of `KeyBindingRef` in `SignedInput`, `DerivedInput`, and `GateAttestation`. `KeyBindingRef` can be retained as a lookup key type for the `KeyBindingRepository`, but the attestation chain must carry the full binding for verification independence.  
  **Files:** `crates/gyre-common/src/attestation.rs:92,157,223` (`key_binding: KeyBindingRef` in three structs), `crates/gyre-common/src/key_binding.rs:18-37` (full `KeyBinding` struct), spec §2.2-2.3 lines 93-149, §4.1 line 335, §5.1 line 481.
