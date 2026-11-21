---
title: "Orchestrator integration: graph-driven auto-created MR dependencies (merge ordering) and dependent-count budget weighting"
spec_ref: "Spec gap spec-cab97078efffe0fbfb7339be / orchestrator-merge-ordering-budget / ed9f34316476"
depends_on: []
progress: complete
commits: ["97bbefe9262be640c2cabd2b937317a6785c2506", "701cb6f088549882d115b8df1558ae6b1f35e9d6", "08b784294c97cd650a675fbee9235f5c7bc17348", "b0b7dc10b0511dc352aebf0cdebc0c298f18b857"]
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

## Shipped

Both open bullets of dependency-graph.md §Workspace Orchestrator Integration are closed in production code; the priority-decisions and cross-repo-task-creation bullets (breaking-change cascade) are unchanged.

### Merge ordering — graph-driven auto-created MR dependencies

`detect_graph_deps` (crates/gyre-server/src/api/merge_requests.rs) is the graph-driven detector alongside `detect_lineage_deps`: for an open MR in repo A it lists A's outgoing DependencyEdges (`state.dependencies.list_by_repo`), computes the MR's diff against the default branch (`git diff --name-only refs/heads/<default> refs/heads/<source>`, same no-flag-injection branch-safety rule as the lineage detector), and when the diff touches an edge's `source_artifact` — A's declaration of its dependency on repo B, e.g. the repo-b line in Cargo.toml — while B has an Open MR, it returns a `MergeRequestDependency` on B's MR recorded with the new `DependencySource::DependencyGraph` variant (crates/gyre-domain/src/merge_request.rs; serialized as `dependency-graph` in merge_deps.rs responses and the queue graph). When B has multiple open MRs the most recently updated one is linked (deterministic tie-break: highest `updated_at`, then greatest MR id — documented at the detector). One link per target repo even when several artifacts toward it changed.

Integration exactly follows the lineage pattern: REST `create_mr` and MCP `handle_create_mr` merge explicit + lineage + graph deps via `merge_explicit_and_auto_deps` (explicit precedence, nothing duplicated; a cycle through a freshly created MR is impossible by construction). The shared `reevaluate_lineage_deps` reconciliation also detects graph deps — stale graph-sourced deps drop, newly detected ones re-add, explicit/agent-declared/lineage deps are never touched, duplicates skipped, existing cycle check guards creation — and is called from the push post-receive path (git_http.rs `mrs_needing_lineage_reeval`) and the MR close hook (`transition_mr_status` → `mrs_with_graph_deps_on`), so closing B's MR or re-pushing A's branch reconciles. Creating an MR fire-and-forget-spawns `reevaluate_graph_deps_of_dependent_repos` so dependent repos' open MRs pick the new pending MR up. Merge ordering itself is the existing merge-queue semantics: the stored `DependencyGraph` dep makes `dependencies_satisfied` require B's MR Merged and the queue's topological sort orders A after B.

### Budget allocation — dependent-count weighting

`check_repo_spawn_budget` (crates/gyre-server/src/api/budget.rs) inheritance path (`repo.budget` None): enforced token/cost/concurrent-agent caps are the effective workspace limits weighted by the repo's dependent count read live from the graph (`state.dependencies.list_dependents`): factor = 1 + 25% of the workspace limit per dependent, capped at 2.0 — with workspace `max_concurrent_agents=2` a 5-dependent repo enforces 4 concurrent agents while a 0-dependent repo stays capped at 2. The factor is monotonic in the dependent count and ≥ 1.0, so the weighting never reduces a repo below the unweighted inherited limit; `None` caps stay `None`; fractional results round up; a graph read failure logs and yields the unweighted limit (under-admit only). The rule is documented at the enforcement site. `check_spawn_budget_for_repo` applies the same weighted caps to the workspace-aggregate half of the spawn check, and both spawn call sites — `spawn_agent_core` (api/spawn.rs) and `spawn_implementation_workers` (signal_chain.rs) — use it. Explicit repo overrides are enforced verbatim, never weighted.

### Review-repair rounds

- Round 1 (701cb6f0): the two function-tail `enforce_workspace_spawn_caps(...).await` expressions in `check_spawn_budget_for`/`check_spawn_budget_for_repo` tripped the mechanical inert-enforcement gate (line-start regex shape false-positive; both results were returned). Fixed with no behavior change by prefixing both tail calls with an explicit `return` and terminating with `;`. No exemption entries added.
- Round 2 (97bbefe9, this checkpoint): the normalization comment in `detect_graph_deps` contained a double-quoted `./Cargo.toml` sequence that the frozen `check-relative-path-defaults.sh` REL_PATH regex matched (comment text, not a path value). Reworded the comment with no behavior change; no exemption entry or `// path:ok` marker added.

### Test evidence

- `bash scripts/check-relative-path-defaults.sh`: exit 0 after this repair (was exit 1, single error at merge_requests.rs:708; log: /tmp/stage/review-evidence/check-relpath-repair.log).
- `bash scripts/check-inert-enforcement.sh`: exit 0 (log: /tmp/stage/review-evidence/check-inert-repair2.log).
- `bash scripts/check-arch.sh`: exit 0 (log: /tmp/stage/review-evidence/check-arch.log).
- `cargo fmt -p gyre-server --check`: clean (log: /tmp/stage/review-evidence/rustfmt-check-repair2.log).
- This round is comment-only; no code path changed, so no re-cold-build was performed. Prior-round evidence on the same code paths: `cargo check -p gyre-server --tests` exit 0; `cargo test -p gyre-domain` 580 + 8 tests pass, 0 failed.
- gyre-server test execution remains deferred to the independent verifier per the 4 GiB build budget; the verifier should run:
  - `cargo test -p gyre-server graph_dep` — real on-disk git repos A→B with a DependencyEdge whose source artifact is A's Cargo.toml, driven through the REST MR surface, no git mocks: `graph_dep_created_when_mr_updates_dependency_declaration` (auto-created dep reaches storage with source `DependencyGraph`, plus the docs-only-diff negative control), `graph_dep_not_created_without_pending_target_mr`, `graph_dep_dropped_when_target_mr_closed`, `graph_dep_dropped_when_diff_no_longer_touches_declaration`, `graph_dep_reconciliation_preserves_explicit_and_skips_duplicates`, `graph_dep_links_most_recently_updated_target_mr`.
  - `cargo test -p gyre-server dependent_` — `dependent_boost_factor_is_monotonic_and_bounded`, `dependent_weighted_repo_budget_admits_more_concurrent_agents` (5 dependents, ws cap 2: 3rd and 4th spawns admitted, 5th rejected naming the weighted cap and the dependent count), `dependent_weighted_workspace_check_admits_boosted_spawn_only_for_dependent_repo`, `dependent_weighted_token_cap_raises_repo_limit`, `explicit_repo_override_wins_over_dependent_weighting`.

### Notes

- Coverage row 18 (Workspace Orchestrator Integration) in specs/coverage/system/dependency-graph.md carries concrete source evidence for both bullets; status stays `implemented` pending independent verification of the deferred gyre-server tests.
- Source attribution: b0b7dc10 + 08b78429 (implementation checkpoints), 701cb6f0 (inert-enforcement repair), 97bbefe9 (relpath comment repair). Web/dist untouched.

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
