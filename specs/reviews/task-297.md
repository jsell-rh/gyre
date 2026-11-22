# Review — task-297: Gate all implementation work on approved specs

Candidate: `b3ea371acc2269af44f83a39c9ec05e1060fbeed`
Base: `8546a781f772f05c823bbf4df6127bec812e2152`
Reviewer round: fresh independent review of the exact candidate (no earlier
approval carried forward). Evidence: `/tmp/stage/review-evidence/`
(audit.md, probes.md).

## Verdict: APPROVED

All three contract gates are real, fail-closed production implementations,
and the approval-side behavior is preserved unchanged.

### Gate 1 — task creation (REST + MCP)

`api/tasks.rs::create_task` and `mcp.rs::handle_create_task` both reject
`task_type=implementation` unless `spec_ref` passes the new
`api/gates.rs::verify_implementation_spec_ref`, which resolves `path@sha`
against BOTH the durable per-SHA approval row (the same
`verify_spec_ref` the merge gates use) and the registry entry (must be
`Approved` with `current_sha` exactly the pinned SHA — an approval
invalidated by a later push fails). The gate runs before any task row is
written; errors are 4xx (REST) / `SPEC_APPROVAL_REQUIRED` (MCP) and name
the requirement. Delegation/Coordination/untyped tasks are unaffected.
Storage errors fail closed.

### Gate 2 — scheduler (defense in depth)

`signal_chain.rs::spawn_implementation_workers` re-verifies each Backlog
Implementation task's `spec_ref` before claiming; on failure the task stays
Backlog with the observable `spec-approval-missing` label, a logged reason,
no agent record and no InProgress transition; the label is cleared on a
later successful claim. The cascade-test exemption is bounded to
(label present AND `spec_ref` absent): I verified `task_type` and
`spec_ref` are immutable after creation on every mutation surface (REST
`update_task` and MCP `handle_update_task` touch neither; all production
`task_type =` sites are new-task constructions), labels are mutable but a
gated task always retains its `spec_ref`, and both create surfaces reject
Implementation without an approved `spec_ref` regardless of labels — so the
exempt shape is reachable only through
`merge_processor::trigger_cascade_tests` (verified: sets Implementation +
cascade-test label, no `spec_ref`). The exemption is recorded as an inline
amendment to agent-runtime.md §1 reconciling a genuine conflict with
dependency-graph.md §Cascade Testing (task-280's forge-filed cascade
tests), following the repo's established amendment-note pattern. The
coordination leg of the legitimate chain was unblocked by pinning the
approved source `spec_ref` on Coordination tasks (inherited by their review
sub-tasks), with tests asserting the chain still spawns.

### Gate 3 — direct spawn

`api/spawn.rs::spawn_agent` returns 403 naming the single-trigger
requirement for non-system principals binding a worker to an Implementation
task; `is_system_principal()` is the unforgeable discriminator
(`agent_id == "system" && user_id.is_none()`). Interrogation spawns remain
(AC-required) and interrogation agents are write-denied with a 30-minute
TTL, so they cannot perform implementation work. The scheduler path calls
`spawn_agent_core` directly as the system principal and is unaffected.

### Approval side preserved

`specs.rs` (approve handler / SpecApproved emission), `spec_registry.rs`
(SHA-change → Pending) and `git_http.rs` (approval revocation) are absent
from the base..candidate diff.

### Tests

The gate tests assert observable outcomes and fail if the gates are
removed (twin approved/unapproved scheduler test, invalidated-by-push test,
cascade-label-no-bypass test, MCP reject-then-approve, real-approve-flow
HTTP test in `tests/api_integration.rs`, real-JWT human-principal 403 +
interrogation 201 spawn test). Test fixtures seed the durable approved
state the approve endpoint leaves behind.

### Probes run here

`cargo check -p gyre-server` (exit 0), `cargo fmt --check` (clean), and the
mechanical invariant scripts (arch, byte-slice, forged-scope, ABAC route
registry, inert-enforcement, mem-port-contracts, fail-open-ref-resolution,
fabricated-scope-defaults) all pass. Server test execution is blocked by
the sandbox build wrapper (4 GiB budget) — the required verifier runs are
listed in `/tmp/stage/review-evidence/probes.md`, including the full suite
(no CI run has executed the exact candidate tree yet; the retained CI log
for run 45049475667 shows exactly one failure on an ancestor, fixed by the
candidate's 5-line test-fixture seed `e167709b`, which mirrors the passing
sibling test).

Note: `check-task-commit-attribution.sh` fails only against the
working-tree copy of `specs/tasks/task-297.md`, which the controller reset
to the pristine assignment body (`commits: []`); the candidate commit
itself carries the complete 10-commit list.

### Non-blocking observations (recorded for future rounds, out of this
assignment's scope)

1. MCP `gyre_spawn_worker` (repo-orchestrator JWT) reaches
   `spawn_agent_core` directly without re-verifying the target task's spec
   approval — the assignment's direct-spawn gate is scoped to
   `POST /api/v1/agents/spawn` and the orchestrator chain is
   system-initiated, but adding `verify_implementation_spec_ref` there
   would be natural defense in depth.
2. `POST /repos/:id/graph/thorough-preview` (system-explorer.md §3,
   pre-existing at base) creates an Implementation task without `spec_ref`
   and spawns a worker with the caller's principal on a throwaway preview
   branch. Not named in the assignment or the discovery gap; a future spec
   reconciliation between system-explorer.md §3 and agent-runtime.md §1 is
   recommended.
3. An external caller can bind an interrogation agent to an Implementation
   task (assigning it and flipping Backlog→InProgress). The AC explicitly
   requires interrogation spawns to keep succeeding and the interrogation
   agent is write-denied, so no implementation work results — noted as a
   task-lifecycle side effect.
