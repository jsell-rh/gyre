# Review — task-137 (Gate failure feedback: inbox delivery, task creation, escalation)

Spec: `specs/system/agent-gates.md` §Gate Failure Feedback (Agent Inbox
Delivery, Deferred: Task Creation, Escalation).
Commits under review: `0e6ab6ad` (feat), `a8c4e4e9` (fix) on candidate
`5f8b87be` against base `9527d6ae`.
Verdict: **needs-revision** — one blocking finding (F1: repeat-failure
inbox delivery suppressed by a never-reset dedup marker; the candidate's own
test cannot pass, so `cargo test --all` fails). Everything else verified
clean. Full probe log and static trace:
`/tmp/stage/review-evidence/task-137-evidence.md`.

## Round 1

Probes run (review sandbox; gyre-server test execution refused by the build
wrapper — infrastructure, not a code result):

- `bash scripts/check-arch.sh`, `check-dead-message-kinds.sh`,
  `check-inert-enforcement.sh`, `check-in-memory-state-stores.sh`,
  `check-fabricated-scope-defaults.sh`, `check-scope-literal-defaults.sh`,
  `check-lossy-secret-conversion.sh`, `check-unbounded-external-http.sh`,
  `check-relative-path-defaults.sh`, `check-migration-sql-portability.sh`
  → all exit 0.
- `/tmp/stage/bin/cargo check -p gyre-server --tests` → exit 0 (8m 41s) —
  the implementer's type-check claim holds.
- `/tmp/stage/bin/cargo test -p gyre-common` → 125 passed.
- Whole-diff inspection; every production hunk reviewed (the large
  gate_executor.rs diff beyond the feedback block is test-fixture
  reordering from the base merge — manifest-gate tests exist verbatim in
  the base).

### Findings

- [BLOCKING] **F1: the GateResult delivery-dedup marker is never reset per
  attempt — the author agent receives no GateResult for failures after the
  first on the same MR, and the candidate's own test cannot pass.**
  `gate_result_delivered_key` (gate_executor.rs:767-776) is
  `{mr}:{gate}:{author}` with no attempt component; it is written after the
  first delivery (984-993) and never removed anywhere (no `kv_remove`/
  `kv_clear` touches it; `arm_gate_result`, which deletes prior gate-result
  rows and bumps the attempt counter on re-enqueue, does not clear it).
  Re-enqueue of the same MR is the production retry path
  (merge_queue.rs: enqueue-after-terminal re-triggers gates; task-136 built
  it; task-137's streak/deferred-task "3+ failures on the same MR"
  presuppose it), so on attempts 2+ the feedback run hits
  `gate_result_already_delivered` and skips the inbox emit. This contradicts
  §Agent Inbox Delivery — the spec's example message is literally
  `"attempt": 2` — and the Ralph-loop retry model. The candidate's own test
  `gate_failure_delivers_gate_result_to_author_inbox` (7446) asserts at
  7538-7545 that "second failed attempt must deliver its own GateResult";
  that emit is suppressed, `eventually` (10 s) times out, and the assert
  fails — so `cargo test --all` fails on this candidate. The implementer
  could not have seen it: the sandbox wrapper refuses gyre-server test
  compilation and `cargo check` passes.

  **Repair path:** clear the delivery marker in `arm_gate_result` (or key
  it by attempt number). Either preserves the intended within-attempt dedup
  between the executor delivery and the Ralph-loop `on_gates_failed`
  fallback while letting each new attempt deliver exactly once. The test
  comment already states the intended design ("marker de-dupes per
  attempt") — the key just omits the attempt.

- [minor] **F2:** `create_gate_failure_task`'s deterministic id
  (`gate-failure-task:{mr}:{gate}`) means that after a gate-failure task
  reaches Done/Cancelled, a later 3+ streak on the same (MR, gate) passes
  the title-prefix dedup but `tasks.create` fails on the duplicate PK
  (SQLite) — only a `warn!`, no new task. Edge case.

- [minor] **F3:** `has_critical_finding`'s unstructured fallback escalates
  to the Overseer (human) on the bare word "critical" anywhere in a
  security gate's output — the candidate's own test triggers it via a file
  path containing "critical". Over-notification of humans, never under.
  `is_security_gate` likewise matches any gate name containing "security".
  Consider a structured severity channel in a later round.

### Verified working (no findings)

- **GateResult MessageKind** (gyre-common): Directed tier, `server_only`
  (an agent cannot forge a gate outcome — the send handler 403s
  agent-originated `gate_result`), all 8 spec fields in the schema
  (`attempt` required u32), round-trip parse. gyre-common suite green.
- **WS broadcast for live sessions** (ws.rs:437-456): Agent-destined bus
  messages now reach the WS connection authenticated as that agent, gated
  by verified identity (agent tokens carry the verified agent id with
  `user_id: None`; API-key/Keycloak callers set `user_id`; the global token
  is Admin/"system" — all excluded). `emit_event` persists Directed
  messages and fans out to WS. The new WS test uses two real connections
  and asserts both delivery and non-delivery — a genuine guard.
- **Ralph-loop fallback** (`on_gates_failed`): now emits the same
  GateResult kind with the real gate type and per-(MR, gate) attempt number
  (was `GateFailure` with a placeholder type and `iteration`), mutually
  de-duplicated with the executor path.
- **Attempt/streak tracking**: per-(MR, gate) attempt counter (task-136) +
  new consecutive-failure streak (KV, reset on pass; a new MR is a new
  key). Deferred task at 3+ failures AND author session ended, with the
  spec's exact title/Backlog/High/labels/description shape, deduped.
- **Escalations**: security persona notified via its instantiated agents
  (nearest-wins resolution, skip-and-log when unresolvable); critical
  findings → Escalation bus message + SecurityFinding notifications to
  workspace Owners + tenant Admins; 3+ streak → Directed Escalation to the
  repo orchestrator (spawn-on-demand with workspace fallback); all gates
  failed → queue entries failed through `merge_processor::fail_queue_entry`
  (single terminal-status/analytics path, with the a8c4e4e9 terminality
  re-check preventing a double `queue.processed` emit) + "Reassess
  approach" Backlog/High task carrying the gates' real outputs.
- **Spec/coverage updates**: message-bus.md amended with the kind, schema
  row, and the documented server_only exception. Note: coverage row 9's
  "implemented" flip is hollow on the repeat-failure delivery path until F1
  is repaired — it should not stand as-is in the revision round.

### Deferred checks for the independent verifier

1. `cargo test -p gyre-server gate_failure_delivers_gate_result_to_author_inbox`
   — **expected FAIL** at "second failed attempt must deliver its own
   GateResult" (the F1 proof).
2. Full `cargo test --all` (F1 makes this fail).
3. Expected-green new tests (confirm):
   `gate_failure_streak_escalates_and_creates_deferred_task`,
   `gate_pass_resets_failure_streak`, `critical_finding_detection`,
   `security_gate_failure_notifies_persona_and_escalates_critical`,
   `all_gates_failed_removes_mr_from_queue_and_creates_reassessment_task`,
   `ws_agent_directed_message_reaches_own_live_session_only`, ralph_loop
   `gate_failure_respawns_agent_with_fresh_context`.

Test-inflation check: the new tests are genuine behavioral guards (real
polling of the message store, real WS connections with a negative-delivery
assertion, streak threshold with below-threshold absence assertions, exact
spec-shape assertions on the created tasks). The one contradictory test
(F1) fails rather than confirms — it is a correct specification of the
intended behavior that the production code does not implement.

Generated-output observation: this diff touches no `web/dist/` or web
source; nothing to record for the verifier's bundle stage.
