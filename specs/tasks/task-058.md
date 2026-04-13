---
title: "Populate `meta_spec_set_sha` in `InputContent` at Spec Approval Time"
spec_ref: "authorization-provenance.md §2.2 (InputContent.meta_spec_set_sha), §3.2 (strategy-implied constraint: agent.meta_spec_set_sha == input.meta_spec_set_sha)"
depends_on: 
  - task-006
progress: complete
coverage_sections: []
commits: []
---

## Spec Excerpt

From `authorization-provenance.md` §2.2:

> ```
> InputContent {
>   ...
>   meta_spec_set_sha:    string          -- hash of bound meta-spec set at approval time
>   ...
> }
> ```

From `authorization-provenance.md` §3.2 (Strategy-Implied Constraints):

> **From `meta_spec_set_sha`:** The meta-spec set active at approval time is recorded. The constraint verifies the implementing agent's meta-spec set matches.
>
> ```cel
> // Implied: agent's meta-spec set must match what was approved
> agent.meta_spec_set_sha == input.meta_spec_set_sha
> ```

## Current State

- **`InputContent.meta_spec_set_sha`** is always set to `String::new()` (empty string) at spec approval time (`crates/gyre-server/src/api/specs.rs:569`).
- The **dry-run endpoint** (`POST /api/v1/constraints/dry-run`) already computes the correct SHA from the workspace's meta-spec set via `state.meta_spec_sets.get(&ws_id)` and SHA-256 hashing (`specs.rs:1610-1625`).
- The strategy-implied constraint `agent.meta_spec_set_sha == input.meta_spec_set_sha` will always mismatch when the agent has a non-empty meta-spec set, because the input side is always empty string.

## Implementation Plan

1. **Look up workspace meta-spec set SHA at approval time**: In `approve_spec()` (`crates/gyre-server/src/api/specs.rs`), before constructing the `InputContent`, look up the workspace's current meta-spec set from `state.meta_spec_sets` (same KV store used by the dry-run endpoint). Compute `SHA256(json)` to produce the hash.

2. **Reuse the existing pattern**: The dry-run endpoint at line 1610-1625 of `specs.rs` already has the exact logic:
   ```rust
   let meta_spec_set_sha = match state.meta_spec_sets.get(&ws_id).await {
       Ok(Some(json)) => {
           use sha2::{Digest, Sha256};
           let mut hasher = Sha256::new();
           hasher.update(json.as_bytes());
           format!("{:x}", hasher.finalize())
       }
       _ => String::new(),
   };
   ```
   Extract this into a shared helper or duplicate it in the approval path.

3. **Test**: Add a test that approves a spec with a workspace that has a meta-spec set, and verify the resulting `SignedInput.content.meta_spec_set_sha` is non-empty and matches the expected SHA.

## Acceptance Criteria

- [ ] `approve_spec()` populates `InputContent.meta_spec_set_sha` from the workspace's current meta-spec set
- [ ] The SHA is computed as `SHA256(meta_spec_set_json)` (consistent with dry-run and constraint_check)
- [ ] When no meta-spec set is configured for the workspace, the SHA remains empty string (graceful fallback)
- [ ] Test verifies non-empty SHA when workspace has a meta-spec set
- [ ] `cargo test --all` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `crates/gyre-server/src/api/specs.rs` — focus on the `approve_spec` handler and the dry-run endpoint's meta-spec SHA logic
3. Read `crates/gyre-server/src/constraint_check.rs` — see `build_agent_context()` for how the agent-side meta-spec SHA is computed
4. Apply the same SHA computation logic in the approval path
5. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- `feat(provenance): populate meta_spec_set_sha in InputContent at spec approval time (TASK-058)`
