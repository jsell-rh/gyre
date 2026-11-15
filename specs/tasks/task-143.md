---
title: "Implement attestation bundle verification protocol"
spec_ref: "agent-gates.md §Part 3 Verification"
depends_on:
  - task-142
progress: not-started
coverage_sections:
  - "agent-gates.md §Verification"
commits: []
---

## Spec Excerpt

From `agent-gates.md` §Part 3 Verification:

Anyone who trusts Gyre's OIDC issuer can verify the entire bundle:

1. Verify bundle_signature against Gyre's OIDC issuer
2. For each gate, verify its signature against the signer's OIDC identity
3. Verify the spec approval signature
4. Verify the author's commit signatures via Rekor transparency log
5. Check that all gate results reference the same MR and spec

This can be done offline, after the fact, by auditors, compliance teams, or other Gyre instances (federation).

## Implementation Plan

1. **Define `AttestationVerifier` port trait** in `gyre-ports`:
   ```rust
   #[async_trait]
   pub trait AttestationVerifier: Send + Sync {
       async fn verify_bundle(&self, bundle: &MergeAttestationBundle) -> Result<VerificationResult>;
       async fn verify_gate_signature(&self, gate: &GateAttestation) -> Result<bool>;
       async fn verify_spec_approval(&self, approval: &SpecRefAttestation) -> Result<bool>;
   }
   ```

2. **Implement `VerificationResult` type** in `gyre-domain`:
   ```rust
   pub struct VerificationResult {
       pub valid: bool,
       pub bundle_signature_valid: bool,
       pub gate_results: Vec<GateVerificationResult>,
       pub spec_approval_valid: Option<bool>,
       pub consistency_check: bool, // all gates reference same MR/spec
       pub errors: Vec<String>,
   }
   ```

3. **Implement verification logic** in `gyre-domain` (pure function):
   - Verify structural consistency: all gate results reference the same `mr_id`
   - Verify spec_ref consistency: all gates that reference a spec use the same SHA
   - Delegate cryptographic verification to the `AttestationVerifier` port

4. **Add REST endpoint** `POST /api/v1/attestations/verify`:
   - Accepts a `MergeAttestationBundle` JSON body
   - Returns `VerificationResult`
   - This enables offline verification by external tools

5. **Add CLI command** `gyre verify-attestation <commit-sha>`:
   - Reads the attestation bundle from the merge commit's git note
   - Runs verification
   - Prints human-readable result

6. **Tests:**
   - Unit test: valid bundle passes verification
   - Unit test: tampered gate signature fails verification
   - Unit test: mismatched MR IDs across gates fails consistency check
   - Unit test: missing bundle_signature fails verification
   - Integration test: end-to-end verify of a bundle produced by the merge processor

## Acceptance Criteria

- [ ] `AttestationVerifier` port trait defined
- [ ] `VerificationResult` type captures per-component verification status
- [ ] Structural consistency checks (same MR/spec across all gates)
- [ ] Cryptographic signature verification for bundle, gates, and spec approval
- [ ] `POST /api/v1/attestations/verify` endpoint
- [ ] CLI `gyre verify-attestation` command
- [ ] Tests pass

## Agent Instructions

- Read task-142 output for the `MergeAttestationBundle` type definition
- Read `crates/gyre-ports/` for port trait conventions
- Read `crates/gyre-cli/src/` for CLI command registration patterns
- Read `docs/api-reference.md` and `crates/gyre-server/src/api/mod.rs` for route registration
- The verification logic should be a pure domain function — no I/O in the consistency checks
- Cryptographic verification delegates to the port trait (infrastructure concern)
