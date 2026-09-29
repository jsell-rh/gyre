---
title: "Merge dependencies — branch lineage auto-detection"
spec_ref: "merge-dependencies.md §2. Auto-Detected: Branch Lineage"
depends_on: []
progress: not-started
coverage_sections:
  - "merge-dependencies.md §2. Auto-Detected: Branch Lineage"
commits: []
---

## Spec Excerpt

### §2. Auto-Detected: Branch Lineage (merge-dependencies.md)

When a MR's source branch was created from another MR's branch (not the default branch), the forge automatically creates a dependency. This prevents child branches from merging before their parent.

**Detection logic:**
1. At MR creation time, inspect the source branch's merge base with the default branch
2. If the merge base is a commit that is the HEAD of another open MR's source branch, create a `BranchLineage` dependency
3. If the parent MR is later rebased, the dependency is re-evaluated

## Implementation Plan

1. **Branch lineage detection at MR creation**: In the MR creation handler (`api/merge_requests.rs` or `mcp.rs` gyre_create_mr), after creating the MR:
   - Run `git merge-base <source_branch> <default_branch>` to find the common ancestor
   - List all open MRs in the same repo
   - For each open MR, check if its source branch HEAD is an ancestor of the new MR's source branch (i.e., the new branch was forked from the other MR's branch)
   - If found, create a `MergeRequestDependency` with `source: DependencySource::BranchLineage`

2. **Re-evaluation on rebase**: When a push updates an MR's source branch (detected in git_http.rs post-receive processing), re-run the lineage check. Remove stale lineage dependencies and add new ones.

3. **Git operations**: Use the existing `jj_ops.rs` or raw git commands via `tokio::process::Command`. The `git merge-base --is-ancestor` command is the key primitive.

4. **Tests**: Add integration tests that:
   - Create MR A from `main`, create MR B forked from MR A's branch → verify B depends on A
   - Merge MR A → verify dependency is satisfied for B
   - Create MR C from `main` (no lineage) → verify no auto-dependency

## Acceptance Criteria

- [ ] MR creation auto-detects branch lineage dependencies
- [ ] `DependencySource::BranchLineage` used for auto-detected edges
- [ ] Lineage re-evaluated on branch push/rebase
- [ ] Stale lineage dependencies cleaned up
- [ ] Integration tests cover fork-from-branch and fork-from-main scenarios
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/merge-dependencies.md` §"2. Auto-Detected: Branch Lineage". The MR creation code is in `crates/gyre-server/src/api/merge_requests.rs` (REST) and `crates/gyre-server/src/mcp.rs` (MCP gyre_create_mr tool). The dependency management code is in `crates/gyre-server/src/api/merge_deps.rs`. Git operations are in `crates/gyre-server/src/jj_ops.rs` and `crates/gyre-server/src/git_refs.rs`. The post-receive push processing is in `crates/gyre-server/src/git_http.rs`.
