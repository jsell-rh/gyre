# TASK-028 Review — R1

**Reviewer:** Verifier  
**Verdict:** `needs-revision` (6 findings)

---

## Findings

- [ ] **F1: Speculative merge skips branches whose dependencies are already merged.**  
  In `speculative_merge.rs`, the wave algorithm checks `speculated_mr_ids.contains(dep_id)` (line 155) to determine if a dependency is satisfied. `speculated_mr_ids` only contains MR IDs that were speculated clean in the **current run**. If a dependency MR is already merged (its agent is no longer active, no worktree exists), its ID is never added to `speculated_mr_ids`. The dependent branch is incorrectly skipped with "unsatisfied dependency order," even though the dependency is fully satisfied by being merged. Compare with `merge_processor.rs:38-44` which correctly considers a dependency satisfied when `dep_mr.status == MrStatus::Merged`.  
  **Fix:** Before the wave loop, pre-populate `speculated_mr_ids` with the IDs of all MRs referenced in any branch's `dep_mr_ids` that already have status `Merged`.

- [ ] **F2: No tests for speculative merge dependency-aware behavior.**  
  The acceptance criteria require "Tests cover… speculative merge." The speculative merge was significantly rewritten (200+ lines of new wave-based ordering logic, atomic group readiness checks), but no new tests were added. The three existing tests (`run_once_with_no_agents`, `speculative_result_serializes`, `conflict_result_serializes`) predate TASK-028 and test only empty-agent runs and serialization. No test verifies: (a) branches with unsatisfied deps being skipped, (b) branches processed in dependency order, (c) atomic group members treated as a unit, (d) the "remaining branches" skip path, or (e) the F1 scenario (already-merged deps).

- [ ] **F3: Speculative merge does not distinguish order-dependent vs order-independent conflicts.**  
  The spec says (merge-dependencies.md §Speculative Merge Integration): *"Report 'this group will conflict if merged in this order' vs. 'these MRs conflict regardless of order'."* The `SpeculativeResult` struct has no field for this distinction. `speculate_branch` reports all conflicts identically — there is no attempt to re-speculate in an alternate order to differentiate the two cases.

- [ ] **F4: MCP `gyre_create_mr` tool missing `depends_on` parameter — REST-MCP parity violation.**  
  The REST `POST /api/v1/merge-requests` now accepts an optional `depends_on` field with validation, cycle checking, and lineage dep merging. The MCP `gyre_create_mr` tool schema (`mcp.rs:185-197`) does not include `depends_on` in its `inputSchema`, and `handle_create_mr` (`mcp.rs:714-745`) does not read or process this parameter. MCP callers cannot set creation-time dependencies. This violates HSI §11 MCP-REST parity.

- [ ] **F5: `set_dependencies` hardcodes `AgentDeclared` source for all PUT callers.**  
  The spec distinguishes §1 "Explicit: Orchestrator or Agent Declares" from §3 "Agent-Declared: Runtime Discovery" as separate dependency source categories, both using `PUT /api/v1/merge-requests/{id}/dependencies`. The handler (`merge_deps.rs:187`) hardcodes `DependencySource::AgentDeclared` for all callers. When an orchestrator explicitly declares dependencies post-creation (spec §1 example), the dependency is incorrectly labeled as "agent-declared" instead of "explicit." The handler does not extract `AuthenticatedAgent` and therefore cannot differentiate caller types. The task's own implementation plan (§3) states: *"Agent-declared deps via PUT /dependencies get source: AgentDeclared if the caller is an agent JWT, Explicit if human."*

- [ ] **F6: Dead variable `speculated_clean` in speculative_merge.rs.**  
  Line 137 creates `let mut speculated_clean: HashSet<String>` and line 195 inserts into it, but the variable is never read anywhere in the function. Only `speculated_mr_ids` is used for dependency satisfaction checks. This is dead code.
