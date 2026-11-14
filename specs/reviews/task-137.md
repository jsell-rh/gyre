# Review — task-137 (Gate failure feedback: inbox delivery, task creation, escalation)

Spec: `specs/system/agent-gates.md` §Gate Failure Feedback (Agent Inbox Delivery, Deferred: Task
Creation, Escalation).
Base `9fb371ac` → candidate `eb00fe5` (product commits `0e6ab6a`, `a8c4e4e`, `fdb0a05`, `f03a3c83`;
earlier review rounds ff8f761e-repairs and verification-repair already folded in).
Verdict: **approved** — every acceptance criterion is backed by real production code and hard
tests; the retained verification findings are repaired at source level.

## Probes run (evidence in `/tmp/stage/review-evidence/`)

- `git diff 9fb371ac..eb00fe5` — 8 files, all in scope (message.rs, gate_executor.rs,
  merge_processor.rs one-line `pub(crate)`, ralph_loop.rs, ws.rs, coverage row 9, message-bus.md
  schema row, task-137.md). No scripts/verifier/unrelated-task edits; no web/dist edits.
- Trailing-whitespace check on added crate lines (precise grep, `[ \t]$`): **0 matches** (the
  earlier 102-count was a `[ \t]`-in-ERE false positive matching lines ending in `t`).
