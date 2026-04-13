# Coverage: Merge Request Dependencies

**Spec:** [`system/merge-dependencies.md`](../../system/merge-dependencies.md)
**Last audited:** 2026-04-13 (full audit — bulk reclassification from not-started)
**Coverage:** 15/17 (3 n/a)

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | Problem | 2 | n/a | - | Context/rationale — no implementable requirement. |
| 2 | Solution | 2 | n/a | - | Context/rationale — no implementable requirement. |
| 3 | Dependency Model | 2 | implemented | - | MergeRequestDependency struct in gyre-domain/src/merge_request.rs with target_mr_id, source (DependencySource), optional_note. |
| 4 | MR Domain Extension | 3 | implemented | - | MergeRequest has depends_on: Vec<MergeRequestDependency> and atomic_group: Option<String>. All spec fields present. |
| 5 | Dependency Rules | 3 | implemented | - | Cycle detection and self-dependency rejection in merge_deps.rs handler. |
| 6 | Three Ways Dependencies Are Established | 2 | implemented | - | DependencySource enum has Explicit, BranchLineage, AgentDeclared variants. |
| 7 | 1. Explicit: Orchestrator or Agent Declares | 3 | implemented | - | PUT /api/v1/merge-requests/:id/dependencies endpoint. MCP gyre_create_mr tool accepts depends_on. |
| 8 | 2. Auto-Detected: Branch Lineage | 3 | task-assigned | task-167 | DependencySource::BranchLineage exists as enum variant but no auto-detection logic. No git history analysis at MR creation time to discover parent branches. |
| 9 | 3. Agent-Declared: Runtime Discovery | 3 | implemented | - | Agents can declare dependencies via MCP tool or REST API at runtime. |
| 10 | Atomic Groups | 2 | implemented | - | atomic_group: Option<String> on MergeRequest. Merge processor handles atomic group semantics. |
| 11 | Atomic Group Rules | 3 | implemented | - | merge_processor.rs enforces all-or-nothing merge for atomic groups. |
| 12 | Merge Queue Integration | 2 | implemented | - | merge_processor.rs:50-664: merge_atomic_group() locks queue, validates members, merges in topological order. |
| 13 | Processing Algorithm | 3 | implemented | - | Topological sort of dependencies. deps_satisfied() in speculative_merge.rs checks all required MRs merged. |
| 14 | Priority Within Dependency Tiers | 3 | implemented | - | Priority ordering within dependency-satisfied tiers. Higher priority MRs processed first. |
| 15 | Visualization | 3 | implemented | - | MergeQueueGraph.svelte (308 lines): Interactive DAG with atomic group boundaries, dependency edges, pan/zoom, hover highlighting. GET /api/v1/merge-queue/graph provides data. |
| 16 | Speculative Merge Integration | 2 | implemented | - | speculative_merge.rs integrates dependency awareness. Pre-populates merged deps (F1 fix). Skips MRs with unsatisfied dependencies. |
| 17 | Failure Handling | 2 | implemented | - | rollback_atomic_group() in merge_processor.rs handles failures with full rollback and requeue. AtomicGroupFailed MessageKind emitted. |
| 18 | Orchestrator Integration | 2 | implemented | - | Orchestrator receives dependency-related events (TaskCreated, MrCreated) via message bus. |
| 19 | API Surface | 2 | implemented | - | PUT/GET/DELETE /api/v1/merge-requests/:id/dependencies/:dependency_id. Full CRUD. |
| 20 | Relationship to Existing Specs | 2 | n/a | - | Cross-reference section — no implementable requirement. |
