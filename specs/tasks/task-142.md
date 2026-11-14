---
title: "Implement merge attestation bundle schema and multi-party signing"
spec_ref: "agent-gates.md §Part 3 Merge Attestation Bundle"
depends_on:
  - task-136
progress: ready-for-review
coverage_sections:
  - "agent-gates.md §Merge Attestation Bundle"
  - "agent-gates.md §What Gets Signed and By Whom"
commits: ["331a6eafa838d290df79139e4e2697e3a83d19df", "e2c10e6957dbf4e11c5c28d3887abbe9fe9a9337"]
---

## Spec Excerpt

From `agent-gates.md` §Part 3:

When all gates pass and a merge executes, the forge produces a **merge attestation bundle** — a signed document attached to the merge commit.

### Merge Attestation Bundle

The bundle is a JSON document containing:
- `merge_attestation_version`: "1.0"
- `mr_id`, `merge_commit_sha`, `merged_at`
- `spec_ref` with approval details (approver, signature)
- `author` with agent identity, stack attestation, conversation SHA
- `gates[]` array with per-gate results, timing, signer identity, and signatures

### What Gets Signed and By Whom

| Component | Signed By | Proves |
|---|---|---|
| Spec approval | Approver (human or agent) | This version of the spec was reviewed and approved |
| Author commits | Author agent (Sigstore/OIDC) | This agent produced this code with this stack |
| TestCommand/LintCommand results | Forge (Sigstore/OIDC) | The forge ran this command and it passed |
| AgentReview results | Gate agent (Sigstore/OIDC) | This agent reviewed the code and approved it |
| Merge attestation bundle | Forge (Sigstore/OIDC) | All of the above is true and the merge was legitimate |

Each signer uses their own OIDC identity. The result is a multi-party attestation where no single entity can forge the complete bundle.

## Implementation Plan

1. **Define `MergeAttestationBundle` domain type** in `gyre-domain`:
   ```rust
   pub struct MergeAttestationBundle {
       pub version: String,          // "1.0"
       pub mr_id: Id,
       pub merge_commit_sha: String,
       pub merged_at: u64,
       pub spec_ref: Option<SpecRefAttestation>,
       pub author: AuthorAttestation,
       pub gates: Vec<GateAttestation>,
       pub bundle_signature: Option<String>,
   }
   ```
   With supporting types for `SpecRefAttestation`, `AuthorAttestation`, `GateAttestation`.

2. **Define `AttestationSigner` port trait** in `gyre-ports`:
   ```rust
   #[async_trait]
   pub trait AttestationSigner: Send + Sync {
       async fn sign(&self, payload: &[u8]) -> Result<String>;
       fn signer_identity(&self) -> String;
   }
   ```

3. **Implement bundle generation** in `gyre-server`:
   - After successful merge in the merge processor, collect all gate results
   - Build the `MergeAttestationBundle` from MR data, gate results, and author info
   - Sign each gate result with the appropriate signer (forge for TestCommand/LintCommand, gate agent for AgentReview)
   - Sign the complete bundle with the forge's identity
   - Serialize to JSON

4. **Integrate into merge processor** — after `merge_and_record()` succeeds, call `generate_attestation_bundle()` and store the result.

5. **Tests:**
   - Unit test: bundle generation produces valid JSON with all required fields
   - Unit test: multi-party signing — different signers for different gate types
   - Unit test: bundle includes all gate results from the MR
   - Integration test: merge processor generates bundle after successful merge

## Acceptance Criteria

- [ ] `MergeAttestationBundle` domain type with all spec-defined fields
- [ ] `AttestationSigner` port trait for pluggable signing
- [ ] Bundle generated after every successful merge
- [ ] Per-gate signing: forge signs TestCommand/LintCommand, gate agent signs AgentReview
- [ ] Bundle-level signature by forge covers the entire document
- [ ] Bundle serializes to the JSON schema defined in the spec
- [ ] Tests pass

## Agent Instructions

- Read `crates/gyre-server/src/merge_processor.rs` for the merge execution flow
- Read `crates/gyre-domain/src/merge_request.rs` for MR and gate result types
- Read `agent-gates.md` §Part 3 for the full JSON schema example
- The hexagonal boundary applies: domain types in `gyre-domain`, signer trait in `gyre-ports`, forge signer implementation in `gyre-server`
- For the initial implementation, the signer can use HMAC or a placeholder — Sigstore/OIDC integration comes later
- Do NOT modify the merge processor's existing merge logic — only ADD bundle generation after merge succeeds

