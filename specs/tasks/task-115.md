---
title: "Implement spec approval signal chain & orchestrators"
spec_ref: "agent-runtime.md §1 Agent Lifecycle"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "agent-runtime.md §The Model"
  - "agent-runtime.md §1. Agent Lifecycle"
  - "agent-runtime.md §The Signal Chain"
  - "agent-runtime.md §Phase 1: Spec Approval Triggers Orchestration"
  - "agent-runtime.md §Phase 2: Workspace Orchestrator — Cross-Repo Coordination"
  - "agent-runtime.md §Phase 3: Repo Orchestrator — Task Decomposition"
commits: ["fc7939f7d647bb52b35fe53abed237739886b0f6", "30cc63e7bc35165d3841872c54cdf1492e880840", "79b06a7ae27ba9f1da101b8f824ecb08c831cca1", "7f9f113cf2c00c7ce2366d1204a1ed75893fe97d", "e62a9d7e8943ad0ece7495c2ae9b8f3e5ad57d78", "0a13a2ee8fd20987829dab377cebbe75e2edc768", "e2153b406a60c0565053e1792731eef3d1d1557c"]
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

## Shipped

Checkpoint-recovery round (2026-10-10, assignment base `a1751da1`, candidate
`4519192e`, HEAD under review `309c7000` = merge of the base into this
branch): product surface at HEAD verified byte-identical to the candidate
(`git diff 4519192e..HEAD -- crates/ scripts/` → empty); all four
attribution commits in HEAD ancestry. No product files changed this round —
this round rebuilt, re-ran the focused probes, and recorded fresh evidence.

Implementation: `7f9f113c` (retained from the checkpointed rounds; product
surface unchanged since).

- **Phase 1** (`api/specs.rs`): `approve_spec` records the ledger approval,
  emits the bus `SpecApproved` message (`Destination::Workspace(ws)`,
  payload `{repo_id, spec_path, spec_sha, approved_by, approval_id}` —
  `workspace_id` added server-side to scope the registry, distinct from the
  bus destination), then invokes `signal_chain::on_spec_approved` to route
  the workspace-destined signal server-side.
- **Phase 2** (`signal_chain.rs`): `OrchestratorRegistry` — per-workspace and
  per-repo `Arc<Mutex<()>>` maps behind one `Arc` (stable lock identity across
  `AppState` clones). `on_spec_approved` acquires the workspace lock, ensures a
  live workspace-orchestrator agent via the task-093 spawn core
  (workspace-scoped JWT, keypair, budget), delivers the message to its inbox
  (`Destination::Agent`, acked), and runs cross-repo coordination: delegation
  task (`task_type: Delegation`, `spec_path: path@sha`) for the spec's repo,
  coordination tasks (`task_type: Coordination`) for `spec_links` dependents,
  priority-4 `CrossWorkspaceSpecChange` notifications for cross-workspace
  dependents' Admins/Owners. Orchestrator transitions to Idle after processing
  (spawned on demand, not long-lived).
- **Phase 3**: `spawn_task_scheduler` (30 s loop, started in `main.rs`, job
  registry tracked for `/healthz` and `POST /admin/jobs/task_scheduler/run`).
  `scheduler_run_once` claims Backlog Delegation/Coordination tasks
  (Backlog→InProgress claim makes re-entry idempotent), acquires the per-repo
  registry lock, spawns the repo orchestrator via the task-093 core when none
  is live, and processes: Delegation → reads the approved spec at the pinned
  SHA (real `git cat-file` via `read_git_file`), decomposes (LLM judgment via
  `state.llm` when configured; deterministic one-per-`##`-section fallback
  without) into ordered Implementation sub-tasks carrying `spec_ref path@sha`,
  `parent_task_id`, `order`, `depends_on` chaining, then marks the delegation
  task Done. Coordination → impact assessment (LLM or conservative
  deterministic), then Done. `Implementation` and untyped tasks stay Backlog —
  Phase 4's worker-spawn path (task-118); `spawn_agent_core` independently
  rejects Delegation/Coordination task types.
- **Incidental fix**: `reject_spec`'s notification fanout fabricated a tenant
  scope via `entry.repo_id.unwrap_or("default")` — also a repo/tenant type
  confusion. Now resolves the tenant from the workspace record, skips+logs
  when unresolvable; exemption entry deleted, `FROZEN_EXEMPTION_COUNT` 7→6.

Test evidence (all run fresh this round at HEAD `309c7000`):
`cargo test -p gyre-server --lib signal_chain` → 10 passed, 0 failed;
`--lib api::specs` → 73 passed, 0 failed (incl.
`approve_spec_emits_spec_approved_and_triggers_chain`); `--lib
api::orchestrator` → 6 passed, 0 failed; `--lib jobs` → 11 passed, 0 failed;
`cargo build -p gyre-server` → exit 0 (15m 16s). `mcp.rs` is untouched by
this branch and the base merge — prior 71/71 applies to identical source.
Attribution gate (`check-task-commit-attribution.sh`) and
`check-fabricated-scope-defaults.sh` → OK. Sandbox transport restriction
recorded (`tcp_listener_probe: unsupported, errno 95`,
`/tmp/stage/capabilities.json`): listener-bound tests (`tty::`, `ws::`,
`otlp_receiver::`, `git_http::`, gyre-cli `ws_integration`) cannot run here —
unchanged from base, must run on host/CI; `cargo test --all` and exact-head
GitHub checks remain for the verification gate. Evidence:
`/tmp/stage/review-evidence/task-115-evidence.txt`.

Contract repair (finding `4ba66d19`, 2026-10-10): the prior round recorded
completion by editing the contract itself — ticking the Acceptance Criteria
checklist, rewriting the last criterion's text, and inserting `## Shipped`
between the criteria and Agent Instructions. Restored all four contract
sections byte-identical to the assignment template; completion is recorded
only in the sanctioned places — frontmatter (`progress:`, `commits:`) and
this end-of-file `## Shipped` section.
