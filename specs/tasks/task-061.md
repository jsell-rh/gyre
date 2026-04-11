# TASK-061: Populate Attestation-Chain ABAC Subject Attributes

**Spec reference:** `authorization-provenance.md` §7.2

**Progress:** `needs-revision`
**Review:** [`specs/reviews/task-061.md`](../reviews/task-061.md)

**Git commits:**
- `07a1f928` feat(abac): populate attestation-chain subject attributes (TASK-061)

---

## Context

The spec defines three new ABAC subject attributes derived from the attestation chain:

| Attribute | Source | Description |
|---|---|---|
| `subject.chain_depth` | Attestation chain | How many derivation steps from the root |
| `subject.root_signer` | Root `SignedInput` | Who originally authorized the chain |
| `subject.constraint_count` | Accumulated constraints | How many constraints apply |

These attributes must be available in the `AttributeContext` for ABAC policy evaluation so that custom policies can condition on attestation chain properties. Example policy: "Deny merge when chain_depth > 5" or "Only allow merges where root_signer is a specific user."

The `builtin:require-signed-authorization` policy (already seeded at `abac_middleware.rs:644`) does NOT use these attributes — it only checks `subject.type != "system"`. This task adds the attributes for use by custom and trust-preset policies.

## Current State

- `chain_depth` exists on `AttestationMetadata` (`gyre-common/src/attestation.rs:205`) and is stored in the `chain_attestations` table.
- `constraint_count` is computed during verification at `api/provenance.rs:457` and `constraint_check.rs:1231` but not exposed to ABAC.
- `root_signer` is not computed anywhere in the codebase — only referenced in the spec (`authorization-provenance.md:625,699`).
- The ABAC `AttributeContext` is built at `abac_middleware.rs:781-796` with `subject.type`, `subject.global_role`, `subject.tenant_id`, and JWT claims. No attestation-chain attributes are set.

## Implementation

### 1. Compute `root_signer` from attestation chain

In `gyre-domain` or `gyre-server`, add a helper that walks the attestation chain to the root `SignedInput` and extracts the signer identity:

```rust
/// Extract the root signer from an attestation chain.
/// Returns the `user_identity` from the root SignedInput's key_binding.
pub fn root_signer(chain: &[Attestation]) -> Option<String> {
    chain.iter()
        .find(|a| a.metadata.chain_depth == 0)
        .and_then(|a| match &a.input {
            AttestationInput::Signed(si) => Some(si.key_binding.user_identity.clone()),
            _ => None,
        })
}
```

### 2. Populate attributes in push/merge ABAC evaluation

The push flow (`git_http.rs`) and merge flow (`merge_processor.rs`) both perform per-handler ABAC evaluation. In these code paths, after loading the attestation chain for the submitted work:

- Look up the attestation chain via `ChainAttestationRepository::find_by_task` or `find_by_commit`.
- Compute: `chain_depth` (from leaf attestation), `root_signer` (from root), `constraint_count` (from accumulated explicit + strategy-implied + gate constraints).
- Set on the `AttributeContext` before calling `policy_engine::evaluate`:
  ```rust
  ctx.set_number("subject.chain_depth", chain_depth as i64);
  ctx.set("subject.root_signer", &root_signer);
  ctx.set_number("subject.constraint_count", constraint_count as i64);
  ```

### 3. Update dry-run endpoint

The ABAC dry-run endpoint (`api/policies.rs`) already supports `subject.attestation_level`. Extend the `DryRunSubject` struct with optional `chain_depth`, `root_signer`, and `constraint_count` fields so that the dry-run can test policies against these attributes.

### 4. Tests

- Unit test: `root_signer()` extracts identity from chain root.
- Unit test: `root_signer()` returns `None` for empty chain or chain without `SignedInput`.
- Integration test: push flow populates `subject.chain_depth`, `subject.root_signer`, `subject.constraint_count` in ABAC context.
- Integration test: custom ABAC policy conditioning on `subject.chain_depth > 5` denies deep chains.
- Dry-run test: verify new fields are accepted in dry-run requests.

## Agent Instructions

1. Read the current ABAC middleware code at `crates/gyre-server/src/abac_middleware.rs:780-803`.
2. Read the push-time ABAC evaluation in `crates/gyre-server/src/git_http.rs` (search for `policy_engine::evaluate`).
3. Read the merge-time ABAC evaluation in `crates/gyre-server/src/merge_processor.rs` (search for `policy_engine::evaluate`).
4. Implement the `root_signer()` helper.
5. Add attribute population at each ABAC evaluation point for push/merge actions.
6. Extend the dry-run endpoint.
7. Write tests.
8. Run `cargo test --all` and `cargo fmt --all` before pushing.
9. Update this task file: set progress to `in-progress` when starting, `ready-for-review` when done. Add git commit SHAs.
