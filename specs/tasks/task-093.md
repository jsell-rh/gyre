---
title: "Platform Model Orchestrator Lifecycle Protocol"
spec_ref: "platform-model.md §3 Two-Level Orchestration"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "platform-model.md §3 Two-Level Orchestration"
  - "platform-model.md §3 Workspace Orchestrator"
  - "platform-model.md §3 Repo Orchestrator"
commits: ["0346b089db211c060ff44f08b80d782b94efc405", "6f9af6286c3188657a6746e277777ab818584d14", "b277fff9697fac6b1284131169a99f970c46d055", "45d6d869ac7408a8557975bcb195478c4edb716e", "c9743839d1b784fb7ce4c46f9d536fdd72e1d197", "d7e588d9ffc25b8a5755dc411e4dba650caa599b", "098fe57564253fa29f4d738b16ae5b8ee32f8eae", "73b1c92efc920a9725e0531219b030c3f3ecc22d", "c158eb6853538d989766f84b3f4b236f7fb73ad7", "c83dc841a282381a36a130853c846de4c13e1195", "d2f3765dc9be6aaf8ef418a1193e8f6b644e73a5", "2ae69e973fce1e3a316205af1f238c3a457bd458"]
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

Two-level orchestrator lifecycle per platform-model.md §3, shipped and reviewed complete across rounds 1–5 (records: specs/reviews/task-093.md rounds 1–3; later-round records live with the pipeline review branches): `OrchestratorType` on agents (migration `2026-09-30-000052`), one-live-per-scope spawn endpoints `POST /api/v1/workspaces/:id/orchestrator/spawn` and `POST /api/v1/repos/:id/orchestrator/spawn` (REST + MCP via shared `_core` fns, 409 on live conflict, inline tenant containment in both handlers — load the entity, compare `auth.tenant_id`, `Forbidden` on mismatch — plus `check_repo_abac` on the repo route, budget gate, default-name respawn that auto-suffixes past dead tombstones while explicit names keep the strict 400), workspace-/repo-scoped orchestrator JWTs, built-in `workspace-orchestrator`/`repo-orchestrator` personas seeded as approved meta-specs, MCP tool suite with tier gates (`gyre_spawn_repo_orchestrator`, `gyre_list_repo_orchestrators`, `gyre_cross_repo_task` at workspace tier; `gyre_decompose_spec`, `gyre_spawn_worker` at repo tier), fail-closed persona validation (`validate_persona` returns `Result`, propagated with `?`), and auto-restart + escalation on every terminal path (stale-detector abort, `/fail`, `/stop`) via shared `handle_orchestrator_death` with terminal-state guards, a one-live-per-scope gate inside `restart_orchestrator`, budget-symmetric replacement, and informational escalation naming the replacement.

### This repair round (contract restoration + base-deletion reconciliation)

The round-5 review checkpoint rewrote this task file (reverted `progress`, truncated the commit attribution, deleted the Shipped record) — a review cannot change the task contract. This round restored the task's operational records (progress `ready-for-review`, full commit attribution, Shipped) against the assigned contract, which is unchanged. It also reconciled the assignment base `e5df7847`'s file deletions in source (`0346b089`): the unrouted legacy drain-on-read inbox handlers (`api/agent_messages.rs`, root `messages.rs` — dead since message bus Phase 3) lost their module declarations, porting base commit `055a41ae`; the CLI's four persona prompt embeds were retargeted from the deleted `bootstrap/personas/` copies to the canonical `specs/personas/<slug>.md` files (byte-identical content verified); the `rust-callgraph` helper scripts were dropped with the extractor's Rust arm degrading gracefully per its port contract. `web/dist` is executor-restored from the assignment base and untouched here.

### Test evidence (this sandbox, exact head `0346b089`)

`cargo check -p gyre-server --lib` clean (6m15s), `cargo check -p gyre-cli` clean, `cargo test -p gyre-cli --bin gyre` 129/129 (includes the persona-embed tests), `cargo fmt --check` clean, `scripts/check-arch.sh` OK, `scripts/check-relative-path-defaults.sh` OK. Probe log: `/tmp/stage/review-evidence/task-093-repair-round.txt`. Server test compilation exceeds this sandbox's budget and runs in the independent verifier, which must execute: `cargo test -p gyre-server --lib api::orchestrator`, `--lib api::spawn`, `--lib stale_agents`, `--lib mcp::tests`, `--lib abac_middleware`, the integration suites (`auth_integration`, `api_integration`, `git_integration`, `e2e_ralph_loop`, `conversation_integration`, `explorer_ws_integration`, `m18_oidc_integration`, `graph_integration`), full `cargo test --all`, and all-target Clippy.
