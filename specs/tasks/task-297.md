---
title: "Gate all implementation work on approved specs"
spec_ref: "Spec gap spec-41a92c20354c6a108e6074e3 / approval-single-trigger / 4a3f940495a8"
depends_on: []
progress: ready-for-review
commits: ["569ea13821554e618ee7446aecd69c72a35a8ce7", "d5662b9e702f3691db12a406b3d9e8b5c4363e6f", "6c7ce4091376fc85d674281dc7559842a3bed04e"]
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

Commits `d5662b9e` (pipeline checkpoint: the three gates, the shared
verifier, the scheduler tests) and `569ea138` (gate-hardening repair
and test-fixture migration) implement agent-runtime.md §1's
single-trigger property at the task layer; `6c7ce409` repairs the one
retained verification finding (clippy::question_mark at gates.rs:875)
without behavior change. The approval side (ledger row, SpecApproved
emission, modified-spec invalidation to Pending) is untouched.

### Behavior

- **Shared verifier** — `verify_implementation_spec_ref`
  (crates/gyre-server/src/api/gates.rs) resolves a `path@sha` against
  BOTH the durable per-SHA approval row (`verify_spec_ref`, the same
  ledger query the merge gates use) and the registry entry for the path
  (must still be `Approved` with `current_sha` exactly the pinned SHA,
  so an approval invalidated by a later push fails even if a row
  revocation was ever missed). Fail-closed on both.
- **Task-creation gate** — the REST handler (api/tasks.rs
  `create_task`) and the MCP `gyre_create_task` handler (mcp.rs) reject
  `task_type: implementation` unless the gate passes: 400
  `InvalidInput` naming the single-trigger requirement (HTTP) /
  `SPEC_APPROVAL_REQUIRED` tool error (MCP), no task row written.
  Delegation/Coordination and untyped tasks are unaffected. MCP
  `gyre_decompose_spec` sub-tasks now pin the parent's spec_path +
  spec_ref so the legitimate chain passes the gate.
- **Scheduler gate (defense in depth)** — the Phase-4 scan
  (signal_chain.rs `spawn_implementation_workers`) re-verifies the
  spec_ref before claiming; on failure the task stays Backlog with the
  observable `spec-approval-missing` label and a logged reason — no
  agent record, no InProgress transition. The label is cleared on a
  later successful spawn (e.g. after re-approval), alongside the
  existing `budget-exhausted` flag.
- **Direct-spawn gate** — POST /api/v1/agents/spawn rejects
  (403, naming the requirement) any request that would create a worker
  bound to an Implementation task from a non-scheduler caller
  (human/agent principals; the genuine system principal and
  `agent_type: interrogation` Phase-8 spawns are exempt). The
  in-process scheduler spawns through `spawn_agent_core` directly and
  is unaffected.

### Verification-repair commit `6c7ce409`

The retained verification run (checks.sh, exit 1) flagged exactly one
finding: `clippy::question_mark` at gates.rs:875 on
`verify_implementation_spec_ref`'s first ledger check. Rewriting that
branch as a bare `verify_spec_ref(...)?;` statement would trade the
lint for a `check-inert-enforcement.sh` failure (statement-position
`verify_*` call), so the result is now bound and its reason re-wrapped
with the spec_ref context (`"spec approval ledger check for
'<spec_ref>' failed: {reason}"`) — same fail-closed rejection, same
caller-visible messages (the REST/MCP/scheduler wrappers already name
the single-trigger requirement; no test asserts the inner reason
text). Probes: `cargo clippy -p gyre-server --lib` → api/gates.rs
emits zero warnings (the lib's remaining 112 are pre-existing in
untouched files); `rustfmt --check` and
`check-inert-enforcement.sh` (repo-wide) both pass.

### Test evidence

Tests live with the implementation commits (type-checked in-sandbox;
server test execution is deferred to the independent verifier per the
build-budget constraint — commands recorded in
/tmp/stage/review-evidence):

- `tasks_create_implementation_requires_approved_spec`
  (tests/api_integration.rs) — AC 1 over the real approve endpoint:
  unapproved spec_ref → 400 + no task; after
  POST /api/v1/specs/:path/approve the identical request → 201,
  Backlog, spec_ref pinned. Delegation task creation unaffected.
- `mcp_create_task_implementation_requires_approved_spec` (mcp.rs) —
  AC 1 for the MCP surface: tool error naming
  `SPEC_APPROVAL_REQUIRED`/single trigger, no task row; succeeds after
  approval.
- `scheduler_skips_unapproved_spec_and_spawns_approved_twin` and
  `scheduler_stops_spawn_when_approval_invalidated_by_push`
  (signal_chain.rs) — AC 2 / verification intent: a Backlog
  Implementation task inserted directly into storage against a Pending
  (or push-invalidated) registry entry produces zero agent records and
  stays Backlog with the `spec-approval-missing` label, while the
  approved twin spawns exactly one worker.
- `spawn_rejects_implementation_worker_for_external_callers`
  (api/spawn.rs) — AC 3: human principal (verified Keycloak user JWT)
  → 403 naming the requirement, no agent record, task untouched;
  interrogation spawn from a non-system (agent JWT) principal still
  201s.
- `agent_jwt_cannot_spawn_onto_foreign_task` updated for the new
  contract: the agent-principal spawn is 403 at the direct-spawn gate,
  and the core task-claim boundary keeps a direct
  `spawn_agent_core` probe asserting the `scoped to task` rejection so
  the boundary is not silently untested.

### Fixture migration (behavior-preserving)

Existing suites that created `task_type: implementation` tasks through
the now-gated surfaces were migrated to seed the durable approved-spec
state (the same state POST /api/v1/specs/:path/approve records) and pin
it in the task body: spawn.rs (async `app()` fixture + 39 custom-state
tests), mcp.rs (signal-chain-fields, search/update analytics), admin.rs
(`boundary_task`), search.rs, tests/e2e_ralph_loop.rs,
tests/git_integration.rs, tests/auth_integration.rs,
tests/m18_oidc_integration.rs. `mem::seed_approved_spec` (cfg(test))
seeds the production-parity state.

### Deferred verification (verifier must run)

`cargo test -p gyre-server --lib` (signal_chain, mcp, api::spawn,
api::tasks, api::admin, api::search suites) and
`cargo test -p gyre-server --test api_integration --test e2e_ralph_loop
--test git_integration --test auth_integration --test m18_oidc_integration`
— full command list and evidence in
/tmp/stage/review-evidence/deferred-server-tests.txt.
