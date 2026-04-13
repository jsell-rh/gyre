---
title: "Implement gate failure feedback: inbox delivery, task creation, escalation"
spec_ref: "agent-gates.md §Gate Failure Feedback"
depends_on:
  - task-136
progress: not-started
coverage_sections:
  - "agent-gates.md §Gate Failure Feedback"
commits: []
---

## Spec Excerpt

From `agent-gates.md` §Gate Failure Feedback:

**Design Principle:** Speed matters — gate failure is a Ralph loop event, not a separate workflow.

**Agent Inbox Delivery:** Forge delivers `GateResult` message to author agent's inbox:

```json
{
  "type": "GateResult",
  "mr_id": "MR-042",
  "gate_name": "security-review",
  "gate_type": "AgentReview",
  "status": "Failed",
  "output": "ChangesRequested: SQL query in handlers.rs:47 uses string interpolation...",
  "spec_ref": "specs/system/identity-security.md@abc123",
  "attempt": 2,
  "gate_agent_id": "security-gate-7"
}
```

**Delivery Model:**
- In Ralph loop: agent's session typically ended before gates run. Message goes to inbox, read by next session.
- For live sessions: also broadcast via WebSocket for immediate reaction.

**Deferred Task Creation:** Forge creates task when author agent has converged/been stopped AND gate has failed 3+ times on same MR.

**Escalation:**
- Security gate failure: security persona notified. Critical findings escalate to Overseer (human).
- 3+ consecutive failures on same gate: repo orchestrator notified (task may need re-scoping).
- All gates failed: MR removed from merge queue. Task created to reassess approach.

## Implementation Plan

1. **GateResult message type:**
   - Add `GateResult` variant to MessageKind enum in `gyre-common`
   - Fields: mr_id, gate_name, gate_type, status (Passed/Failed), output, spec_ref, attempt, gate_agent_id

2. **Inbox delivery on gate failure:**
   - When a required gate fails, create GateResult message for author agent
   - Deliver to agent's inbox via existing message infrastructure
   - Include full gate output for context

3. **WebSocket broadcast for live agents:**
   - If author agent has an active WebSocket session, broadcast GateResult immediately
   - Agent can react in current session without waiting for next inbox poll

4. **Gate attempt tracking:**
   - Track failure count per gate per MR
   - Increment on each gate failure
   - Reset on gate pass or new MR push

5. **Deferred task creation (3+ failures):**
   - After 3+ failures on same gate for same MR, AND author agent session ended:
   - Create task with title: `"Gate failure: {gate_name} on MR #{mr_id} (attempt {n})"`
   - Status: Backlog, Priority: High, Labels: `["gate-failure", "auto-created"]`
   - Description includes: gate output, spec_ref, attempt count, author agent id

6. **Escalation protocol:**
   - Security gate failure: notify security persona agent
   - Critical security findings: escalate to Overseer via Escalation message
   - 3+ consecutive failures on same gate: notify repo orchestrator
   - All gates failed on an MR: remove from merge queue, create reassessment task

## Acceptance Criteria

- [ ] GateResult MessageKind defined with all required fields
- [ ] Gate failure delivers GateResult to author agent's inbox
- [ ] Live agents receive WebSocket broadcast of GateResult
- [ ] Gate failure attempt count tracked per gate per MR
- [ ] 3+ failures + session ended → deferred task created (Backlog, High priority)
- [ ] Security gate failure → security persona notified
- [ ] Critical security findings → Overseer escalation
- [ ] 3+ consecutive failures → repo orchestrator notified
- [ ] All gates failed → MR removed from queue + reassessment task
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/agent-gates.md` §Gate Failure Feedback (all three subsections: Agent Inbox Delivery, Deferred Task Creation, Escalation). Message infrastructure: `gyre-common/src/protocol.rs` for MessageKind enum, `gyre-server/src/api/messages.rs` for delivery. WebSocket: `gyre-server/src/ws.rs` for broadcast. Task creation: `gyre-server/src/api/tasks.rs`. Escalation: check existing Escalation MessageKind usage. Ralph loop model: `specs/system/agent-runtime.md` §1 for how agents read inbox on session start.
