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
commits: ["86415e4bbe32a0e16de22cc5d9e08a43442c788d", "147d82592bbd4feb5ac1d299de9d124d0f144366", "de294c12051b3c1fe1e295c9847ffc0608dd29e8", "f5df0c01b81a0e72ff454f34a6ac6ffa1620778a", "336dd201779aed45093e5b6fe49cbb6ab9bea85e", "2a6911885e0fbefa33c17ed912003f9ead0a3f52", "fc7939f7d647bb52b35fe53abed237739886b0f6", "30cc63e7bc35165d3841872c54cdf1492e880840", "d01c07d6fb7219f12157b3b49faf85d471cdc833", "79b06a7ae27ba9f1da101b8f824ecb08c831cca1", "7f9f113cf2c00c7ce2366d1204a1ed75893fe97d", "e62a9d7e8943ad0ece7495c2ae9b8f3e5ad57d78", "0a13a2ee8fd20987829dab377cebbe75e2edc768", "e2153b406a60c0565053e1792731eef3d1d1557c"]
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
Round-4 review repair (review checkpoint `829bba20`, fix commit `86415e4b`
on `pipeline/task-115/813e9bfe4ae14ed197c8bf46b9104450-1`). The single
finding of the round is fixed and verified against both failure modes it
named.

### F1 — duplicate Diesel migration version 20261010000056

Commit `336dd201` had added `2026-10-10-000056_task_spec_ref`, deriving the
same Diesel version as the base's pre-existing
`2026-10-10-000056_meta_spec_approved_content_hash`. Diesel runs only one
migration per version, so on a fresh database the `meta_specs.approved_content_hash`
column was never created and every meta-spec insert failed — the repo's
migration-sequence invariant (AGENTS.md, `scripts/check-migration-versions.sh`)
was violated. Renumbered the directory to the next unused sequence
`2026-10-10-000057_task_spec_ref` (derived version `20261010000057`);
migration body unchanged. Both storage backends embed the same
`migrations/` dir, so SQLite and PostgreSQL pick up the fix together, and a
database that ran the broken duplicate (which recorded `20261010000056` as
the meta_spec migration, the alphabetical first) now runs the renumbered
migration as a new version, creating the missing `tasks.spec_ref` column.

### Test evidence (head `86415e4b`, clean tree)

- Pre-fix reproduction confirmed both named failure modes:
  `check-migration-versions.sh` exit 1 naming the `20261010000056` pair;
  `--lib sqlite::meta_spec` 6 failures (`table meta_specs has no column
  named approved_content_hash`); `no_duplicate_migration_version_prefixes`
  failure on the same pair.
- Post-fix: `check-migration-versions.sh` OK; `check-migration-sql-portability.sh` OK;
  `cargo test -p gyre-adapters --lib sqlite::` → **266 passed, 0 failed**
  (broken-tree baseline: 258 passed / 7 failed), including `sqlite::meta_spec`
  (10 passed), `sqlite::task` (11 passed, round-3 F1 regression intact), and
  the duplicate-version guard; `cargo test -p gyre-server --lib signal_chain`
  → **19 passed, 0 failed**; `check-task-commit-attribution` OK after the
  frontmatter update.
- Evidence: `/tmp/stage/review-evidence/task-115-round4-migration-renumber.txt`
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
no-ledger-SHA hard error, sub-task repo/workspace scoping, round-3
(`147d8259`: TaskRepository::create workspace_id restoration in both task
adapters + `create_persists_workspace_scope`; empty-SHA spec_ref hard-fail
in `decompose_delegation` + `delegation_with_empty_sha_spec_ref_stays_blocked`),
and the rebase repairs (`88e97e0d`, `e4a8e9b8` — import-list-only conflicts
resolved as the union).
