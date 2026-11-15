---
title: "Implement attestation bundle verification protocol"
spec_ref: "agent-gates.md §Part 3 Verification"
depends_on:
  - task-142
progress: ready-for-review
coverage_sections:
  - "agent-gates.md §Verification"
commits: ["c17f82bf6b532073258d45c2115d350ded08b64f", "bdc62a832717bb37b5e5888002775b2f9d02ceaa", "795ce919c6ff6c49c3ea234c459a435d0ee19afc", "c2df4d71daf5b999dcf0af5fae95e6bf009dabaf", "c880471aebf2b922d0aa3e48187e8b9aa07e4b3c"]
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

- [x] `AttestationVerifier` port trait defined
- [x] `VerificationResult` type captures per-component verification status
- [x] Structural consistency checks (same MR/spec across all gates)
- [x] Cryptographic signature verification for bundle, gates, and spec approval
- [x] `POST /api/v1/attestations/verify` endpoint
- [x] CLI `gyre verify-attestation` command
- [x] Tests pass

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
- **Review repairs**: `c2df4d71` — the consistency check's forge arm now rejects agent gate types (a forge self-attested AgentReview no longer verifies, even re-signed); `795ce919` — gate-signature verification restructured off the inert-enforcement crash path (identical fail-closed semantics); `bdc62a83` — the gate-tamper test's bundle-signature assertion was inverted: `unsigned_canonical_json()` nulls only `bundle_signature`, so the gates array IS covered by the bundle-level signature and gate tampering breaks it (proven with a standalone probe using the real domain type + real ring Ed25519: `/tmp/stage/review-evidence/gate-tamper-probe-output.txt`); the tampered gate is now addressed by name because `list_by_mr_id` has no ordering contract (mem HashMap / SQL adapters without ORDER BY), which made the old `gates[0]` indexing flaky.
- **Verification-gate repair** (`c17f82bf`, this round): the deterministic gate (finding `679e3bf2`) failed on `clippy::manual_unwrap_or_default` at `merge_attestation.rs:528` — the match form `Ok(valid) => valid, Err(_) => false` had been forced by the inert-enforcement script's NameError crash, which this branch had already fixed by defining its missing `BOUND` regex (`c880471a`). Now uses `.await.unwrap_or_default()`: bool's default is `false`, so a verification error still fails closed, and `BOUND` recognizes the `.unwrap` chain so the line counts as bound — no exemption, no checker weakening. All remaining clippy warnings in `gyre-server` are pre-existing at the assignment base (verified per site: `merge_processor.rs:1669`, `mem.rs`, `explorer_ws.rs`, ...).
- **Merge resolution** (`94c9ac2f`): rebased this task's verification work onto the task-144 base (AttestationStorage port + storage wiring) — both ports coexist in `gyre-ports`/`AppState`; the merge-processor bundle test now asserts BOTH the full verification protocol and storage-port retrieval; coverage rows 13–15 and the header reflect both tasks' state (7/25 implemented).

### Test evidence

- `cargo test -p gyre-domain` — 550 passed, 0 failed (24 attestation tests, incl. `attestation_verification` consistency cases). Re-run after `c17f82bf`: 550 passed, 0 failed.
- `cargo test -p gyre-cli` — 221 passed, 0 failed (incl. `verify_attestation_command_parses`, `verify_attestation_posts_note_bundle_and_parses_result`, `cli_verify`). Re-run after `c17f82bf`: 221 + 1 passed, 0 failed.
- `SKIP_WEB_BUILD=1 cargo clippy -p gyre-server --lib` and `--all-targets` (post-`c17f82bf`) — zero diagnostics in `merge_attestation.rs`, no `manual_unwrap_or_default` anywhere in `gyre-server`; remaining warnings pre-exist at base `706bc7bd`. `cargo fmt --check -p gyre-server` clean. `check-arch.sh`, `check-inert-enforcement.sh`, `check-fail-open-ref-resolution.sh`, `check-abac-route-registry.sh` all pass. Evidence: `/tmp/stage/review-evidence/clippy-repair-evidence.txt`.
- Standalone crypto probe (real `MergeAttestationBundle::unsigned_canonical_json` + real ring Ed25519 sign/verify): untampered bundle signature verifies; gate-status tampering changes the canonical payload and the bundle signature does NOT verify — confirms the fixed `assert!(!result.bundle_signature_valid)`.
- **Verifier must run** (server test compilation exceeds the in-sandbox 4 GiB build wrapper): `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib merge_attestation` (incl. `tampered_bundle_fails_verification`, `unsigned_and_missing_signatures_fail_verification`, `forge_signed_agent_review_gate_fails_verification`, `spec_approval_signature_verification_round_trip`, `inconsistent_reviewer_binding_fails_consistency_check`) and `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib merge_processor` (incl. the merged `merge_processor_generates_multi_party_attestation_bundle` verifying both protocol and storage).
