---
title: "Merge dependencies — branch lineage auto-detection"
spec_ref: "merge-dependencies.md §2. Auto-Detected: Branch Lineage"
depends_on: []
progress: complete
coverage_sections:
  - "merge-dependencies.md §2. Auto-Detected: Branch Lineage"
commits: ["6d665d59b6da61bf643ae0875d8fc1dfe0f4efc9", "eede92716ecc3498a859515dd8022c267afb3f97", "27e8e0ad8730c135301b4e48c6b9fc009ba4d1d6", "e390c781981a4ed1463a32e8bf42a06cbbd87761", "15ecde9c5a55dad0fb24aa787ed94712492a9294"]
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

## Shipped

Branch lineage auto-detection for merge dependencies (merge-dependencies.md
§2) is fully implemented in production code:

- **Detection at MR creation** — `detect_lineage_deps`
  (crates/gyre-server/src/api/merge_requests.rs) runs on both creation
  surfaces: REST `create_mr` and MCP `gyre_create_mr`. For each open MR in
  the same repo it resolves the candidate branch tip and the merge-base
  with the new source branch; merge-base == candidate tip creates a
  `DependencySource::BranchLineage` edge. Candidates whose tip is already
  contained in the default branch are skipped (spec: fork must be from
  "another MR's branch, not the default branch").
- **Re-evaluation on push/rebase** — git_http.rs post-receive processing
  calls `mrs_needing_lineage_reeval` + `reevaluate_lineage_deps` for every
  push. Covers both spec directions: the pushed branch's own MR, and child
  MRs holding a lineage edge whose *parent* branch moved (spec §2 item 3).
  Stale lineage edges are dropped, newly-detected ones added,
  explicit/agent-declared dependencies are preserved, and cycle creation is
  guarded before persisting.
- **Merge gating** — the merge processor's `dependencies_satisfied`
  requires dep status == Merged, so a child cannot merge before its
  lineage parent.

### Test evidence

Focused probes on branch head a4d5c823 (this assignment, evidence saved to
`/tmp/stage/review-evidence/task-167-focused-tests.md`):

- `cargo test -p gyre-server --lib api::merge_requests::tests::lineage` —
  9/9 passed against real on-disk git repos built with git plumbing:
  fork-from-MR-branch creates the edge, fork-from-main creates none, parent
  merged satisfies the dep, rebase removes stale edges, re-push after a
  later fork adds new edges, explicit deps are preserved through
  re-evaluation, parent-only push re-checks dependents, cross-target-branch
  forks detected, candidates contained in the default branch skipped.
- `cargo test -p gyre-server --lib push_rebases_parent_branch_drops_child_lineage_dep` —
  1/1 passed: end-to-end push through `git_receive_pack` triggers
  post-receive lineage re-evaluation and drops the child's stale edge.

Full gates (`cargo test --all`, arch checks, CI) are owned by the
verification stage.

### Assignment note

Checkpoint recovery: the implementation landed in the commits listed in
this task's frontmatter (all present in branch history). The previous
assignment aborted during sandbox pre-flight on a GitHub API TLS timeout —
an infrastructure failure, not a code state. This assignment verified the
implementation on the current head (after the base merge of 722179f0) and
flipped the task to ready-for-review.
