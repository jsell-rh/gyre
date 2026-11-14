# Review — task-136 (agent-gates.md §Merge Processor Integration)

Spec: `specs/system/agent-gates.md` §Merge Processor Integration.
Candidate: `679fa87bb460a7d3a6876fc3a9876b17363e0c55` (base `c844ea52ad9389b1435ba5a240f87ee297f34764`).
Verdict: **approved** (round 1). Probe evidence: `/tmp/stage/review-evidence/task-136-review.md`.

## What was reviewed

The section's core flow shipped with tasks 134/135 and was verified present in the candidate
tree before judging the delta: enqueue triggers `trigger_gates_for_mr`
(`api/merge_queue.rs:117-134`, already-queued guard prevents double-arming; same trigger on
the reconciliation auto_merge path, `reconciliation.rs:1262-1277`), all gates spawn
concurrently (`tokio::spawn` per gate), the processor polls `check_gates_for_mr` every cycle
with the two-pass decision (`merge_processor.rs:1850-1872`: Ok(true) proceed, Ok(false)
requeue, Err → `fail_queue_entry`; `gate_executor.rs:2945-3014` with live RequiredApprovals
promotion), Pending → Running → Passed/Failed transitions in `run_gate`, and the merge
attestation snapshots all gate results with enriched types
(`merge_processor.rs:2614-2679`, `promote_to_attestation` at 1491). The candidate's delta
closes the two parts that were actually hollow, and both are real:

- **Attempt-scoped gate re-arming.** In the base, each trigger appended fresh Pending rows
  (new UUID, upsert-on-id never conflicts) while the merge decision polls `list_by_mr_id`,
  which is not attempt-scoped — a stale `Failed` row vetoed every re-enqueued MR forever, so
  the spec's Ralph-loop retry path ("addresses the feedback, re-pushes, and gates re-run")
  was dead on arrival. The candidate adds
  `GateResultRepository::delete_by_mr_id_and_gate_id` (SQLite + PostgreSQL + mem, all three
  verified; pair-scoping adapter test green against real SQLite) and `arm_gate_result`,
  which replaces the gate's prior rows with a fresh Pending row, plus a per-(MR, gate)
  attempt counter in the port-backed KV store (`store!` wiring → durable in DB-backed
  deployments). `fail_queue_entry` leaves the MR Open, so re-enqueue is genuinely reachable.
  Late writes from a prior attempt's still-running gate cannot clobber the new attempt's
  row: `update_status` is a pure UPDATE-by-id, and the prior attempt's result id was
  deleted. The regression test kills the base bug (asserts one row per gate after
  re-trigger and the Err → Ok(false) → Ok(true) sequence; the base appended a second row
  and stayed Err forever).
- **GateResult author feedback.** On a required-gate failure the author's spawning user now
  receives the full payload (gate name, type, failure output, spec_ref, attempt count,
  gate_agent_id) in the notification body and title, and the GateFailure workspace event
  carries the attempt number and the real forge-spawned gate-agent id. The notification's
  tenant scope is resolved from the MR's workspace and skipped (logged) when unresolvable —
  the call site previously passed a hardcoded fabricated `"default"` tenant, exactly what
  `check-fabricated-scope-defaults.sh` forbids (gate passes). The failure test runs a real
  failing command (`false`) and asserts the full payload on both attempts, selecting the
  attempt-2 notification by its payload's attempt field (the round-1 flake fix — sound,
  since `list_for_user` is newest-first at epoch-second granularity). The unresolvable-
  workspace test asserts zero notifications (fail-closed, no fabricated scope).

## Probes run (evidence file above)

- `cargo test -p gyre-adapters --lib sqlite::quality_gate::tests::delete_by_mr_and_gate_scopes_to_the_pair`
  → 1 passed, exit 0 (real SQLite, both directions + cross-pair scoping).
- `cargo check -p gyre-server --tests` → exit 0 (covers new server tests and the PG adapter).
- `cargo clippy -p gyre-ports --all-targets` → 0 warnings — durable finding dc04012
  (changed-lines clippy `double_must_use` on `GateResultRepository`) is repaired by 38f35818.
- `check-mem-port-contracts.sh`, `check-fabricated-scope-defaults.sh`,
  `check-scope-literal-defaults.sh` → all exit 0.
- gyre-server test execution is delegated to the verifier per the 4 GiB build budget; the
  required test list is recorded in the evidence file.

## Scope/attribution checks

Product-surface task-labeled commits on the branch are exactly a05c7286, bfae8b78, 38f35818 —
all listed in the task's `commits:` frontmatter; the remaining task-labeled commits touch
only `specs/`. The coverage row for §8 is marked implemented with an honest note that
`cargo test --all` is pending the independent verifier — no fake completion. "Create task if
session ended" belongs to §Gate Failure Feedback (task-137, still task-assigned) per the task
decomposition; this task's plan item 4 requires only the GateResult notification, which is
shipped.

Minor (not blocking, recorded for completeness — follow-up hardening, not a contract gap):

- **Enqueue→arming is not atomic (TOCTOU, pre-existing class).** The queue entry becomes
  visible before gate rows are armed, and `arm_gate_result`'s delete→save is two awaits. A
  processor poll landing inside these windows sees a partial row set: between enqueue and
  the first delete, a stale `Failed` row can spuriously fail the fresh entry (fail-closed,
  self-healing on the next re-enqueue); between the first gate's delete and its save — the
  only fail-open configuration, since later-armed gates already hold fresh Pending rows —
  a single-gate MR presents an empty set (`decide([])` → Ok(true)) and could merge before
  the failed gate re-runs. This race class pre-exists in the approved base (on first
  enqueue the entry is visible while zero gate rows exist — an empty set → Ok(true) window
  that spans the entire gate-listing phase, strictly larger than the re-arm window), and
  this change converts a permanent veto into a millisecond-scale window, so it is a strict
  improvement rather than a regression. Closing it fully is follow-up work: arm all gates
  before enqueueing the queue entry (also closes the first-enqueue window), or scope the
  merge decision to the current attempt.
- Overlapping attempts (re-enqueue while a prior attempt's long-running gate is still in
  flight) can attribute the wrong attempt number to the old run's completion events —
  cosmetic (event/attempt labeling), the gate-result rows themselves are correctly
  isolated by the row replacement.

## Verifier must run

`cargo test --all`; in particular the four new/repaired tests
(`gate_executor::tests::reenqueue_replaces_failed_gate_result_and_waits`,
`gate_executor::tests::failing_gate_notifies_author_with_gate_result_payload`,
`notifications::tests::gate_failure_notification_carries_deep_link`,
`notifications::tests::gate_failure_notification_skipped_when_workspace_unresolvable`)
plus the full `gate_executor`, `merge_queue`, `merge_processor`, and `ralph_loop` modules,
and `checks.sh` (prior clippy failure repaired, verified here).
