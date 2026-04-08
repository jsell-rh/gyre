# TASK-008: Authorization Provenance — Phase 3: Enforcement

**Spec reference:** `authorization-provenance.md` §6, §7.2, §8 Phase 3  
**Depends on:** TASK-007  
**Progress:** `ready-for-review`  
**Review:** [specs/reviews/task-008.md](../reviews/task-008.md)  
**Note:** R6 — F6 resolved: extracted `GateAttestation::signable_bytes()` shared helper in `gyre-common`, used by both `gate_executor` (sign) and `verify_output_signatures` (verify) to ensure sign/verify message parity by construction. Round-trip sign-then-verify test added.

## Spec Excerpt

Phase 3 enables enforcement:
- Enable `builtin:require-signed-authorization` ABAC policy
- Push and merge are rejected if attestation chain is invalid or constraints fail
- Derived inputs produced for orchestrator → agent delegation
- Legacy `AttestationBundle` still produced for backward compatibility

## Implementation Plan

1. **Add `builtin:require-signed-authorization`** — immutable Deny policy (priority 998):
   - Deny push/merge for non-system subjects unless valid attestation chain exists
   - Seed at server startup alongside `builtin:require-human-spec-approval`

2. **Push-time enforcement** — Change push handler from audit-only to enforcing:
   - Reject pushes with invalid/missing attestation chains
   - Return clear error message with failing constraint details
   - Agent receives the rejection as a `PushRejected` Event-tier message

3. **Merge-time enforcement** — Change merge processor from audit-only to enforcing:
   - Block merge if any constraint fails
   - Surface in Inbox as priority-3 notification with constraint failure details

4. **Derived input on agent spawn** — Extend spawn handler (§7.4):
   - Create workload `KeyBinding` for new agents
   - Create `DerivedInput` from parent task's attestation
   - Inject into agent environment

5. **Verification endpoint** — `GET /api/v1/repos/{id}/attestations/{commit_sha}/verification`:
   - Returns the full `VerificationResult` tree
   - ABAC: `resource_type: "attestation"`, read action

6. **Export endpoint** — `GET /api/v1/repos/{id}/attestations/{commit_sha}/bundle`:
   - Returns `VerificationBundle` for offline verification
   - ABAC: `resource_type: "attestation"`, export action

## Acceptance Criteria

- [ ] `builtin:require-signed-authorization` policy seeded at startup
- [ ] Pushes without valid attestation chains are rejected
- [ ] Merges without valid attestation chains are blocked
- [ ] Agent spawn produces `DerivedInput` and `KeyBinding`
- [ ] Verification endpoint returns complete `VerificationResult` tree
- [ ] Export endpoint returns `VerificationBundle`
- [ ] Legacy `AttestationBundle` still produced alongside chain attestation
- [ ] Integration tests for enforcement at push and merge time
- [ ] `cargo test --all` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `authorization-provenance.md` §6, §7.2, §7.4, §8 Phase 3
3. Read `crates/gyre-server/src/api/spawn.rs` for agent spawn
4. Read `crates/gyre-server/src/policy_engine.rs` for built-in policies
5. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- feat(provenance): implement authorization provenance Phase 3 — enforcement (TASK-008)
