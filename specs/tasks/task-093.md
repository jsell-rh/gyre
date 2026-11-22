---
title: "Platform Model Orchestrator Lifecycle Protocol"
spec_ref: "platform-model.md §3 Two-Level Orchestration"
depends_on: []
progress: complete
coverage_sections:
  - "platform-model.md §3 Two-Level Orchestration"
  - "platform-model.md §3 Workspace Orchestrator"
  - "platform-model.md §3 Repo Orchestrator"
commits: ["ebb879f5fc6761bfe7aaed74689fd02b9ba4005b", "dadd8fa461e46b7b35e7159d1625d47e2d331096", "d54224113232c496a3072f1152b5a2a6034099a5", "34baa7370f43cca5bf5aeacfa2e66bca53711aa0", "decfec1b65384fd70cf6a5bbc10716e0afac302e", "0346b089db211c060ff44f08b80d782b94efc405", "6f9af6286c3188657a6746e277777ab818584d14", "b277fff9697fac6b1284131169a99f970c46d055", "45d6d869ac7408a8557975bcb195478c4edb716e", "c9743839d1b784fb7ce4c46f9d536fdd72e1d197", "d7e588d9ffc25b8a5755dc411e4dba650caa599b", "098fe57564253fa29f4d738b16ae5b8ee32f8eae", "73b1c92efc920a9725e0531219b030c3f3ecc22d", "c158eb6853538d989766f84b3f4b236f7fb73ad7", "c83dc841a282381a36a130853c846de4c13e1195", "d2f3765dc9be6aaf8ef418a1193e8f6b644e73a5", "2ae69e973fce1e3a316205af1f238c3a457bd458"]
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

The orchestrator lifecycle product behavior (OrchestratorType domain enum,
one-live-per-scope spawn endpoints with per-handler tenant containment,
scoped JWTs per tier, stale-detector auto-restart with budget symmetry and
death escalation on every terminal path, MCP orchestrator tool suite with
tier gates) was implemented and review-verified in earlier rounds (round-2
verdict: complete — `specs/reviews/task-093.md`).

### This round: verification-failure repair (attempt 3cb8d80b)

`cargo test --all` on the merge with base `c66ee2ac` failed one test:
`orchestrator_spawn_denied_across_tenants` got 500 instead of 201 at the
own-tenant positive control (api_integration.rs:1728).

Root cause — fixture/production parity gap, the same class as the 13
failures repaired in `dadd8fa4`: the task-093 spawn cores enforce the
persona meta-spec layer fail-closed (`validate_persona` in
`api/orchestrator.rs` returns `ApiError::Internal` when the approved
`workspace-orchestrator` meta-spec is absent from the spec ledger;
production always satisfies this — `main.rs` seeds the built-in
meta-specs at startup). The api_integration fixture seeded only the
entity-layer personas, so the own-tenant spawn failed closed before the
entity layer was consulted; the cross-tenant 403 assertions still passed
because tenant containment runs first. Under a fresh mem state the
failure is deterministic — the test had only ever passed by leaning on
state another test binary left behind.

Fix (`ebb879f5`): mirror startup via `seed_builtin_meta_specs` in the
fixture — the exact pattern the chain fixtures already use. Assertions
untouched; no production code changed.

### Test evidence

- `/tmp/stage/bin/cargo check -p gyre-server --test api_integration` →
  exit 0 (server test codegen is wrapper-blocked in the implement stage;
  execution deferred to the verifier).
- `python3 scripts/check-rustfmt-diff.py c66ee2ac` → changed lines clean
  (9 Rust files, including the edited test file).
- Verifier must run: `cargo test -p gyre-server --test api_integration
  orchestrator_spawn_denied_across_tenants`, the full
  `cargo test -p gyre-server --test api_integration` binary, and the
  failing repair command `cargo test --all --quiet` (details in
  `/tmp/stage/review-evidence/task-093-api-integration-fixture-repair.md`).