- `cargo fmt --check -p gyre-server -p gyre-common` → **exit 0**.
- Clippy site `ws.rs:804` (`redundant_pattern_matching` from the retained verification log):
  now `got.ok().flatten().is_some()` — fixed.
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` → **exit 0** (6m26s; full cold check).
- `cargo test -p gyre-common` → **125/125 passed** (MessageKind schema/serialization suite).
- `scripts/check-dead-message-kinds.sh` → OK (GateResult has emitters).
- `scripts/check-arch.sh` → OK.
- `scripts/check-task-commit-attribution.sh` → **OK at candidate HEAD** (the working-tree failure
  observed first is the executor's unstaged revert of task-137.md emptying `commits:`; stash/pop
  probe at HEAD passes — see `check-attribution-head.txt`).

## Verification of the retained verification finding (checks.sh exit 1 on `cd5a2504`)

All three concrete failures are repaired in `f03a3c83`, verified at `eb00fe5`: trailing
whitespace (0 matches), rustfmt changed-lines (exit 0 across the three files), clippy
`redundant_pattern_matching` (source fixed). Everything else in that checks.sh run was already
green (arch, ABAC registry/exempt handlers, MCP write tools, migration versions/portability,
dead message kinds, byte-slice, relative paths, fail-open refs, attribution, mem port contracts,
fabricated scopes, lossy secrets, scope literals, inert enforcement, forged scope fields,
forwarded headers, in-memory stores, unbounded HTTP).

## Acceptance criteria — independent assessment

- **GateResult MessageKind with all fields** — `gyre-common/src/message.rs`: Directed tier,
  `server_only` (forge authority — an agent cannot forge a gate outcome; the send handler's
  `kind.server_only() && origin != Server → 403` covers it via the generic mechanism), payload
  schema `{mr_id*, gate_name*, gate_type, status, output, spec_ref, attempt*(u32),
  gate_agent_id}` matching the spec's example field-for-field. `message-bus.md` amended with the
  schema row and the server_only exception (spec-amended rather than contradicted — correct
  lifecycle use).
- **Inbox delivery on failure** — `gate_failure_feedback` (gate_executor.rs) emits a Directed
  GateResult via `emit_event` (Server origin, signed, persisted through `messages.store`), read
  back by the agent poll path (`list_unacked`). The executor path is attempt-scoped-deduped
  against the Ralph loop's `on_gates_failed` fallback (both mark the same KV key
  `{mr}:{gate}:{author}:{attempt}`), so every re-enqueued attempt reaches the author exactly
  once while the spec's `attempt: 2` re-run semantics hold. Test
  `gate_failure_delivers_gate_result_to_author_inbox` proves full field set + attempt advance
  across re-enqueues through the real `trigger_gates_for_mr` surface.
- **WebSocket broadcast for live agents** — `ws.rs` now delivers Agent-destined bus messages on
  verified identity (`caller.agent_id == destination`, per-agent token auth via
  `authenticate_token`; users and the global Admin token excluded). Test
  `ws_agent_directed_message_reaches_own_live_session_only` is a real end-to-end probe over a
  live TCP listener with two authenticated sessions and a negative leak assertion — not
  self-confirming.
- **Attempt tracking per gate per MR** — per-(MR, gate) counter (task-136 KV `gate_attempts`,
  reported in message/notification/task title) plus the new consecutive-failure streak
  (`gate_failure_streak`), incremented on failure and reset on pass; a new MR is a new key.
  `gate_pass_resets_failure_streak` proves the reset with a real flaky→fixed→flaky gate cycle.
- **3+ failures + session ended → deferred task** — streak ≥ 3 AND `author_session_ended`
  (status ≠ Active, or vanished author; fail-closed on lookup error) creates the task with the
  spec's exact title/status/priority/labels/description. Deduped among non-terminal tasks by
  title prefix; deterministic id scoped to (MR, gate, attempt) so a post-terminal later streak
  can still create its task. `gate_failure_streak_escalates_and_creates_deferred_task` includes
  the negatives (no task, no orchestrator escalation before failure 3).
- **Security gate failure → persona notified** — every agent instantiated from the
  nearest-wins-resolved security persona receives the GateResult in its inbox
  (`security_gate_failure_notifies_persona_and_escalates_critical`).
- **Critical findings → Overseer escalation** — `has_critical_finding` parses the security
  persona's structured "Critical Findings" section (must exist AND carry content), falling back
  to explicit severity labels only, word-boundary checked; a coincidental "critical" word in
  echoed command output does not page a human (thorough false-positive unit coverage in
  `critical_finding_detection`). Escalation = `Escalation` bus message + `SecurityFinding`
  notifications to workspace Owners + tenant Admins — verified end-to-end in the test with both
  recipient classes seeded.
- **3+ consecutive failures → orchestrator notified** — Directed `Escalation` to the live repo
  orchestrator (spawn-on-demand via `ensure_repo_orchestrator_for_failure` with
  workspace-destined fallback). Tested, including the before-3 negative.
- **All gates failed → queue removal + reassessment task** — every non-terminal queue entry for
  the MR failed through the single `merge_processor::fail_queue_entry` path (terminality
  re-checked to avoid double analytics), plus the "Reassess approach" Backlog/High task with
  per-gate output. `all_gates_failed_removes_mr_from_queue_and_creates_reassessment_task` drives
  a real enqueued entry to terminal Failed and asserts the task shape.
- **`cargo test --all`** — gyre-server test compilation belongs to the independent verifier
  (4 GiB budget). Type-check (`cargo check -p gyre-server`) and gyre-common tests pass here; the
  previous full checks.sh run was green on every gate except the three formatting findings now
  repaired.

## Tests the verifier must run

- `cargo test -p gyre-server gate_executor::tests` (in particular:
  `gate_failure_delivers_gate_result_to_author_inbox`,
  `gate_failure_streak_escalates_and_creates_deferred_task`,
  `security_gate_failure_notifies_persona_and_escalates_critical`,
  `all_gates_failed_removes_mr_from_queue_and_creates_reassessment_task`,
  `gate_pass_resets_failure_streak`, `critical_finding_detection`,
  `failing_gate_notifies_author_with_gate_result_payload`,
  `reenqueue_replaces_failed_gate_result_and_waits`).
- `cargo test -p gyre-server ws::tests` (`ws_agent_directed_message_reaches_own_live_session_only`).
- `cargo test -p gyre-server ralph_loop::tests` (`gate_failure_respawns_agent_with_fresh_context`
  exercises the GateResult fallback delivery assertions).
- Full `cargo test --all`, `cargo clippy --all-targets`, `cargo fmt --check`, and `bash
  /tmp/stage/checks.sh` end-to-end.

## Non-blocking observations (recorded for completeness, no action required)

- The all-gates-failed reassessment task triggers only from a required gate's failure feedback.
  If an optional gate fails after the last required gate's feedback already ran, that ordering
  misses the reassessment task (the MR still leaves the queue through the merge processor's
  required-failure path, so the merge-queue half of the bullet holds regardless).
- The executor's emit→mark window for the delivery marker is not atomic; a Ralph-loop fallback
  running exactly inside it would double-deliver one inbox message (benign duplicate).
- Re-enqueueing while a gate run is in flight can mislabel that run's attempt number — a
  pre-existing consequence of task-136's arm/replace design, not a task-137 regression.

Test-inflation check: none. All new tests drive real surfaces (spawned gate runs, real
commands, KV/repo/message/notification/task/merge-queue state) and include negative assertions
that fail if the behavior regresses.

Scope check: clean — no production surface outside the task's contract was modified
(`merge_processor.rs` is a one-line visibility change to reuse the single fail path).
