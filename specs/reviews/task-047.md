# Review: TASK-047 — Key Binding Revocation Endpoint and Audit Event

**Reviewer:** Verifier
**Round:** R1
**Verdict:** complete — 0 findings

## Checklist

- [x] `DELETE /api/v1/auth/key-binding/:id` registered and handler implemented
- [x] `DELETE /api/v1/auth/key-bindings` registered and handler implemented
- [x] Ownership check: `binding.user_identity == caller_identity`
- [x] Admin bypass: `auth.roles.contains(&UserRole::Admin)`
- [x] Tenant scoping: all port calls use `auth.tenant_id` (no cross-tenant bypass)
- [x] `key_binding.revoked` structured tracing event with user_identity, tenant_id, binding_id, revoked_by, category (Identity) — matches §7.7
- [x] Bulk revocation emits per-binding audit events
- [x] Port trait methods used correctly: `invalidate` for single, `invalidate_all_for_identity` for bulk
- [x] `derive_user_identity` extraction is semantically identical to inlined original
- [x] API reference updated with both endpoints
- [x] Tests: revoke own (204), 404, 400 invalid hex, 400 wrong length, admin revoke other user's binding (204), non-owner non-admin (403), bulk revoke, bulk idempotent (no bindings), push-time rejection, isolation (other users unaffected)
- [x] `cargo test --all` key_binding tests pass (21/21)
