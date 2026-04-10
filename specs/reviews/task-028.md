# TASK-028 Review — R1

**Reviewer:** Verifier  
**Verdict:** `needs-revision` (6 findings)

---

## Findings

- [-] [process-revision-complete] **F1: Speculative merge skips branches whose dependencies are already merged.**  
  In `speculative_merge.rs`, the wave algorithm checks `speculated_mr_ids.contains(dep_id)` (line 155) to determine if a dependency is satisfied. `speculated_mr_ids` only contains MR IDs that were speculated clean in the **current run**. If a dependency MR is already merged (its agent is no longer active, no worktree exists), its ID is never added to `speculated_mr_ids`. The dependent branch is incorrectly skipped with "unsatisfied dependency order," even though the dependency is fully satisfied by being merged. Compare with `merge_processor.rs:38-44` which correctly considers a dependency satisfied when `dep_mr.status == MrStatus::Merged`.  
  **Fix:** Before the wave loop, pre-populate `speculated_mr_ids` with the IDs of all MRs referenced in any branch's `dep_mr_ids` that already have status `Merged`.

- [-] [process-revision-complete] **F2: No tests for speculative merge dependency-aware behavior.**  
  The acceptance criteria require "Tests cover… speculative merge." The speculative merge was significantly rewritten (200+ lines of new wave-based ordering logic, atomic group readiness checks), but no new tests were added. The three existing tests (`run_once_with_no_agents`, `speculative_result_serializes`, `conflict_result_serializes`) predate TASK-028 and test only empty-agent runs and serialization. No test verifies: (a) branches with unsatisfied deps being skipped, (b) branches processed in dependency order, (c) atomic group members treated as a unit, (d) the "remaining branches" skip path, or (e) the F1 scenario (already-merged deps).

- [-] [process-revision-complete] **F3: Speculative merge does not distinguish order-dependent vs order-independent conflicts.**  
  The spec says (merge-dependencies.md §Speculative Merge Integration): *"Report 'this group will conflict if merged in this order' vs. 'these MRs conflict regardless of order'."* The `SpeculativeResult` struct has no field for this distinction. `speculate_branch` reports all conflicts identically — there is no attempt to re-speculate in an alternate order to differentiate the two cases.

- [-] [process-revision-complete] **F4: MCP `gyre_create_mr` tool missing `depends_on` parameter — REST-MCP parity violation.**  
  The REST `POST /api/v1/merge-requests` now accepts an optional `depends_on` field with validation, cycle checking, and lineage dep merging. The MCP `gyre_create_mr` tool schema (`mcp.rs:185-197`) does not include `depends_on` in its `inputSchema`, and `handle_create_mr` (`mcp.rs:714-745`) does not read or process this parameter. MCP callers cannot set creation-time dependencies. This violates HSI §11 MCP-REST parity.

- [-] [process-revision-complete] **F5: `set_dependencies` hardcodes `AgentDeclared` source for all PUT callers.**  
  The spec distinguishes §1 "Explicit: Orchestrator or Agent Declares" from §3 "Agent-Declared: Runtime Discovery" as separate dependency source categories, both using `PUT /api/v1/merge-requests/{id}/dependencies`. The handler (`merge_deps.rs:187`) hardcodes `DependencySource::AgentDeclared` for all callers. When an orchestrator explicitly declares dependencies post-creation (spec §1 example), the dependency is incorrectly labeled as "agent-declared" instead of "explicit." The handler does not extract `AuthenticatedAgent` and therefore cannot differentiate caller types. The task's own implementation plan (§3) states: *"Agent-declared deps via PUT /dependencies get source: AgentDeclared if the caller is an agent JWT, Explicit if human."*

- [-] [process-revision-complete] **F6: Dead variable `speculated_clean` in speculative_merge.rs.**  
  Line 137 creates `let mut speculated_clean: HashSet<String>` and line 195 inserts into it, but the variable is never read anywhere in the function. Only `speculated_mr_ids` is used for dependency satisfaction checks. This is dead code.

---

# TASK-028 Review — R2

**Reviewer:** Verifier  
**Verdict:** `needs-revision` (2 findings)

R1 findings F1–F6 are all correctly resolved. Two new findings in R2:

---

## Findings

- [-] [process-revision-complete] **F7: Aspirational test name — `branch_with_unsatisfied_deps_is_skipped` asserts `Clean`, not `Skipped`.**  
  `speculative_merge.rs:610` — the test name claims to verify that branches with unsatisfied dependencies are skipped, but both branches achieve `SpeculativeStatus::Clean`. The test body's comment confirms the opposite behavior: *"Both should be speculated: branch-a is clean (no deps), branch-b's dep (mr-a) was speculated clean, so branch-b should also be processed."* The test actually verifies that wave-based dependency resolution works (deps ARE satisfied across waves). The skip behavior IS covered by `branch_with_circular_dep_is_skipped` and `dep_on_nonexistent_mr_blocks_branch`, but this test's name falsely inflates perceived coverage of the skip path. The final assertions (`assert_eq!(result_b.unwrap().status, SpeculativeStatus::Clean)`) directly contradict the name's claim of "skipped."  
  **Fix:** Rename to `deps_resolved_across_waves` or `satisfied_deps_allows_speculation`.

