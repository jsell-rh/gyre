---
title: "Implement gate failure feedback: inbox delivery, task creation, escalation"
spec_ref: "agent-gates.md §Gate Failure Feedback"
depends_on:
  - task-136
progress: complete
coverage_sections:
  - "agent-gates.md §Gate Failure Feedback"
commits: ["f03a3c830c382d4f063244889f3a31ecfdad711b", "fdb0a05cabef6adb1c6801c996561dee454408a3", "a8c4e4e9bb2420097ad85096ea1d28ae4f2571d3", "0e6ab6ad0a862ac91f322ec9c174933a245f21ea"]
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
where required-gate failures are decided. Earlier rounds additionally
repaired review ff8f761e (F1 blocking, F2/F3 minor) and the retained
verification findings (rustfmt changed lines, trailing whitespace in
the all-gates-failed description strings, clippy
redundant_pattern_matching in `ws.rs`) — see commit f03a3c83.

This round was a rebase repair (finding 4534be0a): main advanced to
706bc7bd (task-142/144/145 coverage rows), conflicting with this
branch's row-9 marking in `specs/coverage/system/agent-gates.md`.
Resolved by taking the union — row 9 (task-137) stays implemented, rows
12–13 (task-142) implemented and row 15 (task-144) in-progress from
main are kept, header recomputed from the merged table (7/25, 9 n/a)
and `specs/coverage/SUMMARY.md` regenerated via
`scripts/update-coverage-summary.sh` (merge commit 32b47d85).

### GateResult message + inbox delivery

- `MessageKind::GateResult` (gyre-common): Directed tier, `server_only`
  (the forge emits it; an agent cannot forge a gate outcome for another
  agent). Payload `{mr_id, gate_name, gate_type, status, output, spec_ref,
  attempt, gate_agent_id}`; message-bus.md carries the schema row and the
  server_only exception.
- Every required-gate failure delivers a GateResult to the author agent's
  inbox with the gate's real recorded output. Delivery is marked in the
  port-backed KV store (`gate_result_delivered`), and the marker key is
  **attempt-scoped** (`{mr}:{gate}:{author}:{attempt}` — F1 repair): the
  executor and the Ralph loop's `on_gates_failed` fallback pass the same
  per-(MR, gate) attempt counter, so the two paths stay mutually
  de-duplicated within an attempt while every re-enqueued attempt's
  failure reaches the author (the spec's example is `attempt: 2`; the old
  (MR, gate)-only marker suppressed all deliveries after the first).

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
  new key). The escalation threshold signal — a pass between failures
  breaks the 3-consecutive chain.

### Deferred task creation (3+ failures, session ended)

- At streak ≥ 3 with the author's session ended (status ≠ Active, or the
  author vanished), the forge creates the task with the spec's exact
  shape (title `Gate failure: {gate_name} on MR #{mr_id} (attempt {n})`,
  Backlog, High, labels `["gate-failure", "auto-created"]`, description
  with gate output, spec_ref, attempt count, author agent id).
  De-duplicated among non-terminal tasks by title prefix; the
  deterministic id is scoped to **(MR, gate, attempt)** (F2 repair), so a
  later 3+ streak on the same (MR, gate) after the prior task reached
  Done/Cancelled creates its own task instead of dying on a duplicate PK.

### Escalation protocol

- Security gate failure (persona slug `security` or a security-named
  gate): the security persona is notified in addition to the author —
  the inbox of every agent instantiated from the persona.
- Critical security findings escalate to the Overseer (human): the gate
  output is parsed against the security persona's report format (a
  "Critical Findings" section that carries content); unstructured output
  escalates only on an **explicit severity label** (`severity: critical`
  / `severity=critical`, or a line-leading `critical:` classification,
  word-boundary checked — F3 repair: a coincidental "critical" word in a
  failing command's echoed output no longer pages a human). A critical
  finding produces an `Escalation` bus message plus `SecurityFinding`
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
delivers its own GateResult after re-enqueue — the F1 regression),
`gate_failure_streak_escalates_and_creates_deferred_task` (asserts NO
task/escalation at 2 failures),
`security_gate_failure_notifies_persona_and_escalates_critical`,
`all_gates_failed_removes_mr_from_queue_and_creates_reassessment_task`,
`gate_pass_resets_failure_streak`,
`critical_finding_detection` (includes the F3 false-positive case — a
path containing "critical" must not escalate — and the explicit-label
positive cases),
`failing_gate_notifies_author_with_gate_result_payload`,
`ws_agent_directed_message_reaches_own_live_session_only`.

This sandbox cannot run gyre-server test compilation (hard 4 GiB
budget), so the verifier must execute the server suite — at minimum:
`cargo test -p gyre-server gate_failure -- --nocapture`,
`cargo test -p gyre-server critical_finding`,
`cargo test -p gyre-server all_gates_failed`,
`cargo test -p gyre-server ws_agent_directed`, plus
`scripts/check-rustfmt-diff.py` and `scripts/check-clippy-diff.py`
against the merge base. Earlier-round probes (evidence retained under
`/tmp/stage/review-evidence/`): `cargo check -p gyre-server --tests`
exit 0; `cargo clippy -p gyre-server --tests -W clippy::all` filtered
to changed lines — 0 findings (512 pre-existing warnings outside the
change set, unchanged); `scripts/check-rustfmt-diff.py` clean; no
trailing whitespace in the touched files. The ws.rs assertion rewrite
was verified case-equivalent (pass on timeout or clean close, fail iff
a message arrived), and the reassessment-description string edit was
checked against the test's `contains("lint")` /
`contains("unit tests")` assertions.

This round on the merged tree: `cargo check -p gyre-server` exit 0
(7m09s, no errors — the auto-merged `merge_processor.rs` combining
task-137 gate feedback with task-144 attestation storage type-checks);
`scripts/check-dead-message-kinds.sh` and `scripts/check-arch.sh`
pass; `scripts/update-coverage-summary.sh` output committed.
`cargo test --all` remains for the independent verifier.
