# TASK-006: Authorization Provenance — Phase 1: Key Binding and Signed Input (non-enforcing)

**Spec reference:** `authorization-provenance.md` §2, §7.1, §8 Phase 1  
**Depends on:** TASK-003, TASK-004, TASK-005  
**Progress:** `not-started`

## Spec Excerpt

Phase 1 introduces key binding and signed input production in audit-only mode:
- Implement `KeyBinding` exchange on spec approval
- Produce `SignedInput` on every spec approval
- Store attestation chains alongside existing `AttestationBundle`
- Verification runs in audit-only mode: log results, do not reject
- No changes to agent spawn or push flow

## Implementation Plan

1. **Key Binding endpoint** — `POST /api/v1/auth/key-binding`:
   - Client sends ephemeral Ed25519 public key + IdP session proof
   - Server verifies IdP session, countersigns binding, stores public key
   - Returns the `KeyBinding` document
   - ABAC: per-handler auth (user-scoped)

2. **Extend spec approval** — Amend `POST /api/v1/specs/approve`:
   - Accept optional `output_constraints` and `scope` fields in request body
   - Require valid `KeyBinding` (look up by user_id)
   - Create `SignedInput` with signed content hash
   - Store in `ChainAttestationRepository` alongside existing `SpecApproval`

3. **Trust Anchor CRUD** — `GET/POST/PUT/DELETE /api/v1/tenants/:id/trust-anchors`:
   - Tenant-scoped trust anchor management
   - ABAC: `resource_type: "attestation"`, Admin role required

4. **Audit-only verification** — In the push handler, run attestation chain verification but only log results (do not reject pushes that fail verification)

5. Wire new repositories into `AppState`

## Acceptance Criteria

- [ ] Key binding endpoint creates and stores ephemeral key pairs
- [ ] Spec approval produces `SignedInput` when `KeyBinding` is available
- [ ] Spec approval works without `KeyBinding` (graceful degradation for Phase 1)
- [ ] Trust anchors are manageable via REST API
- [ ] Attestation chains are stored alongside existing `AttestationBundle`
- [ ] Push handler logs verification results without rejecting
- [ ] Integration tests for key binding and signed input creation
- [ ] `cargo test --all` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `authorization-provenance.md` §2, §7.1, §8 Phase 1
3. Read `crates/gyre-server/src/api/specs.rs` for the approval endpoint
4. Read `crates/gyre-server/src/git_http.rs` for the push handler
5. Add audit logging using the existing tracing infrastructure
6. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

_(none yet)_
