# Review: TASK-006 — Authorization Provenance Phase 1

**Reviewer:** Verifier  
**Commit:** `aa3e6fbe`, `28e517d7`, `d71a15fe`, `314c604b`  
**Round:** R5  
**Date:** 2026-04-08

## Findings

- [x] [process-revision-complete] **F1 — Trust anchor endpoints lack tenant isolation (cross-tenant authorization bypass)** — RESOLVED in `d71a15fe`. All five handlers now verify `auth.tenant_id != tenant_id` and return 403 Forbidden on mismatch. Test `trust_anchors_cross_tenant_returns_403` verifies the fix.

- [x] [process-revision-complete] **F2 — Key binding endpoint does not verify `user_signature` against `public_key`** — RESOLVED in `d71a15fe`. Handler now performs Ed25519 proof-of-possession verification via `ring::signature::UnparsedPublicKey::verify()` (key_binding.rs lines 75–87). Tests `create_key_binding_invalid_signature_rejected` and `create_key_binding_mismatched_key_signature_rejected` cover both garbage-signature and wrong-key-pair cases.

- [x] [process-revision-complete] **F3 — `SignedInput.signature` is platform-signed instead of user-signed** — RESOLVED in `d71a15fe`. Approval now accepts optional `user_content_signature` in the request body, verifies it against `key_binding.public_key` over the InputContent hash (specs.rs lines 559–574), and stores the user's signature bytes as `SignedInput.signature`. Without `user_content_signature`, no SignedInput is created (Phase 1 graceful degradation). Test `approve_spec_creates_signed_input_when_key_binding_exists` performs end-to-end crypto round-trip verification. Test `approve_spec_with_key_binding_but_no_user_signature_skips_signed_input` verifies graceful degradation.

- [x] [process-revision-complete] **F4 — Push-time audit-only verification does not verify `SignedInput.signature` cryptographically** — RESOLVED in `314c604b`. `verify_attestation_audit_only` now performs Ed25519 signature verification via `ring::signature::UnparsedPublicKey::verify()` over `SHA256(serde_json::to_vec(&signed.content))` against `signed.key_binding.public_key` (git_http.rs lines 1692–1715). Tests `verify_attestation_audit_only_valid_signed_input` (real Ed25519 roundtrip) and `verify_attestation_audit_only_forged_signature` (64-byte forged signature correctly rejected) verify the fix. Tests `verify_attestation_audit_only_expired_key_binding` and `verify_attestation_audit_only_excessive_chain_depth` confirm signature verification succeeds independently of other check failures.

## R5 Verification Summary

All 4 findings from R1–R4 confirmed resolved. 20 TASK-006 tests pass. No new findings.

Verified:
- Key binding endpoint: Ed25519 PoP, base64 decode, 32-byte pubkey validation, TTL cap, tenant-scoped storage
- Spec approval: user-signed InputContent hash, crypto verification against KeyBinding, graceful degradation
- Trust anchors: full CRUD lifecycle, tenant isolation on all 5 handlers, duplicate detection
- Audit-only verification: Ed25519 signature verification, key binding expiry, valid_until, chain depth ≤ 10
- Domain types: PersonaRef (structured, not String), ScopeConstraint, full KeyBinding embedded
- Migration: 3 tables, all spec-required indexes, matching down.sql
- Port traits: match spec §5.4 ChainAttestationRepository, §2.3 KeyBindingRepository, §1.1 TrustAnchorRepository
- SQLite adapters: tenant isolation, expiry filtering, revocation support
