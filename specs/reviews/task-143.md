# Review — task-143 (Implement attestation bundle verification protocol)

Spec: `specs/system/agent-gates.md` §Part 3 "Verification" (coverage row 14).
Candidate under review: `7ec42b64` (base `706bc7bd`). Product commits: `c880471a`
(implementation), `c2df4d71`, `795ce919`, `bdc62a83` (review repairs from rounds 1–2);
merge `94c9ac2f` resolved the task-144 base.
Verdict: **complete** — all acceptance criteria hold with real implementations; the
round-2 blocking finding is repaired and independently re-proven.

## Round 3 (this review)

Probes run (checkout at `7ec42b64`; evidence in `/tmp/stage/review-evidence/`):

- `cargo test -p gyre-domain` → **550 + 8 passed, 0 failed** (incl. all 24
  `attestation_verification` / `merge_attestation` consistency + canonical-payload tests).
- `cargo test -p gyre-cli` → **221 + 1 passed, 0 failed** (incl.
  `verify_attestation_command_parses`,
  `verify_attestation_posts_note_bundle_and_parses_result` — asserts the real POST
  body/URI against a mock server — and `reads_bundle_from_real_git_note`, which drives
  real `git init/notes add/notes show` in a tempdir and proves a missing note is an
  error, not a silent pass).
- `cargo check -p gyre-server --tests` → clean (6m52s). Server test *execution* is
  refused by the in-sandbox 4 GiB build wrapper (infrastructure constraint, not a code
  defect) — verifier-must-run list below.
- Standalone crypto probe (`gate-tamper-probe.rs`, real
  `gyre-domain::merge_attestation` types + real ring Ed25519, same payload/primitive
  pair as `ForgeAttestationVerifier`): untampered bundle+gate signatures verify;
  tampering a gate's `status` changes `unsigned_canonical_json()` (which nulls only
  `bundle_signature`, so the `gates` array is covered) and the bundle signature does
  NOT verify, the tampered gate's own signature fails, the other gate's stays valid;
  `unsigned_spec_approval_json` round-trips and is tamper-sensitive. Output:
  `gate-tamper-probe-output.txt` — this independently confirms the round-2 repair
  direction (`assert!(!result.bundle_signature_valid)` after gate tampering).
- Mechanical gates: `check-arch`, `check-abac-route-registry`,
  `check-abac-exempt-handlers`, `check-inert-enforcement`,
  `check-relative-path-defaults`, `check-mem-port-contracts`,
  `check-byte-slice-truncation`, `check-in-memory-state-stores`, `check-api-auth`,
  `check-conditional-test-guards` → all exit 0. `check-task-commit-attribution` →
  exit 0 against the candidate's `specs/tasks/task-143.md` content (all four product
  SHAs in `commits:`; the working-tree failure observed first was the controller's
  staged revert of the task file to its assignment-time state, not a candidate defect).
- `check-crypto-verify` / `check-assertionless-tests` exit 2 with awk syntax errors —
  reproduced identically at the base commit (sandbox awk-dialect issue, pre-existing,
  unrelated to this candidate). Their intent is covered manually: `verify_ed25519`
  decodes then really verifies with ring (no decode-without-verify), and the new tests
  all carry real assertions.

Verified working (no findings):

- **Port**: `AttestationVerifier` in `gyre-ports/src/attestation_verifier.rs` with the
  task's exact three-method signature; `gyre-ports` already depends on `gyre-domain`
  (same pattern as `AttestationSigner`), arch gate green.
- **Domain types**: `VerificationResult` (`valid`, `bundle_signature_valid`,
  `gate_results[]`, `spec_approval_valid: Option<bool>`, `consistency_check`,
  `errors[]`) and `GateVerificationResult` — matches the task's specified shape.
