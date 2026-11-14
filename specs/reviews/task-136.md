# Review — task-136 (Integrate gate evaluation into merge processor flow)

Spec: `specs/system/agent-gates.md` §Merge Processor Integration.
Base `c844ea52` → candidate `dc465e43` (branch pipeline/task-136/39b9dc4fb22f411684f06963d40530e8-1).
Product commits: `a05c7286` (attempt-scoped gate re-arming + GateResult author feedback),
`bfae8b78` (round-1 F1 test fix). Docs/process: `92382690`, `1fd45a81`, `e5966858`,
`6f69b488`, `dc465e43`.
Verdict (round 2): **approved** — round-1 finding F1 repaired and verified; full-diff
re-review found no blocking defects.

## Round 1 (prior review, candidate e5966858)

One finding, test-only: `failing_gate_notifies_author_with_gate_result_payload` indexed
`notifs[1]` for the attempt-2 notification, but `list_for_user` returns newest-first within
a priority band at epoch-second `created_at` granularity, so when the two GateFailure
creations straddled a wall-clock second boundary the attempt-2 notification was
`notifs[0]` — an intermittent failure of the `cargo test --all` acceptance criterion
against correct production code. Everything else verified clean.

## Round 2 — F1 repair verified

`bfae8b78` touches `crates/gyre-server/src/gate_executor.rs` only (36+/2-). The test now
waits for two notifications (`eventually(len == 2)`) and selects the attempt-2
notification by its payload's `attempt` field (`gate_executor.rs:6578-6580`), then asserts
the full GateResult payload (type, mr_id, gate name/type, status, spec_ref, attempt,
gate_agent_id, agent_id) and title — mirroring the attempt-1 assertions. Selection by
payload field is order-independent by construction; no list-position indexing remains.
`cargo check -p gyre-server --tests` exit 0 (test target compiles).

## Round 2 — full-diff independent re-review (nothing carried forward)

Probes (evidence: `/tmp/stage/review-evidence/task-136-round2.md` and `*.out`):

- `cargo test -p gyre-adapters delete_by_mr_and_gate_scopes_to_the_pair` → **ok** (real
  SqliteStorage on a temp DB: pair-scoped delete, sibling gate/MR rows untouched, no-op on
  unknown pair).
- `cargo check -p gyre-server --tests` → exit 0.
- Invariant scripts all exit 0: arch, mem-port-contracts, scope-literal-defaults,
  fabricated-scope-defaults, mcp-write-tools, inert-enforcement, lossy-secret-conversion,
  dead-message-kinds, forwarded-header-trust, relative-path-defaults,
  byte-slice-truncation.
- `check-task-commit-attribution.sh` exit 1 **against the working tree only** — the
  controller's dispatch reset the (uncommitted) task file to the original `commits: []`
  body (matches job.json verbatim). The committed candidate records both product commits
  (`bfae8b78`, `a05c7286`) in its frontmatter. Not a candidate defect.

Verified working (no findings):

- **Retry path is real.** Each enqueue is one gate attempt: `arm_gate_result` deletes the
  gate's prior GateResult rows for the MR (new port method
  `GateResultRepository::delete_by_mr_id_and_gate_id`, implemented in SQLite, PostgreSQL,
  and mem — pair-scoped in all three, no-op on unknown pair, with a pair-scoped adapter
  regression test) and saves a fresh Pending row; `bump_gate_attempt` keeps the per-(MR,
  gate) counter in the port-backed KV store (`gate_attempts`). Before this, a stale
  `Failed` row survived every re-enqueue and vetoed the MR forever via
  `check_gates_for_mr` → `list_by_mr_id`, killing the spec's Ralph-loop retry path.
  `reenqueue_replaces_failed_gate_result_and_waits` fails on the pre-change code by
  construction (append → 2 rows, stale Failed keeps `check_gates_for_mr` Err).
- **Enqueue triggers gates exactly once.** `merge_queue.rs` POST handler has an
  already-queued guard before `trigger_gates_for_mr`; the reconciliation auto_merge path
  triggers on its own enqueue. Gates still run concurrently (one `tokio::spawn` per gate).
- **Author feedback carries the full GateResult payload.** `notify_gate_failure` now takes
  gate type, failure output, spec_ref, attempt, and gate_agent_id; the notification body is
  the §Gate Failure Feedback payload and the title carries the attempt number. The
  GateFailure workspace event also carries `attempt` and the real forge-spawned gate agent
  id (from `GateConstraints.gate_agent_id`) instead of always the display form.
- **Fabricated tenant removed.** The call site previously passed a hardcoded `"default"`
  tenant; the tenant scope is now resolved from the MR's workspace and the inbox
  notification is skipped (logged) when unresolvable — fail-closed, matching
  check-fabricated-scope-defaults (passes). Tests assert the real resolution
  (`tenant-1` read back from the workspace row) and the skip path.
- **Merge decision logic unchanged and correct** (task-135's two-pass
  `decide_gate_readiness`: required-failure → Err before waiting → Ok(false) requeue →
  Ok(true) all-passed); the attestation bundle still snapshots all gate results with types
  at merge time (`merge_processor.rs:2616-2679`).

Test-inflation check: none. The new tests drive real production paths (real adapter on
temp DB, real trigger/check/notification functions); the retry regression test fails on
the pre-change code; the notification tests assert resolved (not echoed) values.

Deferred to the independent verifier (build wrapper refuses gyre-server test compilation
in this sandbox — infrastructure restriction, not a code defect): `cargo test --all`, in
particular `gate_executor::tests::reenqueue_replaces_failed_gate_result_and_waits`,
`gate_executor::tests::failing_gate_notifies_author_with_gate_result_payload`,
`notifications::tests::gate_failure_notification_carries_deep_link`,
`notifications::tests::gate_failure_notification_skipped_when_workspace_unresolvable`,
plus the full `gate_executor`, `merge_queue`, `merge_processor`, and `ralph_loop` modules.

Minor (non-blocking, recorded for completeness):

- **Attempt-counter read race:** `run_gate` reads `gate_attempt_count` after the gate
  command completes. If a re-enqueue lands while a prior attempt's slow gate is still in
  flight, the late failure's notification/event reports the new attempt number (and
  duplicates it with the new attempt's own). No status contamination — the late
  `update_status` targets the prior attempt's deleted result id (no-op) — and no wrong
  merge decision; telemetry-only, narrow window.
- `merge_queue.rs:117-118` comment ("would insert a second set of Pending GateResult
  rows") predates the re-arm semantics (a re-trigger now replaces rows and bumps attempt
  counters); the guard's behavior remains correct.
- `arm_gate_result` logs-and-continues when the clearing delete fails — under storage
  failure a stale `Failed` row could survive and veto a re-enqueue; exceptional-condition
  degradation with a warning, consistent with the surrounding error handling.