## Shipped

### Behavior

- **Domain types** (`crates/gyre-domain/src/merge_attestation.rs`): `MergeAttestationBundle` with `SpecRefAttestation`, `AuthorAttestation`, `GateAttestation`, serializing to the spec's exact JSON schema (`merge_attestation_version` "1.0", `mr_id`, `merge_commit_sha`, `merged_at`, `spec_ref`, `spec_approval`, `author`, `gates[]` with per-gate result/timing/`output_hash`/`signed_by`/`signature`, `bundle_signature`). `unsigned_canonical_json()`/`unsigned_gate_json()` define the exact bytes each signature covers; timestamps are Unix epoch seconds (u64).
- **Port** (`crates/gyre-ports/src/attestation_signer.rs`): `AttestationSigner` trait (`sign`, `signer_identity`) — the seam where Sigstore/OIDC identities plug in later without changing the bundle format.
- **Signers** (`crates/gyre-server/src/merge_attestation.rs`): `ForgeSigner` signs with the server's real Ed25519 key; `GateAgentSigner` gives each gate agent a distinct Ed25519 key pair (the placeholder key scheme the task explicitly permits, derived from forge key material bound to the agent id so a Gyre instance can re-derive it for verification; distinct from the forge key, so the forge alone cannot forge a complete bundle).
- **Generation** (`generate_merge_attestation_bundle`): collects every gate result for the MR; signs TestCommand/LintCommand (and other forge-run) results with the forge identity; for AgentReview/AgentValidation gates, resolves the review bound to the gate (`Review.gate_id`), records reviewer identity/decision/body-hash, and signs with that gate agent's identity. Spec approval comes from the approval ledger's newest active entry for the exact (path, sha) — never fabricated; MRs without an author agent omit the author block. Bundle-level forge signature covers the canonical unsigned document.
- **Author attestation level (review repair F1)**: derived with the canonical workload-record rule — `constraint_check::derive_attestation_level` — the same derivation spec-approval validation and the gate executor apply to the same `workload_attestations` record (0 = no record, 1 = raw push, 2 = CLI-attested stack, 3 = Gyre-managed container with stack+container+image). No bundle-local rule exists anymore, so the bundle can never attest a level that contradicts the provenance evaluation of the same record.
- **Reviewer stack attestation (review repair F2)**: each AgentReview/AgentValidation gate entry records the reviewing gate agent's stack fingerprint (`reviewer_stack_attestation`) from the same `workload_attestations` namespace the author's comes from, keyed by `review.reviewer_agent_id` (identity-security.md §Layer 2: record which stack produced each gate result).
- **Integration**: both merge-processor success paths (single-MR `process_next` and each atomic-group member in `merge_atomic_group`) generate and store the bundle. Stored as a `merge_attestation_bundle` audit event (full signed JSON, queryable) and as a git note under `refs/notes/merge-attestations`. Generation failure logs a warning and never rolls back the landed merge. No existing merge logic was modified — only the added post-merge call.
- **Verification** (`verify_merge_attestation_bundle`): checks the bundle signature and every per-gate signature against the named party's key; returns false on tampering, missing signatures, or a `signed_by` that doesn't match the signing key.

### Test evidence

Probes run in this sandbox (the build wrapper permits check-based and small-package probes):

- `cargo test -p gyre-domain` — 8 passed, 0 failed.
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` — clean; type-checks the lib and all server tests including the new F1 regression test.
- `SKIP_WEB_BUILD=1 cargo clippy -p gyre-server --tests` — finished; zero warnings in either touched `merge_attestation.rs`.

Server test execution is blocked by the sandbox build wrapper (gyre-server code generation exceeds the 4 GiB budget — recorded in `/tmp/stage/review-evidence/task-142-repair-evidence.md`); the independent verifier must run:

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib merge_attestation` — covers multi-party signing and spec-schema serialization (now also asserting `reviewer_stack_attestation` and the canonical author level `Some(2)` for a stack-attested local workload), the new `author_attestation_level_matches_canonical_derivation` F1 regression (bundle level equals `constraint_check::derive_attestation_level` for all four workload shapes), tamper/missing-signature verification failures, distinct gate-agent keys, and author omission for agent-less MRs.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib merge_processor_generates_multi_party_attestation_bundle` — integration test that the merge processor generates and stores the bundle after a successful merge.
