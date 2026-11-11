# Review: TASK-135 — Gate Chain Configuration and Gate Agent Lifecycle

**Reviewer:** Independent review (pi runtime)
**Date:** 2026-04-13
**Candidate:** `20a72f88ed40d3dcc9106691ca1970b73bd09e49` (base `cba416b46a73d3568f745d7b4eae49a0700a90a6`)
**Verdict:** approved

---

## Scope

`specs/system/agent-gates.md` §Gate Chains (Composable Policies) and §Gate Agent
Lifecycle, per `specs/tasks/task-135.md`. Product commits: `583c4a07d`,
`d71555220` (repair round f0f41ed), `6ccbdff1` (merge reconciliation with the
task-118 base).

## Evidence (independently run)

- `cargo test -p gyre-domain` — pass (exit 0).
- `cargo test -p gyre-adapters --lib sqlite::` — 316 passed / 0 failed,
  including the new chain-ordering tests (`list_by_repo_orders_by_chain_position`,
  `list_by_repo_position_tie_falls_back_to_created_at`) and the review
  attribution round-trip tests through real SQLite.
- `cargo check -p gyre-server` — pass (exit 0, 5m23s).
- Mechanical checks — pass: `check-arch`, `check-abac-route-registry`,
  `check-inert-enforcement`, `check-migration-sql-portability`,
  `check-mem-port-contracts`, `check-fabricated-scope-defaults`,
  `check-scope-literal-defaults`, `check-migration-versions`.
  `check-task-commit-attribution` fails against the *staged working tree*
  only because review staging reverted `specs/tasks/task-135.md` to its
  pre-task frontmatter; run against the candidate commit's own file content
  the check passes (exit 0). Recorded in
  `/tmp/stage/review-evidence/check-task-commit-attribution-head.log`.
- Static source verification (see per-point findings below).
- Logs under `/tmp/stage/review-evidence/`.

Server test *execution* is blocked by the review sandbox's build wrapper
(4 GiB budget — infrastructure restriction, not a code defect). The verifier
must run:

```
cargo test -p gyre-server --lib gate_chain
cargo test -p gyre-server --lib required_approvals
cargo test -p gyre-server --lib check_gates
cargo test -p gyre-server --lib advisory
cargo test -p gyre-server --lib agent_review
cargo test -p gyre-server --lib decide_gate_readiness
cargo test -p gyre-server --lib ralph_loop::tests::gate_failure_respawns_agent_with_fresh_context
cargo test --all   # task acceptance criterion
```

## Acceptance criteria

- **Ordered chains of mixed gate types** — met. `quality_gates.position`
  (migration `2026-11-12-000061_gate_chain_config`; portable SQL — `ADD COLUMN
  ... DEFAULT`, no dialect functions; next free sequence verified by
  `check-migration-versions.sh`). SQLite and Postgres adapters `ORDER BY
  position, created_at`; mem adapter sorts identically (port-contract check
  passes). `PUT/GET /repos/:id/gate-chain` accepts/returns the spec's exact
  `[[gates]]` TOML (`name`, `type` PascalCase or snake_case, `command`,
  `persona`, `min_approvals`, `required`); `create_gate` appends.
- **Parallel execution on enqueue** — met (pre-existing, verified in source):
  `trigger_gates_for_mr` creates a Pending GateResult per gate and
  `tokio::spawn`s one task per gate; called from `merge_queue.rs` enqueue and
  `reconciliation.rs`.
- **Per-MR gate status Pending→Running→Passed/Failed** — met; the previously
  hollow RequiredApprovals stub ("delegated to merge processor", always
  Passed) is now a real live count of distinct Approved reviewers: Running
  until N distinct approvals, promoted to Passed by the merge processor's
  re-check and persisted.
- **All required gates pass before merge** — met: `check_gates_for_mr` →
  `decide_gate_readiness` (pure two-pass: required failures → Err, required
  Pending/Running → Ok(false) requeue, else Ok(true)).
