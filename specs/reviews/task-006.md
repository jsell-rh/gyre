# Review: TASK-006 — Authorization Provenance Phase 1

**Reviewer:** Verifier  
**Commit:** `aa3e6fbe`  
**Round:** R1  
**Date:** 2026-04-08

## Findings

- [ ] **F1 — Trust anchor endpoints lack tenant isolation (cross-tenant authorization bypass)**
  `crates/gyre-server/src/api/trust_anchors.rs` lines 76–205: All five handlers (`list`, `create`, `get`, `update`, `delete`) accept `tenant_id` from the URL path parameter but never verify it matches `auth.tenant_id`. The `require_admin()` helper (line 209) only checks the caller's role, not their tenant scope. Any Admin-role user can manage trust anchors for any tenant by changing the path parameter. The spec §1.1 states trust anchors are "tenant-scoped" — cross-tenant access violates this scoping. Compare with `key_binding.rs` which correctly uses `auth.tenant_id` directly (line 128). **Fix:** Add `if auth.tenant_id != tenant_id { return Err(ApiError::Forbidden(...)) }` in each handler, or use `auth.tenant_id` in place of the path parameter.

- [ ] **F2 — Key binding endpoint does not verify `user_signature` against `public_key`**
  `crates/gyre-server/src/api/key_binding.rs` lines 68–72: The handler decodes `user_signature` from base64 but never performs Ed25519 signature verification against the submitted `public_key`. The spec §2.3 step 3–4 states: "Client constructs the `KeyBinding` document, signs it with the private key. Platform verifies the user's IdP session is valid, countersigns the binding." Without signature verification, any authenticated caller can submit an arbitrary public key and claim to own it. The `user_signature` bytes are decoded and stored but never cryptographically verified. This is not a Phase 1 deferral — Phase 1 defers *push-time constraint enforcement*, not *key binding integrity*. A binding created without signature verification can produce `SignedInput` attestations attributable to a key the caller doesn't control.
