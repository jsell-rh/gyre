# Review — task-297 (Gate all implementation work on approved specs)

Spec: `specs/system/agent-runtime.md` §1 The Signal Chain — "There is no other
trigger. The `SpecApproved` event is the universal starting signal."
(gap `spec-41a92c20354c6a108e6074e3` / approval-single-trigger / `4a3f94049a8`).
Candidate under review: `d6f85599cbf1c7c8bad594d7c8b254d15d2d1082` (branch
`pipeline/task-297/b58440c70f674c6583e67d0b2a7c143a-1`, base
`2b5189b913ab83590c1cd9c972db0fdb17e04d66`; product commits `d5662b9e`,
`569ea138`, clippy repair `6c7ce409`, process/docs commits `549e8c2e`,
`ab692ab0`, `8127a199`, `87d20717`, `bf0a84b8`, `d6f85599`).
Verdict: **approved** — all three required gates are real, fail-closed, correctly
placed, and covered by tests that fail if the gate is removed; no findings.

## Round 1

Probes run (checkout at candidate `d6f85599`; the working tree carries only the
controller's reset of `specs/tasks/task-297.md` frontmatter, which is not part
of the product diff):

- `/tmp/stage/bin/cargo check -p gyre-server` → **exit 0** (8m00s).
- `/tmp/stage/bin/cargo check -p gyre-server --tests` → **exit 0** (4m06s) —
  all new/modified lib and integration test code type-checks, including the
  `seed_approved_spec` fixtures.
- `cargo test -p gyre-server` → **blocked by the executor build wrapper**
  (gyre-server codegen + model exceeds 4 GiB). Not a code defect; the required
  verifier probes are recorded in `/tmp/stage/review-evidence/blocked-test-probe.txt`.
- `scripts/check-arch.sh`, `check-inert-enforcement.sh`,
  `check-abac-exempt-handlers.sh`, `check-mem-port-contracts.sh`,
  `check-abac-route-registry.sh`, `check-mcp-write-tools.sh`,
  `check-dead-message-kinds.sh` → all exit 0.
- Full diff read of every touched file (gates.rs, tasks.rs, mcp.rs, spawn.rs,
  signal_chain.rs, mem.rs, admin.rs, search.rs, all five test targets, coverage
  rows 3 and 7 of `specs/coverage/system/agent-runtime.md`); grep sweeps for
  every `tasks.create` / `TaskType::Implementation` production callsite, the
  task-update path, remaining unseeded implementation-task creations in tests,
  and CLI/web task-creation surfaces.
- Evidence saved under `/tmp/stage/review-evidence/`.

Verified working (no findings):

- **Shared verifier is real and fail-closed.**
  `verify_implementation_spec_ref` (api/gates.rs) resolves a `path@sha` against
  BOTH (1) the durable per-SHA approval row via the pre-existing
  `verify_spec_ref` — the same ledger query the merge gates use
  (`find_by_spec_sha` + `is_active()`: approved_at set, not revoked/rejected;
  format and 40-hex validation) — and (2) the registry entry for the path,
  requiring `approval_status == Approved` AND `current_sha` exactly the pinned
  SHA, so an approval invalidated by a later push (registry sync → Pending +
  new `current_sha`, post-receive row revocation) fails even if one mechanism
  were ever missed. Lookup errors return Err (fail-closed). The
  `rsplit_once('@').expect(...)` is post-validation (verify_spec_ref's success
  guarantees the split), consistent with the same parse in verify_spec_ref.
- **Gate 1 — task creation (AC 1).** REST `create_task` (api/tasks.rs) parses
  `task_type` and `spec_ref` from the body, then — before
  `state.tasks.create` — rejects `task_type: implementation` with 400
  `InvalidInput` naming the single-trigger requirement and the approve
  endpoint; missing spec_ref also rejects. MCP `gyre_create_task` (mcp.rs)
  applies the identical gate before its create, returning a
  `SPEC_APPROVAL_REQUIRED` tool error. Delegation/Coordination/untyped are
  unaffected. No bypass found: `update_task` (PUT) cannot set `task_type` or
  `spec_ref`; the only other production `tasks.create` sites (gate_executor,
  dep_staleness, compose, orchestrator `cross_repo_task_core`) create untyped
  or Coordination tasks only; the orchestrator chain (signal_chain.rs) and the
  MCP `gyre_decompose_spec` sub-tasks pin the parent's approved
  path@sha spec_ref (the MCP decompose handler now propagates
  `spec_path`/`spec_ref`), so the legitimate chain passes. AC 1 is covered
  end-to-end by `tasks_create_implementation_requires_approved_spec`
  (api_integration.rs) which drives the REAL approve endpoint
  (`POST /api/v1/specs/:path/approve` — verified statically: sha-must-match-
  current check, durable ledger row create+approve, registry status update)
  and asserts 400 + no task row before, 201 + pinned spec_ref after;
  `mcp_create_task_implementation_requires_approved_spec` covers the MCP twin
  (isError, message content, no task row; success after seeding the durable
  approval state).
