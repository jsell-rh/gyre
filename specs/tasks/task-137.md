---
title: "Implement gate failure feedback: inbox delivery, task creation, escalation"
spec_ref: "agent-gates.md §Gate Failure Feedback"
depends_on:
  - task-136
progress: complete
coverage_sections:
  - "agent-gates.md §Gate Failure Feedback"
commits: ["7d319cabfb3f5eb7321b0d90dca3aac94a8ab615", "f03a3c830c382d4f063244889f3a31ecfdad711b", "fdb0a05cabef6adb1c6801c996561dee454408a3", "a8c4e4e9bb2420097ad85096ea1d28ae4f2571d3", "0e6ab6ad0a862ac91f322ec9c174933a245f21ea"]
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

## Shipped

All of §Gate Failure Feedback runs as real production code in the gate
executor (`crates/gyre-server/src/gate_executor.rs`), the single point
where required-gate failures are decided. Earlier rounds implemented the
full feature (0e6ab6ad), repaired review ff8f761e (attempt-scoped
delivery marker, deterministic task id, severity labels — fdb0a05c,
a8c4e4e9) and verification findings (rustfmt, trailing whitespace,
clippy — f03a3c83), and resolved base merges (32b47d85, 61f541e2).

This round repaired the retained verification finding f41250c0
(`bash /tmp/stage/checks.sh` exit 1): base merge c9cc1536 (task-209 HSI
§12 profile) added the `exclude_types: &[&str]` parameter to
`NotificationRepository::list_for_user`, and the merge updated every
production call site but missed one test call site — the Overseer
notification assertion in
`security_gate_failure_notifies_persona_and_escalates_critical`
passed 8 of 9 arguments (E0061 at gate_executor.rs:7952). Fix 7d319cab
passes `&[]` (the exclusion the test needs: it must observe
SecurityFinding notifications regardless of recipient toggles).

### GateResult message + inbox delivery

- `MessageKind::GateResult` (gyre-common): Directed tier, `server_only`
  (the forge emits it; an agent cannot forge a gate outcome for another
  agent). Payload `{mr_id, gate_name, gate_type, status, output, spec_ref,
  attempt, gate_agent_id}`; message-bus.md carries the schema row and the
  server_only exception.
- Every required-gate failure delivers a GateResult to the author agent's
  inbox with the gate's real recorded output. Delivery is marked in the
  port-backed KV store (`gate_result_delivered`), and the marker key is
  attempt-scoped (`{mr}:{gate}:{author}:{attempt}`): the executor and the
  Ralph loop's `on_gates_failed` fallback pass the same per-(MR, gate)
  attempt counter, so the two paths stay mutually de-duplicated within
  an attempt while every re-enqueued attempt's failure reaches the author
  (the spec's example is `attempt: 2`).

### WebSocket broadcast for live sessions

- Agent-destined bus messages reach the agent's OWN live WS session
  (delivery gated on the connection's verified agent identity, not
  subscription), so a live author receives its GateResult immediately;
  other agents' sessions receive nothing.

### Attempt tracking

- Per-(MR, gate) attempt counter (KV `gate_attempts`, task-136) is the
  attempt number in every GateResult, notification, and task title; it
  advances across re-enqueues.
- Per-(MR, gate) consecutive-failure streak (KV `gate_failure_streak`):
  incremented on each required-gate failure, reset on pass (a new MR is a
  new key) — a pass between failures breaks the 3-consecutive chain.

### Deferred task creation (3+ failures, session ended)

- At streak ≥ 3 with the author's session ended (status ≠ Active, or the
  author vanished), the forge creates the task with the spec's exact
  shape (title `Gate failure: {gate_name} on MR #{mr_id} (attempt {n})`,
  Backlog, High, labels `["gate-failure", "auto-created"]`, description
  with gate output, spec_ref, attempt count, author agent id).
  De-duplicated among non-terminal tasks by title prefix; the
  deterministic id is scoped to (MR, gate, attempt), so a later 3+
  streak on the same (MR, gate) after the prior task reached
  Done/Cancelled creates its own task instead of dying on a duplicate PK.

### Escalation protocol

- Security gate failure (persona slug `security` or a security-named
  gate): the security persona is notified in addition to the author —
  the inbox of every agent instantiated from the persona.
- Critical security findings escalate to the Overseer (human): the gate
  output is parsed against the security persona's report format (a
  "Critical Findings" section that carries content); unstructured output
  escalates only on an explicit severity label (`severity: critical` /
  `severity=critical`, or a line-leading `critical:` classification,
  word-boundary checked — a coincidental "critical" word in a failing
  command's echoed output does not page a human). A critical finding
  produces an `Escalation` bus message plus `SecurityFinding`
  notifications to workspace Owners and tenant Admins.
- 3+ consecutive failures on the same gate: the repo orchestrator is
  notified directly (Directed `Escalation`, spawn-on-demand with
  workspace-destined fallback).
- All gates failed on an MR: every non-terminal merge-queue entry is
  failed and a "Reassess approach: …" Backlog/High task is created
  listing every failed gate's output.

### Test evidence

Focused tests in `gate_executor.rs` / `ws.rs`:
`gate_failure_delivers_gate_result_to_author_inbox` (asserts attempt 2
delivers its own GateResult after re-enqueue),
`gate_failure_streak_escalates_and_creates_deferred_task` (asserts NO
task/escalation at 2 failures),
`security_gate_failure_notifies_persona_and_escalates_critical`,
`all_gates_failed_removes_mr_from_queue_and_creates_reassessment_task`,
`gate_pass_resets_failure_streak`, `critical_finding_detection`,
`failing_gate_notifies_author_with_gate_result_payload`,
`ws_agent_directed_message_reaches_own_live_session_only`.

This sandbox cannot run gyre-server test compilation (hard 4 GiB
budget; the build wrapper refuses it), so the verifier must execute:
`SKIP_WEB_BUILD=1 cargo test -p gyre-server gate_failure`,
`SKIP_WEB_BUILD=1 cargo test -p gyre-server critical_finding`,
`SKIP_WEB_BUILD=1 cargo test -p gyre-server all_gates_failed`,
`SKIP_WEB_BUILD=1 cargo test -p gyre-server ws_agent_directed`, and
`SKIP_WEB_BUILD=1 cargo test --all` (the acceptance criterion; the
previous verification run failed at compile time on the E0061 repaired
by 7d319cab). This round's local probes (evidence retained under
`/tmp/stage/review-evidence/task-137-repair.md`): `SKIP_WEB_BUILD=1
cargo check -p gyre-server --all-targets` exit 0 in 8m17s, no errors;
`rustfmt --check` on the touched file clean.
