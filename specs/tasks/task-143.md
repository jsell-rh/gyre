---
title: "Implement attestation bundle verification protocol"
spec_ref: "agent-gates.md §Part 3 Verification"
depends_on:
  - task-142
progress: ready-for-review
coverage_sections:
  - "agent-gates.md §Verification"
commits: ["c880471aebf2b922d0aa3e48187e8b9aa07e4b3c"]
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

- **Port** (`crates/gyre-ports/src/attestation_verifier.rs`): `AttestationVerifier` trait — `verify_bundle` (full protocol → `VerificationResult`), `verify_gate_signature`, `verify_spec_approval` — the seam where external/OIDC key resolution plugs in for federation verification without changing the bundle format.
- **Domain types** (`crates/gyre-domain/src/attestation_verification.rs`): `VerificationResult` (`valid`, `bundle_signature_valid`, `gate_results[]`, `spec_approval_valid: Option<bool>`, `consistency_check`, `errors[]`) and `GateVerificationResult`; `check_bundle_consistency` is the pure, I/O-free spec step 5 — non-empty `mr_id`/`merge_commit_sha`, well-formed `path@sha` `spec_ref`, `spec_approval` only with a `spec_ref`, per-gate signature presence, `signed_by` matching the gate type's signing party and the recorded reviewer identity (`agent:<id>`/`reviewer_agent_id`/`reviewer_oidc_sub` binding), author `oidc_sub`/`agent_id` binding; `assemble_verification_result` is the shared scoring rule. `SpecRefAttestation::unsigned_spec_approval_json()` defines the exact bytes an approval signature covers (canonical-form parity with the bundle/gate helpers from task-142).
- **Verifier** (`crates/gyre-server/src/merge_attestation.rs`): `ForgeAttestationVerifier` implements the port with the task-142 key scheme — the forge's real Ed25519 server key (public half at `/.well-known/jwks.json`, the "OIDC issuer" a verifier trusts) verifies bundle signatures and forge-signed gate results; gate-agent signatures verify against the per-agent re-derived keys. The bool `verify_merge_attestation_bundle` is replaced; the single shared `verify_ed25519` primitive keeps sign/verify parity in one function pair. Wired into `AppState.attestation_verifier` (plus the two test AppState constructors in `mem.rs`/`middleware.rs`).
- **REST** (`crates/gyre-server/src/api/attestations.rs`): `POST /api/v1/attestations/verify` — caller-supplied `MergeAttestationBundle`, returns the per-component `VerificationResult`; a failed verification is a 200 with `valid: false` plus findings (only transport/processing failures error; a non-bundle body is 4xx). ABAC: `attestation`/`read` — registered in the resolver (route-registry check green); verification is a pure evaluation of the submitted document, so no platform state is read or mutated.
- **CLI**: `gyre verify-attestation <commit-sha>` — reads the bundle from the merge commit's git note (`--repo`, `--notes-ref refs/notes/merge-attestations`), POSTs it to the verify endpoint, prints the per-component human-readable result, exits 1 on failed verification. Server/token resolve from `--server/--token` or `~/.gyre/config`.
- **Spec approval signatures (honest tri-state)**: absent approval/signature → `spec_approval_valid: None` with an explicit finding ("not cryptographically countersigned") — never silently scored as passed; a present signature is really verified — agent approvers against the same per-agent key scheme the gate signers use, other approver identities reported as not platform-verifiable (Err → `Some(false)` + finding) pending the Sigstore/OIDC issuer integration, the same deferral task-142's reviewed signer design makes.
- **Spec step 4 (Rekor) is out of this task's contract**: the bundle format carries no author commit SHAs to check and the plan's port/trait/result surface (steps 1–3 + 5) is fully implemented; Rekor publication/verification is task-144's storage scope.
- **CI repair** (`scripts/check-inert-enforcement.sh`): fixed a latent `NameError` — `BOUND` was referenced by the METH_CALL branch since the script's introduction but never defined, so any statement-position consumed `.verify_*()` method call crashed the script instead of being evaluated. Defined per the documented intent (`.await` alone still flags as inert); verified against a probe file (inert call flagged, bound call passed).

### Test evidence

Probes run in this sandbox (build wrapper permits check-based and small-package probes; gyre-server test compilation is refused in-sandbox, below):

- `cargo test -p gyre-domain` — **549 passed, 0 failed** (includes 10 new `attestation_verification` tests: consistent bundle, empty mr_id, reviewer/signer mismatch, command-gate-signed-by-agent, missing gate signature, spec_approval without spec_ref, malformed spec_ref, author oidc_sub mismatch, wrong version, result assembly scoring).
- `cargo test -p gyre-cli` — **221 passed, 0 failed** (includes 4 new: command parsing, client POSTs the note bundle to `/api/v1/attestations/verify` and parses the per-component result via a mock HTTP server, human-readable rendering names failing components, and reading a bundle from a real git note in a temp repo incl. missing-note error).
- `cargo check -p gyre-ports` — clean; `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` — 0 errors; clippy clean on all changed lines.
- `cargo fmt --check` — clean. Architecture, ABAC route-registry, api-auth, inert-enforcement (now fixed), and the invariant check scripts — all green.

Server tests the independent verifier must execute (build wrapper refuses gyre-server test compilation here — 4 GiB budget, recorded in `/tmp/stage/review-evidence/task-143-evidence.md`):

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib merge_attestation` — full-protocol verification on a generated bundle; tampered gate status (that gate's result invalid, others valid); tampered `merged_at` (bundle signature invalid); swapped cross-party gate signature; missing bundle/gate signatures; reviewer-identity consistency failure; spec-approval signature round trip (agent-approver sign → `Some(true)`, tampered → `Some(false)`, user-approver opaque signature → `Some(false)` + not-verifiable finding); unsigned-approval finding; agent-less-MR bundle still valid.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib api::attestations` — endpoint tests: valid bundle → 200/valid; tampered → 200 with `valid: false`; missing bundle signature reported; non-bundle body rejected.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib merge_processor_generates_multi_party_attestation_bundle` — end-to-end: bundle produced and stored by the merge processor verifies through the full protocol.
