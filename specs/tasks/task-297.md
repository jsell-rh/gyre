---
title: "Gate all implementation work on approved specs"
spec_ref: "Spec gap spec-41a92c20354c6a108e6074e3 / approval-single-trigger / 4a3f940495a8"
depends_on: []
progress: ready-for-review
commits: ["611bc5ffae121a96eb1becf6c233901e541d881f", "ae87cf5bb64387c932c827b613d3c8d99afaf2b0", "52397d1622d72bfe9f63fe42ca85484a124c1ae5", "6c7ce4091376fc85d674281dc7559842a3bed04e", "569ea13821554e618ee7446aecd69c72a35a8ce7", "d5662b9e702f3691db12a406b3d9e8b5c4363e6f", "bb42b05c13f78e387cd5fa3d3f41ac17d4979124"]
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

The single-trigger gates shipped in the earlier rounds of this task and
are unchanged: the task-creation gate on POST /api/v1/tasks and MCP
`gyre_create_task` (approved-ledger resolution at the pinned
path@sha), the Phase-4 scheduler gate with the `spec-approval-missing`
skip label, and the direct-spawn 403 for task-bound implementation
workers on POST /api/v1/agents/spawn.

### This round: CI repair bb42b05c (storage-isolation regression)

The candidate CI run (PR head 01752c5d, nextest run 45014043762)
failed `git_integration::push_records_agent_commit_provenance` at
git_integration.rs:187 — POST /api/v1/repos returned an error body
with no `id` inside the `create_repo` helper. Root cause: task-308's
durable-default change (87e08778) added
`gyre_server::select_in_memory_storage()` to the pre-existing
`start_server` helpers but missed the two state-returning
`start_server_with_state` helpers this task had added in the meantime
(git_integration.rs, auth_integration.rs). Those harnesses built state
with `GYRE_DATABASE_URL` unset — now durable SQLite at the platform
default data path — so every such test process (plus its merge
processor) became a concurrent writer on the operator's real default
database file; the provenance test's repo insert lost the write race
against `merge_queue_auto_merges_mr_and_commit_on_main` on the same
file. Both helpers now opt into in-memory storage exactly like
`start_server` (commit `bb42b05c`, test-harness-only: no product code,
no assertion changed).

### Test evidence

- `cargo check -p gyre-server --tests` — exit 0 on `bb42b05c` (log:
  /tmp/stage/review-evidence/cargo-check-task297-ci-repair.log).
- `python3 scripts/check-rustfmt-diff.py HEAD^1` — exit 0, changed
  lines clean.
- Server test execution remains mechanically deferred to the
  independent verifier (the sandbox build wrapper blocks gyre-server
  test codegen within the model's 4 GiB budget). Required probes:
  `cargo nextest run -p gyre-server git_integration` (the previously
  failing `push_records_agent_commit_provenance` plus both
  `merge_queue_*` twins that shared the default DB),
  `cargo nextest run -p gyre-server auth_integration` (the other fixed
  harness), then the full `cargo nextest run --all` (the failed CI run
  was fail-fast cancelled with 150 tests not run).
- Analysis and probe records:
  /tmp/stage/review-evidence/task-297-ci-repair.md.
- No assertion was weakened; the fix restores the isolation these
tests were written against (they were green before the task-308
  durable default landed in this branch).
