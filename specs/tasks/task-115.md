---
title: "Implement spec approval signal chain & orchestrators"
spec_ref: "agent-runtime.md §1 Agent Lifecycle"
depends_on: []
progress: complete
coverage_sections:
  - "agent-runtime.md §The Model"
  - "agent-runtime.md §1. Agent Lifecycle"
  - "agent-runtime.md §The Signal Chain"
  - "agent-runtime.md §Phase 1: Spec Approval Triggers Orchestration"
  - "agent-runtime.md §Phase 2: Workspace Orchestrator — Cross-Repo Coordination"
  - "agent-runtime.md §Phase 3: Repo Orchestrator — Task Decomposition"
commits: ["1f50d0dc0574a01176f09d052a6812593f4d73ce", "86415e4bbe32a0e16de22cc5d9e08a43442c788d", "147d82592bbd4feb5ac1d299de9d124d0f144366", "de294c12051b3c1fe1e295c9847ffc0608dd29e8", "f5df0c01b81a0e72ff454f34a6ac6ffa1620778a", "336dd201779aed45093e5b6fe49cbb6ab9bea85e", "2a6911885e0fbefa33c17ed912003f9ead0a3f52", "fc7939f7d647bb52b35fe53abed237739886b0f6", "30cc63e7bc35165d3841872c54cdf1492e880840", "d01c07d6fb7219f12157b3b49faf85d471cdc833", "79b06a7ae27ba9f1da101b8f824ecb08c831cca1", "7f9f113cf2c00c7ce2366d1204a1ed75893fe97d", "e62a9d7e8943ad0ece7495c2ae9b8f3e5ad57d78", "0a13a2ee8fd20987829dab377cebbe75e2edc768", "e2153b406a60c0565053e1792731eef3d1d1557c"]
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
Rebase repair (merge commit `dac3c7e65`, onto base `b5a872087` —
task-108 admin impersonation): auto-merge resolved every file except
one content conflict in `.github/workflows/e2e.yml`, where both sides
pin the e2e job to ubuntu-24.04. Merged the rationale comments into one
— kept the base's E2E run-ID evidence (`42586542960`, browser-install
failure on ubuntu-26.04) and the branch's chromium r1208→r1254
visual-baseline risk — with `runs-on: ubuntu-24.04` unchanged. No
source-code conflicts: the signal chain merged cleanly over task-108's
auth/middleware/spawn changes.

### Test evidence (head `dac3c7e65`)

- `cargo test -p gyre-server --lib` -> **1636 passed, 0 failed**
  (includes all 19 signal_chain tests, the api::orchestrator and
  stale_agents suites, and the base's new impersonation tests).
- `cargo build --all` -> clean.
- Evidence: `/tmp/stage/review-evidence/task-115-rebase-b5a872087.txt`.
- Full workspace suites, architecture checks, all-target Clippy, and
  GitHub CI remain for the verification gate.

### Prior rounds (context)

Phases 1-3 of the signal chain: approve handler (ledger record + bus
`SpecApproved` emission + `signal_chain::on_spec_approved` routing behind
the base's mode-based approval resolution and Approved-transition gate),
`OrchestratorRegistry` per-workspace/per-repo mutexes, spawn-on-demand
workspace orchestrator with delegation + coordination tasks and
cross-workspace p4 notifications, task scheduler distinguishing
Delegation/Coordination/Implementation, repo orchestrator spawn with
spec-pinned decomposition into ordered Implementation sub-tasks. Repair
rounds: scheduler double-processing guards (cycle run lock, re-read after
repo mutex, LLM latency bounds - `de294c12`), false bulk-ack of the
orchestrator inbox, sub-task repo/workspace scoping, round-3 (`147d8259`:
TaskRepository::create workspace_id restoration + empty-SHA spec_ref
hard-fail), round-4 (`86415e4b`: duplicate migration version renumbered
to `2026-10-10-000057_task_spec_ref`), rustfmt gate repair (`1f50d0dc0`),
e2e runner pin (`787510310`, ubuntu-24.04 — Playwright 1.58.2 has no
chromium build for ubuntu-26.04; an upgrade to 1.65.0 was rejected
because it would move chromium r1208→r1254 and risk the committed
visual baselines, which this sandbox cannot revalidate because every
browser CDN is egress-blocked), and earlier rebase repairs (`88e97e0d`,
`e4a8e9b8`, `1325137c8` merging base `fefde2946`), and the rebase onto
base `774737638` (merge `e0408d527`, task-099 spawn-core/persona
changes — signal chain adapted to the `&Arc<AppState>` + 3-tuple
spawn-core contract and fail-closed orchestrator persona resolution).
