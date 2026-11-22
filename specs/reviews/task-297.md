# Review — task-297 "Gate all implementation work on approved specs"

- Candidate: `80a69f8ac4b99bf4503f3ee7ef051e04af0c4346` (base `80439c18`)
- Round: CI-repair round (`ae87cf5b` test scoping + `80a69f8a` process notes)
- Verdict: **not approved** — 1 blocking finding
- Evidence: `/tmp/stage/review-evidence/task-297-review.md`

## What was verified as real

All three gates are genuine production enforcement, not stubs:

1. **Task-creation gate.** `verify_implementation_spec_ref`
   (api/gates.rs) fail-closes on BOTH the durable per-SHA approval row and
   the registry entry (Approved at exactly the pinned SHA — a later push
   moving `current_sha`/Pending fails). The REST handler (api/tasks.rs)
   returns 4xx before any task row is written; MCP `gyre_create_task`
   returns a `SPEC_APPROVAL_REQUIRED` tool error before the row is
   written. `update_task` cannot bypass (no task_type/spec_ref in
   `UpdateTaskRequest`). Tested against the REAL approve endpoint
   (`tasks_create_implementation_requires_approved_spec`) and the MCP
   approve-then-retry flow.
2. **Scheduler gate.** `spawn_implementation_workers` re-verifies before
   claiming; failure leaves the freshest Backlog row labeled
   `spec-approval-missing` with a logged reason, no agent, no InProgress;
   the label clears on a later successful claim. Twin tests
   (unapproved/approved, invalidated-by-push) are real behavioral probes.
3. **Direct-spawn gate.** REST spawn rejects worker spawns onto
   Implementation tasks for non-system principals (403 naming the
   requirement), interrogation spawns still succeed; MCP
   `gyre_spawn_worker` remains orchestrator-token-only (system chain).

The CI-repair commit is test-only and correct: `MemTaskRepository::list`
iterates a HashMap (mem.rs:1077/1104), the old unscoped `find` flipped
between the delegation sub-task and the gated coordination twin, and the
new `parent_task_id == delegation.id` scoping preserves the assertion
strength. No earlier approval transfers; this verdict was re-derived from
the current candidate's source.

Probes: `cargo check -p gyre-server --tests` exit 0, `cargo fmt --check`
clean, check-arch / inert-enforcement / mcp-write-tools /
abac-exempt-handlers / lossy-secret / scope-defaults / dead-kinds all
exit 0. gyre-server test execution is barred by the build wrapper in this
sandbox — required deferred probes are listed in the evidence file.

## Blocking finding

**The coordination leg of the legitimate chain is permanently stalled.**
The workspace orchestrator creates the Coordination task with `spec_path`
only (no `spec_ref`), `assess_coordination` propagates the parent's
`None` to its review sub-task (`task_type: Implementation`), and the new
Phase-4 gate then holds that sub-task Backlog forever with
`spec-approval-missing` — no agent, no recovery path (`UpdateTaskRequest`
cannot set `spec_ref`; nothing re-pins existing tasks). This contradicts
the task contract's own claim that "the orchestrator chain ... pins the
approved SHA, so the legitimate chain passes the gate" and that
Coordination tasks are "unaffected", and contradicts canonical
agent-runtime.md §1 (Phase 2 → Phase 4: coordination sub-tasks flow to
mechanical worker spawning). Reproducible from the task's own fixture
(the "gated coordination twin" the CI-repair commit documents); at the
base the sub-task spawned a worker. Fix: pin
`spec_ref = spec_path@spec_sha` on the Coordination task at creation
(both values in scope — they are already embedded in the task's
description), so the review sub-task inherits the approved pinned ref and
the gate resolves it; the same resolve-and-pin already exists in
`decompose_delegation` for the delegation leg. Sibling site: MCP
`handle_decompose_spec` blindly clones the parent's `Option<spec_ref>`
with no ledger fallback, so sub-tasks of a spec_path-only delegation
stall the same way — fix together.

The candidate's rationalization ("that is the specced single-trigger
behavior, not a defect") reinterprets the canonical Phase-2→Phase-4 flow
without a spec amendment — the repo goal explicitly bans implementing
around a spec that way. Fail-closed is the right default for the gate,
but here the approved ref the sub-task derives from is known at creation
time and simply was not pinned.

## Non-blocking observations

- `verify_implementation_spec_ref` resolves the ledger by path only (no
  workspace scoping); the task contract defines the ledger as the
  criterion, so this matches the assignment, but cross-workspace pinning
  within a tenant is accepted by the gate by design.
- Coverage rows 3/7 in specs/coverage/system/agent-runtime.md were updated
  with concrete source evidence and test names; the claims match the code
  except that row 3's "the legitimate chain passes the gate" repeats the
  violated invariant above.
