# Review — task-283 (Graph-driven auto-created MR dependencies + dependent-count budget weighting)

Spec: `specs/system/dependency-graph.md` §Workspace Orchestrator Integration (merge ordering + budget allocation bullets).
Base: `8e2e7458a7adeadd887c7dbc413c49d5cc4de47a` · Candidate: `3a46b7771c2ab894c3e637844dedab1d9783ebb9`.
Verdict: **needs-revision** — one blocking finding (R2-F1, mechanical-gate regression); the two feature implementations themselves are real and verified sound by code inspection + probes.

## Round 2 (this review; round 1 = candidate `b8cc5c62`, finding repaired in `701cb6f0`)

### Probes run (candidate worktree / main checkout at `3a46b777`)

- `bash scripts/check-inert-enforcement.sh` → **exit 0** — the round-1 finding (inert tail calls at budget.rs:499/529) is genuinely repaired by the explicit `return ...;` prefix in `701cb6f0` (log: `check-inert.log`).
- `bash scripts/check-relative-path-defaults.sh` → **exit 1** — **1 error at `crates/gyre-server/src/api/merge_requests.rs:708`** (candidate-introduced; base `8e2e7458` exits 0 — logs: `check-relpath-cand.log` / `check-relpath-base.log`). See R2-F1.
- `bash scripts/check-arch.sh`, `check-mcp-write-tools.sh`, `check-byte-slice-truncation.sh`, `check-dead-message-kinds.sh`, `check-task-commit-attribution.sh` (clean candidate worktree), `check-fabricated-scope-defaults.sh`, `check-scope-literal-defaults.sh`, `check-lossy-secret-conversion.sh`, `check-unbounded-external-http.sh`, `check-mem-port-contracts.sh`, `check-fail-open-ref-resolution.sh`, `check-in-memory-state-stores.sh`, `check-concurrent-shared-write.sh` → **all exit 0** (logs: `check-arch.log`, `check-mcp-write.log`, `check-byte-slice.log`, `check-attribution.log`, `r1–r8.log`, `check-failopen.log`, `check-memstate.log`).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` (via `/tmp/stage/bin/cargo`) → **exit 0**, 6m54s, 0 errors (log: `cargo-check-gyre-server.log`) — validates the new `DependencySource::DependencyGraph` variant against every exhaustive match and the non-test surface of both touched crates.
- gyre-server test compilation/execution deferred to the independent verifier per the 4 GiB budget (list below).

### Verified working (no findings)

**Merge ordering — real graph-driven auto-creation.** `detect_graph_deps` (merge_requests.rs) lists the MR repo's outgoing `DependencyEdge`s (`dependencies.list_by_repo`, port semantics confirmed: source-repo filter), computes the MR's real diff (`git -C <repo.path> diff --name-only refs/heads/<default> refs/heads/<source>`, same `is_safe_branch` no-flag-injection rule and same `refs/heads/` ref form as the existing `detect_lineage_deps` — works for bare server repos and test working repos alike), and when the diff touches an edge's `source_artifact` while the target repo has an Open MR, returns a `MergeRequestDependency` with the NEW `DependencySource::DependencyGraph` variant (serialized `"dependency-graph"` in both merge_deps.rs surfaces; queue consumption via `dependencies_satisfied` is source-agnostic and `would_create_cycle` guards the reconciliation write). Most-recently-updated target tie-break (`max_by_key((updated_at, id))`) is deterministic and documented at the detector. Integration is exactly the lineage pattern: REST `create_mr` + MCP `handle_create_mr` merge explicit/lineage/graph via `merge_explicit_and_auto_deps` (explicit precedence, no duplicates; fresh-id cycle argument is sound), `reevaluate_lineage_deps` now detects graph deps in the same pass — stale graph deps dropped, explicit/agent/lineage deps preserved, duplicates skipped, cycle guard before write — and it is reached from (a) the pre-existing push post-receive hook (git_http.rs:1581-1588, unchanged, piggybacks graph on lineage), (b) the new close hook in `transition_mr_status` (`mrs_with_graph_deps_on`, correctly placed AFTER the Closed status is persisted so re-detection sees it), and (c) the new reverse-direction fire-and-forget `reevaluate_graph_deps_of_dependent_repos` on MR creation. Tests drive the real REST surface against real on-disk git repos with real branch content — they fail pre-fix (the detector and the inheritance-path enforcement did not exist).

**Budget allocation — real dependent-count weighting.** `check_repo_spawn_budget` inheritance path (`repo.budget` None) now enforces per-repo caps = effective workspace limits × `dependent_boost_factor(dependents)` = `1 + 0.25·n` capped at 2.0 (monotonic, ≥ 1.0 → never below the unweighted inherited limit; `None` caps stay `None`; ceil rounding), with the dependent count read live from `state.dependencies.list_dependents` (port semantics confirmed: target-repo filter). The concurrent-agent cap counts live agents via `count_active_agents_in_repo` (agents + worktrees, real store reads), and per-repo token/cost usage is genuinely charged in production (budget.rs record path ~line 1035), so the token/cost enforcement reads real state. Graph-read failure logs and yields the unweighted limit (under-admit only). `check_spawn_budget_for_repo` applies the same weighted caps to the workspace-aggregate half — a functional necessity (the plain workspace check would reject at the unweighted limit and the per-repo boost could never take effect), bounded at 2x, and repos WITH an override get the plain caps (override wins verbatim; `explicit_repo_override_wins_over_dependent_weighting` proves both halves). Both repo-scoped spawn sites (`spawn_agent_core`, `spawn_implementation_workers`) migrated; workspace-level spawn sites (orchestrator, meta-spec, stale-agent respawn) correctly keep the plain check. Per-repo unweighted cap can never bind tighter than the workspace aggregate (repo active ≤ ws active ≤ ws cap), so no regression for 0-dependent repos.

**Prior-round repair verified:** the two `enforce_workspace_spawn_caps` tail calls are now `return ...;` statements — `check-inert-enforcement.sh` exits 0 with no exemption entries.

**Docs/coverage:** coverage row 18 (`specs/coverage/system/dependency-graph.md`) claims `implemented` with accurate source pointers and honestly notes the deferred verifier tests and the still-open DependencyPolicy mem-only item (task-199). Commit attribution frontmatter matches the branch commits (attribution check exit 0 on the clean candidate worktree).

### Findings

- [blocking] **R2-F1: `check-relative-path-defaults.sh` regresses to exit 1 — comment text matches the relative-path-literal regex.** `crates/gyre-server/src/api/merge_requests.rs:708` (added by this task, inside `detect_graph_deps`):

  ```rust
  // Normalize "./Cargo.toml" and "Cargo.toml" to the same artifact
  // path git reports.
  ```

  The checker's `REL_PATH` regex (`"\.\/[A-Za-z]`, scripts/check-relative-path-defaults.sh:56) matches the double-quoted `"./Cargo.toml"` **inside the comment text**; the actual code (`strip_prefix("./")`, line 712) does not match (bare `./` is exempt). Base `8e2e7458` exits 0, so the failure is candidate-introduced. Shape false-positive like round 1's inert-enforcement finding, but the gate is a frozen repo invariant (AGENTS.md; pre-commit hook `.pre-commit-config.yaml:496`) and exemption entries must never grow — CI fails on this candidate as-is. **Repair (trivial, no behavior change):** reword the comment so no double-quoted `"./..."` sequence appears, e.g. `// Normalize a leading ./ prefix (./Cargo.toml vs Cargo.toml) to the` / `// artifact path git reports.` — unquoted `./` does not match the regex. Do NOT add an exemption-file entry or an inline `// path:ok` marker.

