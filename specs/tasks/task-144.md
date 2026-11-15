---
title: "Implement attestation bundle storage targets"
spec_ref: "agent-gates.md §Part 3 Storage"
depends_on:
  - task-142
progress: not-started
coverage_sections:
  - "agent-gates.md §Storage"
commits: []
---

## Spec Excerpt

From `agent-gates.md` §Part 3 Storage:

Merge attestation bundles are stored:
- As a **git note** on the merge commit (accessible via `git notes show`)
- In the **audit_events** table for queryability
- In **Rekor** transparency log for non-repudiation
- In the **AIBOM** for each release that includes the merge commit

## Implementation Plan

1. **Define `AttestationStorage` port trait** in `gyre-ports`:
   ```rust
   #[async_trait]
   pub trait AttestationStorage: Send + Sync {
       async fn store(&self, bundle: &MergeAttestationBundle) -> Result<()>;
       async fn retrieve_by_commit(&self, commit_sha: &str) -> Result<Option<MergeAttestationBundle>>;
       async fn retrieve_by_mr(&self, mr_id: &Id) -> Result<Option<MergeAttestationBundle>>;
   }
   ```

2. **Implement git note storage** in `gyre-adapters`:
   - After merge, write the serialized bundle JSON as a git note on the merge commit
   - Use `git notes add -f --ref=attestations <sha>` with the bundle JSON
   - Retrieval via `git notes show --ref=attestations <sha>`

3. **Implement audit_events storage** in `gyre-adapters`:
   - Record the attestation bundle as an audit event with `event_type: "merge_attestation"`
   - Store the full bundle JSON in the `details` field
   - Index by `mr_id` and `merge_commit_sha` for queryability

4. **Rekor integration (stub):**
   - Define a `RekorClient` port trait for transparency log submission
   - Implement a no-op adapter initially with a TODO for Rekor API integration
   - Log a warning when Rekor is not configured

5. **AIBOM integration point:**
   - Add a method to retrieve all attestation bundles for a set of commit SHAs (for release AIBOM generation)
   - This is a query method on `AttestationStorage`, not a separate storage target

6. **Wire into merge processor** — after bundle generation (task-142), call all storage backends.

7. **Tests:**
   - Integration test: bundle stored as git note, retrievable via `git notes show`
   - Integration test: bundle stored in audit_events, queryable by MR ID
   - Unit test: retrieval by commit SHA returns correct bundle
   - Unit test: retrieval by MR ID returns correct bundle

## Acceptance Criteria

- [ ] `AttestationStorage` port trait with store/retrieve operations
- [ ] Git note storage: bundle written to `refs/notes/attestations` on merge commit
- [ ] Audit events storage: bundle queryable by MR ID and commit SHA
- [ ] Rekor integration stubbed with no-op adapter
- [ ] AIBOM query method for bulk attestation retrieval by commit SHAs
- [ ] Merge processor stores bundle after generation
- [ ] Tests pass

## Agent Instructions

- Read task-142 output for the `MergeAttestationBundle` type
- Read `crates/gyre-server/src/merge_processor.rs` for the merge flow
- Read `crates/gyre-adapters/src/` for adapter implementation patterns
- Read `crates/gyre-ports/src/audit.rs` (or equivalent) for the audit event recording pattern
- For git note operations, use `std::process::Command` to run git commands (same pattern as existing git operations in the codebase)
- The hexagonal boundary applies: port traits in `gyre-ports`, implementations in `gyre-adapters`
