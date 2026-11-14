---
title: "Integrate gate evaluation into merge processor flow"
spec_ref: "agent-gates.md §Merge Processor Integration"
depends_on:
  - task-134
  - task-135
progress: not-started
coverage_sections:
  - "agent-gates.md §Merge Processor Integration"
commits: []
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