- **Step 5 (pure domain)**: `check_bundle_consistency` is I/O-free and enforces the
  invariants that make the bundle's single-`mr_id`/`spec_ref` binding sound (the
  task-142 schema attests MR/spec at bundle level, not per gate — the module doc
  explains this; a literal "mismatched MR IDs across gates" is unrepresentable in the
  dependency-owned schema, and the implemented failure modes — empty `mr_id`, malformed
  `spec_ref`, approval-without-ref, missing gate signature, signer/reviewer identity
  binding both directions, author `oidc_sub` binding, version check — are each pinned
  by a real domain test that fails when the check is broken).
- **Steps 1–3 (crypto)**: `ForgeAttestationVerifier` verifies the bundle signature and
  each gate signature against the named party's real key (forge Ed25519 / per-agent
  re-derived keys — same derivation as the task-142 signers), and the spec approval
  signature over `unsigned_spec_approval_json`. Honest tri-state: absent approval
  signature → `None` + explicit finding (never silently passed); unknown-approver
  signatures → `Some(false)` + error (fail-closed). The previously-shipped inverted
  assertion is fixed in the correct direction (probe above).
- **REST**: `POST /api/v1/attestations/verify` registered in `api/mod.rs` and in the
  ABAC resolver (`attestation`/`read` — coherent: pure evaluation of a caller-supplied
  document, no platform state read or mutated); failed verification is 200 +
  `valid:false` + findings; non-bundle body → 422. Endpoint tests drive the real
  router with `build_state` and real signatures.
- **CLI**: `gyre verify-attestation <commit-sha>` reads the bundle from the merge
  commit's git note via real `git notes show`, POSTs it, prints per-component result,
  exits 1 on failure; docs (`docs/cli.md`, `docs/api-reference.md`) updated.
- **Integration**: `merge_processor` test now runs the full protocol over a bundle the
  merge processor actually produced and stored (protocol + storage-port retrieval).
- **Script repair** (`795ce919`): `check-inert-enforcement.sh`'s `BOUND` regex was
  referenced at its use site since the script's introduction but never defined (latent
  NameError on any consumed statement-position `.verify_*/...` call — verified at
  base). The added definition is faithful to the documented consumption rule
  (`.await` alone is not consumption) and strictly enables the check; the gate exits 0
  on the candidate tree. Gate-signature verification itself was restructured to a
  `match` whose `Err` arm fails closed — semantically identical to `.unwrap_or(false)`.
- **Coverage honesty**: row 14 → implemented with the Rekor step-4 deferral stated in
  the row itself; row 15 (task-144) still tracks Rekor as OPEN, so §Part 3 is not
  falsely reported complete at spec level.

Non-blocking observations (recorded, no action required for this task):

- **Spec step 4 (Rekor) is deferred to task-144** — documented in the coverage row and
  the module doc. The bundle format (owned by dependency task-142) carries no author
  commit SHAs to check, and the real Rekor client is task-144's plan step 4; this
  task's acceptance criteria do not include Rekor. The deferral is the one reviewed in
  round 1 (`task-142's reviewed deferral`), not a new reinterpretation.
- `body_that_is_not_a_bundle_is_rejected_400` asserts `UNPROCESSABLE_ENTITY` (422) —
  axum's actual `Json` rejection code; the test name's "400" is cosmetic.
- CLI `--repo` defaults to `.` — the standard CLI idiom (like `git -C`); the
  relative-path-defaults gate passes and the flag is documented for path-qualified use.

Test-inflation check: none found. Domain consistency tests mutate real fields and
assert specific error strings; server tests sign with the real `ForgeSigner` /
`GateAgentSigner` and assert per-component outcomes (including the subtle
"forge-signed AgentReview is rejected on the signing-party rule even though every
signature cryptographically verifies"); CLI tests exercise the real wire format and
real git notes. Each fails when the behavior it pins breaks.

**Verifier must run** (refused in-sandbox by the 4 GiB build wrapper):
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib merge_attestation`
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib merge_processor`
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib api::attestations`

Verdict: **complete** — approved.
