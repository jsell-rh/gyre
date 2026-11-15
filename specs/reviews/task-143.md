# Review: task-143 — Implement attestation bundle verification protocol

Comparison base: 103d665e777cee400cd7b9c19e3fbf2afe5d9c44 → candidate 6a4e3ee2d2a4f799e55ccc888d8090be0fb6c694 (3 commits; implementation commit c880471a).
Diff: 20 files, +1639/−94. No exemption-file changes, no deleted tests, no gate weakening
(`scripts/` change is a bug fix to `check-inert-enforcement.sh`, verified below).

## R1 Finding (blocking)

### R1-1: Signing-table consistency check is one-directional — a forge-signed AgentReview gate verifies as a valid multi-party bundle

`check_bundle_consistency` (crates/gyre-domain/src/attestation_verification.rs) enforces
the spec's "What Gets Signed and By Whom" table in only one direction: a command gate
(TestCommand/LintCommand) signed by an agent is flagged ("command-run gate results are
signed by the forge"), but the `None => {}` arm (i.e. `signed_by == "forge"`) never checks
the gate type. An AgentReview/AgentValidation gate attestation signed by `"forge"` passes
the consistency check with zero errors, and `ForgeAttestationVerifier::verify_gate_signature`
then verifies that signature against the forge key — so the full protocol
(`verify_bundle`) returns `valid: true`, `consistency_check: true` for a bundle in which
the forge self-attested an agent review.

Executable proof (independent probe, standalone crate depending on the candidate's
gyre-domain, no repo modification; source and output in
`/tmp/stage/review-evidence/probe-forge-signed-agent-gate.txt` and `/tmp/probe-consistency/`):
an `AgentReview` gate with `reviewer_agent_id: "security-gate-7"`,
`reviewer_oidc_sub: "agent:security-gate-7"`, `signed_by: "forge"` →
`check_bundle_consistency` returns `[]`.

Why this matters:

- The spec's signing table is normative: "AgentReview results — Gate agent … This agent
  reviewed the code and approved it", and the section's stated purpose is "a multi-party
  attestation where no single entity can forge the complete bundle". Part 3's problem
  statement names a compromised merge processor as the adversary the verification protocol
  defends against; a verifier that accepts forge-self-attested agent reviews certifies
  exactly that forgery as valid.
- The module's own doc comment claims the check enforces "a `signed_by` identity that
  matches the gate type's signing party" — a documented-but-partial enforcement claim,
  the flaw class this repo's own standards (goal rule 1, check-inert-enforcement
  rationale) treat as a gap, not a style nit.
- The shipped test suite pins the enforced direction
  (`command_gate_signed_by_agent_fails_consistency`) but has no test for the missing one,
  so the gap is invisible to the candidate's tests.

Caveat honestly recorded: under task-142's placeholder key scheme, per-agent keys are
derived from the forge key, so a forge-key holder can already impersonate an agent
(`signed_by: "agent:<id>"` with a valid derived-key signature). The missing arm does not
weaken cryptography beyond what the placeholder scheme permits — but it does let the
forge *openly* attribute an agent review to itself and pass verification, it contradicts
the check's own documentation, and it becomes cryptographically meaningful the moment
the Sigstore/OIDC key scheme lands behind the same port. Fix is small: in the forge arm,
error when `is_agent_gate_type(&gate.gate_type)`, plus a consistency test for it (and a
server-side test that the full protocol rejects a forge-signed AgentReview gate).

## Verified as real (independent evidence)

- **Port + domain types**: `AttestationVerifier` trait (gyre-ports/src/attestation_verifier.rs)
  with `verify_bundle`/`verify_gate_signature`/`verify_spec_approval`;
  `VerificationResult`/`GateVerificationResult` (gyre-domain/src/attestation_verification.rs)
  match the task's prescribed shapes. `check_bundle_consistency` is pure (no I/O); crypto
  is delegated to the port. Hexagonal boundary clean (`check-arch.sh` green).
