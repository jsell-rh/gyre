---
title: "Platform Model Orchestrator Lifecycle Protocol"
spec_ref: "platform-model.md §3 Two-Level Orchestration"
depends_on: [task-220]
progress: ready-for-review
coverage_sections:
  - "platform-model.md §3 Two-Level Orchestration"
  - "platform-model.md §3 Workspace Orchestrator"
  - "platform-model.md §3 Repo Orchestrator"
commits: ["b277fff9697fac6b1284131169a99f970c46d055", "45d6d869ac7408a8557975bcb195478c4edb716e", "c9743839d1b784fb7ce4c46f9d536fdd72e1d197", "d7e588d9ffc25b8a5755dc411e4dba650caa599b", "098fe57564253fa29f4d738b16ae5b8ee32f8eae", "73b1c92efc920a9725e0531219b030c3f3ecc22d", "c158eb6853538d989766f84b3f4b236f7fb73ad7", "c83dc841a282381a36a130853c846de4c13e1195", "d2f3765dc9be6aaf8ef418a1193e8f6b644e73a5", "2ae69e973fce1e3a316205af1f238c3a457bd458"]
review: specs/reviews/task-093.md
---

## Spec Excerpt

### Workspace Orchestrator

One per workspace. Sees all repos. Handles cross-repo concerns. Uses the `workspace-orchestrator` persona.

**Responsibilities:**
- Observe cross-repo state (blocked repos, dependency chains, budget usage)
- Resolve cross-repo conflicts (two repos competing for same shared library change)
- Route cross-repo work requests (Repo A needs X from Repo B)
- Enforce the spec escalation protocol
- Allocate budget across repos when contention exists
- Spawn/restart repo orchestrators

**Token scope:** workspace-level. Can read all repos, create cross-repo tasks and MR dependencies, communicate with all repo orchestrators.

### Repo Orchestrator

One per repo. Manages the Ralph loop for its repo. Uses the `repo-orchestrator` persona.

**Responsibilities:**
- Run the Ralph loop: observe, plan, dispatch, monitor, reconcile
- Decompose specs into tasks
- Spawn worker agents with scoped tokens
- Manage the repo's merge queue
- Escalate cross-repo needs to the Workspace Orchestrator

**Token scope:** repo-level. Can spawn agents, manage tasks/MRs, interact with the forge — all within its repo.

## Implementation Plan

1. **Orchestrator agent type in domain model:**
   - Add `OrchestratorType` enum to `Agent` domain entity: `WorkspaceOrchestrator`, `RepoOrchestrator`, `Worker` (default)
   - The `SpawnAgentRequest` already has `agent_type` (used for interrogation) — extend it to support `"workspace-orchestrator"` and `"repo-orchestrator"`
   - Workspace orchestrator gets workspace-scoped JWT; repo orchestrator gets repo-scoped JWT

2. **Workspace orchestrator spawning:**
   - Add `POST /api/v1/workspaces/:id/orchestrator/spawn` endpoint (or extend existing spawn)
   - Validates: only one workspace orchestrator active per workspace at a time
   - Uses `workspace-orchestrator` persona (must exist and be approved)
   - JWT has workspace scope (can read all repos in workspace)
   - Creates appropriate DerivedInput from the workspace's authorization chain

3. **Repo orchestrator spawning:**
   - Workspace orchestrator (or human via API) can spawn repo orchestrators
   - `POST /api/v1/repos/:id/orchestrator/spawn` or via standard spawn with type
   - Validates: only one repo orchestrator active per repo at a time
   - Uses `repo-orchestrator` persona
   - JWT has repo scope

4. **Orchestrator restart on failure:**
   - If a workspace orchestrator's agent dies (stale agent detector), auto-restart it
   - If a repo orchestrator dies, the workspace orchestrator is notified and can restart it
   - Add `restart_on_failure: bool` field to agent config

5. **Orchestrator-specific MCP tools:**
   - Workspace orchestrator: `gyre_spawn_repo_orchestrator`, `gyre_list_repo_orchestrators`, `gyre_cross_repo_task`
   - Repo orchestrator: `gyre_decompose_spec`, `gyre_spawn_worker` (existing tools but with validation)

