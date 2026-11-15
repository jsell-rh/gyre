# Review: task-143 — Implement attestation bundle verification protocol

Comparison base: 706bc7bd2250aac1768a39687a468355ff8afda7 → candidate
afca45b22ce2670f0ecf2b151df9cc7d9c87ced3 (branch includes a merge of the
task-144 base, 94c9ac2f — both the `AttestationVerifier` and
`AttestationStorage` ports coexist).
Diff: 20 files, +1755/−96 (gyre-ports trait, gyre-domain verification module,
gyre-server verifier + REST endpoint, gyre-cli command, scripts fix, docs,
coverage/task records). No exemption-file changes; no deleted tests; no
generated web/dist changes.

## R1 Findings

No blocking findings. Evidence:

### Spec conformance (agent-gates.md §Part 3 Verification)

- **`AttestationVerifier` port** (`gyre-ports/src/attestation_verifier.rs`):
  matches the task plan's trait signature exactly (`verify_bundle` /
  `verify_gate_signature` / `verify_spec_approval`, `#[async_trait]`,
  `Send + Sync`). Registered and re-exported in `gyre-ports/src/lib.rs`
  alongside the sibling attestation ports. Hexagonal boundary verified:
  `scripts/check-arch.sh` passes; the port depends on gyre-domain types only
  (same convention as `attestation_signer`).
- **`VerificationResult`** (`gyre-domain/src/attestation_verification.rs`):
  every field from the plan's sketch is present (`valid`,
  `bundle_signature_valid`, `gate_results: Vec<GateVerificationResult>`,
  `spec_approval_valid: Option<bool>`, `consistency_check`, `errors`), plus
  per-gate `GateVerificationResult` with gate identity, signer, signature
  validity, and per-gate errors — an auditor sees which component failed.
- **Steps 1–3 (crypto)**: `ForgeAttestationVerifier`
  (gyre-server/src/merge_attestation.rs) implements the port with real
  Ed25519 (`ring`) — the forge server key for the bundle signature and
  forge-signed gates, per-agent re-derived keys for `agent:<id>` signers.
  This is the same key scheme task-142's signers produce (placeholder per
  the reviewed task-142 deferral; the port is the documented seam for the
  Sigstore/OIDC replacement). Fail-closed semantics verified by reading:
  unknown/empty signer identities → `Ok(false)`; verification errors →
  `.await.unwrap_or_default()` (bool default = false).
- **Step 5 (consistency, pure domain function)**:
  `check_bundle_consistency` is I/O-free and crypto-free (grep-verified:
  only serde + crate types). Because the task-142 bundle format carries
  `mr_id`/`spec_ref` at bundle level (gates do not repeat them), step 5 is
  implemented as the invariants that make the bundle-level binding sound:
  non-empty `mr_id`/`merge_commit_sha`, well-formed `path@sha` spec_ref,
  `spec_approval` only with a `spec_ref`, per-gate signature presence, and
  a `signed_by` matching the gate type's signing party **in both
  directions** (command gate signed by an agent → rejected;
  AgentReview/AgentValidation signed by the forge → rejected), plus the
  reviewer `oidc_sub`/`agent_id` binding and the author binding. The
  reinterpretation is documented in the module docs, and the
  "mismatched MR IDs" plan-test is impossible in this format — the
  nearest equivalents (empty `mr_id`, malformed/absent `spec_ref`) are
  tested.
- **REST endpoint**: `POST /api/v1/attestations/verify`
  (api/attestations.rs) — caller-supplied bundle, per-component result;
  failed verification is 200 + `valid: false` + findings (correct design:
  the *answer* is the verdict). Route registered in `api/mod.rs` AND in
  the ABAC resolver (`attestation`/`read`) —
  `scripts/check-abac-route-registry.sh` passes. Reachability verified:
  builtin policies `developer-write-access` (800), `agent-scoped-access`
  (700), `readonly-get-only` (600), `admin-all-operations` (900) all allow
  `read` on `*`, so the endpoint is not dead behind `default-deny`. The
  route sits under `require_auth_middleware` like every `/api/v1` route.
- **CLI**: `gyre verify-attestation <commit-sha>` — reads the bundle from
  the merge commit's git note via real `git notes show`, POSTs to the
  verify endpoint, prints per-component result, exits 1 on failure.
- **Spec step 4 (Rekor)**: deferred to task-144's storage scope, honestly
  documented in the coverage row and module docs — the bundle format
  carries no author commit SHAs to check, and the task's own
  implementation plan (steps 1–6) does not include Rekor. Consistent with
  the task-144 row that keeps the Rekor gap open.

### Independent probes (evidence in /tmp/stage/review-evidence/)

- `cargo test -p gyre-domain attestation` — 24 passed, 0 failed.
- `cargo test -p gyre-cli verify_attestation` +
  `reads_bundle_from_real_git_note` /
  `verification_result_rendering_names_failing_components` — 5 passed,
  0 failed (the git-note test runs real `git init`/`git notes`).
