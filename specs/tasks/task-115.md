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
commits: ["f5df0c01b81a0e72ff454f34a6ac6ffa1620778a", "336dd201779aed45093e5b6fe49cbb6ab9bea85e", "fc7939f7d647bb52b35fe53abed237739886b0f6", "30cc63e7bc35165d3841872c54cdf1492e880840", "79b06a7ae27ba9f1da101b8f824ecb08c831cca1", "7f9f113cf2c00c7ce2366d1204a1ed75893fe97d", "e62a9d7e8943ad0ece7495c2ae9b8f3e5ad57d78", "0a13a2ee8fd20987829dab377cebbe75e2edc768", "e2153b406a60c0565053e1792731eef3d1d1557c", "de294c12051b3c1fe1e295c9847ffc0608dd29e8"]
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

Repair round (review finding `52d8b49d`, fix commit `de294c12` on
`pipeline/task-115/86b8a34e24314f26a52dc202f5efbfc4-1`, assignment base
`d81b0fe4`):

### F1 — scheduler double-processing (signal_chain.rs, rig_vertexai.rs)

The pre-fix claim protocol could process one Delegation task twice:
`scheduler_run_once` had no cycle serialization (the 30 s loop plus
`POST /admin/jobs/task_scheduler/run` — whose `JobRegistry::trigger` runs
the handler unserialized — could both claim one Backlog task via the
non-atomic list/transition/update read-modify-write), and its stale
InProgress re-claim (60 s) could re-run a still-live decomposition whose
LLM call was unbounded (`reqwest::Client::new()` in rig_vertexai.rs:103).
Fixed by layering three real guards:

- **Cycle run lock** (`OrchestratorRegistry::scheduler_run_lock`):
  `scheduler_run_once` holds one lock for the whole scan→claim→process
  sequence, so concurrent cycles serialize and never race on the claim;
  a mid-run task can never be re-selected because no cycle scans while
  another executes. The stale InProgress re-selection remains purely as
  crash recovery (task left claimed by a dead process).
- **Re-read after the per-repo mutex** (defense in depth): the claimed
  task is re-fetched after acquiring the repo lock and processed only
  from the freshest snapshot; a task no longer InProgress (completed,
  cancelled, or released by a prior run) is skipped instead of
  re-decomposed.
- **Latency bounds**: `LLM_CALL_TIMEOUT` (120 s `tokio::time::timeout`)
  wraps both orchestrator `predict_json` calls (timeout degrades to the
  same deterministic fallback as an LLM error), and every Vertex AI HTTP
  client is now built with `.timeout(...)` (completion/streaming client
  180 s, token-exchange clients 15 s, metadata server 2 s). All five
  `unbounded-external-http` exemption entries are deleted and
  `FROZEN_EXEMPTION_COUNT` lowered 5→0 — the invariant now passes with
  zero exemptions.

### F2 — false bulk-ack of the orchestrator inbox

`run_workspace_orchestrator` ended with `messages.acknowledge_all`,
recording EVERY inbox message as processed — including Escalations the
stale-agent detector delivers to a live workspace orchestrator, which no
consumer had seen; the documented crash-recovery poll
(`GET /api/v1/agents/:id/messages?acknowledged=false`) then hid them. The
run now acks only the `SpecApproved` message whose payload matches the
signal it consumed (`list_unacked` + per-message `acknowledge`); other
messages stay unacked for their real consumers.

### F3 — decorative read and fail-open empty SHA

- Phase 2 step 1's spec read discarded its result (a git subprocess per
  approval for nothing). The read is now consumed: the delegation task's
  description carries the spec's title/section-count/size summary derived
  from the real blob (`summarize_spec_content`), and an unreadable blob
  is warned, not silently ignored.
