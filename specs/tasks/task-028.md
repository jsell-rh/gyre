# TASK-028: Merge Dependencies — Dependency Metadata and Creation-Time Binding

**Spec reference:** `merge-dependencies.md` §Three Ways Dependencies Are Established, §Speculative Merge Integration  
**Depends on:** None (extends data model and API, independent of queue algorithm)  
**Progress:** `ready-for-review`  
**Review:** [`specs/reviews/task-028.md`](../reviews/task-028.md)

## Spec Excerpt

From `merge-dependencies.md` §Explicit: Orchestrator or Agent Declares:

> ```json
> POST /api/v1/merge-requests
> {
>   "title": "Add user API endpoints",
>   "source_branch": "feat/user-api",
>   "target_branch": "main",
>   "depends_on": ["MR-041"]
> }
> ```

From §Agent-Declared: Runtime Discovery:

> ```json
> PUT /api/v1/merge-requests/MR-042/dependencies
> {
>   "depends_on": ["MR-041"],
>   "reason": "MR-041 adds the UserPort trait that this code implements"
> }
> ```
>
> The reason is recorded for audit and helps the repo orchestrator understand the dependency graph.

From §Auto-Detected: Branch Lineage:

> Auto-detected dependencies are marked as `source: "branch-lineage"` in the dependency record. Agents or the repo orchestrator can override or remove them.

From §Speculative Merge Integration:

> Only speculate on valid merge orderings (don't try merging B before A if B depends on A).
> Speculate on atomic groups as a unit.

## Current State

**MR creation** (`crates/gyre-server/src/api/merge_requests.rs`):
- `CreateMrRequest` does NOT include a `depends_on` field — dependencies can only be set post-creation via `PUT /api/v1/merge-requests/:id/dependencies`
- Branch lineage auto-detection IS implemented via `detect_lineage_deps()` (line 293)

**Dependency metadata:**
- `SetDependenciesRequest` accepts an optional `reason` field (line 32 of `merge_deps.rs`) but it is NOT persisted — the field is ignored
- No `source` field tracking whether a dependency is `explicit`, `branch-lineage`, or `agent-declared`
- Dependencies are stored as a flat `Vec<Id>` on the MergeRequest with no per-dependency metadata

**Speculative merge** (`crates/gyre-server/src/speculative_merge.rs`):
- Runs every 60 seconds, checks all active agent branches
- Does NOT consider dependency ordering — speculates on all branches independently
- Does NOT handle atomic groups as a unit

## Implementation Plan

1. **Add `depends_on` to MR creation:**
   - Extend `CreateMrRequest` in `merge_requests.rs` with `depends_on: Option<Vec<String>>`
   - In `create_mr()`, validate dependencies exist and cycle-check before creating the MR
   - Merge with auto-detected lineage deps (explicit takes precedence, lineage adds to the set)
   - Use the existing `would_create_cycle()` from `merge_deps.rs`

2. **Dependency metadata model:**
   - Add a `MergeRequestDependency` struct to `gyre-domain`:
     ```rust
     pub struct MergeRequestDependency {
         pub target_mr_id: Id,      // the dependency MR
         pub source: DependencySource,  // Explicit | BranchLineage | AgentDeclared
         pub reason: Option<String>,
     }
     ```
   - Add `DependencySource` enum: `Explicit`, `BranchLineage`, `AgentDeclared`
   - Store as JSON in the existing `depends_on` column (extend the serialization from `Vec<Id>` to `Vec<MergeRequestDependency>`)
   - Backward compat: deserialize plain `Vec<String>` as `Explicit` source with no reason

3. **Persist reason and source:**
   - `SetDependenciesRequest` already has `reason` — persist it in the dependency metadata
   - `detect_lineage_deps()` sets `source: BranchLineage` automatically
   - Agent-declared deps via `PUT /dependencies` get `source: AgentDeclared` if the caller is an agent JWT, `Explicit` if human

4. **Expose metadata in responses:**
   - Extend `DependenciesResponse` to include source and reason per dependency:
     ```json
     {
       "mr_id": "MR-042",
       "depends_on": [
         {"mr_id": "MR-041", "source": "branch-lineage", "reason": null},
         {"mr_id": "MR-043", "source": "agent-declared", "reason": "MR-043 adds the UserPort trait"}
       ],
       "dependents": [...]
     }
     ```
   - Extend `QueueGraphResponse` nodes to include source per dependency edge

5. **Speculative merge dependency awareness:**
   - In `speculative_merge.rs`, load the dependency graph for the repo's queued MRs
   - Skip speculating on MR B if MR A (its dependency) hasn't been speculated clean yet
   - Process atomic group members as a single speculative unit
   - Report: distinguish "conflicts regardless of order" from "conflicts only in this order"

6. **Add tests:**
   - MR creation with `depends_on` → dependencies set and cycle-checked
   - Dependency source tracking: explicit, branch-lineage, agent-declared
   - Reason persistence and retrieval
   - Backward compat: old-format `depends_on` deserializes correctly
   - Speculative merge skips MRs with unsatisfied dependency order

## Acceptance Criteria

- [ ] `POST /api/v1/merge-requests` accepts optional `depends_on` field
- [ ] Creation-time dependencies are validated (exist, no cycle) and merged with lineage deps
- [ ] Each dependency records its source (`explicit`, `branch-lineage`, `agent-declared`)
- [ ] Dependency `reason` is persisted and returned in responses
- [ ] `GET /api/v1/merge-requests/:id/dependencies` returns source and reason per dependency
- [ ] Speculative merge respects dependency ordering (does not speculate B before A)
- [ ] Backward compatibility: existing MRs with plain `Vec<Id>` dependencies continue to work
- [ ] Tests cover creation-time deps, source tracking, reason persistence, and speculative merge
- [ ] `cargo test --all` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `merge-dependencies.md` §Three Ways Dependencies Are Established and §Speculative Merge Integration
3. Read `crates/gyre-server/src/api/merge_requests.rs` — focus on `CreateMrRequest` and `create_mr()`
4. Read `crates/gyre-server/src/api/merge_deps.rs` — focus on request/response types and `would_create_cycle()`
5. Read `crates/gyre-domain/src/merge_request.rs` — the MergeRequest domain model
6. Read `crates/gyre-server/src/speculative_merge.rs` — the speculative merge job
7. Extend the domain model, then wire creation-time deps, then metadata, then speculative merge
8. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- `feat(merge-deps): add dependency metadata model and creation-time binding (TASK-028)`
