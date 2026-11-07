---
title: "Platform Model Orchestrator Lifecycle Protocol"
spec_ref: "platform-model.md §3 Two-Level Orchestration"
depends_on: [task-220]
progress: ready-for-review
coverage_sections:
  - "platform-model.md §3 Two-Level Orchestration"
  - "platform-model.md §3 Workspace Orchestrator"
  - "platform-model.md §3 Repo Orchestrator"
commits: ["6f9af6286c3188657a6746e277777ab818584d14", "b277fff9697fac6b1284131169a99f970c46d055", "45d6d869ac7408a8557975bcb195478c4edb716e", "c9743839d1b784fb7ce4c46f9d536fdd72e1d197", "d7e588d9ffc25b8a5755dc411e4dba650caa599b", "098fe57564253fa29f4d738b16ae5b8ee32f8eae", "73b1c92efc920a9725e0531219b030c3f3ecc22d", "c158eb6853538d989766f84b3f4b236f7fb73ad7", "c83dc841a282381a36a130853c846de4c13e1195", "d2f3765dc9be6aaf8ef418a1193e8f6b644e73a5", "2ae69e973fce1e3a316205af1f238c3a457bd458"]
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

Two-level orchestrator lifecycle per platform-model.md §3: `OrchestratorType` on agents (migration `2026-09-30-000052`), one-live-per-scope spawn endpoints `POST /api/v1/workspaces/:id/orchestrator/spawn` and `POST /api/v1/repos/:id/orchestrator/spawn` (REST + MCP via shared `_core` fns, 409 on live conflict, inline tenant containment in both handlers — load the entity, compare `auth.tenant_id`, `Forbidden` on mismatch — plus `check_repo_abac` on the repo route, budget gate, default-name respawn that auto-suffixes past dead tombstones while explicit names keep the strict 400), workspace-/repo-scoped orchestrator JWTs, built-in `workspace-orchestrator`/`repo-orchestrator` personas seeded as approved meta-specs, MCP tool suite with tier gates (`gyre_spawn_repo_orchestrator`, `gyre_list_repo_orchestrators`, `gyre_cross_repo_task` at workspace tier; `gyre_decompose_spec`, `gyre_spawn_worker` at repo tier), and auto-restart + escalation on every terminal path (stale-detector abort, `/fail`, `/stop`) via shared `handle_orchestrator_death` with terminal-state guards in `fail_agent`/`stop_agent`, a one-live-per-scope gate inside `restart_orchestrator`, budget-symmetric replacement, and informational escalation naming the replacement. Reviewed complete through rounds 1–4 (specs/reviews/task-093.md).

Verification round 4 (candidate `93f9467a` on base `66a5fc67`) fixed two retained findings at the root (commit `6f9af628`): `seed_builtin_policies` now returns `anyhow::Result` and propagates create/lookup errors (main.rs aborts startup via `?`), deleting the warn-continue exemption per its own contract, and both orchestrator spawn handlers now inline tenant containment (load the entity, compare `auth.tenant_id`, `Forbidden` on mismatch) instead of delegating to helpers.

Rebase round 5 (this assignment, merge commit `bdb88ef21`): rebased the branch onto the new upstream base `4d6759a0` (task-084 HSI Policies ↔ Trust Level, task-087 span-payload containment, task-099 inert-enforcement extensions). The upstream base had independently landed the same fail-closed `seed_builtin_policies` fix and the same orchestrator/traces containment fixes the candidate shipped, so the 18 conflicted files resolved as: candidate's fixed `orchestrator.rs` (validate_persona returns `Result` and is propagated with `?`; inline tenant containment) + base's fixed `traces.rs` and bound-and-branched `merge_processor.rs` attestation evaluation + merged fail-closed seeding body. Consequence in the frozen exemption baselines (deletions only, per each file's own contract — no new or re-anchored entries): `abac-exempt-handlers-exemptions.txt` drops both orchestrator entries (candidate's fix) and the `traces.rs:150` entry (base's fix), leaving only frozen-baseline legacy debt; `inert-enforcement-exemptions.txt` drops the `merge_processor.rs` entries (base binds and branches on the attestation evaluation) and the `orchestrator.rs` validate_persona entries (candidate's fix), leaving no entries. The two orchestrator routes remain properly registered in the ABAC resolver (the base's stale route-registry exemption lines for them are gone in the merge result). Evidence (this sandbox, exact merge commit): `check-abac-exempt-handlers` OK (86 handlers), `check-inert-enforcement` OK, `check-warn-continue-creation` OK, `check-abac-route-registry` OK, `check-arch` OK, `cargo fmt --check` clean; focused tests `abac_middleware` 10/10, `api::orchestrator` 24/24, `api::spawn` 35/35, `mcp::` 82/82, and integration suites `auth_integration` 21/21, `api_integration` 66/66, `git_integration` 16/16, `e2e_ralph_loop` 1/1, `conversation_integration` 3/3, `explorer_ws_integration` 9/9, `m18_oidc_integration` 8/8, `graph_integration` 37/37 (TCP listeners permitted in this sandbox). Full workspace suites, all-target Clippy, and exact-head GitHub CI remain the verification/publication stages' gates.
