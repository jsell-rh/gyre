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
commits: ["7f9f113cf2c00c7ce2366d1204a1ed75893fe97d", "e62a9d7e8943ad0ece7495c2ae9b8f3e5ad57d78", "0a13a2ee8fd20987829dab377cebbe75e2edc768", "e2153b406a60c0565053e1792731eef3d1d1557c"]
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

- [x] Spec approval emits `SpecApproved` message on the bus
- [x] `OrchestratorRegistry` with per-workspace and per-repo mutexes
- [x] Workspace orchestrator auto-spawned on `SpecApproved`
- [x] Workspace orchestrator creates delegation tasks for the spec's repo
- [x] Workspace orchestrator creates coordination tasks for dependent repos
- [x] Repo orchestrator auto-spawned on delegation task creation
- [x] Repo orchestrator decomposes spec into ordered sub-tasks
- [x] Task scheduler distinguishes Delegation/Coordination/Implementation task types
- [x] Exactly-one-active semantics enforced for both orchestrator types
- [ ] `cargo test --all` passes (listener-bound tests unverifiable in this sandbox — tcp accept errno 95; host/CI must run the full suite; 1095/1203 lib tests + 442 other-crate tests pass here)

## Shipped

**Phase 1 — SpecApproved emission + interception** (`api/specs.rs`): the approve
handler records the ledger approval, emits the bus message
(`Destination::Workspace`, payload `{repo_id, spec_path, spec_sha, approved_by,
approval_id}` — `workspace_id` added server-side for registry scoping), then
invokes `signal_chain::on_spec_approved` to route the signal.

**Phase 2 — workspace orchestrator** (`signal_chain.rs`): `OrchestratorRegistry`
holds per-workspace and per-repo `Arc<Mutex<()>>` maps behind one `Arc` (stable
lock identity across `AppState` clones) — exactly-one-active processing per
scope. `on_spec_approved` acquires the workspace lock, ensures a live
workspace-orchestrator agent via the task-093 spawn core (`spawn_workspace_orchestrator_core`:
scoped JWT, keypair, budget), delivers the SpecApproved message to its inbox
(`Destination::Agent`, acked by the run), and performs cross-repo coordination:
delegation task (`task_type: Delegation`, `spec_path: path@sha`) for the spec's
repo, coordination tasks (`task_type: Coordination`) for `spec_links`
dependents, and priority-4 `CrossWorkspaceSpecChange` notifications for
cross-workspace dependents' Admins/Owners. Orchestrator transitions to Idle
after processing (spawned-on-demand, not long-lived).

**Phase 3 — repo orchestrator**: `spawn_task_scheduler` (started in `main.rs`,
registered in the job registry for `/healthz` liveness and
`POST /admin/jobs/task_scheduler/run`) runs a 30 s cycle:
`scheduler_run_once` claims Backlog Delegation/Coordination tasks
(Backlog→InProgress claim makes re-entry idempotent), acquires the per-repo
registry lock, spawns the repo orchestrator via `spawn_repo_orchestrator_core`
when none is live, and processes: Delegation → read the approved spec at the
pinned SHA (real `git cat-file` via `read_git_file`), decompose into ordered
Implementation sub-tasks (LLM judgment through `state.llm` when configured —
persona prompt + real spec content, JSON sub-task proposals; deterministic
one-per-`##`-section fallback otherwise), each sub-task carrying
`spec_ref path@sha`, `parent_task_id`, `order`, and `depends_on` chaining the
predecessor; delegation task then transitions Review→Done. Coordination →
impact assessment (LLM or conservative deterministic: one review sub-task),
then Done. `Implementation` and untyped tasks are left Backlog (Phase 4's
worker-spawn path — the task_type discriminator).

**Incidental fix** (owned by this branch because the approve-handler diff
shifted the line past its frozen exemption): `reject_spec`'s spec-rejected
notification fanout fabricated a tenant scope via
`entry.repo_id.unwrap_or("default")` — also a repo/tenant type confusion.
Now resolves the tenant from the workspace record; unresolvable scope skips
the fanout with a warning (the pattern `emit_reconciliation_completed`
established). Exemption entry deleted, `FROZEN_EXEMPTION_COUNT` 7→6.

**Test evidence** (sandbox transport restriction applies —
`tcp_listener_probe: unsupported, errno 95`, see
`/tmp/stage/capabilities.json`; full `cargo test --all` including the
listener-bound `tty::`/`ws::`/`otlp_receiver::`/`git_http::` tests and the
gyre-cli `ws_integration` test must run on host/CI):

- `cargo test -p gyre-server --lib signal_chain` — 10 passed (registry
  per-scope/stable-identity/serialization, full-chain Phase 2 spawn +
  delegation + coordination + inbox delivery + ack, no-workspace no-op,
  second-signal reuse (exactly-one-active, both signals in one inbox),
  Phase 3 ordered decomposition with depends_on chaining + Done,
  coordination processing, untyped-task immunity, MCP create-task
  signal-chain fields).
- `cargo test -p gyre-server --lib api::specs` — 73 passed, including
  `approve_spec_emits_spec_approved_and_triggers_chain` (bus payload shape,
  `Destination::Workspace` routing, orchestrator spawn, delegation task with
  `path@sha`) and the full approval/rejection suite.
- Filtered lib suite (listener-bound modules skipped):
  1095 passed, 0 failed.
- gyre-common/ports/domain/adapters: 344 passed. gyre-cli lib: 94 passed.
- Invariant scripts: all 19 relevant checks OK, including
  `check-fabricated-scope-defaults.sh` after the fix.
- rustfmt clean on changed files; clippy: zero warnings in changed files
  (24 pre-existing in gyre-domain/gyre-common, untouched).

Evidence captured at `/tmp/stage/review-evidence/task-115-evidence.txt`.

## Agent Instructions

Read `specs/system/agent-runtime.md` §1 (all of it, Phases 1-8) for full context. The spec approval handler is in `gyre-server/src/api/specs.rs` — grep for `approve`. Message bus types are in `gyre-common/src/message.rs` (MessageKind::SpecApproved already exists). Agent spawning is in `gyre-server/src/api/spawn.rs`. Task types (Delegation, Coordination, Implementation) are defined in `gyre-domain/src/task.rs`. MCP tools are in `gyre-server/src/mcp.rs`. The existing `domain_events.rs` handles event emission patterns.