- **Crypto is real**: `ForgeAttestationVerifier` (gyre-server/src/merge_attestation.rs)
  verifies real Ed25519 signatures (ring) over the canonical unsigned payloads
  (`unsigned_canonical_json` / `unsigned_gate_json` / new `unsigned_spec_approval_json`)
  against the forge key and per-agent re-derived keys; `verify_ed25519` is the single
  shared primitive. No structural-check stand-ins for `verify()`.
- **REST endpoint**: `POST /api/v1/attestations/verify` registered in `api/mod.rs`,
  ABAC resolver entry `attestation`/`read` (route-registry, exempt-handler, and api-auth
  checks all green); failed verification is 200 + `valid: false` + findings (correct
  semantics — a failed verification is an answer, not a transport error). Endpoint tests
  use real `build_state` + real `ForgeSigner` signatures, not mocks of the code under test.
- **CLI**: `gyre verify-attestation <commit-sha>` reads the bundle from the merge
  commit's git note via real `git notes show` (missing note → explicit error, not a
  silent pass), POSTs to the verify endpoint, renders per-component results, exits 1 on
  failure. Documented in docs/cli.md and docs/api-reference.md.
- **Spec-approval tri-state is honest**: absent approval/signature → `None` + explicit
  finding (never silently passed); present signature really verified; user-approver
  Sigstore signatures fail closed (`Some(false)` + "no platform-verifiable key" finding)
  — documented deferral matching task-142's reviewed signer design.
- **Tests run in this sandbox** (build wrapper permits these):
  - `cargo test -p gyre-domain` — 549 passed, 0 failed (10 new attestation_verification
    tests, each asserting a specific error for a specific tamper).
  - `cargo test -p gyre-cli` — 221 passed, 0 failed (4 new: command parsing, client
    POST/parse via mock HTTP server, rendering, real-git-note read incl. missing-note
    error).
  - `cargo check -p gyre-server --tests` (SKIP_WEB_BUILD=1) — clean, 0 errors.
  - `cargo clippy -p gyre-domain -p gyre-ports -p gyre-cli` — exit 0, no diagnostics on
    any new file.
- **check-inert-enforcement.sh fix verified**: the previously-undefined `BOUND` regex
  (NameError on any statement-position consumed `.verify_*()` method call since the
  script's introduction) is now defined; probe file confirms an inert
  `verifier.verify_x(v).await;` is flagged while `.unwrap_or(false)`/`.is_ok()` tails
  pass. Full-repo run green.
- **merge_processor end-to-end test updated** to drive the full protocol
  (`state.attestation_verifier.verify_bundle`) on a processor-produced bundle — but see
  deferred server tests below.

## Deferred to the independent verifier (build wrapper refuses gyre-server codegen in this sandbox — 4 GiB budget; wrapper output saved in review-evidence)

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib merge_attestation`
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib api::attestations`
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib merge_processor_generates_multi_party_attestation_bundle`
- Full workspace suite + all-target clippy + fmt (standard verification pass).

## Non-blocking observations (recorded, not findings)

- **Spec step 4 (Rekor) is not implemented** and is honestly documented as task-144's
  storage scope: the task-142 bundle format carries no author commit SHAs, so there is
  nothing to check yet; the assigned task's implementation plan and acceptance criteria
  omit Rekor. Coverage row 14 is marked implemented with this deferral noted in-line —
  the coverage auditor should keep row 14's note visible when task-144 lands.
- **The CLI is not fully offline**: it verifies via the server endpoint because the
  placeholder key scheme keeps all key material server-side (agent keys derive from the
  forge key). The spec's "offline, after the fact" capability is served by the endpoint
  for external tools holding the issuer's public key once the OIDC key scheme arrives;
  the port is the documented seam. Within this task's plan (steps 4–5 as written), this
  is conformant.
- Endpoint test `body_that_is_not_a_bundle_is_rejected_400` asserts 422 — cosmetic name,
  correct behavior.
- `scripts/check-crypto-verify.sh` fails with an awk syntax error — pre-existing at the
  base commit (script untouched by this candidate); filed for infrastructure, not this
  task.

## Verdict

**Changes requested** (R1-1). Everything else in the task contract is implemented for
real and independently verified. The one-directional signing-table check is a small,
well-specified fix in the new security-critical code with executable proof of the gap.
