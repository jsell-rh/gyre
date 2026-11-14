---
title: "Implement gate failure feedback: inbox delivery, task creation, escalation"
spec_ref: "agent-gates.md §Gate Failure Feedback"
depends_on:
  - task-136
progress: ready-for-review
coverage_sections:
  - "agent-gates.md §Gate Failure Feedback"
commits: ["a8c4e4e9bb2420097ad85096ea1d28ae4f2571d3", "0e6ab6ad0a862ac91f322ec9c174933a245f21ea"]
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

- [x] GateResult MessageKind defined with all required fields
- [x] Gate failure delivers GateResult to author agent's inbox
- [x] Live agents receive WebSocket broadcast of GateResult
- [x] Gate failure attempt count tracked per gate per MR
- [x] 3+ failures + session ended → deferred task created (Backlog, High priority)
- [x] Security gate failure → security persona notified
- [x] Critical security findings → Overseer escalation
- [x] 3+ consecutive failures → repo orchestrator notified
- [x] All gates failed → MR removed from queue + reassessment task
- [ ] `cargo test --all` passes — gyre-server test compilation is refused by
  the sandbox build wrapper (4 GiB budget); `cargo check --all --tests` is
  green and the required test list is recorded in
  `/tmp/stage/review-evidence/task-137-evidence.md` for the independent
  verifier

## Agent Instructions

Read `specs/system/agent-gates.md` §Gate Failure Feedback (all three subsections: Agent Inbox Delivery, Deferred Task Creation, Escalation). Message infrastructure: `gyre-common/src/protocol.rs` for MessageKind enum, `gyre-server/src/api/messages.rs` for delivery. WebSocket: `gyre-server/src/ws.rs` for broadcast. Task creation: `gyre-server/src/api/tasks.rs`. Escalation: check existing Escalation MessageKind usage. Ralph loop model: `specs/system/agent-runtime.md` §1 for how agents read inbox on session start.

## Shipped

All of §Gate Failure Feedback now runs as real production code in the gate
executor (`crates/gyre-server/src/gate_executor.rs`), which task-136 made
the single point where required-gate failures are decided.

### GateResult message + inbox delivery

- New `MessageKind::GateResult` (gyre-common): Directed tier (signed,
persisted until acked), `server_only` — the forge emits it; an agent cannot
forge a gate outcome for another agent. Wire name `gate_result`; payload
schema `{mr_id, gate_name, gate_type, status, output, spec_ref, attempt,
gate_agent_id}` (`attempt` required, u32). message-bus.md amended with the
kind, schema row, and the server_only exception (inline task-137 note).
- On every required-gate failure, `gate_failure_feedback` delivers a
GateResult to the author agent's inbox (`Destination::Agent`, persisted,
read by the next session) with the gate's real recorded output. Delivery is
marked in the port-backed KV store (`gate_result_delivered`), and the Ralph
loop's `on_gates_failed` fallback emission now uses the same kind/fields
(real gate type + attempt number, previously `GateFailure` with a
placeholder `gate_type` and `iteration`) and the same marker, so the two
paths are mutually de-duplicated — the author reads each failure once.

### WebSocket broadcast for live sessions

- `ws.rs` previously dropped ALL Agent-destined messages from the WS
(`Destination::Agent(_) => false`), which also violated message-bus.md
§WebSocket ("delivered only to the WebSocket connection authenticated with
a JWT whose sub matches id"). Agent-destined bus messages now reach the
agent's OWN live session — delivery gated on the connection's verified
agent identity (per-agent token; user identities and the global token are
excluded), not on subscription. A live author receives its GateResult
immediately; other agents' sessions receive nothing.

### Attempt tracking

- Task-136's per-(MR, gate) attempt counter (KV `gate_attempts`) is the
attempt number reported in every GateResult, notification, and task title
(advances across re-enqueues; task-136 tests unchanged and still green by
inspection of the compile + recorded for the verifier).
- New per-(MR, gate) consecutive-failure streak (KV `gate_failure_streak`):
incremented on each required-gate failure, reset on pass (a new MR is a new
key — "reset on gate pass or new MR push"). This is the escalation
threshold signal, so a pass between failures breaks the 3-consecutive
chain.

### Deferred task creation (3+ failures, session ended)

- At streak ≥ 3, when the author's session has ended (status ≠ Active:
converged/Idle, Stopped, Failed, Dead — or the author vanished), the forge
creates the task with the spec's exact shape: title `Gate failure:
{gate_name} on MR #{mr_id} (attempt {n})`, Backlog, High, labels
`["gate-failure", "auto-created"]`, description carrying the gate output,
spec_ref, attempt count, and author agent id. `task_type` stays None
(informational — the repo orchestrator picks it up in its next OBSERVE
cycle, same rule as push-hook auto-tasks). De-duplicated per (MR, gate)
among non-terminal tasks, with a deterministic task id so concurrent
gate feedback cannot double-create (all adapters upsert on id).

### Escalation protocol

- Security gate failure (persona slug `security` or a security-named gate):
the security persona is notified in addition to the author — the persona
is a template, so its notification surface is the inbox of every agent
instantiated from it (persona_id match, nearest-wins persona resolution
repo → workspace → tenant).
- Critical security findings escalate to the Overseer (human): the gate
output is parsed against the security persona's report format (a
"Critical Findings" section that carries content; unstructured outputs
fall back to a standalone "critical" marker). A critical finding produces
an `Escalation` bus message (severity critical) plus `SecurityFinding`
notifications to the workspace Owners and tenant Admins — the Overseer
resolution surface per user-management.md §Notification Routing.
- 3+ consecutive failures on the same gate: the repo orchestrator is
notified directly (Directed `Escalation` in its inbox via
`ensure_repo_orchestrator_for_failure` — spawn-on-demand with
workspace-destined fallback, same routing as `notify_task_blocked`).
- All gates failed on an MR: every non-terminal merge-queue entry for the
MR is failed (`merge_processor::fail_queue_entry`, now `pub(crate)` —
removal from the queue with the same analytics/terminal semantics as the
processor's own failure path) and a "Reassess approach: all gates failed on
MR #…" Backlog/High task is created listing every failed gate's output.

### Test evidence

New focused tests in `gate_executor.rs` and `ws.rs` (each kills a real bug
if the behavior breaks; list + rationale in
`/tmp/stage/review-evidence/task-137-evidence.md`):
- `gate_failure_delivers_gate_result_to_author_inbox`
- `gate_failure_streak_escalates_and_creates_deferred_task` (asserts NO
  task/escalation at 2 failures — threshold enforcement)
- `security_gate_failure_notifies_persona_and_escalates_critical`
- `all_gates_failed_removes_mr_from_queue_and_creates_reassessment_task`
- `gate_pass_resets_failure_streak`
- `critical_finding_detection` (report-format parsing)
- `ws_agent_directed_message_reaches_own_live_session_only`
- ralph_loop inbox assertions updated to GateResult with real fields

`cargo check --all --tests` green; `cargo test -p gyre-common` green (125
passed). gyre-server test execution is refused by the sandbox build
wrapper (4 GiB budget) — the verifier must run the recorded server test
list plus the standard full gates.
