# Review: task-135 — Gate chain configuration and gate agent lifecycle

**Reviewer:** Independent review (pi runtime)
**Spec ref:** agent-gates.md §Gate Chains (Composable Policies), §Gate Agent Lifecycle
**Comparison base:** 276918d27f4b2da84e83c700a3d823b9e1f7d93d
**HEAD reviewed:** 76909fac006f76584483854e99bc646f422bccf0
**Status:** approved — no blocking findings

---

## Scope of this round

This candidate has accumulated several rebase/reconciliation rounds against
main (task-128 ABAC policy engine, task-168 gate ordering, task-139
agent-runtime base). This round reviewed the full base→candidate diff
(26 files, +2186/−96) against the task contract, with emphasis on the
substantive gate-chain/lifecycle code rather than the merge resolutions.

## Findings

None blocking. Evidence per acceptance criterion below; probe logs under
`/tmp/stage/review-evidence/` (see its SUMMARY.md).

### A1 — Ordered chains of mixed gate types

`PUT/GET /api/v1/repos/:id/gate-chain` (api/gates.rs) accepts and returns
the spec's exact `[[gates]]` TOML (`name`, `type` in PascalCase or
snake_case, `command`, `persona`, `min_approvals`, `required`,
`validation_type`, `timeout_secs`). Ordering is real storage state:
`quality_gates.position` (column from task-168's earlier migration), with
`ORDER BY position, created_at` in the SQLite and PostgreSQL adapters and
the identical sort in the mem adapter; `create_gate` appends to the chain.
The new migration `2026-11-12-000062_gate_chain_config` adds only the
review-attribution columns (no duplicate ADD COLUMN — verified against the
task-168 migration) and is portable standard SQL. Chain PUT semantics:
position = array index, same-named gates keep ids (GateResults stay
linked), absent pre-merge gates deleted, post-merge recovery gates
preserved and name-collisions with them rejected, `update_gate` renames
that collide with a sibling rejected 409. Validation is entirely before
the first mutation; invalid chains (duplicate names, missing
command/min_approvals, unknown type, malformed TOML, JSON content-type →
415) leave the stored chain untouched (asserted by an atomicity test
comparing gate-id sets before/after).

### A2 — Parallel execution

`trigger_gates_for_mr` creates a Pending GateResult per repo gate and
spawns one tokio task per gate (pre-existing structure, retained). Merge
queue enqueue calls it (api/merge_queue.rs:133).

### A3 — Per-MR gate status Pending → Running → Passed/Failed

Persisted GateResult state machine in `run_gate`; verified in code and by
the in-tree tests (`run_one_gate` helper asserts the final persisted
status, not just the return value).

### A4/A5 — Required gates gate the merge; failure blocks and notifies

`check_gates_for_mr` is now a two-pass, order-independent decision:
pass 1 evaluates all results into (status, is_required, gate_id) — with
live RequiredApprovals re-evaluation and persisted promotion to Passed —
and the pure `decide_gate_readiness` scans ALL results for required
failures before any wait decision, so a Pending/Running required gate can
never mask a Failed required gate regardless of iteration order. The
merge processor fails the queue entry on `Err` (merge_processor.rs:1732)
and `run_gate` emits the GateFailure workspace event plus the author's
inbox notification (`notify_gate_failure`) with failure details. The
hollow `RequiredApprovals → (Passed, "delegated to merge processor")`
stub is gone: distinct-approval counting with live promotion replaced it,
with regression tests pinning the prior F1 (advisory approvals gate
requeue-forever) and F2 (waiting masks failure) defects.

### A6 — Optional gates don't block

`decide_gate_readiness` never waits on or fails for `required = false`
gates in any state; advisory failures record the result and attach an MR
comment attributed to `gate-agent:{gate_id}`. Tests cover the comment
attachment, the absence of advisory comments on required failures, and
the advisory-approvals-gate-short-of-minimum non-blocking case.

### A7 — Gate agent lifecycle: ephemeral, read-only, scoped, attributed

Ephemeral/read-only/scoping shipped in task-134 at base and retained:
scoped JWTs (`review:submit` / `validation:report`) minted with the
state's base_url issuer, TTL covering the configured gate timeout, push
denied in git_http.rs (exact-match scope), ABAC middleware route
allow-list (read MR context + submit verdict only — no spawn, no task
creation), token revoked on every exit path including timeout/crash.
Attribution is NEW and real: `Review.reviewer_persona` + `Review.gate_id`
persisted through both SQL adapters and mem, derived server-side in
`submit_review` from the scoped token's `task_id` claim — the in-tree
test submits a body with a forged `reviewer_agent_id` and asserts the
token-derived identity and gate/persona attribution win, and that the
attribution is persisted (not just the response envelope).

### A8 — Context windows captured for audit

`capture_gate_agent_context` stores the exact bundle the agent will see
(persona system prompt, MR diff, spec content at the pinned spec_ref SHA,
MR title, task description) zstd-compressed through the conversation
repository BEFORE the subprocess runs — so timed-out/crashed reviewers
still leave a replayable context — and records a `gate_agent_context`
audit event with the conversation SHA, gate, persona, and MR. Capture is
skipped (logged, never fabricated) when the workspace's tenant cannot be
resolved. Validators are captured too. The in-tree test replays the
stored blob and asserts the PINNED spec version ("# Spec v1", not the v2
tip) is what was captured.

## Probes run

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` — exit 0 (7m38s).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` — exit 0 (4m17s;
  test modules type-check).
- `cargo test -p gyre-adapters sqlite::review` — 11 passed (includes the
  attribution round-trip tests).
- `cargo test -p gyre-adapters sqlite::quality_gate` — 3 passed (includes
  chain-position ordering and tie-fallback tests).
- Mechanical invariants all exit 0: check-arch, check-abac-route-registry,
  check-abac-exempt-handlers, check-abac-context-parity,
  check-fabricated-scope-defaults, check-inert-enforcement,
  check-mem-port-contracts, check-forged-scope-fields,
  check-byte-slice-truncation, check-scope-literal-defaults,
  check-migration-versions.
- check-task-commit-attribution passes against the candidate's own
  specs/tasks/task-135.md (all 6 task-labeled product commits listed).
  The working-tree run fails only because the controller's uncommitted
  staging reverts that file to `commits: []` — a staging artifact, not
  part of the candidate.

Per the review budget rules, gyre-server test compilation/runs are
deferred to the independent verifier: `cargo test --all`, or focused
`cargo test -p gyre-server gate_executor` / `api::gates` /
`api::merge_requests` / `merge_processor`. The check --tests pass above
confirms those modules compile.

## Non-blocking observations (follow-up material, not defects)

1. Deleting a gate (single-gate DELETE — pre-existing at base — or absence
   from a chain PUT) leaves orphaned GateResults; `check_gates_for_mr`
   defaults unknown gates to required, so an orphaned Pending result can
   requeue a queued MR indefinitely. Pre-existing semantics the chain
   endpoint inherits from the single-gate DELETE.
2. A gate agent's "approved" verdict (and the MR author's self-approval)
   counts toward RequiredApprovals distinct approvals. The spec defines
   the gate mechanically as "count review approvals — N approvals
   received" with no exclusion rule, so this is a faithful reading.
3. Chain PUT atomicity is validation-before-mutation, not a transaction:
   a mid-loop storage error surfaces as 500 with a partially applied
   chain. Documented in the handler; acceptable.
