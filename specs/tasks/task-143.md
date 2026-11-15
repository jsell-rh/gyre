---
title: "Implement attestation bundle verification protocol"
spec_ref: "agent-gates.md §Part 3 Verification"
depends_on:
  - task-142
progress: ready-for-review
coverage_sections:
  - "agent-gates.md §Verification"
commits: ["c880471aebf2b922d0aa3e48187e8b9aa07e4b3c", "c2df4d71daf5b999dcf0af5fae95e6bf009dabaf"]
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

## Shipped

### Behavior

- **Port** (`gyre-ports/src/attestation_verifier.rs`): `AttestationVerifier` trait — `verify_bundle` (full protocol → `VerificationResult`), `verify_gate_signature`, `verify_spec_approval` — the seam where external/OIDC key resolution plugs in for federation verification without changing the bundle format.
- **Domain types** (`gyre-domain/src/attestation_verification.rs`): `VerificationResult` (`valid`, `bundle_signature_valid`, `gate_results[]`, `spec_approval_valid: Option<bool>`, `consistency_check`, `errors[]`) and `GateVerificationResult`; `check_bundle_consistency` is the pure, I/O-free spec step 5 — non-empty `mr_id`/`merge_commit_sha`, well-formed `path@sha` `spec_ref`, `spec_approval` only with a `spec_ref`, per-gate signature presence, and a `signed_by` that matches the gate type's signing party **in both directions** (a command gate signed by an agent is rejected, and an AgentReview/AgentValidation gate signed by the forge is rejected — the multi-party property of agent-gates.md §What Gets Signed and By Whom), plus the reviewer-identity binding (`agent:<id>`/`reviewer_agent_id`/`reviewer_oidc_sub`) and the author `oidc_sub`/`agent_id` binding; `assemble_verification_result` is the shared scoring rule.
- **Verifier** (`gyre-server/src/merge_attestation.rs`): `ForgeAttestationVerifier` implements the port with the task-142 key scheme — the forge's real Ed25519 server key (public half at `/.well-known/jwks.json`) verifies bundle signatures and forge-signed gate results; gate-agent signatures verify against per-agent re-derived keys. Wired into `AppState.attestation_verifier`.
- **REST** (`gyre-server/src/api/attestations.rs`): `POST /api/v1/attestations/verify` — caller-supplied `MergeAttestationBundle`, per-component `VerificationResult`; failed verification is 200 with `valid: false` plus findings. ABAC `attestation`/`read`, registered in the resolver.
- **CLI**: `gyre verify-attestation <commit-sha>` — reads the bundle from the merge commit's git note, POSTs it to the verify endpoint, prints the per-component result, exits 1 on failure.
- **Spec approval signatures (honest tri-state)**: absent → `None` + explicit finding, never silently passed; present signatures really verified (agent approvers against the per-agent key scheme; other identities reported not platform-verifiable pending the Sigstore/OIDC integration — task-142's reviewed deferral).
- **Spec step 4 (Rekor)** stays with task-144's storage scope: the bundle format carries no author commit SHAs to check.
- **Review repair (c2df4d71)**: the consistency check's forge arm previously never checked the gate type, so a forge self-attested AgentReview passed and verified against the forge key (`valid: true`). The forge arm now errors on agent gate types; a domain consistency test and a server-side full-protocol test (forge key genuinely signs the AgentReview gate, bundle re-signed — still `valid: false` on the step-5 rule) pin the fix. Evidence: `/tmp/stage/review-evidence/task-143-forge-signed-agent-gate-fix.txt`.

### Test evidence

- `cargo test -p gyre-domain` — 550 passed, 0 failed (11 `attestation_verification` tests incl. the new forge-signed-agent-gate case).
- `cargo test -p gyre-cli` — 221 passed, 0 failed (command parsing, mock-server verify POST + result parsing, human-readable rendering, git-note reading).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` — 0 errors; fmt clean; no clippy warnings in changed files.
- Verifier must run (wrapper refuses gyre-server test compilation in-sandbox, 4 GiB budget): `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib merge_attestation` (incl. new `forge_signed_agent_review_gate_fails_verification` and the end-to-end merge-processor bundle verification), `--lib api::attestations`.
