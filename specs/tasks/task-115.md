---
title: "Implement spec approval signal chain & orchestrators"
spec_ref: "agent-runtime.md §1 Agent Lifecycle"
depends_on: []
progress: not-started
coverage_sections:
  - "agent-runtime.md §The Model"
  - "agent-runtime.md §1. Agent Lifecycle"
  - "agent-runtime.md §The Signal Chain"
  - "agent-runtime.md §Phase 1: Spec Approval Triggers Orchestration"
  - "agent-runtime.md §Phase 2: Workspace Orchestrator — Cross-Repo Coordination"
  - "agent-runtime.md §Phase 3: Repo Orchestrator — Task Decomposition"
commits: []
---

## Spec Excerpt

From `agent-runtime.md` §1 Phases 1-3:

**The Signal Chain:** Spec approval is the single trigger for all agent work. The `SpecApproved` event is the universal starting signal.

**Phase 1 — Spec Approval Triggers Orchestration:**
When a human approves a spec (`POST /api/v1/specs/:path/approve`):
1. Records approval in the spec approval ledger
2. Creates a `SpecApproved` message on the message bus (payload: `{repo_id, spec_path, spec_sha, approved_by, approval_id}`)
3. The workspace orchestrator receives the message

**Phase 2 — Workspace Orchestrator:**
An LLM agent with `workspace-orchestrator` persona. Spawned on demand (not long-lived). Its job is cross-repo impact analysis and delegation:
1. Reads approved spec content
2. Queries `spec_links` for cross-repo dependencies
3. Creates a **delegation task** (`task_type: Delegation`) for the spec's repo
4. Creates **coordination tasks** for dependent repos
5. For cross-workspace dependencies: creates priority-4 notifications

The server maintains an **orchestrator registry** per workspace — exactly-one-active semantics via a per-workspace mutex.

**Phase 3 — Repo Orchestrator:**
When a delegation task is created, the task scheduler spawns the repo orchestrator (per-repo mutex for exactly-one-active semantics). The repo orchestrator:
1. Reads delegation task + approved spec
2. Decomposes into ordered sub-tasks via `task.create`
3. Each sub-task has: spec_ref, parent_task_id, order, depends_on
4. Marks delegation task Completed

## Implementation Plan

1. **SpecApproved message emission:**
   - In the spec approval handler (`POST /api/v1/specs/:path/approve`), after recording approval, emit a `SpecApproved` message via the message bus
   - Message uses `Destination::Workspace(workspace_id)` routing
   - Payload: `{repo_id, spec_path, spec_sha, approved_by, approval_id}`

2. **Orchestrator registry:**
   - Add `OrchestratorRegistry` struct to `gyre-server` — tracks active orchestrator agent IDs per workspace and per repo
   - Per-workspace mutex for workspace orchestrator (exactly one active at a time)
   - Per-repo mutex for repo orchestrator
   - If an orchestrator is already active, messages queue in its inbox
   - If none active, spawn one and deliver the message

3. **Workspace orchestrator spawning:**
   - Listen for `SpecApproved` messages (register as a message consumer)
   - On receipt: check if workspace orchestrator is active → if not, spawn one using the `workspace-orchestrator` persona
   - The orchestrator receives the SpecApproved message in its inbox
   - Orchestrator uses MCP tools: `task.create` to create delegation/coordination tasks, `spec_links.query` to check dependencies

4. **Repo orchestrator spawning:**
   - Task scheduler detects new `Delegation` tasks (check `task_type` field)
   - Spawns repo orchestrator with `repo-orchestrator` persona and repo-scoped JWT
   - Per-repo mutex ensures exactly one active
   - Orchestrator reads spec, decomposes into sub-tasks with `order` and `depends_on`

5. **Task scheduler enhancements:**
   - Distinguish `Delegation` tasks (trigger repo orchestrator) from `Implementation` tasks (trigger worker agent)
   - Distinguish `Coordination` tasks (trigger repo orchestrator for impact assessment)
   - Respect `depends_on` and `order` fields for task sequencing

## Acceptance Criteria

- [ ] Spec approval emits `SpecApproved` message on the bus
- [ ] `OrchestratorRegistry` with per-workspace and per-repo mutexes
- [ ] Workspace orchestrator auto-spawned on `SpecApproved`
- [ ] Workspace orchestrator creates delegation tasks for the spec's repo
- [ ] Workspace orchestrator creates coordination tasks for dependent repos
- [ ] Repo orchestrator auto-spawned on delegation task creation
- [ ] Repo orchestrator decomposes spec into ordered sub-tasks
- [ ] Task scheduler distinguishes Delegation/Coordination/Implementation task types
- [ ] Exactly-one-active semantics enforced for both orchestrator types
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/agent-runtime.md` §1 (all of it, Phases 1-8) for full context. The spec approval handler is in `gyre-server/src/api/specs.rs` — grep for `approve`. Message bus types are in `gyre-common/src/message.rs` (MessageKind::SpecApproved already exists). Agent spawning is in `gyre-server/src/api/spawn.rs`. Task types (Delegation, Coordination, Implementation) are defined in `gyre-domain/src/task.rs`. MCP tools are in `gyre-server/src/mcp.rs`. The existing `domain_events.rs` handles event emission patterns.
