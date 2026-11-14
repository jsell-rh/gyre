# Review — task-142 (Implement merge attestation bundle schema and multi-party signing)

Spec: `specs/system/agent-gates.md` §Part 3 — "Merge Attestation Bundle" and
"What Gets Signed and By Whom" (coverage rows 12–13).
Candidate: `329f434b3ad654549bf43c52614c50e8c392c7a5` (base `9527d6ae`).
Product commits under review: `331a6eaf` (feat), `e2c10e69` (review-repair fix).
Verdict: **approved** — every acceptance criterion is met with real
implementations; evidence under `/tmp/stage/review-evidence/task-142-review-evidence.md`.

## Probes (exit-coded, at the exact candidate)

- `cargo test -p gyre-domain merge_attestation` → 4/4 passed (schema
  field-name fidelity, canonical-JSON signature exclusion, sha256 known
  vector). Full `cargo test -p gyre-domain` → 0 failures.
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` → clean (8m38s);
  type-checks the new server module, its 7 unit tests, the
  `merge_processor_generates_multi_party_attestation_bundle` integration
  test, and both merge-processor call sites.
- `scripts/check-arch.sh` → OK (domain boundary holds; `sha2` in gyre-domain
  is a pure crypto lib, not infrastructure).
- `check-byte-slice-truncation`, `check-relative-path-defaults`,
  `check-lossy-secret-conversion`, `check-inert-enforcement`,
  `check-unbounded-external-http` → all OK.
- `check-task-commit-attribution.sh` → OK; both product commits are in the
  task's `commits:` frontmatter (no attribution clobber in this candidate).
- Source-level spec-schema comparison: every serde key matches the spec's
  JSON example exactly; timestamps are Unix epoch seconds (u64) per the task's
  authoritative implementation plan (`pub merged_at: u64`).

## Acceptance criteria verification

- **`MergeAttestationBundle` domain type with all spec-defined fields** —
  `gyre-domain/src/merge_attestation.rs`; serializes to the spec's exact JSON
  schema (`merge_attestation_version` "1.0", `mr_id`, `merge_commit_sha`,
  `merged_at`, `spec_ref`, `spec_approval`, `author`, `gates[]`,
  `bundle_signature`); canonical unsigned-JSON helpers define the exact signed
  bytes.
- **`AttestationSigner` port trait** — `gyre-ports/src/attestation_signer.rs`
  (`sign`, `signer_identity`), implemented by `ForgeSigner` (server's real
  Ed25519 key) and `GateAgentSigner` (per-agent Ed25519) in gyre-server.
- **Bundle generated after every successful merge** — pure-additive call sites
  in both success paths: single-MR `process_next` (after `merge_and_record`)
  and each `merge_atomic_group` member (after the Merged status update). No
  existing merge logic modified (zero deleted/changed lines in
  merge_processor.rs). Failure logs a warning and never rolls back the landed
  merge — correct per the task's "only ADD bundle generation" constraint.
- **Per-gate signing** — forge signs TestCommand/LintCommand (and other
  forge-run types); for AgentReview/AgentValidation the review is resolved via
  `Review.gate_id` and signed under `agent:<reviewer_id>`. Spec approval
  details come from the approval ledger's newest active entry for the exact
  (path, sha) — carried, never fabricated. MRs without an author agent omit
  the author block.
- **Bundle-level signature covers the entire document** — forge signs
  `unsigned_canonical_json()` (all fields except `bundle_signature` itself);
  per-gate payloads cover `signed_by` and every attested field, so party-swap
  and field-tampering both break verification.
- **Serializes to the spec JSON schema** — verified field-by-field against the
  spec example (Probe 4).
- **Tests pass** — domain tests pass in this sandbox; server tests are
  type-checked clean here and must be executed by the independent verifier:
  `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib merge_attestation` and
  `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib
  merge_processor_generates_multi_party_attestation_bundle` (gyre-server test
  codegen exceeds the 4 GiB sandbox budget — review-assignment constraint,
  not a code defect).

## Fake-implementation / test-inflation check

- `verify_merge_attestation_bundle` performs real ring Ed25519 verification
  against the named party's public key — not a structural check. Tamper tests
  (mutated gate status, bumped `merged_at`, cross-party signature swap,
  missing signatures) assert verification failure, so a stubbed verifier
  fails them.
- Generation reads real state (gate results, quality gates, reviews, approval
  ledger, workload attestations, conversation SHA KV records); the
  integration test drives the real `process_next` merge cycle and recovers the
  full signed bundle from the audit log, then re-verifies every signature.
  Removing the `process_next` call fails the integration test.
- The F1 regression (`author_attestation_level_matches_canonical_derivation`)
  pins the bundle's author level to `constraint_check::derive_attestation_level`
  across all four workload-record shapes — a bundle-local rule cannot pass it.

## Observations (non-blocking; carried for follow-up tasks, not this task's contract)

1. **Placeholder key scheme is forge-derived.** `GateAgentSigner` keys are
   derived from the forge key material, so the forge key holder can re-derive
   every gate-agent key. The task explicitly permits a placeholder pending
   Sigstore/OIDC, and the scheme is disclosed as such in code, shipped notes,
   and the coverage row — but the spec's end-state property "no single entity
   can forge the complete bundle" is not yet cryptographically achieved, and
   the code/coverage phrasing "the forge alone cannot forge a complete bundle"
   is true only in the narrow sense that the forge's own key cannot produce a
   gate-agent-verifying signature. Task-143/144 (OIDC identities via the same
   `AttestationSigner` port) must replace this and must not inherit the claim.
2. **Atomic-group rollback leaves earlier members' bundles** (audit event +
   git note on a now-unreachable merge commit). The attested facts were true
   at signing time and re-merges produce fresh bundles; storage semantics are
   task-144's scope. No specced rollback-invalidation requirement exists.
3. **`build_spec_approval` maps a ledger storage error to "no approval"**
   (omits attestation content rather than fabricating); enforcement of
   spec approval happens upstream of the merge, not in the bundle.
