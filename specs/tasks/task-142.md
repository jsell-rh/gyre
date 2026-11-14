---
title: "Implement merge attestation bundle schema and multi-party signing"
spec_ref: "agent-gates.md §Part 3 Merge Attestation Bundle"
depends_on:
  - task-136
progress: not-started
coverage_sections:
  - "agent-gates.md §Merge Attestation Bundle"
  - "agent-gates.md §What Gets Signed and By Whom"
commits: []
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