- **Gate 2 — scheduler, defense in depth (AC 2).** The Phase-4 scan
  (`spawn_implementation_workers`, signal_chain.rs) re-verifies the spec_ref
  before claiming: on failure it re-reads the freshest row (same
  stale-snapshot defense as the budget flag), adds the observable
  `spec-approval-missing` label only when still Backlog/Implementation and not
  already labeled, logs the reason (`tracing::info!` with task_id, spec_ref,
  reason), and `continue`s — no agent record, no InProgress transition, no
  budget consumed. The label is cleared on a later successful spawn (same
  retain as the budget flag). Covered by
  `scheduler_skips_unapproved_spec_and_spawns_approved_twin` (Pending registry
  entry + no ledger row inserted directly into storage → zero agents, stays
  Backlog, label present; approved twin → exactly one worker, InProgress) and
  `scheduler_stops_spawn_when_approval_invalidated_by_push` (registry moved to
  Pending + new SHA, rows revoked via `revoke_all_for_path` → no spawn).
  Removing the gate makes the twin test fail (twin A would spawn).
- **Gate 3 — direct spawn (AC 3).** The REST `spawn_agent` handler rejects a
  worker bound to an Implementation task with 403 naming the single-trigger
  requirement for every non-system principal (`is_system_principal()` is the
  genuine discriminator: global token's `agent_id == "system"` AND
  `user_id.is_none()` — not a spoofable name match); `agent_type:
  "interrogation"` is exempt (exact-match, consistent with the core's own
  interrogation check at spawn.rs:723), keeping the Phase-8 kind working for
  agent principals; task-lookup failure fails closed (Internal). The
  task-claim boundary inside `spawn_agent_core` is untouched and now has a
  direct core probe (foreign-task spawn via a task-scoped principal asserts
  the claim-scoping error), so it stays tested even though the REST surface
  now rejects earlier. Covered by
  `spawn_rejects_implementation_worker_for_external_callers`: a verified
  Keycloak user JWT (Developer role) spawning onto an Implementation task →
  403 + message + no agent record + task stays Backlog unassigned; an agent
  principal spawning `agent_type: "interrogation"` on the same surface → 201.
  Removing the gate block makes the test fail at the first FORBIDDEN assert.
- **Approval side preserved.** The diff does not touch the approve handler's
  ledger/emission logic, `spec_registry.rs` invalidation, or the post-receive
  revocation path.
- **No test inflation.** The new tests assert observable outcomes (status
  codes, error text naming the requirement, absence of task rows/agent
  records, Backlog status, skip label, exactly-one-worker) that break when the
  corresponding gate is removed. Pre-existing tests were amended only by
  seeding the durable approval state and pinning spec_refs (the exact state
  the approve endpoint leaves behind: registry entry Approved at the SHA plus
  an active ledger row) — no assertion weakened; the task-263 own-task respawn
  expectation was changed to 403 because the spec now requires exactly that,
  with a compensating direct-core probe added for the claim boundary.
- **Coverage rows 3 and 7** of `specs/coverage/system/agent-runtime.md` were
  updated with concrete source evidence and test names that exist and
  type-check; no unrelated rows touched.

Findings: none.

Minor (non-blocking, recorded for completeness — none contradict the task
contract or the canonical spec within the assigned scope):

- The MCP `gyre_spawn_worker` orchestrator tool calls `spawn_agent_core`
  directly and is not spec-gated. Out of the task's named scope (the contract
  names POST /api/v1/agents/spawn); the tool requires a repo-orchestrator
  token, which is system-minted on the SpecApproved chain, and the
  task-creation/scheduler gates bound the tasks such an orchestrator would
  legitimately see. Worth a follow-up if the single-trigger property is ever
  extended to agent-principal spawn surfaces.
- The thorough-preview surface (api/graph.rs `thorough_preview`) creates an
  Implementation task in storage (no spec_ref) and spawns via the core
  in-process. Pre-existing, deliberately ceremony-free per meta-spec
  reconciliation §5 (preview runs skip ceremony; the preview branch is the
  deliverable), and outside the three gated surfaces named by the task.
- The web UI create-task form (web/src/components/RepoMode.svelte) still sends
  `task_type: 'implementation'` with `spec_path` but no `spec_ref`; the server
  now (correctly) rejects those requests with a descriptive error. Frontend
  follow-up to pin an approved `spec_ref`, not a server-side defect.
- `verify_implementation_spec_ref` resolves the ledger by path only (no
  caller-scope validation of the spec's workspace/repo), so a task could pin
  another tenant's approved path@sha. This is the pre-existing path-keyed
  ledger semantics shared with the merge gates (`verify_spec_ref`), not
  introduced by this task; the task text specifies exactly the
  ledger-at-pinned-SHA check that was implemented.

Verifier must run (recorded in
`/tmp/stage/review-evidence/blocked-test-probe.txt`): the five new task-297
tests listed there plus the full `cargo test -p gyre-server` suite (lib + all
integration targets) for fixture-migration regressions.
