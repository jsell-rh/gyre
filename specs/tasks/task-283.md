---
title: "Orchestrator integration: graph-driven auto-created MR dependencies (merge ordering) and dependent-count budget weighting"
spec_ref: "Spec gap spec-cab97078efffe0fbfb7339be / orchestrator-merge-ordering-budget / ed9f34316476"
depends_on: []
progress: not-started
commits: ["b0b7dc10b0511dc352aebf0cdebc0c298f18b857"]
---

## Required behavior

The priority-decisions and cross-repo-task-creation bullets are real (breaking-change cascade) and unchanged. This task closes the other two bullets. (1) MERGE ORDERING — "cross-repo MR dependencies can be auto-created from the dependency graph. If Repo A's MR updates its dependency on Repo B, and Repo B has a pending MR with the new version, the forge creates the MR dependency automatically": add a graph-driven detector alongside detect_lineage_deps, integrated into the same reevaluate_lineage_deps reconciliation (called from the MR create/update paths): for an open MR in repo A, list A's DependencyEdges (state.dependencies.list_by_repo); when the MR's diff (source branch vs default branch, git diff --name-only) touches an edge's source_artifact — i.e. the MR updates A's dependency declaration on repo B (adding or changing the reference/pinned version) — and repo B has a pending (Open) MR, automatically create a MergeRequestDependency from A's MR to B's MR, recorded with a NEW DependencySource variant (e.g. DependencyGraph) so it is distinguishable from Explicit/BranchLineage/AgentDeclared edges; reconciliation drops graph-sourced deps that are no longer detected (stale), never drops explicit ones, and skips duplicates — exactly the lineage pattern. When repo B has multiple open MRs, link the most recently updated one (deterministic, documented at the detector). The existing merge-queue dependency semantics then order the merges (A's MR waits for B's), and the existing dependency cycle check guards creation. (2) BUDGET ALLOCATION — "repos with many dependents may warrant higher agent budgets": in the repo-budget inheritance path (check_repo_spawn_budget in crates/gyre-server/src/api/budget.rs, when repo.budget is None and the repo inherits the workspace limit), weight the enforced caps by the repo's dependent count read from the live dependency graph (state.dependencies.list_dependents): a defined, monotonic, bounded boost (e.g. +25% of the workspace limit per dependent, capped at 2x the workspace limit) applied to the enforced token/cost/concurrent-agent caps at spawn time. Explicit repo overrides always win unchanged; the weighting never reduces a repo below the unweighted inherited limit; the rule is documented at the enforcement site.

## Acceptance Criteria

- [ ] Repo A has a DependencyEdge on repo B; an MR opened in A whose diff changes A's repo-b dependency declaration (e.g. the repo-b line in Cargo.toml), while B has an open MR, automatically carries a MergeRequestDependency on B's MR with the new DependencySource; A's MR merge is ordered after B's by the existing merge-queue dependency semantics.
- [ ] Closing B's MR, or updating A's MR so its diff no longer touches the dependency declaration, drops the graph-sourced MR dependency on re-evaluation; explicitly-set MR dependencies are never dropped and are not duplicated.
- [ ] Two repos in a workspace, neither with an explicit budget override, one with N>0 dependents and one with none: the dependent-heavy repo's enforced spawn budget is measurably higher — e.g. with workspace max_concurrent_agents=2 and the boost cap at 2x, a repo with 5 dependents may run 4 concurrent agents while the 0-dependent repo is capped at 2 — enforced at spawn by check_repo_spawn_budget with the dependent count read from the graph.
- [ ] A repo with an explicit budget override keeps exactly that override regardless of dependent count.

## Verification intent

Create the A→B DependencyEdge, open an MR in B, then open an MR in A that updates the repo-b line in Cargo.toml: pre-fix the ONLY writer of MergeRequestDependency records is the manual set_dependencies API — no call path in git_http.rs, merge_processor.rs, or merge_requests.rs creates one from a graph edge — so A's MR carries no dependency and can merge before B's MR; the probe must fail until the graph-driven auto-creation exists. Companion budget probe: with workspace max_concurrent_agents=2, spawn a 3rd agent on a repo with 5 dependents and no override — pre-fix nothing in api/budget.rs reads dependent counts, so the spawn is rejected exactly as for a 0-dependent repo; post-fix the dependent-weighted cap admits it (and the same spawn on the 0-dependent repo is still rejected).

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/dependency-graph.md`. Source: `678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce`; plan: `1745ed1044f3de3976ac34455f356a3300421816ea7a6e7574e2e5d9ec4ad3a4`.

[
  {
    "id": "589b5beaab7b22bb95d9b7ee",
    "decision": "gap",
    "requirement_quote": "- **Merge ordering:** cross-repo MR dependencies can be auto-created from the dependency graph. If Repo A's MR updates its dependency on Repo B, and Repo B has a pending MR with the new version, the forge creates the MR dependency automatically.",
    "detail": "Two of the four bullets are real: priority decisions and cross-repo task creation are implemented through the breaking-change cascade — process_breaking_changes queries the graph's dependents and files High-priority 'Breaking change in ...' tasks in each dependent repo, and cascade test tasks are similarly per-dependent. The section stays open on the other two bullets. Merge ordering: no code links DependencyEdge to the MR-dependency system — the only writer of MergeRequestDependency records is the manual set_dependencies API (plus its cycle check); inspected call paths in git_http.rs, merge_processor.rs, and merge_deps.rs contain no automatic creation of an MR dependency from a graph edge, so 'the forge creates the MR dependency automatically' never happens. Budget allocation: nothing in the budget system (api/budget.rs) considers dependent counts — grep for dependents-based budget logic finds nothing; repos with many dependents get no budget treatment.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/merge_deps.rs",
        "quote": "pub async fn set_dependencies(",
        "detail": "The MR-dependency system's only entry point is the explicit set_dependencies handler (caller-declared depends_on IDs with cycle detection); no automatic path derives MR dependencies from DependencyEdge records."
      },
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "                    format!(\"Breaking change in {repo_name}: {description}\"),",
        "detail": "The satisfied bullets: breaking-change handling uses the graph to file High-priority tasks in every dependent repo (priority decisions + cross-repo task creation), which is the real portion of this section."
      }
    ]
  }
]
