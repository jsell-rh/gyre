---
title: "Platform Model Orchestrator Lifecycle Protocol"
spec_ref: "platform-model.md §3 Two-Level Orchestration"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "platform-model.md §3 Two-Level Orchestration"
  - "platform-model.md §3 Workspace Orchestrator"
  - "platform-model.md §3 Repo Orchestrator"
commits: ["decfec1b65384fd70cf6a5bbc10716e0afac302e", "0346b089db211c060ff44f08b80d782b94efc405", "6f9af6286c3188657a6746e277777ab818584d14", "b277fff9697fac6b1284131169a99f970c46d055", "45d6d869ac7408a8557975bcb195478c4edb716e", "c9743839d1b784fb7ce4c46f9d536fdd72e1d197", "d7e588d9ffc25b8a5755dc411e4dba650caa599b", "098fe57564253fa29f4d738b16ae5b8ee32f8eae", "73b1c92efc920a9725e0531219b030c3f3ecc22d", "c158eb6853538d989766f84b3f4b236f7fb73ad7", "c83dc841a282381a36a130853c846de4c13e1195", "d2f3765dc9be6aaf8ef418a1193e8f6b644e73a5", "2ae69e973fce1e3a316205af1f238c3a457bd458", "34baa7370f43cca5bf5aeacfa2e66bca53711aa0"]
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

Product behavior for this task (OrchestratorType domain enum, one-live-per-scope
spawn endpoints with tenant containment, scoped JWTs per tier, stale-detector
auto-restart with budget symmetry and death escalation, MCP orchestrator tool
suite with tier gates) was implemented and review-verified in earlier rounds
(review round-2 verdict: complete — see `specs/reviews/task-093.md`). The prior
assignment for this round was interrupted mid-merge while folding main
(`047d6524`) into the branch, leaving committed conflict markers in four files.

This round finished that merge resolution in commit
`34baa7370f43cca5bf5aeacfa2e66bca53711aa0`:

### What was resolved

- `crates/gyre-cli/src/bootstrap.rs` — main's single-source persona
  definitions (`gyre_domain::BUILTIN_PERSONA_DEFS` re-export) replace the
  branch's private embeds; the CLI consumer already used the domain field
  names.
- `crates/gyre-server/src/abac_middleware.rs` — both sides' route-registry
  additions kept (task-093 orchestrator/spawn + main's
  commits/:sha/signature/verification) and main's drifted-built-in-policy
  refresh seeding adopted.
- `crates/gyre-server/src/api/orchestrator.rs` — adopted main's
  supersede-rename name resolution (a name held by a dead row is released by
  renaming the row `-superseded-N`; a live holder still rejects with 400);
  removed the superseded `unique_default_name` helper and a stray brace from
  the interrupted edit. Test modules are the union: main's task-115
  system-orchestrator heartbeat-refresh regression plus launch/analytics
  tests, alongside this task's revision tests (wrapper tenant containment,
  budget symmetry, disconnect-behavior inheritance, fail/stop death
  handling, escalation payload flag). Respawn tests re-pinned to the
  canonical-name-reuse semantics; the contradictory
  `explicit_name_conflict_still_rejected_when_dead_agent_holds_it` test was
  dropped — supersede coverage for explicit names (and the live-holder 400)
  is pinned by `spawn_frees_name_of_stopped_predecessor`.
- `crates/gyre-server/src/mcp.rs` — test-module union: main's §1
  token-scoping suite (worker/workspace-orchestrator scope gates) plus this
  task's `gyre_message_send` RBAC write-gate tests.

### Test evidence (this sandbox, resolved tree)

- `cargo check -p gyre-server`, `-p gyre-server --tests`,
  `--workspace --all-targets` — all clean, no warnings.
- `cargo test -p gyre-domain` → 555+8 passed; `-p gyre-ports -p gyre-common`
  → 125 passed; `-p gyre-cli` → 221 passed + 1 doc.
- All mechanical gates pass: check-arch, abac-route-registry,
  abac-exempt-handlers, mcp-write-tools, inert-enforcement,
  mem-port-contracts, dead-message-kinds, byte-slice-truncation,
  relative-path-defaults, fail-open-ref-resolution, migration-versions,
  fabricated-scope-defaults, lossy-secret-conversion, scope-literal-defaults,
  forwarded-header-trust, in-memory-state-stores,
  unbounded-external-http, migration-sql-portability, forged-scope-fields.

gyre-server test codegen is blocked by the sandbox build wrapper (4 GiB
budget); the independent verifier must run:
`cargo test -p gyre-server --lib api::orchestrator`,
`cargo test -p gyre-server --lib -- mcp::tests::mcp_scope`,
`cargo test -p gyre-server --lib -- mcp::tests::mcp_message_send`,
`cargo test -p gyre-server --lib stale_agents`, and `cargo test --all`.
