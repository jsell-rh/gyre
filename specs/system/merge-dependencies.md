# Merge Request Dependencies

## Problem

When multiple agents work in parallel, their outputs have implicit ordering constraints that nobody tracks. Agent B's code calls a function Agent A hasn't merged yet. The merge queue processes B first because it finished faster. B breaks main. The queue retries, fails, creates a task, the repo orchestrator intervenes. All preventable - we knew B depended on A.

With 8+ concurrent agents, this happens constantly. The merge queue becomes a trial-and-error serializer instead of an intelligent pipeline.

## Solution

MRs can declare dependencies on other MRs. The merge queue respects the dependency graph, processing MRs in topological order. Dependencies can be explicit (declared by agents or the repo orchestrator), auto-detected (from branch lineage), or atomic (must merge together).

## Dependency Model

### MR Domain Extension

```rust
pub struct MergeRequest {
    // ... existing fields ...
    pub depends_on: Vec<Id>,        // MR IDs that must merge before this one
    pub atomic_group: Option<String>, // group ID for atomic merge sets
}
```

### Dependency Rules

1. **Dependencies form a DAG.** Cycles are rejected at declaration time.
2. **A dependent MR cannot be processed** until all its dependencies have status `Merged`.
3. **If a dependency is Closed (abandoned)**, the dependent MR is flagged for reassessment - the dependency may no longer be needed, or the approach may need to change.
4. **Dependencies are transitive for ordering** but not for gating. If A -> B -> C, C waits for both A and B. But C's gates only run against C's diff, not the combined diff.

## Three Ways Dependencies Are Established

### 1. Explicit: Orchestrator or Agent Declares

When the repo orchestrator dispatches related work, it declares dependencies upfront:

```json
POST /api/v1/merge-requests
{
  "title": "Add user API endpoints",
  "source_branch": "feat/user-api",
  "target_branch": "main",
  "depends_on": ["MR-041"]
}
```

Or added after creation:

```json
PUT /api/v1/merge-requests/{id}/dependencies
{
  "depends_on": ["MR-041", "MR-043"]
}
```

This is the primary mechanism. The repo orchestrator knows the task decomposition and can declare ordering at dispatch time.

### 2. Auto-Detected: Branch Lineage

If Agent B's branch was created from Agent A's feature branch (not from main), the forge infers a dependency:

```
main
  \-- feat/add-storage-port  (MR-041, Agent A)
        \-- feat/use-storage-port  (MR-042, Agent B)
```

The forge detects that `feat/use-storage-port` is based on `feat/add-storage-port` (not on main) and auto-creates: MR-042 depends_on MR-041.

Auto-detected dependencies are marked as `source: "branch-lineage"` in the dependency record. Agents or the repo orchestrator can override or remove them.

### 3. Agent-Declared: Runtime Discovery

An agent working on a task realizes it needs something that doesn't exist in main yet. It declares the dependency:

```json
PUT /api/v1/merge-requests/MR-042/dependencies
{
  "depends_on": ["MR-041"],
  "reason": "MR-041 adds the UserPort trait that this code implements"
}
```

The reason is recorded for audit and helps the repo orchestrator understand the dependency graph.

## Atomic Groups

Sometimes MRs must merge together with no gap for other work to slip between them. Examples:
- A database migration and the code that uses the new schema
- A port trait definition and its adapter implementation
- A spec change and the code that implements the change

```json
PUT /api/v1/merge-requests/MR-041/atomic-group
{
  "group": "user-feature",
  "members": ["MR-041", "MR-042"]
}
```

### Atomic Group Rules

1. All members must have all gates passed before any member merges.
2. The merge processor locks the queue and merges all members in dependency order with no interleaving.
3. If any member fails to merge (conflict), the entire group is rolled back and requeued.
4. Atomic groups imply dependency ordering among members (if not already declared).

## Merge Queue Integration

### Processing Algorithm

