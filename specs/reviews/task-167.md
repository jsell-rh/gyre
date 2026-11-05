# Review — task-167 (Merge dependencies — branch lineage auto-detection)

Spec: `specs/system/merge-dependencies.md` §2 "Auto-Detected: Branch Lineage".
Candidate: `85be9fd488cb113eb1a845b610d9d7d198ce3c14` (base `722179f0`). All 5 frontmatter
commits verified in candidate history; net diff touches only task-scoped files
(`api/merge_requests.rs`, `git_http.rs`, `mcp.rs`, coverage row, task file).
Verdict: **approved**.

## Probes (evidence: `/tmp/stage/review-evidence/task-167-focused-tests.md`)

- `cargo test -p gyre-server --lib api::merge_requests::tests::lineage` → 9/9 passed
  (real on-disk bare git repos built with git plumbing, driven through the REST surface).
- `cargo test -p gyre-server --lib push_rebases_parent_branch_drops_child_lineage_dep` →
  1/1 passed (end-to-end push through `git_receive_pack` with a real packfile; the test
  asserts the ref actually moved before asserting the lineage effect).
- **Mutation A** (detection disabled: `detect_lineage_deps` returns `vec![]`): 7/9 lineage
  tests fail — the two survivors are the correctly-negative no-dependency tests. The tests
  bind to real detection, not self-confirming.
- **Mutation B** (git_http post-receive wiring disabled via `if false &&`): the end-to-end
  push test fails with "lineage re-evaluation after parent-branch push never ran" — the
  wiring itself is genuinely covered.
- Neighboring suites green: `api::merge_requests::tests` 30/30, `api::merge_deps` 13/13,
  `merge_processor` 49/49.
- Mechanical checks green: check-arch, check-fail-open-ref-resolution,
  check-byte-slice-truncation, check-inert-enforcement, check-relative-path-defaults,
  check-mcp-write-tools.
- Tree verified clean at the candidate after every probe; mutations restored in-command
  from byte-exact backups.

## Acceptance criteria

- **MR creation auto-detects lineage** — `detect_lineage_deps` called from REST `create_mr`
  (merge_requests.rs:346) and MCP `handle_create_mr` (mcp.rs), passing `repo.default_branch`
  per spec ("merge base with the default branch"). For each open MR in the repo it resolves
  the candidate tip and the merge-base with the new source branch; merge-base == candidate
  tip ⇒ `BranchLineage` edge. Proven by mutation A.
- **`DependencySource::BranchLineage` used** — edges constructed with that source; REST
  response serializes `branch-lineage`, asserted in tests.
- **Re-evaluated on push/rebase, both directions** — `git_receive_pack` post-receive task
  calls `mrs_needing_lineage_reeval` (own branch pushed, *and* children holding a lineage
  edge whose parent branch moved — spec §2 item 3) + `reevaluate_lineage_deps`. Proven by
  mutation B and `lineage_parent_rebase_rechecks_dependents`.
- **Stale lineage deps cleaned up** — `reevaluate_lineage_deps` keeps non-lineage deps,
  drops undetected lineage edges, re-adds current detection, cycle-guards before persisting.
  `lineage_reevaluated_on_rebase_removes_stale_dep` and
  `lineage_reevaluation_preserves_explicit_deps` cover both directions.
- **Integration tests** — fork-from-MR-branch, fork-from-main, parent-merged, cross-target,
  parked-on-default-tip skip, re-push after fork, explicit-dep preservation, parent-only
  push, end-to-end receive-pack push. All against real git state.
- **`cargo test --all`** — owned by the verification stage; every test added by this task
  plus the neighboring dependency suites ran green in focused probes.

## Spec-conformance notes

- The implementation checks "candidate tip is an ancestor of the new source branch"
  (merge-base(new, cand) == cand tip) rather than the literal "merge-base(new, default)
  == cand tip". This matches the task's own implementation plan verbatim and is a strict
  superset: it also catches forks from mid-branch commits of the parent MR while excluding
  forks from the default branch (via the contained-in-default skip, itself tested by
  `lineage_parent_contained_in_default_not_detected`).
- Creation-time cycle check is unnecessary and correctly absent: a brand-new MR id cannot
  be reachable from any pre-existing dependency edge, so a lineage edge from the new MR
  cannot close a cycle. Re-evaluation (where cycles are possible) is cycle-guarded.
- Branch names passed to git are validated (no leading `-`, no `..`) and passed as discrete
  argv entries — no injection surface. `rev_resolve` failure yields empty string, which
  `is_contained_in` treats as "contains nothing" (candidates not skipped) — conservative
  in the direction of more detection, never a fail-open on a security signal.
- Coverage row 8 updated `task-assigned → implemented` (not `verified`) — correct per the
  no-fake-completion rules; auditor confirmation remains a separate stage.

## Non-blocking observation

In `reevaluate_lineage_deps`, when the cycle guard fires the entire update is skipped,
including removal of stale lineage edges. Fail-closed (the child stays gated) and only
reachable when a would-be lineage parent already explicitly depends on the child; noted
for a future hardening pass, not a spec violation.