- `decompose_delegation`'s ledger fallback mapped an unresolvable SHA to
  `""` and completed the delegation with sub-tasks pinned to a malformed
  `path@` spec_ref — the fail-open class the
  `check-fail-open-ref-resolution` invariant bans for `resolve_ref`. A
  missing ledger SHA is now a hard error: the run fails, the claim is
  released to Blocked, and a later cycle retries once the ledger
  resolves. ("Cannot determine the approved blob" is never "empty blob
  is fine".)

### Phase 3 step 2 — codebase state in decomposition (equivalent effect)

Canonical §Phase 3 step 2 ("Reads the repo's current codebase state (via
MCP tools)") is now implemented with equivalent effect: the repo
orchestrator's decomposition reads the repo's tracked file tree at HEAD
(`read_repo_tree`, real `git ls-tree`, bounded to 200 paths with an
explicit truncation marker) and feeds it to the LLM decomposition prompt
alongside the spec content. Disclosure: the mechanism is a server-side
git read, not MCP-tool mediation — the orchestrator run is server-driven
(likewise task creation goes through the task store rather than the
`task.create` MCP tool, equivalent effect); no autonomous tool loop
exists. With this, every Phase 3 step has a real implementation behind
it, via equivalent-effect mechanisms for the tool-mediated wordings.

### Prior rounds (context)

Phase 1 (`api/specs.rs` approve handler: ledger record + bus
`SpecApproved` emission + server-side routing via
`signal_chain::on_spec_approved`), Phase 2 (`OrchestratorRegistry`
per-workspace/per-repo mutexes, spawn-on-demand workspace orchestrator,
delegation + coordination tasks, cross-workspace p4 notifications), and
Phase 3 (task scheduler, repo orchestrator spawn, spec-pinned
decomposition into ordered Implementation sub-tasks with
`spec_ref`/`parent_task_id`/`order`/`depends_on`, coordination
assessment) — see commit `f5df0c01` and earlier. Prior repair fixed
sub-task repo/workspace scoping (`f5df0c01`) and the
`reject_spec` fabricated-tenant fallback (exemption deleted,
`FROZEN_EXEMPTION_COUNT` 7→6).

### Test evidence (all run fresh this round at `de294c12`)

`cargo test -p gyre-server --lib signal_chain` → **18 passed, 0 failed**
(6 new regression tests; two of them verified to fail deterministically
against the pre-fix code by temporarily reverting the fixes:
`orchestrator_run_does_not_ack_escalations` — escalation falsely acked;
`delegation_without_resolvable_sha_stays_blocked` — task completed with
a malformed empty-SHA spec_ref). New tests:
`concurrent_scheduler_cycles_do_not_double_process` (end-to-end no
duplicate sub-task sets from concurrently entered cycles),
`scheduler_cycles_serialize_on_the_run_lock` (deterministic: a second
cycle blocks while the first holds the run lock — fails if the
acquisition is removed), `stale_claim_crash_recovery_processes_once`
(crash-recovery re-claim processes exactly once),
`delegation_description_carries_spec_summary` (real git repo; the
delegation description carries the title/section count derived from the
read blob), `read_repo_tree_lists_tracked_files` (real git repo;
tracked-file listing, truncation marker, unreadable repo → None).
Related suites: `--lib api::specs` → 74 passed; `--lib
api::orchestrator` → 7 passed; `--lib jobs` → 11 passed; `--lib mcp::`
→ 78 passed; `cargo test -p gyre-adapters --lib llm::` → 23 passed,
0 failed. Invariant gates at `de294c12`: `check-arch.sh`,
`check-fail-open-ref-resolution.sh`, `check-fabricated-scope-defaults.sh`,
`check-scope-literal-defaults.sh`, `check-in-memory-state-stores.sh`,
`check-inert-enforcement.sh`, `check-byte-slice-truncation.sh`,
`check-dead-message-kinds.sh`,
`check-unbounded-external-http.sh` (zero exemptions remaining) — all OK.
Diff-scoped clippy (`check-clippy-diff.py`) and
`check-task-commit-attribution.sh` run at this commit — see evidence
file. Sandbox transport restriction (unchanged from base,
`/tmp/stage/capabilities.json`): listener-bound tests (`tty::`, `ws::`,
`otlp_receiver::`, `git_http::`, gyre-cli `ws_integration`) cannot run
here; `cargo test --all` and exact-head GitHub checks remain for the
verification gate. Evidence:
`/tmp/stage/review-evidence/task-115-evidence.txt`.