## Acceptance Criteria

- [x] OrchestratorType enum added to Agent domain
- [x] Workspace orchestrator spawn endpoint validates one-per-workspace
- [x] Workspace orchestrator gets workspace-scoped JWT
- [x] Repo orchestrator spawn validates one-per-repo
- [x] Repo orchestrator gets repo-scoped JWT
- [x] Stale orchestrator auto-restart works
- [x] Workspace orchestrator can spawn repo orchestrators via MCP
- [x] `cargo test --all` passes

## Agent Instructions

Read `specs/system/platform-model.md` §3 "Two-Level Orchestration" for the full spec. The agent spawn flow is in `gyre-server/src/api/spawn.rs`. The existing `agent_type` field handles "interrogation" — extend it. The `workspace-orchestrator` and `repo-orchestrator` personas should be registered as built-in personas (see `gyre-domain/src/policy.rs` builtin_policies pattern). JWT minting is in the spawn flow — workspace-scoped JWTs need workspace_id in claims without repo restriction.

## Shipped

Two-level orchestrator lifecycle per platform-model.md §3: `OrchestratorType` on agents (migration `2026-09-30-000052`), one-live-per-scope spawn endpoints `POST /api/v1/workspaces/:id/orchestrator/spawn` and `POST /api/v1/repos/:id/orchestrator/spawn` (REST + MCP via shared `_core` fns, 409 on live conflict, tenant containment via `check_workspace_tenant`, budget gate, default-name respawn that auto-suffixes past dead tombstones while explicit names keep the strict 400), workspace-/repo-scoped orchestrator JWTs, built-in `workspace-orchestrator`/`repo-orchestrator` personas seeded as approved meta-specs, MCP tool suite with tier gates (`gyre_spawn_repo_orchestrator`, `gyre_list_repo_orchestrators`, `gyre_cross_repo_task` at workspace tier; `gyre_decompose_spec`, `gyre_spawn_worker` at repo tier), and auto-restart + escalation on every terminal path (stale-detector abort, `/fail`, `/stop`) via shared `handle_orchestrator_death` with terminal-state guards in `fail_agent`/`stop_agent`, a one-live-per-scope gate inside `restart_orchestrator`, budget-symmetric replacement, and informational escalation naming the replacement. Reviewed complete through rounds 1–4 (specs/reviews/task-093.md).

Verification-repair round 3 (this assignment, base `1d9547de` merged as `0d149f95`): the retained verification log (candidate `a2cdeaf5`, base `88bd81bf`) failed on exactly two findings, both now resolved on this tree. (1) E0063 at `orchestrator.rs:586`: the base's task-099 approval-provenance work added `approved_content_hash: Option<String>` to `gyre_domain::MetaSpec`; the branch's unapproved-persona kill-test constructs a Pending persona literal and needed the field — set to `None`, matching the field contract (populated only on transition to Approved; a Pending persona has no approval). Commit `b277fff9`. (2) The `check-abac-exempt-handlers` ERROR on `spawn_workspace_orchestrator` and the stale route-registry exemption WARNs from that log were artifacts of the older candidate tree: on the current tree the two orchestrator routes are resolver-registered, the exemption file carries no orchestrator entries, and the handler consults `auth` via `check_workspace_tenant` with a real Forbidden branch — `check-abac-exempt-handlers` (86 handlers), `check-abac-route-registry`, and the warn-continue exemption delta (HEAD vs base identical) all pass. Focused probes after the fix: `api::orchestrator` 24/24 (includes all restart/death/escalation tests), `api::spawn` 34/34, `mcp::` 80/80, `agents::` 7/7, restart-filtered 5/5; rustfmt clean on the changed file; inert-enforcement, MCP-write-tools, task-commit-attribution checks green. Sandbox cannot bind a TCP listener (capabilities.json), so no live server smoke test; exact-head GitHub CI remains the host gate.
