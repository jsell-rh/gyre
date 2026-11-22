---
title: "Gate all implementation work on approved specs"
spec_ref: "Spec gap spec-41a92c20354c6a108e6074e3 / approval-single-trigger / 4a3f940495a8"
depends_on: []
progress: complete
commits: ["52397d1622d72bfe9f63fe42ca85484a124c1ae5", "6c7ce4091376fc85d674281dc7559842a3bed04e", "569ea13821554e618ee7446aecd69c72a35a8ce7", "d5662b9e702f3691db12a406b3d9e8b5c4363e6f"]
---

## Required behavior

Per agent-runtime.md §1 The Signal Chain: 'There is no other trigger. The SpecApproved event is the universal starting signal.' (1) Task-creation gate: the HTTP task-create handler (crates/gyre-server/src/api/tasks.rs) and the MCP gyre_create_task tool (crates/gyre-server/src/mcp.rs) must reject creating a task with task_type 'implementation' unless its spec_ref resolves to a spec that is currently Approved in the spec approval ledger at exactly the pinned SHA (path@sha); unapproved, Pending, invalidated, or unresolvable spec_ref yields a 4xx error (HTTP) / tool error (MCP) naming the requirement, and no task is created. Delegation and Coordination tasks and untyped (pre-approval push-hook) tasks are unaffected — the orchestrator chain creates them from SpecApproved and pins the approved SHA, so the legitimate chain passes the gate. (2) Scheduler gate (defense in depth): the Phase-4 scan in crates/gyre-server/src/signal_chain.rs must, before claiming a Backlog Implementation task and spawning a worker, verify the task's spec_ref still resolves to an Approved ledger entry at the pinned SHA (an approval invalidated by a later push must stop the spawn); on failure the task stays Backlog with an observable skip label and a logged reason — no agent record, no InProgress transition. (3) Direct-spawn gate: POST /api/v1/agents/spawn must no longer create task-bound implementation worker agents for external callers; the endpoint remains for Phase-8 system-initiated kinds (interrogation via agent_type, and equivalent system-initiated spawns), and a request that would create a worker bound to an Implementation task is rejected 403 for non-scheduler callers. The already-satisfied approval-side behavior (ledger row, SpecApproved emission, modified-spec invalidation to Pending) must be preserved unchanged.

## Acceptance Criteria

- [ ] POST /api/v1/tasks (and MCP gyre_create_task) with task_type=implementation and a spec_ref whose spec has no Approved ledger entry at that SHA returns 4xx and creates no task; the identical request after POST /api/v1/specs/:path/approve succeeds and the created task enters the normal chain.
- [ ] With an Implementation/Backlog task referencing an unapproved spec inserted directly into storage, one scheduler cycle creates zero agent records and leaves the task Backlog, while a twin task referencing an approved spec spawns exactly one worker.
- [ ] POST /api/v1/agents/spawn with the task_id of an Implementation task from a human or agent principal returns 403; spawning an interrogation agent (agent_type=interrogation) still succeeds.

## Verification intent

Insert directly into task storage a Backlog task with task_type=implementation and spec_ref 'specs/x.md@<sha>' where the spec ledger shows approval_status Pending; run one scheduler cycle; assert no agent record exists and the task is not InProgress. At the base this fails: the Phase-4 scan (signal_chain.rs) spawns a worker for ANY Backlog Implementation task and never consults the approval ledger, and gyre_create_task / POST /api/v1/tasks accept task_type=implementation with no approval precondition (gyre_create_task's only pre-check is target validation) — implementation work starts with no human spec approval anywhere in the chain.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/agent-runtime.md`. Source: `678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce`; plan: `6a0d0dc0bb142af9f5ac4d1be1c142f795b9b34555ddd9a114ef1f10f0a59e6d`.

[
  {
    "id": "1bd201625dc5fbe0164ced05",
    "decision": "gap",
    "requirement_quote": "Spec approval is the **single trigger** for all agent work. Every path to implementation goes through human approval:",
    "detail": "Requires that spec approval is the ONLY trigger for agent implementation work ('There is no other trigger. The SpecApproved event is the universal starting signal.'). The approval side is real: the approve handler records the ledger row and emits SpecApproved, modified-spec pushes reset approval_status to Pending (spec_registry sync) and revoke approvals via the post-receive hook, and untyped push-hook tasks never spawn agents. However, the single-trigger property is mechanically violated at the task layer: the MCP tool gyre_create_task and the HTTP task-create handler both accept task_type \"implementation\" with no precondition that an approved spec exists, and the Phase-4 scheduler spawns worker agents for ANY Backlog task carrying task_type Implementation — it never checks that the task derives from an approved spec. POST /api/v1/agents/spawn additionally spawns worker agents directly for authenticated callers. Implementation work can therefore start with no human spec approval anywhere in the chain.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/mcp.rs",
        "quote": "\"enum\": [\"implementation\", \"delegation\", \"coordination\"],",
        "detail": "The gyre_create_task MCP tool schema lets any authorized caller create a task with task_type \"implementation\"; the handler maps it to TaskType::Implementation with no spec-approval precondition, so the universal-approval gate is bypassable at task creation."
      },
      {
        "file": "crates/gyre-server/src/signal_chain.rs",
        "quote": "continue; // Delegation/Coordination → orchestrators; untyped → never spawned",
        "detail": "The Phase-4 scheduler scan spawns workers for every Backlog task with task_type Implementation regardless of how the task was created; the scan checks repo status, ordering, max_agents and budget but never checks that the task references an approved spec."
      },
      {
        "file": "crates/gyre-server/src/api/tasks.rs",
        "quote": "if let Some(tt) = req.task_type {\n        task.task_type = Some(parse_task_type(&tt)?);\n    }",
        "detail": "The HTTP task-create handler accepts task_type directly from the request body with no approval precondition; a task created this way with type \"implementation\" is picked up by the scheduler and spawns an agent."
      },
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "quote": ".route(\"/api/v1/agents/spawn\", post(spawn::spawn_agent))",
        "detail": "A direct agent-spawn REST surface exists for authenticated callers (used legitimately for interrogation agents per Phase 8, but it also spawns task-bound worker agents), so implementation agents can be created without any spec approval event."
      },
      {
        "file": "crates/gyre-server/src/spec_registry.rs",
        "quote": "existing.approval_status = ApprovalStatus::Pending;",
        "detail": "Positive wiring for the modified-spec rule: when a spec push changes the blob SHA, sync_spec_ledger resets approval_status to Pending (auto_invalidate_on_change default true), so a modified spec does need re-approval — that part of the signal chain is enforced."
      }
    ]
  }
]