```
LOOP:
  1. Get all Queued entries
  2. Build dependency graph from depends_on + atomic_group
  3. Topological sort (respecting priority within each tier)
  4. For each entry in order:
     a. Are all dependencies Merged? If no, skip.
     b. Are all gates passed? If no, skip.
     c. Is this part of an atomic group?
        - If yes, are ALL group members ready? If no, skip all.
        - If yes, merge all members in sequence (no interleaving).
     d. Merge.
  5. REPEAT
```

### Priority Within Dependency Tiers

Dependencies define ordering constraints. Within a set of MRs that have no dependencies between them (independent work), priority still applies. Critical MRs jump ahead of Medium MRs, but neither can jump ahead of an MR they depend on.

### Visualization

The forge exposes the dependency graph via API and renders it in the web UI:

```
GET /api/v1/merge-queue/graph
```

Returns a DAG of queued MRs with dependency edges, gate status per node, and atomic group boundaries. The dashboard renders this as a visual pipeline showing what's blocked on what.

## Speculative Merge Integration

The speculative merge system (from forge-advantages spec) uses the dependency graph to improve accuracy:

- Only speculate on valid merge orderings (don't try merging B before A if B depends on A)
- Speculate on atomic groups as a unit
- Report "this group will conflict if merged in this order" vs. "these MRs conflict regardless of order"

## Failure Handling

| Scenario | Forge Action |
|---|---|
| Dependency MR is Closed/abandoned | Flag dependent MR, create task: "dependency MR-041 was closed, reassess MR-042" |
| Dependency MR fails gates repeatedly | Escalate to repo orchestrator: "MR-042 is blocked because its dependency MR-041 can't pass gates" |
| Circular dependency detected at declaration | Reject with error: "cycle detected: MR-041 -> MR-042 -> MR-041" |
| Atomic group member fails merge | Roll back all group members, requeue, notify all authors |
| Dependency chain too deep (>10 levels) | Warning to repo orchestrator. Not rejected, but flagged as a decomposition smell. |

## Orchestrator Integration

Dependencies reduce orchestrator workload:

**Before:** Repo orchestrator serializes dependent work by only dispatching Agent B after Agent A's MR merges. This blocks parallelism.

**After:** Repo orchestrator dispatches both agents in parallel with `depends_on` declared. Both agents work simultaneously. The merge queue handles ordering. The orchestrator's decomposition step includes dependency declarations alongside task descriptions.

The Manager Agent spec (`development/manager-agent.md`) should be updated: when dispatching parallel tasks with dependencies, include `depends_on` in the dispatch parameters.

## API Surface

| Endpoint | Method | Purpose |
|---|---|---|
| `PUT /api/v1/merge-requests/{id}/dependencies` | PUT | Set/update dependencies |
| `GET /api/v1/merge-requests/{id}/dependencies` | GET | List dependencies and dependents |
| `PUT /api/v1/merge-requests/{id}/atomic-group` | PUT | Set atomic group membership |
| `GET /api/v1/merge-queue/graph` | GET | Dependency DAG for the entire queue |
| `DELETE /api/v1/merge-requests/{id}/dependencies/{dep_id}` | DELETE | Remove a specific dependency |

## Relationship to Existing Specs

- **Source Control** (`source-control.md`): Merge queue gains dependency-aware ordering. MR domain model gets `depends_on` and `atomic_group` fields.
- **Forge Advantages** (`forge-advantages.md`): Dependency-aware merge queue is a forge-native capability. External forges rely on third-party tools (Mergify) for basic dependency holds.
- **Agent Gates** (`agent-gates.md`): Gates run per-MR, but the queue only processes an MR after its dependencies pass their gates too.
- **Manager Agent** (`development/manager-agent.md`): DISPATCH step gains `depends_on` parameter. Repo orchestrator declares dependencies at dispatch time instead of serializing work.
- **Database Migrations** (`development/database-migrations.md`): Migration MRs and their consuming code MRs are a natural atomic group.
