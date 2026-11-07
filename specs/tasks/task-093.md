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

Verification-repair round 4 (this assignment, candidate `93f9467a`, base `66a5fc67`): the retained checks.sh log failed on two findings, both reproduced locally on a faithful base-plus-candidate merge and both fixed at the root (commit `6f9af628`). (1) `new verification exemptions forbidden`: round 3 had re-anchored the `seed_builtin_policies` warn-continue exemption 728→734, but the controller's exemption delta forbids replacement entries, so a re-anchor can never pass; the fix is the F6 fail-open site itself — `seed_builtin_policies` now returns `anyhow::Result` and propagates create/lookup errors (main.rs aborts startup via `?`; running without the seeded default-deny policy leaves the server unrestricted), idempotency kept via the existence check, and the exemption entry is deleted per the file's own contract. This also cures the line-drift failure mode (the site had already moved to 740 after the base merge, leaving HEAD failing `check-warn-continue-creation` locally). (2) `check-abac-exempt-handlers` FAILED under the upstream verifier (the base's exemption baseline still lists both orchestrator routes, so the handlers are in the checked set): `spawn_workspace_orchestrator` delegated containment to `check_workspace_tenant`, which matches no Rule-1 decision pattern; containment is now inlined in both handlers per the `dismiss_notification` exemplar and the helper is deleted (`spawn_repo_orchestrator`'s previous pass relied on the word "Forbidden" in a comment — now explicit code). Evidence: on the simulated verification merge (first-parent base `66a5fc67`), the exemption delta passes with no new entries, pass 1 (base scripts overlaid) passes all gates including `check-abac-exempt-handlers` OK (88 handlers) and `check-warn-continue-creation` OK with the base's stale exemption file, pass 2 (candidate scripts) passes all gates (86 handlers), rustfmt/clippy changed-lines clean (22 files), whitespace clean; focused tests `abac_middleware` 9/9 (incl. seeding idempotency under the fail-closed signature), `api::orchestrator` 24/24 (incl. cross-tenant Forbidden), `api::spawn` 34/34, `mcp::` 80/80, `auth_integration` 21/21; all test targets compile. The base's route-registry stale-exemption WARN for the two orchestrator routes is inherent to the upstream baseline (base still lists routes the candidate registers) and is non-fatal (exit 0). Sandbox cannot bind a TCP listener (capabilities.json), so no live server smoke test; exact-head GitHub CI remains the host gate.
