# Review — task-143 (agent-gates.md §Part 3 Verification — attestation bundle verification protocol)

Spec: `specs/system/agent-gates.md` §Part 3 "Verification" — anyone who trusts
Gyre's OIDC issuer can verify the entire bundle: (1) bundle_signature against
the issuer, (2) each gate's signature against the signer's identity, (3) the
spec approval signature, (4) author commit signatures via Rekor, (5) all gate
results reference the same MR and spec; offline, after the fact, by auditors,
compliance, or federated instances.

Commits under review (base `c9cc1536` → candidate `00ddc612`): `c880471a`
(implementation), `c2df4d71` (forge-signed-agent-gate consistency rejection),
`795ce919` + `c17f82bf` + `24c2ac7c` (verification-gate repair series for
findings 74454d36/679e3bf2), `bdc62a83` (test assertion fix). The three
unlisted branch commits (`427f754b`, `bf99f3db`, `00ddc612`) touch only the
task file itself. Verdict: **complete**.

## Probes (evidence: /tmp/stage/review-evidence/, see its README)

- `cargo test -p gyre-domain attestation_verification` → 11 passed, 0 failed.
- `cargo test -p gyre-cli verify_attestation` → 2 passed; `--bin gyre
  reads_bundle_from_real_git_note` → 1 passed (real git init/notes
  add/show; missing note → explicit error, never a silent pass).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` → clean (all production code
  incl. `ForgeAttestationVerifier`, endpoint, AppState wiring type-checks).
- Mutation probes (source restored after each, suite re-run green):
  `is_agent_gate_type` forced always-false → 3 domain tests FAILED;
  `assemble_verification_result`'s `valid` hard-coded true →
  `assembly_scores_components_and_reports_errors` FAILED. The tests kill
  real bugs; they are not self-confirming.
- Mechanical gates on the tree: check-arch, check-abac-route-registry,
  check-inert-enforcement, check-relative-path-defaults,
  check-mem-port-contracts, check-in-memory-state-stores,
  check-forged-scope-fields, check-scope-literal-defaults,
  check-unbounded-external-http, check-forwarded-header-trust — all exit 0.
- Sandbox restriction (not a code defect): the executor's build wrapper
  blocks gyre-server test compilation / `--all-targets` checks (4 GiB
  budget). Required verifier probes recorded in
  /tmp/stage/review-evidence/README.md:
  `cargo test -p gyre-server --lib merge_attestation`,
  `--lib merge_processor`, `--lib api::attestations`,
  `cargo check -p gyre-server --all-targets`.

## Verified working (no material findings)

- **Port** (`gyre-ports/src/attestation_verifier.rs`): `AttestationVerifier`
  trait with exactly the planned surface (`verify_bundle` →
  `VerificationResult`, `verify_gate_signature`, `verify_spec_approval`);
  imports only gyre-domain types, consistent with sibling ports; arch gate
  green.
- **Domain types** (`gyre-domain/src/attestation_verification.rs`):
  `VerificationResult`/`GateVerificationResult` match the task's planned
  shape field-for-field (`valid`, `bundle_signature_valid`, `gate_results[]`,
  `spec_approval_valid: Option<bool>`, `consistency_check`, `errors[]`).
  `check_bundle_consistency` is pure (serde + crate types only, no I/O) and
  thorough: version pin, non-empty mr_id/merge_commit_sha, well-formed
  `path@sha` spec_ref, spec_approval only with a spec_ref, per-gate signature
  presence, signing-party rule in BOTH directions (an AgentReview/
  AgentValidation gate signed by the forge is rejected even when its
  signature verifies under the forge key — kills the single-party forgery;
  a command gate signed by an agent is rejected too), reviewer
  identity↔signer binding, `reviewer_oidc_sub`↔`agent:{reviewer_agent_id}`
  binding, author `oidc_sub`↔`agent_id` binding. The "same MR/spec across
  gates" reading is sound for this bundle format: task-142's
  `GateAttestation` carries no per-gate mr_id/spec — the single top-level
  `mr_id`/`spec_ref` bind every gate by construction, and the check enforces
  the invariants that make that binding non-vacuous. The task's planned
  "mismatched MR IDs across gates" unit test is unrepresentable in the
  format; the empty-mr_id and malformed-spec_ref tests pin the same failure
  class, and the adaptation is documented in the module docs.
- **Verifier** (`gyre-server/src/merge_attestation.rs`):
  `ForgeAttestationVerifier` implements the port with real crypto —
  `verify_ed25519` (ring `UnparsedPublicKey::verify`, base64-decoded
  signature) over the canonical unsigned payloads (`unsigned_canonical_json`
  for the bundle, `unsigned_gate_json` per gate — which covers `signed_by`,
  so a party-swap breaks the signature — and the new
  `unsigned_spec_approval_json` for approvals). Key resolution is
  per-named-party: "forge" → the server key whose public half is published at
  `/.well-known/jwks.json` (the issuer a verifier trusts); "agent:<id>" → the
  per-agent key re-derived exactly as task-142's `GateAgentSigner` signs.
  Unknown/empty identities → `Ok(false)`, fail-closed. `verify_bundle`
  combines the pure consistency check (step 5) with the crypto steps into
  the per-component result; gate verification errors fail closed
  (`unwrap_or_default()` on bool = false), and the result is bound before
  use (the 24c2ac7c repair — verified against the base checker: zero
  METH-form statement-position matches remain in the tree, so the base
  script's undefined-`BOUND` crash can no longer trigger either).
- **REST** (`gyre-server/src/api/attestations.rs`): `POST
  /api/v1/attestations/verify` — caller-supplied `MergeAttestationBundle`,
  per-component `VerificationResult`, failed verification is 200 with
  `valid: false` + findings (a verification verdict is data, not a transport
  error); non-bundle body 422. Route registered in the ABAC resolver
  (attestation/read — Developer's builtin allow covers read on `*`), NOT in
  the frozen exemption file; mounted behind require_auth + ABAC middleware;
  no platform state read or mutated, so no per-handler scope check is
  required. Endpoint tests drive the handler with bundles signed by the real
  `ForgeSigner` and assert per-component outcomes.
- **CLI**: `gyre verify-attestation <commit-sha>` — reads the bundle from
  the merge commit's git note via real `git notes show` (args passed as
  argv, no shell), POSTs to the verify endpoint, prints a per-component
  human-readable rendering (pure, unit-tested), exits 1 on failure. The
  `--repo` default `"."` is user-facing cwd semantics (same as git);
  passes check-relative-path-defaults.
- **Honest tri-state spec approval** (step 3): absent approval or unsigned
  approval → `None` + an explicit finding, never silently scored as
  verified; present signatures really verified for agent approvers
  (round-trip test with the real derived key); user-approver Sigstore
  signatures fail closed as "no platform-verifiable key" (Some(false) +
  finding) — task-142's reviewed deferral, and strictly better than the
  base's seeded `"<sigstore signature on spec approval>"` placeholder,
  which this change removes. The merge-processor test was updated to pin
  the honest behavior (approval.signature is None, reported not passed).
- **Spec step 4 (Rekor)** stays with task-144's storage scope — the bundle
  format carries no author commit SHAs to check; the coverage row records
  this explicitly. Task's acceptance criteria do not include Rekor.
- **Wiring**: `build_state` loads the agent signing key once and shares it
  with the verifier; `mem.rs`/`middleware.rs` test states updated; the old
  boolean `verify_merge_attestation_bundle` is fully replaced (no stale
  callers); the merge-processor integration test now verifies its own
  produced bundle through the full protocol.
- **Docs/coverage**: api-reference and cli.md rows match the shipped
  behavior; coverage row 14 `task-assigned`→`implemented` with accurate
  anchors (7/25 matches the actual implemented-row count; the base header's
  5/25 vs 6 implemented rows was a pre-existing miscount, not introduced
  here).

## Non-blocking observations (recorded for the verifier/record)

- `scripts/check-inert-enforcement.sh` gained a definition for `BOUND`,
  which the base script referenced but never defined (NameError crash —
  durable finding 74454d36). Verified empirically that this changes no
  outcome on this tree: zero METH-form matches exist, so the base script
  would not crash here either, and the previously-crashing behavior never
  flagged anything (it aborted). The repair is not self-serving (the
  candidate's own code passes by not matching METH_CALL at all). Note for
  the future: the chosen `BOUND` semantics treat same-line
  `.unwrap_or(`-style chains as consumption, so a genuinely discarded
  `x.verify_y().await.unwrap_or(false);` statement would pass unflagged —
  a latent false negative to tighten when the checker is next touched, not
  a defect in this candidate.
- The endpoint tests build a minimal router (handler + state) without the
  auth middleware stack; route-level ABAC coverage is proven by the
  resolver registration + check-abac-route-registry, and middleware
  behavior is covered by the existing ABAC suite. Acceptable.
- An empty-gates bundle with a valid forge signature scores `valid: true`
  (no gate results to fail). The merge processor always produces gates
  from real gate results; the spec does not require a non-empty gate set.
  Edge case only.
