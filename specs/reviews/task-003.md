# Review: TASK-003 — Authorization Provenance Domain Types and Port Traits

**Reviewer:** Verifier  
**Date:** 2026-04-07 (R2)  
**Verdict:** needs-revision

---

## R1 Findings

- [-] [process-revision-complete] **F1 — `persona_constraints` uses `Vec<String>` but spec defines `PersonaRef[]` (a structured type).**  
  The spec (authorization-provenance.md §2.2, line 107) defines `persona_constraints: PersonaRef[]`. The CEL evaluation context (§3.3, line 263) shows persona_constraints as objects: `"persona_constraints": [{"name": "security"}]`. The strategy-implied constraint derivation (§3.2, line 206) evaluates `agent.persona == input.persona_constraints[0].name` — which requires `.name` field access on each element. The implementation (attestation.rs:68-69) uses `Vec<String>`, which serializes as `["security"]` — a flat string array, not an array of objects. Downstream consequences: TASK-005 (CEL evaluator) will be unable to evaluate `input.persona_constraints[0].name` because a string has no `.name` property. TASK-007 (strategy-implied constraints) will produce incorrect CEL expressions. Fix: define a `PersonaRef` struct with at minimum `name: String`, and change `persona_constraints` to `Vec<PersonaRef>`.  
  **Files:** `crates/gyre-common/src/attestation.rs:68-69` (type definition), spec §2.2 line 107 and §3.3 line 263 (required structure).  
  **Process fixes:** Added checklist item #23 (spec-type fidelity — no simplification drift) to implementation prompt. Addresses array element simplification (`Vec<String>` where spec defines structured type array) with specific enumeration procedure and downstream impact check.

- [-] [process-revision-complete] **F2 — `key_binding` field uses `KeyBindingRef` (2 fields) instead of spec's `KeyBinding` (8 fields) in SignedInput, DerivedInput, and GateAttestation.**  
  The spec defines `key_binding: KeyBinding` on `SignedInput` (§2.2), `DerivedInput` (§4.1), and `GateAttestation` (§5.1), where `KeyBinding` is the full 8-field struct (public_key, user_identity, issuer, trust_anchor_id, issued_at, expires_at, user_signature, platform_countersign — §2.3, lines 129-138). The implementation uses `KeyBindingRef` (attestation.rs:99-105), a 2-field struct (public_key, trust_anchor_id). The spec's verification algorithm (§4.4, line 375) calls `verify_key_binding(attestation.input.key_binding)` — which requires the full `KeyBinding` to verify user_identity, issuer, timestamps, user_signature, and platform_countersign. With only `KeyBindingRef`, none of this is verifiable from the attestation alone. The offline verification bundle (§6.3, lines 631-639) is designed to be self-contained — but with `KeyBindingRef`, the verifier must perform a separate lookup to the `KeyBindingRepository` to reconstruct the full binding, defeating the self-containment property. Fix: embed the full `KeyBinding` struct (from `key_binding.rs`) instead of `KeyBindingRef` in `SignedInput`, `DerivedInput`, and `GateAttestation`. `KeyBindingRef` can be retained as a lookup key type for the `KeyBindingRepository`, but the attestation chain must carry the full binding for verification independence.  
  **Files:** `crates/gyre-common/src/attestation.rs:92,157,223` (`key_binding: KeyBindingRef` in three structs), `crates/gyre-common/src/key_binding.rs:18-37` (full `KeyBinding` struct), spec §2.2-2.3 lines 93-149, §4.1 line 335, §5.1 line 481.  
  **Process fixes:** Same as F1 — checklist item #23 addresses reference type vs full embedded type with specific guidance on checking spec's verification algorithms and offline verification sections for self-containment requirements.

## R2 Findings

- [x] **F1 (resolved R2).** `PersonaRef` struct now exists and `persona_constraints` uses `Vec<PersonaRef>`. Serializes as `[{"name": "security"}]` per spec §3.3. Test `persona_constraints_serialize_as_object_array` confirms. Fixed in commit `c917b5b0`.

- [x] **F2 (resolved R2).** `key_binding` now embeds full `KeyBinding` (8 fields) instead of `KeyBindingRef` in `SignedInput`, `DerivedInput`, and `GateAttestation`. Doc comments explain the self-containment rationale. Fixed in commit `c917b5b0`.

- [ ] **F3 — `GateAttestation.gate_type` and `GateAttestation.status` use `String` but spec defines `GateType` and `GateStatus` (existing enum types).**  
  The spec (authorization-provenance.md §5.1) defines `GateAttestation` with `gate_type: GateType` and `status: GateStatus`. Both `GateType` and `GateStatus` exist as proper Rust enums in `gyre-domain/src/quality_gate.rs` — `GateType` has variants `TestCommand`, `LintCommand`, `RequiredApprovals`, `AgentReview`, `AgentValidation`; `GateStatus` has `Pending`, `Running`, `Passed`, `Failed`. Both derive `Serialize, Deserialize` with `#[serde(rename_all = "snake_case")]`. The implementation (attestation.rs:215-216) uses `pub gate_type: String` and `pub status: String`. This is a spec-type simplification drift — there is no compile-time or parse-time validation that `gate_type` contains a valid gate type value. Code that constructs a `GateAttestation` with `gate_type: "agent_review"` and code that checks for `"AgentReview"` will silently mismatch. The enums currently live in `gyre-domain`, which `gyre-common` cannot import (hexagonal boundary). The fix is to move `GateType` and `GateStatus` to `gyre-common` (they are pure value enums with no domain logic — same pattern as `MessageKind`, `NodeType`, `NotificationType`, etc. which are already in gyre-common) and update `gyre-domain/src/quality_gate.rs` to re-export from gyre-common.  
  **Files:** `crates/gyre-common/src/attestation.rs:215-216` (`String` types), `crates/gyre-domain/src/quality_gate.rs:35-45,55-60` (existing enum definitions).
