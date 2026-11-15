# Review — task-137 (Gate failure feedback: inbox delivery, task creation, escalation)

Spec: `specs/system/agent-gates.md` §Gate Failure Feedback (Agent Inbox Delivery, Deferred: Task Creation, Escalation).
Candidate: `f5943edd` (base `c9cc1536`). Product commits: `0e6ab6ad`, `a8c4e4e9`, `fdb0a05c`, `f03a3c83`, `7d319cab` — all recorded in the task frontmatter.
Verdict: **approved** — no blocking findings; server test execution deferred to the independent verifier (mechanical sandbox restriction, not a code defect).

## Probes run (review sandbox, evidence under /tmp/stage/review-evidence/)

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` → **exit 0** (10m45s cold; verified non-cached re-run prints `Checking gyre-server v0.1.0`, 2m14s, exit 0). This is the probe class that caught the prior round's E0061 (`list_for_user` arity, repaired in `7d319cab` — the Overseer assertion at gate_executor.rs now passes `&[]`, 9 args matching `gyre-ports/src/notification.rs:30`). No rustc warnings.
- `cargo test -p gyre-common` → **125/125 passed** (message-kind schema/`server_only`/tier machinery the new `GateResult` kind rides on).
- Invariant scripts → all OK: `check-arch.sh`, `check-dead-message-kinds.sh` (GateResult has emitters), `check-fabricated-scope-defaults.sh`, `check-scope-literal-defaults.sh`, `check-inert-enforcement.sh`, `check-in-memory-state-stores.sh` (new state is port-backed KV, not a bare `Arc<Mutex<HashMap>>`).
- Full diff inspection `c9cc1536..f5943edd` (9 files; gate_executor.rs, ws.rs, ralph_loop.rs, merge_processor.rs, gyre-common/message.rs, specs). No functions or tests deleted (verified by name-set diff: additions only); merge commit `61f541e2` integrates base cleanly (vs base parent it differs only by the task-137 changes).

## Verified against every acceptance criterion

- **GateResult MessageKind** (`gyre-common/src/message.rs`): Directed tier, `server_only` (forge authority — an agent cannot forge a gate outcome for another agent; the send handler's 403 path covers it), payload schema `{mr_id, gate_name, gate_type, status, output, spec_ref, attempt, gate_agent_id}` with `attempt: U32` required; `as_str`/`from_str`/tier/schema all consistent; `message-bus.md` amended (enum row, server_only exception, schema row).
- **Inbox delivery**: `run_gate` → `gate_failure_feedback` (after final status persist) emits a Directed `GateResult` to the author's inbox via `emit_event` (signed + persisted). Delivery marker in KV is **attempt-scoped** (`{mr}:{gate}:{author}:{attempt}`) so the Ralph loop's `on_gates_failed` fallback (now the same kind, real gate type + attempt) is mutually de-duplicated within an attempt while each re-enqueued attempt delivers again (spec's own example is `attempt: 2`). Test `gate_failure_delivers_gate_result_to_author_inbox` asserts the full field set and attempt advance.
- **WebSocket for live sessions** (`ws.rs`): Agent-destined bus messages now reach the connection whose verified agent identity IS the destination (identity, not subscription; user/global-token callers excluded). This also brings the code in line with message-bus.md §WebSocket routing ("delivered only to the WebSocket connection authenticated … whose sub matches id"). The bus subscription is registered before auth, so no missed-message race. Test `ws_agent_directed_message_reaches_own_live_session_only` asserts delivery to the owner session AND non-delivery to another agent's session (real TCP listener).
- **Attempt tracking**: per-(MR, gate) counter (task-136 KV `gate_attempts`) reported in every message/notification/task title; NEW consecutive-failure streak (KV `gate_failure_streak`) reset on pass — test `gate_pass_resets_failure_streak` proves fail→pass→fail restarts at 1.
- **Deferred task (3+ failures, session ended)**: exact spec shape (title `Gate failure: {gate} on MR #{mr} (attempt {n})`, Backlog/High, labels `["gate-failure","auto-created"]`, description with output/spec_ref/attempts/author); deduped among non-terminal tasks by title prefix; deterministic id per (MR, gate, attempt) contains the concurrent-feedback race. Test asserts NO task and NO escalation at 2 failures, both at 3.
- **Security gate failure → persona notified**: every agent instantiated from the security persona (nearest-wins repo→ws→tenant resolution) gets the GateResult in addition to the author. Test asserts the persona agent's inbox.
- **Critical findings → Overseer**: `has_critical_finding` parses the security persona's structured "Critical Findings" section (must carry content) and, for unstructured output, only explicit severity labels (`severity: critical`, `severity=critical`, line-leading `critical:`) — a coincidental "critical" word does not page a human. Escalation = `Escalation` bus message + `SecurityFinding` notifications to workspace Owners + tenant Admins (deduped). Unit test `critical_finding_detection` covers both directions incl. false-positive classes; integration test drives a real failing command gate.
- **3+ consecutive failures → repo orchestrator**: Directed `Escalation` via `ensure_repo_orchestrator_for_failure` (spawn-on-demand, workspace fallback) — same routing as `ralph_loop::notify_task_blocked`. Test asserts the orchestrator's inbox payload (`consecutive_failures: 3`).
- **All gates failed → queue removal + reassessment**: `handle_all_gates_failed` fails every non-terminal queue entry through the canonical `merge_processor::fail_queue_entry` (terminality re-checked to avoid double analytics) and creates the "Reassess approach" Backlog/High task listing every failed gate's output. Test enqueues a real entry and asserts it goes terminal.
- **`cargo test --all`**: deferred to the verifier (see below). Type-check of lib + all test targets passes; gyre-common suite passes; prior compile failure repaired and re-verified.

## Tests the verifier must execute

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server gate_failure`
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server critical_finding`
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server all_gates_failed`
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server ws_agent_directed`
- `SKIP_WEB_BUILD=1 cargo test --all` (the acceptance criterion)

## Non-blocking observations

- **Canonical-spec OR vs task-contract AND (deferred task creation).** agent-gates.md line 143 reads "converged, been stopped, **or** … failed 3+ times"; the assigned task's excerpt and acceptance criteria use **AND** (3+ failures AND session ended), and the implementation follows the task. The AND reading is the coherent one in the Ralph model: a converged author with an active loop is re-spawned by `on_gates_failed` (the loop is the retry path — a task would duplicate it), while a session-ended author with no loop coming is exactly who the deferred task is for. No spec amendment was made for this reading; the task decomposition itself codified it. Recorded for the spec-fidelity auditor, not a defect against the assigned contract.
- **Escalation protocol runs for required gates only** (feedback entry point is `status == Failed && gate.required`, per the task plan step 2). An optional security gate's advisory failure reaches the author only via the MR comment / Ralph-loop fallback GateResult, not the security-persona/Overseer escalation. Defensible reading of the task plan; recorded for completeness.
- `gate_agent_id` in the Ralph-loop fallback payload is the display identity `gate-agent:{gate_id}` (the fallback path has no live gate agent to name); the executor path carries the real forge-spawned agent id.
- Generated `web/dist/` was not regenerated in this branch — verifier-owned per pipeline rules; not reviewed, not blocking.
