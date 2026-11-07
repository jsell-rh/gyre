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
commits: ["147d82592bbd4feb5ac1d299de9d124d0f144366", "de294c12051b3c1fe1e295c9847ffc0608dd29e8", "f5df0c01b81a0e72ff454f34a6ac6ffa1620778a", "336dd201779aed45093e5b6fe49cbb6ab9bea85e", "2a6911885e0fbefa33c17ed912003f9ead0a3f52", "fc7939f7d647bb52b35fe53abed237739886b0f6", "30cc63e7bc35165d3841872c54cdf1492e880840", "d01c07d6fb7219f12157b3b49faf85d471cdc833", "79b06a7ae27ba9f1da101b8f824ecb08c831cca1", "7f9f113cf2c00c7ce2366d1204a1ed75893fe97d", "e62a9d7e8943ad0ece7495c2ae9b8f3e5ad57d78", "0a13a2ee8fd20987829dab377cebbe75e2edc768", "e2153b406a60c0565053e1792731eef3d1d1557c"]
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

Round-3 review repair (review checkpoint `3f05f838`, fix commit `147d8259`
on `pipeline/task-115/7f4735991bbd470c82b4084838fdc510-1`, assignment base
`88bd81bf`). Both findings of the round are fixed, and each fix carries a
regression test verified to fail deterministically against the pre-fix code.

### F1 — TaskRepository::create dropped workspace_id (sqlite + postgres task adapters)

Commit `336dd201` had displaced `workspace_id` from `NewTaskRow`, the insert
row construction, and the upsert `DO UPDATE` set in BOTH the SQLite and
Postgres task adapters (regression vs base `9bef21a4`). Because
`tasks.workspace_id` is `TEXT NOT NULL DEFAULT 'default'`, every task INSERT
silently landed in the fabricated `default` workspace scope — invisible to
the mem-adapter-backed server tests. Restored all three spots in both
adapters and added the adapter regression test
`create_persists_workspace_scope` (sqlite), asserting create-persistence,
`list_by_workspace` visibility, and the upsert DO UPDATE scope move.
Kill-verified: with the fix reverted the test fails with
`left: Id("default") / right: Id("ws-42")` — the exact fabricated scope from
the finding.

### F2 — decompose_delegation fail-open on an empty-SHA spec_ref

`decompose_delegation`'s `Some(r) if r.contains('@')` branch split
`"path@"` into `(path, "")` without validating the SHA, fabricated sub-tasks
pinned to the malformed `path@` spec_ref, and completed the delegation Done —
the sibling branch of the prior no-ledger-SHA fix. An empty SHA in spec_ref
(reachable via `POST /api/v1/tasks` or MCP `gyre_create_task`, which do not
shape-validate spec_ref) now hard-fails the run: the claim is released to
Blocked, no sub-tasks are created, and a later cycle retries once the ref is
resolvable. The now-redundant `spec_sha.is_empty()` guard on the blob read
was removed (the SHA is guaranteed non-empty by the resolution above).
Mirrored regression test
`delegation_with_empty_sha_spec_ref_stays_blocked`. Kill-verified: with the
bail reverted the test fails with `left: Done / right: Blocked` — the exact
fail-open completion from the finding.

### Test evidence (head `7d75a98a`, clean tree)

- `cargo test -p gyre-adapters --lib sqlite::task` → **11 passed, 0 failed**
- `cargo test -p gyre-server --lib signal_chain` → **19 passed, 0 failed**
- `--lib api::specs` → 74 passed; `--lib api::orchestrator` → 7 passed;
  `--lib jobs` → 11 passed; `--lib mcp::` → 78 passed;
  `cargo test -p gyre-adapters --lib llm::` → 23 passed, 2 ignored
- `SKIP_WEB_BUILD=1 cargo build -p gyre-adapters -p gyre-server --lib` → clean
- Invariant gates re-run OK: check-arch, check-fail-open-ref-resolution,
  check-fabricated-scope-defaults, check-scope-literal-defaults,
  check-mem-port-contracts, check-in-memory-state-stores,
  check-inert-enforcement, check-unbounded-external-http,
  check-task-commit-attribution, check-dead-message-kinds
- Evidence: `/tmp/stage/review-evidence/task-115-round3-evidence.txt`
- Sandbox transport restriction (unchanged from base,
  `/tmp/stage/capabilities.json`): listener-bound tests (`tty::`, `ws::`,
  `otlp_receiver::`, `git_http::`, gyre-cli `ws_integration`) cannot run
  here; `cargo test --all` and exact-head GitHub checks remain for the
  verification gate.

### Prior rounds (context)

Phase 1 (approve handler: ledger record + bus `SpecApproved` emission +
`signal_chain::on_spec_approved` routing), Phase 2 (`OrchestratorRegistry`
per-workspace/per-repo mutexes, spawn-on-demand workspace orchestrator,
delegation + coordination tasks, cross-workspace p4 notifications), Phase 3
(task scheduler, repo orchestrator spawn, spec-pinned decomposition into
ordered Implementation sub-tasks, coordination assessment). Repair rounds:
scheduler double-processing guards (cycle run lock, re-read after repo
mutex, LLM latency bounds — `de294c12`), false bulk-ack of the orchestrator
inbox, decorative spec read consumed into the delegation description,
no-ledger-SHA hard error, sub-task repo/workspace scoping, and the rebase
repair (`88e97e0d`, import-list-only conflict in `main.rs` resolved as the
union).
