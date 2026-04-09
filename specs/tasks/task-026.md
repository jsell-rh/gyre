# TASK-026: Merge Dependencies — Topological Sort and Priority Ordering

**Spec reference:** `merge-dependencies.md` §Merge Queue Integration (Processing Algorithm, Priority Within Dependency Tiers)  
**Depends on:** None (refines existing merge processor)  
**Progress:** `ready-for-review`

## Spec Excerpt

From `merge-dependencies.md` §Merge Queue Integration:

> ```
> LOOP:
>   1. Get all Queued entries
>   2. Build dependency graph from depends_on + atomic_group
>   3. Topological sort (respecting priority within each tier)
>   4. For each entry in order:
>      a. Are all dependencies Merged? If no, skip.
>      b. Are all gates passed? If no, skip.
>      c. Is this part of an atomic group? [...]
>      d. Merge.
>   5. REPEAT
> ```

> **Priority Within Dependency Tiers:** Dependencies define ordering constraints. Within a set of MRs that have no dependencies between them (independent work), priority still applies. Critical MRs jump ahead of Medium MRs, but neither can jump ahead of an MR they depend on.

> **Dependency chain too deep (>10 levels):** Warning to repo orchestrator. Not rejected, but flagged as a decomposition smell.

## Current State

The merge processor (`crates/gyre-server/src/merge_processor.rs` lines 155–173) uses a **naive linear scan**: it iterates all queued entries and takes the first one whose dependencies are satisfied. This:

- Does NOT build a dependency graph or compute topological order
- Does NOT respect priority within independent MRs
- Does NOT warn on deep dependency chains (>10 levels)

The infrastructure for dependencies already exists:
- `dependencies_satisfied()` checks if all `depends_on` MRs have `Merged` status
- `handle_dep_health_issues()` detects closed deps and gate failure escalation
- Cycle detection exists in `crates/gyre-server/src/api/merge_deps.rs`

## Implementation Plan

1. **Build adjacency list from queued entries** — In `process_next()`:
   - Collect all `Queued` entries and their `depends_on` lists
   - Include `atomic_group` implied ordering (group members form a dependency chain)
   - Build a `HashMap<Id, Vec<Id>>` adjacency list

2. **Implement Kahn's algorithm for topological sort:**
   - Compute in-degree for each node
   - Process nodes with in-degree 0 first (no unresolved dependencies)
   - Within each tier (nodes with the same depth), sort by MR priority descending
   - Result: a `Vec<MergeQueueEntry>` in optimal processing order

3. **Process entries in topological order:**
   - Replace the linear scan with iteration over the sorted list
   - For each entry: check `dependencies_satisfied()` (confirms deps are `Merged`, not just queued), check gates, then merge
   - Stop after the first successful merge (existing behavior: one merge per cycle)

4. **Chain depth warning:**
   - During graph construction, compute max chain depth via BFS/DFS from roots
   - If any chain exceeds 10 levels, emit a warning via `tracing::warn!` and create a priority-7 notification for the workspace orchestrator
   - Do NOT reject — this is advisory only

5. **Add tests:**
   - Priority ordering: high-priority MR processes before low-priority when both are independent
   - Dependency ordering: MR with dependency waits even if higher priority
   - Chain depth warning emitted at depth >10
   - Atomic group members ordered correctly within topological sort

## Acceptance Criteria

- [ ] Merge queue processes entries in topological order respecting the dependency DAG
- [ ] Within independent MRs (no dependency relationship), higher priority MRs process first
- [ ] Dependency chains deeper than 10 levels emit a warning notification
- [ ] Existing behavior preserved: closed-dep detection, gate failure escalation, one merge per cycle
- [ ] Tests cover priority ordering, dependency ordering, and depth warning
- [ ] `cargo test --all` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `merge-dependencies.md` §Merge Queue Integration and §Priority Within Dependency Tiers
3. Read `crates/gyre-server/src/merge_processor.rs` — the entire file (focus on `process_next()`)
4. Read `crates/gyre-server/src/api/merge_deps.rs` for existing cycle detection and graph structures
5. Implement the topological sort in `merge_processor.rs`, keeping existing health-check logic
6. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

_(to be filled after commit)_
