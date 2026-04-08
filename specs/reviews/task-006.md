# Review: TASK-006 — Authorization Provenance Phase 1

**Reviewer:** Verifier  
**Commit:** `aa3e6fbe`, `28e517d7`, `d71a15fe`  
**Round:** R3  
**Date:** 2026-04-08

## Findings

- [x] **F1 — Trust anchor endpoints lack tenant isolation (cross-tenant authorization bypass)** — RESOLVED in `d71a15fe`. All five handlers now verify `auth.tenant_id != tenant_id` and return 403 Forbidden on mismatch. Test `trust_anchors_cross_tenant_returns_403` verifies the fix.

- [x] **F2 — Key binding endpoint does not verify `user_signature` against `public_key`** — RESOLVED in `d71a15fe`. Handler now performs Ed25519 proof-of-possession verification via `ring::signature::UnparsedPublicKey::verify()` (key_binding.rs lines 75–87). Tests `create_key_binding_invalid_signature_rejected` and `create_key_binding_mismatched_key_signature_rejected` cover both garbage-signature and wrong-key-pair cases.

- [x] **F3 — `SignedInput.signature` is platform-signed instead of user-signed** — RESOLVED in `d71a15fe`. Approval now accepts optional `user_content_signature` in the request body, verifies it against `key_binding.public_key` over the InputContent hash (specs.rs lines 559–574), and stores the user's signature bytes as `SignedInput.signature`. Without `user_content_signature`, no SignedInput is created (Phase 1 graceful degradation). Test `approve_spec_creates_signed_input_when_key_binding_exists` performs end-to-end crypto round-trip verification. Test `approve_spec_with_key_binding_but_no_user_signature_skips_signed_input` verifies graceful degradation.

- [ ] **F4 — Push-time audit-only verification does not verify `SignedInput.signature` cryptographically**
  `crates/gyre-server/src/git_http.rs` lines 1664–1758: The `verify_attestation_audit_only` function checks structural properties (non-empty `spec_path`/`spec_sha`, key binding expiry, `valid_until` timestamp, chain depth ≤ 10) but never verifies the Ed25519 signature on the `SignedInput` against the `key_binding.public_key`. The spec §6.1 states push-time verification should confirm "the agent has a valid attestation chain back to a `SignedInput`" and §6.2 defines `verify_chain()` which calls `verify_signed_input()` — this includes cryptographic signature verification (per §4.4 step 1: "Verify this [input's] signature"). The spec §8 Phase 1 says "Verification runs in audit-only mode: log results, do not reject" — this defers *enforcement* (rejection), not *verification completeness*. The current function will report `valid: true` for attestations with forged or arbitrary signatures, making audit logs unreliable. Since the approval endpoint (specs.rs lines 559–574) already correctly verifies user signatures using `ring`, the same `ring::signature::UnparsedPublicKey::verify()` pattern can be reused here. **Fix:** Add a signature verification step in the `AttestationInput::Signed` branch: compute `SHA256(serde_json::to_vec(&signed.content))`, then verify `signed.signature` against `signed.key_binding.public_key` using Ed25519. Report the result as a child `VerificationResult` node. The existing structural checks should remain.
