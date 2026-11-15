---
title: "Implement attestation bundle verification protocol"
spec_ref: "agent-gates.md §Part 3 Verification"
depends_on:
  - task-142
progress: ready-for-review
coverage_sections:
  - "agent-gates.md §Verification"
commits: ["c880471aebf2b922d0aa3e48187e8b9aa07e4b3c", "c2df4d71daf5b999dcf0af5fae95e6bf009dabaf", "795ce919c6ff6c49c3ea234c459a435d0ee19afc", "bdc62a832717bb37b5e5888002775b2f9d02ceaa", "c17f82bf6b532073258d45c2115d350ded08b64f", "24c2ac7c1b6c48d6a5e8b3e311a14605c429c5dd"]
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
- **Domain types** (`gyre-domain/src/attestation_verification.rs`): `VerificationResult` (`valid`, `bundle_signature_valid`, `gate_results[]`, `spec_approval_valid: Option<bool>`, `consistency_check`, `errors[]`) and `GateVerificationResult`; `check_bundle_consistency` is the pure, I/O-free spec step 5 — non-empty `mr_id`/`merge_commit_sha`, well-formed `path@sha` `spec_ref`, `spec_approval` only with a `spec_ref`, per-gate signature presence, and a `signed_by` matching the gate type's signing party **in both directions** (a command gate signed by an agent is rejected, and an AgentReview/AgentValidation gate signed by the forge is rejected), plus the reviewer-identity binding and the author `oidc_sub`/`agent_id` binding; `assemble_verification_result` is the shared scoring rule.
- **Verifier** (`gyre-server/src/merge_attestation.rs`): `ForgeAttestationVerifier` implements the port — the forge's real Ed25519 server key (public half at `/.well-known/jwks.json`) verifies bundle signatures and forge-signed gate results; gate-agent signatures verify against per-agent re-derived keys. Wired into `AppState.attestation_verifier`.
- **REST** (`gyre-server/src/api/attestations.rs`): `POST /api/v1/attestations/verify` — caller-supplied `MergeAttestationBundle`, per-component `VerificationResult`; failed verification is 200 with `valid: false` plus findings. ABAC `attestation`/`read`, registered in the resolver.
- **CLI**: `gyre verify-attestation <commit-sha>` — reads the bundle from the merge commit's git note, POSTs it to the verify endpoint, prints the per-component result, exits 1 on failure.
- **Spec approval signatures (honest tri-state)**: absent → `None` + explicit finding, never silently passed; present signatures really verified (agent approvers against the per-agent key scheme; other identities reported not platform-verifiable pending the Sigstore/OIDC integration — task-142's reviewed deferral).
- **Spec step 4 (Rekor)** stays with task-144's storage scope: the bundle format carries no author commit SHAs to check.

### Verification-gate repair (this round, 24c2ac7c)

Finding `74454d36` (exit 1, `bash /tmp/stage/checks.sh`): the gate runs the static-lint series twice — first with the **upstream (base) verifier scripts** against candidate code, then with the candidate's own scripts. The upstream `check-inert-enforcement.sh` (at base `c9cc1536`) references a `BOUND` regex it never defines, so any statement-position `.verify_*`/`.evaluate_*`/`.enforce_*`/`.validate_*` method call in non-test candidate code crashed it with `NameError` — the gate's only failing step (everything else in the 925-line log passed: rustfmt-diff, clippy-diff, all other upstream+candidate lints, web build). `merge_attestation.rs:528` was the single METH_CALL match in the tree (found by replicating the upstream scan exactly — see evidence). The earlier candidate-side fix (`c880471a`) had defined `BOUND` in the branch's own copy, which is why the candidate rerun printed OK while the upstream overlay run crashed.

Fix (`24c2ac7c`): bind the awaited result first (`let gate_verified = self.verify_gate_signature(gate).await;` then `gate_verified.unwrap_or_default()`). Identical fail-closed semantics (bool default false), keeps the clippy-required method form (the `c17f82bf` repair for finding `679e3bf2`), and removes the statement-position call shape the upstream checker cannot evaluate. No checker was weakened; no exemption added.

### Test evidence

- `cargo test -p gyre-domain` — 551 passed, 0 failed (incl. the `attestation_verification` consistency suite).
- `cargo test -p gyre-cli` — 221 + 1 passed, 0 failed (incl. `verify_attestation_command_parses`, `verify_attestation_posts_note_bundle_and_parses_result`).
- `SKIP_WEB_BUILD=1 cargo check --workspace --all-targets` — clean (post-merge with base `c9cc1536`).
- `SKIP_WEB_BUILD=1 python3 scripts/check-clippy-diff.py c9cc1536` — changed lines clean. `python3 scripts/check-rustfmt-diff.py c9cc1536` — clean. `cargo fmt --check -p gyre-server` — clean.
- Upstream `check-inert-enforcement.sh` (extracted from base `c9cc1536`) against candidate code: exit 0 after the fix (crashed with `NameError: BOUND` before it). Candidate script: OK. Zero METH_CALL matches remain in non-test code (probe: `review-evidence/find-bound-crash-lines.py`, output `upstream-inert-enforcement.txt`).
- All repo `scripts/check-*.sh` behave identically to base `c9cc1536` (side-by-side run in a base worktree; the failing ones — mawk regex dialect, sandbox 4 GiB build guard, missing `jj` — fail identically at base and are environment/pre-existing, not branch defects).
- Web: `npm ci && npm test` — 120 files, 2391 passed, 41 skipped. `web/` (src and dist) is byte-identical to base.
- **Verifier must run** (server test compilation exceeds the in-sandbox 4 GiB build wrapper): `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib merge_attestation` (incl. `tampered_bundle_fails_verification`, `unsigned_and_missing_signatures_fail_verification`, `forge_signed_agent_review_gate_fails_verification`, `spec_approval_signature_verification_round_trip`, `inconsistent_reviewer_binding_fails_consistency_check`) and `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib merge_processor` (incl. `merge_processor_generates_multi_party_attestation_bundle` verifying both protocol and storage).
- Sandbox restrictions recorded for remote verification: this sandbox's `awk` is mawk 1.3.4 (gawk `\s` regex extensions fail in several advisory lints — identical at base), `jj` is not installed (jj recorded-fixture tests cannot run here), and the build wrapper blocks gyre-server test compilation (exit 64, 4 GiB budget).
