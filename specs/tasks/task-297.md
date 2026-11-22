---
title: "Gate all implementation work on approved specs"
spec_ref: "Spec gap spec-41a92c20354c6a108e6074e3 / approval-single-trigger / 4a3f940495a8"
depends_on: []
progress: ready-for-review
commits: ["d5662b9e702f3691db12a406b3d9e8b5c4363e6f", "569ea13821554e618ee7446aecd69c72a35a8ce7", "6c7ce4091376fc85d674281dc7559842a3bed04e", "52397d1622d72bfe9f63fe42ca85484a124c1ae5", "ae87cf5bb64387c932c827b613d3c8d99afaf2b0", "611bc5ffae121a96eb1becf6c233901e541d881f"]
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

The single-trigger gates (task-creation gate on POST /api/v1/tasks and
MCP `gyre_create_task`, Phase-4 scheduler gate with the
`spec-approval-missing` skip label, direct-spawn 403 on
POST /api/v1/agents/spawn for task-bound implementation workers) shipped
in the earlier rounds of this task and are unchanged.

### This round: review repair 96f8dc53 (coordination-leg stall)

The Phase-4 gate permanently stalled the coordination leg of the
legitimate chain: `run_workspace_orchestrator` created the Coordination
task with `spec_path` only, and `assess_coordination` propagated the
absent `spec_ref` to its Implementation review sub-task, which stayed
Backlog with the skip label forever (UpdateTaskRequest cannot set
spec_ref; nothing re-pins existing tasks). Fixed in `611bc5ff`:

- `signal_chain.rs` `run_workspace_orchestrator`: the Coordination task
  now pins `spec_ref = spec_path@spec_sha` — the approved SOURCE spec
  version, mirroring the delegation leg's pin — so the review sub-task
  inherits an Approved pinned ref and the Phase-4 gate passes for the
  legitimate chain (single-trigger preserved: the pinned spec is the
  approval that triggered the coordination).
- `mcp.rs` `handle_decompose_spec`: resolve-and-pin before creating any
  sub-task — the parent's `spec_ref` when present, else the ledger's
  current SHA resolved from the parent's bare `spec_path` (the same
  fallback `decompose_delegation` applies to the delegation leg); a
  parent whose spec version cannot be pinned (no ledger SHA, or no spec
  reference at all) is refused with a tool error naming the
  single-trigger requirement and no task row is written, rather than
  fabricating permanently-stalled Backlog rows.
- `docs/api-reference.md`: the `gyre_decompose_spec` row documents the
  resolve-and-pin and refusal semantics.

### Test evidence

- `cargo check -p gyre-server --tests` — exit 0 on this branch head
  `611bc5ff` (includes all edited test code); log at
  /tmp/stage/review-evidence/cargo-check-task297-repair.log.
- `rustfmt --check` on both edited Rust files — clean.
- Server test execution remains mechanically deferred to the independent
  verifier (gyre-server test codegen exceeds this sandbox's build
  budget; the build wrapper permits `cargo check` only). Required
  probes: `cargo nextest run -p gyre-server signal_chain::tests` (the
  decomposes test now asserts BOTH the delegation and coordination
  review sub-tasks spawn — at the broken candidate the coordination twin
  stayed Backlog with the skip label indefinitely; the two unchanged
  gate tests `scheduler_skips_unapproved_spec_and_spawns_approved_twin`
  and `scheduler_stops_spawn_when_approval_invalidated_by_push` must
  stay green), `cargo nextest run -p gyre-server mcp::tests` (updated
  `mcp_decompose_spec_creates_subtasks_in_repo_scope`, new
  `mcp_decompose_spec_refuses_unpinnable_parent`, unchanged
  `mcp_create_task_implementation_requires_approved_spec`), then the
  full `cargo nextest run --all` (still outstanding from the previous
  round's fail-fast-cancelled CI run).
- Analysis and probe records:
  /tmp/stage/review-evidence/task-297-review-repair.md.
- No assertion was weakened: every pre-existing assertion remains, and
  the previously scoped Phase-4 assertions now additionally cover the
  coordination twin.
