# Review — task-136 (Integrate gate evaluation into merge processor flow)

Spec: `specs/system/agent-gates.md` §Merge Processor Integration.
Candidate: `e59668585b6d7e81e76eef2572418f570af7d4ef` (base `32c4faef`, task-135 HEAD).
Product commit under review: `a05c7286` (attempt-scoped gate re-arming + GateResult author
feedback); `92382690`/`1fd45a81`/`e5966858` are docs/process-only.
Verdict: **needs-revision** — one finding (F1, flaky test indexing); the production changes
themselves verified sound.

## Round 1

Probes (evidence: `/tmp/stage/review-evidence/task-136/`):

- `cargo check -p gyre-server --lib --tests` → exit 0 (8m19s).
- `cargo test -p gyre-adapters sqlite::quality_gate` → 4/4 passed, including the new
  pair-scoped `delete_by_mr_and_gate_scopes_to_the_pair`.
- gyre-server test compilation refused by the sandbox build wrapper (4 GiB budget) —
  infrastructure restriction, not a code defect. Verifier must run `cargo test --all`
  (list in the evidence file and below).
- Mechanical guards: arch, mem-port-contracts, fabricated-scope-defaults,
  scope-literal-defaults, inert-enforcement, lossy-secret-conversion all exit 0.
  check-task-commit-attribution exits 1 only against the review stage's staged reset of
  task-136.md (`commits: []`); at the candidate commit the frontmatter lists `a05c7286`,
  the only product-surface task-136 commit — passes on the candidate tree.
- Full diff read for all 8 touched files; call-site and consumer analysis of
  `trigger_gates_for_mr` (merge_queue enqueue, reconciliation auto-enqueue),
  `check_gates_for_mr` (merge_processor steps 4b/4c, ralph_loop watcher), and every
  `list_by_mr_id` consumer (timeline, gates API, attestation snapshot, dep-health counts).

Verified working (no findings):

- **The retry-path fix is real and closes a genuine hole.** Before this change,
  `trigger_gates_for_mr` appended a fresh Pending row per gate while the previous attempt's
  `Failed` row survived; `check_gates_for_mr` polls `list_by_mr_id` (not attempt-scoped), so
  a re-enqueued MR (Ralph-loop retry: author addresses feedback, re-pushes, re-enqueues via
  `enqueue_core`'s terminal-entry retry path) was vetoed forever by the stale failure — the
  spec's re-run semantics were dead on arrival. `arm_gate_result` now deletes the (MR, gate)
  pair's rows via the new `GateResultRepository::delete_by_mr_id_and_gate_id` (SQLite + PG +
  mem, all three adapters implement the port; mem contract enforced in code per the
  port-contract guard) before writing the fresh Pending row. Manifest gates use deterministic
  ids (`manifest-gate:{repo}:{spec}:{persona}`), so their re-arming hits the same pair key —
  the delete covers them too.
- **The regression test is a real guard.** `reenqueue_replaces_failed_gate_result_and_waits`
  drives the production path end-to-end (trigger → run → manual Failed marking →
  `check_gates_for_mr` Err → re-trigger) and asserts the one-row-per-gate replacement and
  the Err→Ok(false)/Ok(true) transition; on the pre-change code both the `results.len()==1`
  and the `is_ok()` assertions fail (rows appended, stale Failed still vetoes).
- **The attempt counter is port-backed** (`KvJsonStore`, `gate_attempts` namespace,
  per-(MR, gate) key), not an in-memory stand-in; restart-safe.
- **Author feedback now carries the full §Gate Failure Feedback payload** (gate name, type,
  failure output, spec_ref, attempt, gate_agent_id) in both the inbox notification and the
  GateFailure workspace event; the notification's tenant scope is resolved from the MR's
  workspace and skipped+logged when unresolvable — the hardcoded fabricated `"default"`
  tenant is gone (fabricated-scope-defaults guard green). The skip-when-unresolvable test
  asserts zero notifications, matching the fail-closed rule.
- **No consumer regressions from replace-on-rearm semantics.** The MR timeline, gates API,
  and the merge-time attestation snapshot (`merge_processor.rs:2564`) all read the current
  attempt's rows — the shipped notes state this intent explicitly. Dep-health "3+ gate
  failures" escalation now counts within the current attempt rather than cumulatively; that
  reads as more correct ("deps with 3+ gate failures" as a current-state health signal), not
  a defect.
- **Scope check:** diff touches only the gate executor, the port + three adapters, mem
  notification repository wiring, notifications, and specs. No out-of-scope edits, no spec
  rewording, no production code weakened. Coverage row 8 marked implemented with an honest
  note that `cargo test --all` is pending the independent verifier.

Findings:

- [-] **F1 (major, test-only): `failing_gate_notifies_author_with_gate_result_payload`
  intermittently fails on correct code — notification-list ordering contradicts the test's
  index assumption.** The test (gate_executor.rs:6560-6576) waits for two GateFailure
  notifications, then reads `notifs[1]` and asserts `body["attempt"] == 2`. But
  `list_for_user` returns newest-first within a priority band — mem.rs:3027-3031
  (`priority_rank asc, then created_at DESC`) and the production SQLite adapter
  (sqlite/notification.rs:170-171, `.order(priority.asc()).then_order_by(created_at.desc())`)
  share this contract — and `notify_rich` stamps `created_at` at epoch-second granularity
  (notifications.rs:126-131). Both notifications are GateFailure (same rank), so `notifs[1]`
  is the attempt-2 notification only when both creations land in the same epoch second
  (Rust's stable sort then preserves insertion order). The two creations are separated by an
  `eventually()` poll cycle (50ms sleep) plus a full trigger→arm→`run_gate("false")`→notify
  round trip — ~50-150ms, more on loaded CI. When they straddle a second boundary,
  `notifs[1]` is the attempt-1 notification, `body["attempt"] == 1`, and the assertion
  fails: a ~5-15% flake against the `cargo test --all` acceptance criterion.
  **Repair path (mechanical):** don't index positionally — select the notification whose
  parsed body has `attempt == 2` (e.g. find by title `(attempt 2)` or by body attempt field)
  and assert its full payload; optionally also assert the attempt-1 notification still
  exists. Same one-line caution applies anywhere else two same-type notifications are
  positionally indexed.

Minor (non-blocking, recorded for completeness):

- `arm_gate_result` treats delete/save failures as warn-and-continue; a failed delete leaves
  the stale-veto window open with only a log. Consistent with the surrounding code's
  best-effort persistence style (`let _ = save` predates this task), and the warning names
  the consequence — acceptable, but a future hardening candidate.
- `run_gate` reads the attempt counter after the gate completes, so a re-enqueue racing a
  still-running gate could label that run's feedback with the newer attempt number.
  Cosmetic mislabeling only; the decision path is unaffected.

Verifier must run (gyre-server test compilation is wrapper-blocked in the review sandbox):
`cargo test --all`, in particular `gate_executor::tests::reenqueue_replaces_failed_gate_result_and_waits`,
`gate_executor::tests::failing_gate_notifies_author_with_gate_result_payload` (expect the F1
flake), `notifications::tests::gate_failure_notification_carries_deep_link`,
`notifications::tests::gate_failure_notification_skipped_when_workspace_unresolvable`, and
the full `gate_executor`, `merge_queue`, `merge_processor`, `ralph_loop` modules.

Test-inflation check: none found beyond F1 — the reenqueue regression test kills the real
pre-change bug; the adapter pair-scoping test asserts exact survivor sets; the notification
tests assert full payloads and the fail-closed skip. No assertionless, conditional-guard, or
mirrored-logic tests in the diff.
