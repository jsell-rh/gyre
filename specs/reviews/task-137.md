# Review: task-137 — Implement gate failure feedback: inbox delivery, task creation, escalation

Comparison base: 9527d6aee7092eb83e89f56776be1db6ac7b781f → candidate
cd5a2504ffbc91650021af58339218c69535a0d3. Diff: 8 files, +2748/−872
(gate_executor.rs bulk; message.rs, ws.rs, ralph_loop.rs, merge_processor.rs
one-line visibility change; coverage + message-bus.md spec sync; task file).

Round 2: the code delta since the round-1 reviewed tree (ff8f761e) is exactly
the repair commit fdb0a05c (attempt-scoped GateResult delivery marker,
attempt-scoped deferred-task id, explicit-severity-label critical detection)
plus docs/process commits — reviewed in full, plus a fresh pass over the whole
feedback path.

## Round-1 findings — repair verification

- **F1 (blocking, delivery marker suppressed repeat attempts): FIXED.**
  `gate_result_delivered_key` is now `{mr}:{gate}:{author}:{attempt}`;
  executor and Ralph-loop fallback both derive the attempt from the same
  per-(MR, gate) KV counter (`gate_attempts`), so within-attempt dedup between
  the two paths is preserved while every re-enqueued attempt's failure reaches
  the author's inbox (spec example is `attempt: 2`). Logic-traced end to end
  (server test execution is blocked by this sandbox's build wrapper); the
  candidate's regression test `gate_failure_delivers_gate_result_to_author_inbox`
  polls the real message store for a GateResult with `attempt == 2` after a
  re-enqueue — with the old key that assertion times out and fails, so the
  test genuinely kills the F1 bug class. Deferred to verifier (see below).
- **F2 (minor, deferred-task PK collision after terminal): FIXED.** Task id
  is now scoped `(MR, gate, attempt)`; the title-prefix dedup only skips
  non-terminal tasks, so a later 3+ streak after Done/Cancelled creates its
  own task instead of dying on a duplicate PK.
- **F3 (minor, bare-word "critical" paged the human Overseer): FIXED.**
  Unstructured output now escalates only on an explicit severity label
  (`severity: critical` / `severity=critical`, or a line-leading `critical:`
  classification, word-boundary checked). The exact function was extracted
  and re-run standalone under rustc with the candidate's assertions plus
  adversarial cases — all pass, including the false-positive class
  (`ls: cannot access '/nonexistent-gyre-critical-finding'` → no escalation)
  and the positive fixture (`ls severity:critical` really exits 2 with the
  label in stderr, and `run_command` combines stderr into the gate output).

## Spec conformance (agent-gates.md §Gate Failure Feedback)

- **GateResult MessageKind** (gyre-common): Directed tier, `server_only`
  (anti-forgery: an agent cannot forge a gate outcome — enforced on both the
  REST send path `messages.rs:110` and the MCP `message.send` gate
  `mcp.rs:2891`), full field set `{mr_id, gate_name, gate_type, status,
  output, spec_ref, attempt, gate_agent_id}` with required-field schema;
  `message-bus.md` amended to carry the schema row and the server_only
  exception (spec lifecycle doc sync, correct direction).
- **Inbox delivery**: every required-gate failure emits a Directed GateResult
  to the author agent via `emit_event` (signed, persisted to the message
  store, read by the next session); carries the gate's real recorded output.
- **WebSocket broadcast for live sessions**: `ws.rs` now delivers
  Agent-destined bus messages to the connection authenticated as that agent
  (identity-gated on the verified per-agent token, not subscription); user
  identities and the global Admin token are excluded. Tested with real
  TCP listener + tokio-tungstenite, including the negative case (another
  agent's session receives nothing).
- **Attempt tracking**: per-(MR, gate) attempt counter (task-136 KV) reported
  in every message/notification/task title; NEW consecutive-failure streak
  incremented on failure, reset on pass (`gate_pass_resets_failure_streak`
  proves fail→pass→fail restarts at 1).
- **Deferred task creation**: streak ≥ 3 AND author session ended
  (status ≠ Active, or vanished author) → task with the spec's exact title /
  Backlog / High / `["gate-failure", "auto-created"]` / description (gate
  output, spec_ref, attempt count, author id). Negative assertions at 2
  failures (no task, no orchestrator escalation) are in the test.
- **Escalation**: security gate failure → GateResult to every agent
  instantiated from the security persona (nearest-wins scope-chain persona
  resolution — repo → workspace → tenant, real lookups); critical findings →
  Escalation bus message + SecurityFinding notifications to workspace Owners
  and tenant Admins (fail-closed tenant resolution, no fabricated scope);
  3+ consecutive failures → Directed Escalation to the repo orchestrator
  (spawn-on-demand via the real `ensure_repo_orchestrator_for_failure`,
  workspace-destined fallback); all gates failed → non-terminal merge-queue
  entries failed through `merge_processor::fail_queue_entry` (terminality
  re-checked before failing to avoid double analytics) + "Reassess approach"
  Backlog/High task listing every failed gate's output.

## Test quality

All new tests drive real production entry points (`trigger_gates_for_mr` with
real `false`/`true` command gates, real KV/message/task/notification/queue
stores, real WS connections) and assert observable state, with negative
assertions below thresholds. No mirrored-logic, stub-backed, or
assertionless tests found. `critical_finding_detection`'s assertions were
re-run standalone by this review (all pass).

## Verification run (this review)

- `cargo check -p gyre-server --tests` → exit 0, no warnings in touched
  files (9m04s; server test *execution* is refused by the build wrapper —
  deferred, see below).
- `cargo test -p gyre-common` → 125 passed.
- `has_critical_finding` extracted + standalone rustc re-run with candidate
  assertions + adversarial cases → all pass, exit 0.
- `ls severity:critical` → exit 2, label in stderr (fixture is real).
- `check-arch.sh`, `check-dead-message-kinds.sh`,
  `check-byte-slice-truncation.sh`, `check-inert-enforcement.sh`,
  `check-in-memory-state-stores.sh`, `check-fabricated-scope-defaults.sh` →
  all exit 0.
- `check-task-commit-attribution.sh` → exit 0 against the candidate's
  committed task-137.md (the failure against the working tree is the
  controller's pre-dispatch reset of the task file, not a candidate defect).
- Evidence: `/tmp/stage/review-evidence/task-137-r2.md`.

## Deferred to the verifier (recorded)

1. `cargo test -p gyre-server --lib gate_executor` (all gate-failure-feedback
   tests, esp. `gate_failure_delivers_gate_result_to_author_inbox` — the F1
   regression).
2. `cargo test -p gyre-server --lib ws` (agent-directed WS delivery).
3. `cargo test -p gyre-server --lib ralph_loop` (fallback delivery + dedup).
4. `cargo test --all` (acceptance criterion) and workspace clippy.

## Notes (non-blocking)

- N1: `run_gate` reads the attempt counter at completion rather than
  capturing it at arming; two overlapping enqueues of the same MR could label
  the first run's feedback with the second attempt's number. Overlapping
  enqueues of one MR while its gates still run is not a production path (the
  merge processor re-enqueues only after terminal states), and the visible
  effect is one correctly-numbered delivery instead of two — not a delivery
  loss. Pre-existing counter semantics from task-136.
- N2: `is_security_gate` matches any gate name containing "security"
  (over-inclusive notification, not under); acceptable heuristic, documented.
- N3: no dedicated regression test for the F2 Done-then-new-streak scenario;
  the attempt-scoped id makes the defect class structurally impossible
  (distinct PKs per attempt), verified by reading.

## Verdict

R2: approved — round-1 F1/F2/F3 repaired with real logic, full §Gate Failure
Feedback contract implemented as production code, tests are genuine killers.
Server test execution deferred to the independent verifier per the build
wrapper; the F1 regression test is the one that must run.
