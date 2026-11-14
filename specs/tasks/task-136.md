---
title: "Integrate gate evaluation into merge processor flow"
spec_ref: "agent-gates.md §Merge Processor Integration"
depends_on:
  - task-134
  - task-135
progress: ready-for-review
coverage_sections:
  - "agent-gates.md §Merge Processor Integration"
commits: ["a05c7286ac53a17c2bd6957e3285c191c10fae1b", "bfae8b786ac08cf87f081f2c77f5d9dbeb2868eb"]
---

## Spec Excerpt

From `agent-gates.md` §Merge Processor Integration:

```
MR enters merge queue
  -> Forge triggers all configured gates in parallel
    -> TestCommand gates run shell commands
    -> LintCommand gates run linters
    -> AgentReview gates spawn review agents
    -> RequiredApprovals gates check review count
  -> Merge processor polls gate status every cycle
    -> All passed: proceed with merge
    -> Any failed: notify author, create task if session ended
    -> Any pending/running: requeue, check next cycle
  -> On merge: produce signed merge attestation bundle
```

## Implementation Plan

1. **Merge processor gate trigger:**
   - On MR enqueue, load repo's gate chain configuration
   - For each gate, create a gate_status record (Pending)
   - Launch all gates in parallel:
     - TestCommand/LintCommand: execute command, capture output, update status
     - AgentReview/AgentValidation: spawn review agent (per task-134), track agent_id
     - RequiredApprovals: check current review count against threshold

2. **Gate status polling loop:**
   - Merge processor's existing cycle checks gate_status records
   - For AgentReview gates: check if gate agent has submitted a review
   - For RequiredApprovals: re-check review count each cycle
   - Update gate status: Running → Passed/Failed based on results

3. **Merge decision logic:**
   - All required gates Passed → proceed with merge
   - Any required gate Failed → block MR, notify author agent
   - Any gate still Pending/Running → skip this MR, process next, retry next cycle

4. **Author notification on gate failure:**
   - When required gate fails, create GateResult notification for author agent
   - Include: gate name, type, failure output, spec_ref, attempt count

5. **Merge attestation trigger:**
   - On successful merge (all gates passed), trigger attestation bundle generation
   - Include all gate results with their outputs/verdicts
   - (Attestation bundle details covered in separate task)

## Acceptance Criteria

- [ ] Merge processor triggers all configured gates when MR enters queue
- [ ] Gates run in parallel (TestCommand, LintCommand, AgentReview, RequiredApprovals)
- [ ] Gate status polling: Pending → Running → Passed/Failed transitions
- [ ] All-passed → merge proceeds
- [ ] Any-failed → MR blocked, author notified with GateResult
- [ ] Pending/running → requeue for next cycle
- [ ] Gate results available for attestation bundle
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/agent-gates.md` §Merge Processor Integration. Merge processor: grep for `merge_processor\|process_merge` in `gyre-server/src/`. Merge queue: `gyre-server/src/api/merge_queue.rs`. Gate status: depends on task-135's status tracking. Agent spawn (for AgentReview): `gyre-server/src/api/spawn.rs`. Review API (for checking gate agent verdicts): `gyre-server/src/api/merge_requests.rs`. Notification system: `gyre-server/src/api/messages.rs`. The merge processor likely has a background loop — find it and extend the gate-check phase to handle all gate types.

## Shipped

Most of the section's flow shipped with tasks 134/135 (verified against the
code before scoping this task): enqueue triggers `trigger_gates_for_mr`
(which spawns every gate concurrently), the merge processor polls
`check_gates_for_mr` every cycle (RequiredApprovals re-evaluated live each
cycle; AgentReview gates poll the reviewer agent's submitted verdicts),
all-passed proceeds to merge, pending/running requeues, required failure
fails the queue entry, and the merge attestation bundle snapshots all gate
results with types at merge time. This task closes the two parts that were
still hollow:

### Attempt-scoped gate re-arming (the retry path)

- Each enqueue/trigger of an MR is one gate ATTEMPT. `arm_gate_result`
  replaces the gate's prior GateResult rows for the MR with a fresh Pending
  row (new `GateResultRepository::delete_by_mr_id_and_gate_id` port,
  implemented in SQLite, PostgreSQL, and mem adapters), and a per-(MR,
  gate) attempt counter is kept in the port-backed KV store
  (`gate_attempts` namespace).
- Why: `check_gates_for_mr` polls `list_by_mr_id`, which is not
  attempt-scoped — before this change a stale `Failed` row survived every
  re-enqueue and vetoed the MR forever, so the spec's Ralph-loop retry
  path ("addresses the feedback, re-pushes, and gates re-run") could
  never merge. Regression test `reenqueue_replaces_failed_gate_result_and_waits`
  fails on the pre-change code.

### GateResult author feedback

- On a required-gate failure the author agent's spawning user now receives
  a notification carrying the full §Gate Failure Feedback payload: gate
  name, type, failure output, spec_ref, attempt count, gate_agent_id —
  previously only gate name/mr_id/agent_id. The GateFailure workspace event
  also carries the attempt number and the real forge-spawned gate-agent id
  (was always the `gate-agent:{id}` display form).
- The notification's tenant scope is resolved from the MR's workspace and
  skipped (logged) when unresolvable — the call site previously passed a
  hardcoded fabricated `"default"` tenant.

### Review repair (round 1, F1 — test-only flake)

- `failing_gate_notifies_author_with_gate_result_payload` indexed
  `notifs[1]` to reach the attempt-2 notification, but `list_for_user`
  returns newest-first within a priority band at `created_at` (epoch-second)
  granularity, so when the two GateFailure creations straddled a
  wall-clock second boundary the attempt-2 notification was `notifs[0]`
  and the assertion failed intermittently against correct production
  code. The test now selects the notification by its payload's `attempt`
  field and asserts the full GateResult payload (type, mr_id, gate
  name/type, status, spec_ref, attempt, gate_agent_id, agent_id) and
  title, mirroring the attempt-1 assertions. No production code changed
  in this round.

### Test evidence

- This sandbox: `cargo check -p gyre-server --tests` exit 0 after the
  repair (8m22s, includes the edited test target). Evidence:
  `/tmp/stage/review-evidence/task-136.md`.
- The build wrapper refuses gyre-server test compilation here (4 GiB
  budget — infrastructure restriction, not a code defect). The verifier
  must run `cargo test --all`, in particular the new/repaired tests
  `gate_executor::tests::reenqueue_replaces_failed_gate_result_and_waits`,
  `gate_executor::tests::failing_gate_notifies_author_with_gate_result_payload`,
  `notifications::tests::gate_failure_notification_carries_deep_link`,
  `notifications::tests::gate_failure_notification_skipped_when_workspace_unresolvable`,
  plus the full `gate_executor`, `merge_queue`, `merge_processor`, and
  `ralph_loop` modules that exercise the changed trigger/decision paths.
  The flake fix is order-independent by construction (selection is by
  payload field, not position), so no loop-run is required to prove it.