## Shipped

agent-runtime.md §1's single-trigger property is enforced at the task
layer by three gates (commits `d5662b9e` + `569ea138`, clippy repair
`6c7ce409`); this round repaired the one retained verification failure
(`52397d16`) and reconciled the branch with main (merge `57485c70`,
coverage-row conflict resolved by keeping both the task-297 row-7 gate
note and the incoming task-299 row-8 note).

### Behavior

- **Shared verifier** — `verify_implementation_spec_ref`
  (crates/gyre-server/src/api/gates.rs) resolves a `path@sha` against
  BOTH the durable per-SHA approval row (`verify_spec_ref`, the ledger
  query the merge gates use) and the registry entry (must still be
  Approved with `current_sha` exactly the pinned SHA, so an approval
  invalidated by a later push fails). Fail-closed on both.
- **Task-creation gate** — REST `create_task` (api/tasks.rs) and MCP
  `gyre_create_task` (mcp.rs) reject `task_type: implementation` unless
  the gate passes: 400 InvalidInput naming the single-trigger
  requirement (HTTP) / `SPEC_APPROVAL_REQUIRED` tool error (MCP), no
  task row written. Delegation/Coordination and untyped tasks are
  unaffected; MCP `gyre_decompose_spec` sub-tasks pin the parent's
  spec_ref so the legitimate chain passes.
- **Scheduler gate (defense in depth)** — the Phase-4 scan
  (signal_chain.rs `spawn_implementation_workers`) re-verifies the
  spec_ref before claiming; on failure the task stays Backlog with the
  observable `spec-approval-missing` label and a logged reason — no
  agent record, no InProgress transition.
- **Direct-spawn gate** — POST /api/v1/agents/spawn rejects (403) any
  request that would create a worker bound to an Implementation task
  from a non-scheduler caller; system-principal and
  `agent_type: interrogation` spawns are exempt.
- The approval side (ledger row, SpecApproved emission, modified-spec
  invalidation to Pending) is untouched.

### Verification repair `52397d16`

The retained full-suite run (`cargo test --all` on candidate
`d6f85599`, exit 101) failed exactly one test:
`api::search::tests::search_emits_analytics_event` (400 vs 201 at
search.rs:342). Root cause — fixture drift, not a gate defect: the
task-297 fixture migration seeded a 40-hex-char approval SHA but typed
one extra `efabcd` repetition into the request body's `spec_ref`
literal (46 chars); `verify_spec_ref` correctly rejects a non-40-char
SHA, so the gate returned 400. Fix: the body's `spec_ref` is now
derived from the same `FIXTURE_SPEC_PATH`/`FIXTURE_SPEC_SHA` constants
the seed writes (the spawn.rs fixture idiom), so the pair cannot drift.
A repo-wide scan of every `seed_approved_spec` pair against every
`path@sha` literal in the same file found no other mismatch
(/tmp/stage/review-evidence/search-test-400-rootcause.txt). No
production behavior changed.

### Test evidence

The gate behavior is covered by
`tasks_create_implementation_requires_approved_spec`
(tests/api_integration.rs — real approve flow: 400 before, 201 after
POST /api/v1/specs/:path/approve),
`mcp_create_task_implementation_requires_approved_spec` (mcp.rs),
`scheduler_skips_unapproved_spec_and_spawns_approved_twin` and
`scheduler_stops_spawn_when_approval_invalidated_by_push`
(signal_chain.rs — direct-storage Backlog task, zero agent records,
`spec-approval-missing` label, approved twin spawns one worker), and
`spawn_rejects_implementation_worker_for_external_callers`
(api/spawn.rs — 403 for human/agent principals, interrogation spawn
still 201). In-sandbox probes this round: `cargo check -p gyre-server
--lib` and `cargo check -p gyre-server --tests` both clean (test-module
compile of the fixed fixture included); rustfmt changed-lines clean;
root-cause scan evidence saved under /tmp/stage/review-evidence. Server
test execution exceeds this sandbox's build budget — the verifier must
run `cargo test -p gyre-server --lib api::search::tests::search_emits_analytics_event`
plus the suites listed above (full `cargo test --all` remains the
deterministic gate).