- **Adversarial probe against the real `check_bundle_consistency` +
  `assemble_verification_result`** (temporary example crate, removed and
  tree restored to clean at afca45b2 after the run; 8 attack shapes not
  covered by the shipped tests): capital `Agent:` prefix,
  reviewer/oidc_sub mismatch, agent gate without recorded reviewer, empty
  `agent:` id, `"forge "` trailing space, `@sha` spec_ref, approval
  without spec_ref, command gate signed by a fully-aligned agent identity
  — **all 8 caught**; both clean baselines pass.
  (`consistency-adversarial-probe.txt`)
- `cargo check -p gyre-server --lib` — clean. `cargo clippy -p
  gyre-server --lib` — exit 0, **zero diagnostics in merge_attestation.rs
  / attestations.rs / attestation_verification.rs**; the 376 remaining
  warnings are in untouched files/lines (verified per site against the
  diff). The durable finding `679e3bf2` (`manual_unwrap_or_default` at
  merge_attestation.rs:528) is confirmed fixed.
- Mechanical checks: `check-arch.sh`, `check-inert-enforcement.sh`,
  `check-abac-route-registry.sh`, `check-relative-path-defaults.sh`,
  `check-in-memory-state-stores.sh`, `check-mem-port-contracts.sh`,
  `check-task-commit-attribution.sh` — all pass. `cargo fmt --check`
  clean on all four touched packages.

### The `scripts/check-inert-enforcement.sh` change (c880471a)

The implementer defined the `BOUND` regex that the script has **referenced
but never defined since its introduction** — verified at base
(`git show 706bc7bd:... | grep BOUND` shows only the use site). Probe
evidence (`base-check.sh` vs fixed script on a synthetic file): the base
script crashes with `NameError` on any consumed statement-position
`.verify_*/.evaluate_*/...` method call; the fixed script correctly flags
a genuinely inert `self.verify_thing(x).await;` and correctly passes the
consumed `let v = ...unwrap_or_default();` form. This is a genuine crash
repair that *enables* enforcement on a code path that previously killed
the checker — not a weakening; no exemption entries were added (the
exemption file is byte-identical to base).

### Test quality (read-verified, not stub-backed)

The server tests use the real signers (`ForgeSigner`/`GateAgentSigner`,
real ring Ed25519) and real tampering: gate-status tamper (asserts the
named gate fails AND the bundle signature breaks — correct, since
`unsigned_canonical_json()` covers the gates array), `merged_at` tamper,
cross-party signature swap, missing signatures, forge self-attested
AgentReview re-signed end-to-end (fails on the step-5 signing-party rule
with the crypto intact — the strongest test in the set), reviewer
rebinding, and a spec-approval sign/verify/user-approver round trip. The
REST tests go through the real axum router with real state. The
merge-processor integration test verifies a bundle the processor actually
produced AND stored (both task-143 protocol and task-144 storage).

### Non-blocking observations (recorded for future hardening)

1. `assemble_verification_result` does not factor per-gate `errors` into
   `valid` — only `signature_valid` counts. Latent only: the sole
   production constructor of `GateVerificationResult`
   (`ForgeAttestationVerifier::verify_bundle`) pushes gate errors
   exclusively alongside `signature_valid: false`, so no real path
   produces errors+valid. A future port implementation could violate the
   invariant; consider folding per-gate errors into the score.
2. CLI `--repo` defaults to `"."` (relative default). Idiomatic for a
   CLI meant to run inside the checkout; `check-relative-path-defaults.sh`
   passes and the flag is explicit and overridable — noted for
   consistency with the relative-default invariant's spirit.
3. Spec approval absent-signature is a tri-state `None` + explicit
   finding and does not fail the bundle — documented, honest, and
   consistent with task-142's ledger model where the signature is
   optional.

### Verifier must run (in-sandbox 4 GiB build limit — infrastructure, not a code defect)

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib merge_attestation`
  (incl. `tampered_bundle_fails_verification`,
  `unsigned_and_missing_signatures_fail_verification`,
  `forge_signed_agent_review_gate_fails_verification`,
  `spec_approval_signature_verification_round_trip`,
  `inconsistent_reviewer_binding_fails_consistency_check`)
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib merge_processor`
  (incl. `merge_processor_generates_multi_party_attestation_bundle`)
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib api::attestations`
- `SKIP_WEB_BUILD=1 cargo clippy -p gyre-server --all-targets` (the lib
  pass is verified clean here; test targets need the verifier)

## Verdict

Approved. The task contract (port trait, VerificationResult, pure
consistency checks, real crypto verification for bundle/gates/spec
approval, REST endpoint, CLI command, tests) is satisfied with real
implementations; the deferrals (Rekor → task-144, Sigstore/OIDC key
resolution → the port seam) are documented, scoped to their owning
tasks, and honestly recorded in the coverage matrix.