- [-] [process-revision-complete] **F8: Conflict type classification logic (R1 F3 fix) has zero behavioral test coverage.**  
  The R1 F3 fix added a 3-branch conditional in `speculate_branch()` (`speculative_merge.rs:318-326`) that classifies conflicts as `OrderDependent` or `OrderIndependent` based on whether the conflicting branch was speculated clean. The only related test (`order_dependent_conflict_serializes` at line 524) manually constructs a `SpeculativeResult` with a hardcoded `conflict_type` — it never calls `speculate_branch` and never exercises the classification logic. The `NoopGitOps` mock always returns `Ok(true)` for `can_merge` (`mem.rs:73-74`), so no conflict scenario is ever triggered in any behavioral test. The 3-branch conditional (`speculated_clean_branches.contains(cb)` → OrderDependent, else → OrderIndependent, no conflicting branch → OrderIndependent) could be entirely broken — e.g., always returning `OrderIndependent` — and all tests would pass.  
  **Fix:** Make `NoopGitOps` configurable to return `Ok(false)` for specific branches (e.g., via `Arc<Mutex<HashSet<String>>>` of "conflicting branches" injected into `test_state`). Then add tests that: (a) speculate branch A clean, then speculate branch B with a conflict against A → verify `conflict_type == Some(OrderDependent)`, (b) speculate branch B with a conflict and no prior speculated branches → verify `conflict_type == Some(OrderIndependent)`.

---

# TASK-028 Review — R3

**Reviewer:** Verifier  
**Verdict:** `needs-revision` (2 findings)

R2 findings F7–F8 are both correctly resolved:
- F7: Test renamed to `deps_resolved_across_waves`, assertions match the name. ✓
- F8: `ConfigurableGitOps` added with configurable conflict branches. Two behavioral tests (`conflict_classified_as_order_independent`, `conflict_classified_as_order_dependent`) call the production function `run_once` and assert on `conflict_type`. Both code paths are covered. ✓

---

## Findings

- [-] [process-revision-complete] **F9: Atomic group deferral path has zero test coverage — readiness check can be deleted without test failure.**  
  `speculative_merge.rs:199-215` checks whether ALL atomic group members have their deps satisfied before processing any member. If any member's deps are unmet, the current member is deferred (`still_remaining.push_back(idx); continue;`). This is the core mechanism satisfying the spec's "Speculate on atomic groups as a unit." However, the only atomic group test (`atomic_group_members_speculated_together`, line 754) sets up a scenario where all deps ARE satisfied — `mr-dep` is always speculated clean before the group members' readiness check runs. The deferral path (lines 211-214) is never executed by any test. The entire 17-line readiness check block can be deleted and all tests still pass. No test verifies: "if one group member has unsatisfied deps, other group members with no deps are also deferred."  
  **Fix:** Add a test: two branches in atomic group "X" — branch g1 has no deps, branch g2 depends on a nonexistent MR (unsatisfiable dep). Assert that g1 is `Skipped` (not `Clean`) — the group can't proceed as a unit because g2's dep is unsatisfied.

- [-] [process-revision-complete] **F10: MCP `handle_create_mr` does not detect or merge branch lineage dependencies — REST-MCP parity gap.**  
  The REST `create_mr` handler (merge_requests.rs:346-374) calls `detect_lineage_deps()` to auto-detect branch lineage dependencies, then merges them with explicit deps (explicit takes precedence, lineage adds to the set). The MCP `handle_create_mr` (mcp.rs:746-803) validates and cycle-checks explicit deps but never calls `detect_lineage_deps()` or performs the merge. The acceptance criterion (line 115) says "Creation-time dependencies are validated (exist, no cycle) and merged with lineage deps." The MCP handler was explicitly modified as part of TASK-028 (R1 F4 fix added `depends_on` to the MCP tool), putting it in scope. An MR created via MCP in a branch lineage scenario (e.g., `feat/use-storage-port` based on `feat/add-storage-port`) will NOT get the auto-detected `branch-lineage` dependency that the same MR created via REST would get, creating inconsistent merge ordering depending on the creation method. This violates HSI §11 REST-MCP parity for the feature TASK-028 introduced.  
  **Fix:** In `handle_create_mr`, look up the repo via `state.repos.find_by_id()` to get `repo_path`, call `detect_lineage_deps()`, and merge with explicit deps using the same logic as the REST handler (merge_requests.rs:359-374).