- **Failed required gate blocks merge and notifies author** — met: failures
  scanned across ALL results before any wait decision (order-independent —
  a waiting gate can no longer mask a failed one, the f0f41ed F2 repair);
  the merge processor fails the queue entry on Err (merge_processor.rs
  candidate selection), and `run_gate` fires `notify_gate_failure` (High
  priority, resolves the author agent's spawning user) on Failed.
- **Optional gates don't block** — met: advisory gates are never waited on
  in any state, including RequiredApprovals short of minimum (F1 repair);
  advisory failures attach an MR comment attributed to `gate-agent:{id}`.
- **Gate agent lifecycle (ephemeral, read-only, scoped, attributed)** — met.
  Ephemeral/read-only/scoped shipped in task-134 (scoped EdDSA JWT
  `review:submit`, push denied, token revoked on exit incl. crash/timeout) —
  unchanged. Attribution is NEW and real: `Review.reviewer_persona` +
  `Review.gate_id` derived server-side in `submit_review` from the scoped
  token's `task_id` claim (the minting gate's id); the body cannot forge it
  (reviewer identity is bound to the token `sub` for scoped tokens, and the
  persona comes from the looked-up gate row). Persisted in both SQL adapters
  (round-trip test passed here) and surfaced in `ReviewResponse`.
- **Context windows captured for audit** — met: before the subprocess runs,
  the exact bundle the agent sees (persona system prompt, MR diff, spec at
  the pinned `spec_ref` SHA, MR title, task description) is zstd-compressed
  into the conversation repository and referenced by a `gate_agent_context`
  audit event (SHA, gate, persona, MR); validators captured too; capture is
  skipped and logged — never stored under a fabricated tenant — when the
  workspace's tenant cannot be resolved.
- **`cargo test --all`** — deferred to the verifier (see commands above);
  every package runnable in this sandbox passed.

## Security / invariant checks performed

- `/api/v1/repos/:id/gate-chain` is registered in the ABAC `RouteResourceMapping`
  (`gate` resource); resolver matching is exact-pattern, no earlier entry
  shadows it; `check-abac-route-registry.sh` passes with no exemption growth.
- Chain PUT gates content-type (`application/toml`/`text/plain`, else 415),
  validates UTF-8, and validates the whole chain (duplicate names, unknown
  types, missing `command`/`min_approvals`, post-merge name collisions)
  before the first mutation.
- `truncate_bytes` is char-boundary safe (existing helper, reused).
- No exemption-file or script changes in the diff.
- The scoped-token → attribution chain was verified end-to-end in source:
  `mint_scoped(gate_agent_id, gate.id, ..., REVIEW_SUBMIT_SCOPE)` →
  auth-extractor signature/expiry validation → claims surfaced →
  `submit_review` derivation.

## Non-blocking observations

1. `count_distinct_approvals` uses `unwrap_or_default()` on `list_reviews`:
   a transient storage error reads as 0 approvals, so a required approvals
   gate keeps the merge waiting (fail-closed for merge safety, but could
   stall). Same defensive pattern as the pre-existing gate-results load in
   `check_gates_for_mr`; not a regression.
2. `set_gate_chain` applies per-gate upserts without a transaction: a storage
   failure mid-loop leaves a partially applied chain. Validation errors are
   atomic (all-or-nothing before mutation, tested); storage-error atomicity
   is not required by §Gate Chains and the spec's TOML surface has no
   transactional requirement. Worth a follow-up if config PUTs ever need
   transactional semantics.
3. `update_status(..., None /* started_at */, ...)` on the live approvals
   promotion NULLs `started_at`, but this exactly matches the pre-existing
   `run_gate` final-update convention in the base; not introduced by this
   task.

## Verdict

The two coverage sections are implemented with real production behavior:
real storage (SQL adapters + portable migration), real evaluation (live
distinct-approval counting, order-independent blocking decision), real
enforcement (merge blocked on required failure, author notified), real
attribution (server-derived from validated JWT claims), real audit capture
(retrievable conversation blob + audit event). Tests are behavior-killing
(full HTTP round-trips against real state, regression tests pinning both
orderings of the failure-precedence bug class and the advisory-never-blocks
bug class), not mirrored logic. Sections 6 and 7 of
`specs/coverage/system/agent-gates.md` marked `implemented` is accurate.
Approve, subject to the verifier running the recorded server test commands.
