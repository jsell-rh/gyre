# Review: task-143 — Implement attestation bundle verification protocol

Comparison base: b8d63b0d115c1b7623480f69010f43d718e336bc → candidate
52b8d64af9693c6dfec81a1333875a25b4537b9e (task commits c880471a, c2df4d71, 795ce919,
all listed in the task frontmatter — attribution check passes at the candidate commit;
the working-tree run of `check-task-commit-attribution.sh` failed only because the
review harness reset `specs/tasks/task-143.md` to its pre-task state in the working
tree, not a candidate defect).
Diff: 20 files, +1740/−94. No exemption-file changes; the only `scripts/` change is
`check-inert-enforcement.sh` defining the previously-referenced-but-undefined `BOUND`
regex (a real latent NameError repair — verified the base script referenced `BOUND`
without defining it — not a weakening; the frozen base copy also passes over the
candidate tree, confirming durable finding 78251ec4's repair).

## Verdict: NOT approved — one blocking finding

### F1 (blocking, test/code): wrong assertion in `tampered_bundle_fails_verification`

`crates/gyre-server/src/merge_attestation.rs:987`:

```rust
assert!(result.bundle_signature_valid, "gate tampering must not break the bundle signature (it covers the tampered bytes) — it fails because the gate signature no longer matches");
```

after `bundle.gates[0].status = "Failed"` (tamper applied post-signing). The bundle
signature covers the **entire** canonical unsigned document **including the `gates`
array** — `unsigned_canonical_json()` clones the bundle and nulls only
`bundle_signature` (crates/gyre-domain/src/merge_attestation.rs:74). Tampering a
gate's status therefore changes the signed payload, and the bundle signature no
longer verifies: `result.bundle_signature_valid` is `false`, so this assertion fails
and `cargo test -p gyre-server --lib merge_attestation` fails at this test.

Decisive evidence (in-sandbox probe, real crypto + real domain canonicalization):
`/tmp/stage/review-evidence/gate-tamper-probe.rs` +
`gate-tamper-probe-output.txt` — a standalone crate depending on gyre-domain's real
`MergeAttestationBundle::unsigned_canonical_json` and real ring Ed25519
sign/verify (the exact `ForgeSigner`/`ForgeAttestationVerifier` primitive pair):

```
untampered bundle sig verifies: true
payload changed by gate-status tamper: true
bundle sig verifies after gate-status tamper: false
```

The in-sandbox build wrapper refuses gyre-server test compilation (4 GiB budget), so
this test never executed before shipping — the task notes themselves list it under
"Verifier must run". The production code is CORRECT here (`bundle_signature_valid:
false` is the right answer — fail-closed); the defect is the test assertion plus its
message, which documents a false security property ("gate tampering must not break
the bundle signature"). Fix: flip to `assert!(!result.bundle_signature_valid)` with a
corrected message (gate tampering breaks both the gate's own signature and the
bundle-level signature, since the bundle signature covers the gates).

## What was independently verified as sound

- **Port + domain types** match the task plan: `AttestationVerifier`
  (`gyre-ports/src/attestation_verifier.rs`) with the three specced methods;
  `VerificationResult`/`GateVerificationResult` in
  `gyre-domain/src/attestation_verification.rs` with every field from the plan.
  Arch check passes (ports→domain allowed, domain stays pure).
- **Pure step-5 consistency check**: `check_bundle_consistency` is I/O-free and
  enforces the signing-party table in BOTH directions (command gate signed by an
  agent rejected; AgentReview/AgentValidation gate signed by the forge rejected —
  repair c2df4d71, pinned by `forge_signed_agent_review_gate_fails_verification`,
  which re-signs with the real forge key so the rejection is the step-5 rule, not a
  crypto artifact). Note: per-gate `mr_id`/spec fields do not exist in the task-142
  bundle format (single top-level `mr_id`/`spec_ref` binds all gates by
  construction), so the plan's "mismatched MR IDs across gates" unit test is not
  implementable in this format; the implemented invariants (non-empty mr_id, 
  well-formed `path@sha` spec_ref, approval↔spec_ref linkage, signer/reviewer/
  oidc_sub bindings) are the honest equivalent — documented in the module header.
- **Crypto is real**: `ForgeAttestationVerifier` verifies real Ed25519 signatures
  (ring) over the same canonical bytes the task-142 signers sign; tamper/swap/
  missing-signature paths fail closed. Spec approval is an honest tri-state
  (absent → `None` + explicit finding; agent-approver signatures really verified;
  unresolvable user-approver Sigstore signatures → `Some(false)` + error,
  fail-closed).
- **REST endpoint**: `POST /api/v1/attestations/verify` registered in
  `api/mod.rs` AND the ABAC resolver (`attestation`/`read` — resource type has
  existing policies); failed verification is 200 + findings (correct design);
  `check-abac-route-registry.sh` and `check-abac-exempt-handlers.sh` pass.
- **CLI**: `gyre verify-attestation <commit-sha>` reads the bundle from the real
  git note (`refs/notes/merge-attestations` — matches task-142's storage ref),
  POSTs it, renders per-component results, exits 1 on failure; missing note is an
  error, never a silent pass.
- **Probes run** (evidence in /tmp/stage/review-evidence/): domain tests
  (`attestation_verification` 11/11 pass), CLI tests (4/4 task tests pass),
  `SKIP_WEB_BUILD=1 cargo check -p gyre-server` (clean, 5m54s), `cargo fmt
  --check` (domain/ports/cli clean), `check-arch.sh`, `check-abac-route-registry.sh`,
  `check-abac-exempt-handlers.sh`, `check-mem-port-contracts.sh`,
  repo + frozen-base `check-inert-enforcement.sh` — all pass.

## Non-blocking observations

- **Spec step 4 (Rekor) deferred**: the verification protocol's step 4 ("verify the
  author's commit signatures via Rekor") is not implemented. The deferral is honest
  and structurally forced: the task-142 bundle format carries no author commit SHAs
  to check, and Rekor itself is task-144's (stubbed) scope. It is documented in
  coverage row 14's notes. Tracking note for the pipeline: no task's
  `coverage_sections` currently owns Rekor *verification* (task-144 covers §Storage)
  — when task-144 lands, step 4 needs an explicit owner or a spec-lifecycle
  amendment.
- The task file's Shipped section references
  `/tmp/stage/review-evidence/task-1....txt` — an ephemeral path outside the repo;
  harmless but unverifiable from the tree.

## Verifier must run (wrapper refuses gyre-server test compilation in-sandbox)

1. `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib merge_attestation` — currently
   FAILS at `tampered_bundle_fails_verification` (F1); must pass after the fix.
2. `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib api::attestations`
3. `cargo test -p gyre-domain` and `cargo test -p gyre-cli` (passed in-sandbox).
4. Full mechanical suite incl. `check-task-commit-attribution.sh` (at the candidate
   commit, not the harness-reset working tree).