### Non-blocking observations (recorded for completeness)

- Cross-workspace `DependencyEdge`s (the edge API does not validate same-workspace, pre-existing) can auto-link MRs across workspaces — but the manual `set_dependencies` API has the same property (NotFound-only validation), so the auto path mirrors existing graph-consumer semantics (as does the pre-existing breaking-change cascade). Not candidate-introduced; not blocking here.
- A re-push in repo B that changes which of B's MRs is most recently updated does not re-evaluate A's graph deps (the post-receive selection re-evaluates MRs in B, and only lineage-linked MRs elsewhere). The acceptance criteria (close-drop, diff-change-drop) are covered; this refinement edge is out of scope.
- `web/dist/` untouched by the candidate — correct; bundle regeneration is verifier-owned.

### Verifier must run (deferred from review per the 4 GiB build budget)

1. After the R2-F1 comment repair: `bash scripts/check-relative-path-defaults.sh` → must exit 0; full pre-commit/CI mechanical suite.
2. `cargo check -p gyre-server --tests` (test-module compilation; non-test check already exit 0 in this review).
3. `cargo test -p gyre-server graph_dep` — `graph_dep_created_when_mr_updates_dependency_declaration` (+ docs-only negative control), `graph_dep_not_created_without_pending_target_mr`, `graph_dep_dropped_when_target_mr_closed`, `graph_dep_dropped_when_diff_no_longer_touches_declaration`, `graph_dep_reconciliation_preserves_explicit_and_skips_duplicates`, `graph_dep_links_most_recently_updated_target_mr`.
4. `cargo test -p gyre-server dependent_` — `dependent_boost_factor_is_monotonic_and_bounded`, `dependent_weighted_repo_budget_admits_more_concurrent_agents`, `dependent_weighted_workspace_check_admits_boosted_spawn_only_for_dependent_repo`, `dependent_weighted_token_cap_raises_repo_limit`, `explicit_repo_override_wins_over_dependent_weighting`.
5. Web/dist rebuild + frontend suite per the standard verifier flow (no frontend source changed in this candidate).
