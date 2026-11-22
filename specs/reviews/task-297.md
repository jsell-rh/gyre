# Review — task-297 (Gate all implementation work on approved specs)

Spec: `specs/system/agent-runtime.md` §1 The Signal Chain ("There is no
other trigger. The `SpecApproved` event is the universal starting signal.").
Candidate under review: `8127a199dd362251025394363d32b80d41fab597`
(branch `pipeline/task-297/049ea6fd951d41888e85862c8709d20e-2`, base
`f0f2ba5da6355cd7ed059f095de03848f3bada50`; product commits `d5662b9e` +
`569ea138`, both attributed in the candidate's task frontmatter).
Verdict: **approved** — all three required gates are real, fail-closed
enforcement against durable state; the approval side is untouched; the
fixture migration is behavior-preserving with no weakened assertion.

## Probes (evidence: /tmp/stage/review-evidence/)

- `/tmp/stage/bin/cargo check -p gyre-server --lib --tests` → exit 0
  (9m40s; second identical run 0.38s, proving the lib, cfg(test) unit
  tests, and all integration-test targets were checked — the follow-up
  commit `569ea138` postdates the prior agent's OOM-interrupted check, so
  this was re-established fresh).
- `check-arch`, `check-inert-enforcement`, `check-abac-route-registry`,
  `check-mcp-write-tools`, `check-fail-open-ref-resolution`,
  `check-forged-scope-fields` → all exit 0.
- `check-task-commit-attribution` → exit 1, but purely a stage artifact:
  the working tree carries the harness's uncommitted reversion of
  `specs/tasks/task-297.md` to its pre-assignment content; the candidate
  commit itself lists both implement commits (`git show
  8127a199:specs/tasks/task-297.md`), which is exactly the required set.
- Server test execution is wrapper-blocked in this stage; the required
  commands are recorded in review-evidence/deferred-server-tests.txt
  (new tests: `tasks_create_implementation_requires_approved_spec`,
  `mcp_create_task_implementation_requires_approved_spec`,
  `scheduler_skips_unapproved_spec_and_spawns_approved_twin`,
  `scheduler_stops_spawn_when_approval_invalidated_by_push`,
  `spawn_rejects_implementation_worker_for_external_callers`).

## Verified

1. **Task-creation gate.** `verify_implementation_spec_ref`
   (api/gates.rs) checks BOTH the durable per-SHA approval row
   (`verify_spec_ref` — the merge-gate ledger query) and the registry
   entry (must be Approved with `current_sha` exactly the pinned SHA, so
   a later push that moves the blob/Pending fails even if a row
   revocation were missed); every failure arm returns Err (fail-closed,
   including registry lookup errors). Both `create_task`
   (api/tasks.rs — 400 InvalidInput naming the single-trigger
   requirement, before any task row is written) and MCP
   `gyre_create_task` (SPEC_APPROVAL_REQUIRED tool error before the
   create) enforce it for `task_type: implementation` only;
   Delegation/Coordination/untyped tasks are unaffected. No mutation
   bypass exists: neither REST `update_task` nor MCP `gyre_update_task`
   accepts task_type/spec_ref. MCP `gyre_decompose_spec` sub-tasks now
   pin the parent's spec_path/spec_ref so the legitimate chain passes.
2. **Scheduler gate.** `spawn_implementation_workers` (signal_chain.rs)
   re-verifies the spec_ref before repo resolution/claim; on failure the
   task stays Backlog with the observable `spec-approval-missing` label
   (persisted from a fresh re-read only while still Backlog+
   Implementation) and a logged reason — no agent record, no InProgress;
   the label clears on a later successful spawn alongside
   `budget-exhausted`. The unapproved twin test inserts the task directly
   into storage (the verification-intent scenario) and the
   push-invalidation test moves the registry to a new SHA and revokes
   the approval rows.
3. **Direct-spawn gate.** POST /api/v1/agents/spawn returns 403 naming
   the requirement for any request that would bind a worker to an
   Implementation task from a non-system principal, unless
   `agent_type: interrogation`. `is_system_principal()` is the genuine
   global-token discriminator (`agent_id == "system" && user_id.is_none()`,
   spoof-shape test pre-exists). The in-process scheduler, Ralph respawn,
   and orchestrator paths call `spawn_agent_core` directly and are
   unaffected; MCP `gyre_spawn_worker` remains repo-orchestrator-token-
   only with the core's task-claim boundary still directly probed by the
   updated `agent_jwt_cannot_spawn_onto_foreign_task`.
4. **Approval side preserved.** No changes to the approve handler,
   spec_registry sync, or post-receive revocation in the diff.
5. **Coverage/task hygiene.** Rows 3 and 7 of
   specs/coverage/system/agent-runtime.md gained only task-297 evidence
   matching the code; the fixture migration (spawn.rs/mcp.rs/admin.rs/
   search.rs + 5 integration files, `mem::seed_approved_spec` cfg(test)
   mirroring the approve endpoint's durable state) weakens no assertion
   — the previously-CREATED own-task respawn expectations were correctly
   flipped to 403 per the new contract, with a direct core probe keeping
   the claim boundary tested.

Minor (non-blocking, recorded for completeness):

- The spec-ledger `find_by_path` port is path-keyed globally (pre-existing
  port semantics shared with the merge gates' `verify_spec_ref`), so the
  gate does not additionally scope the registry check to the caller's
  workspace; a cross-workspace approval reference would still fail at
  decomposition (spec blob read from the task's own repo) and the task
  text does not require that scoping.
- graph.rs thorough-preview (system-explorer.md §3) still creates a
  labeled `spec-preview` Implementation task and spawns via the core
  in-process with rollback — a specced preview exception outside this
  task's required gates.
