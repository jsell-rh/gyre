# Review — task-297 (Gate all implementation work on approved specs)

Spec: `specs/system/agent-runtime.md` §1 The Signal Chain — "Spec approval is
the **single trigger** for all agent work… There is no other trigger. The
`SpecApproved` event is the universal starting signal." (gap
spec-41a92c20354c6a108e6074e3 / approval-single-trigger / 4a3f940495a8).

Candidate under review: `07b3b43b` (branch
`pipeline/task-297/30b62670102f442899c7e1a61ceffb29-1`, base `a2bd3eb0`;
product commits `d5662b9e`, `569ea138`, `6c7ce409`, `52397d16`, `ae87cf5b`,
`611bc5ff`, `bb42b05c`, `69394abf`, `f6a061eb`; process commits `721ba721`,
`07b3b43b` touch only `specs/tasks/task-297.md`).

Verdict: **approved** — all three assigned gates are real, fail-closed, and
tested with kill-shaped tests; the final round (`f6a061eb`) is a
formatting-only repair of the changed-lines rustfmt failures from
verification run `c98d76932dad471388e12011d0384d01`, verified to match the
formatter's output on exactly the flagged regions.

## Probes run (evidence: /tmp/stage/review-evidence/)

- Full diff read, base `a2bd3eb0` → candidate `07b3b43b` (18 files,
  +1685/−87; base confirmed an ancestor, so the diff is task-297's own work
  only — the task-288 content in the branch arrived via the base merge).
- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo check -p gyre-server` → exit 0.
- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo check -p gyre-server --tests` →
  exit 0 (lib + integration test targets type-check; test *execution* is
  deferred to the verifier per the 4 GiB budget — see below).
- Mechanical gates: check-arch, check-inert-enforcement,
  check-mem-port-contracts, check-task-commit-attribution,
  check-mcp-write-tools, check-abac-route-registry,
  check-abac-exempt-handlers → all exit 0.
- Formatting repair: retained failing-run log (`/tmp/stage/findings.log`)
  shows the run failed ONLY the changed-lines rustfmt gate on
  spawn.rs:3724-3725, merge_processor.rs:4118-4121,
  signal_chain.rs:3107-3112. `f6a061eb` touches exactly those regions
  (3 insertions, 12 deletions, join-only). Local rustfmt 1.10.0 emits no
  hunk in any of the three repaired regions (its remaining deltas are
  repo-wide pre-existing import-ordering style, present identically in
  untouched files such as `api/admin.rs` — the pinned formatter differs
  from local defaults). No behavior change: `07b3b43b`/`721ba721` after
  `f6a061eb` touch only the task markdown.
- Fixture SHA sweep: every `@<hex>` spec_ref fixture in gyre-server
  src+tests is exactly 40 hex chars (the gate rejects other lengths at
  runtime, so fixtures are self-checking in the verifier run).

## Verified working

**(1) Task-creation gate (REST + MCP).** Both `create_task`
(api/tasks.rs) and `handle_create_task` (mcp.rs) reject
`task_type=implementation` unless `spec_ref` resolves via
`verify_implementation_spec_ref` (api/gates.rs): TWO fail-closed checks —
an active durable per-SHA approval row (`verify_spec_ref`, the same
agent-gates.md §Spec Approval Ledger query the merge gates use, incl.
path@sha form and 40-hex validation) AND the registry entry still
Approved with `current_sha` exactly the pinned SHA (a later push moving
the entry to Pending/new blob fails even if a row revocation were
missed). The gate sits after `task_type`/`spec_ref` are bound from the
request and BEFORE `state.tasks.create` — no task row is written on
rejection; errors name the requirement (HTTP 400 InvalidInput; MCP
`SPEC_APPROVAL_REQUIRED` tool error). Delegation/Coordination/untyped
tasks are untouched. AC test `tasks_create_implementation_requires_approved_spec`
(api_integration.rs) drives the REAL approve endpoint
(`POST /api/v1/specs/:path/approve`) between the rejected and accepted
requests; `mcp_create_task_implementation_requires_approved_spec` covers
the MCP twin including no-row-written.

**(2) Scheduler gate (defense in depth).** The Phase-4 scan
(spawn_implementation_workers, signal_chain.rs) re-verifies each Backlog
Implementation task's spec_ref with the same two-check resolver BEFORE
the claim; on failure the task stays Backlog with the observable
`spec-approval-missing` label (persisted on a fresh re-read with
status/type/label guards, mirroring the budget-flag stale-snapshot
defense), a logged reason, no agent record, no InProgress transition;
the label is cleared on a later successful spawn. AC tests:
`scheduler_skips_unapproved_spec_and_spawns_approved_twin` (twin
inserted directly into storage, zero agents, Backlog + label; approved
twin spawns exactly one worker) and
`scheduler_stops_spawn_when_approval_invalidated_by_push` (registry
moved to a new blob + `revoke_all_for_path`, spawn stopped). The
legitimate chain is preserved, not just unbroken: the delegation leg
already pinned `path@sha`; this candidate adds the coordination leg's
pin (`run_workspace_orchestrator`) and the review sub-task's inheritance
(`assess_coordination`), and MCP `gyre_decompose_spec` now pins the
parent's approved version (parent `spec_ref`, else ledger-resolved
`spec_path` SHA) and REFUSES unpinnable parents rather than creating
dead Backlog rows — covered by `mcp_decompose_spec_refuses_unpinnable_parent`.

**Cascade-test exemption (spec amendment, `69394abf`).** The scheduler
gate exempts only forge-filed cascade-test tasks: BOTH the
`cascade-test` label AND the absence of a spec_ref. I verified the
exemption is unreachable from public surfaces: both task-creation
surfaces reject `implementation` without an approved `spec_ref`
regardless of labels, and `task_type`/`spec_ref` are not mutable after
creation (`UpdateTaskRequest` carries neither field), so
Implementation+no-spec_ref is reachable only through
`trigger_cascade_tests` (merge_processor.rs, task-280, already in base).
`cascade_label_does_not_bypass_spec_approval_gate` proves a labeled task
that pins ANY spec_ref is still gated. The amendment to
agent-runtime.md §1 is documented in-spec with this bounded
justification — a legitimate spec-lifecycle reconciliation with
dependency-graph.md §Cascade Testing, not a reinterpretation.

**(3) Direct-spawn gate.** The REST `spawn_agent` handler returns 403
naming the single-trigger requirement when a non-system principal
requests a worker bound to an Implementation task;
`agent_type=interrogation` and the genuine system principal
(`is_system_principal`: `agent_id == "system" && user_id.is_none()` —
not forgeable by a user named "system", verified in auth.rs) are exempt.
AC test `spawn_rejects_implementation_worker_for_external_callers`
covers the human-principal 403 (Keycloak user JWT), no-agent/no-claim
effects, and interrogation spawn success for an agent principal;
`agent_jwt_cannot_spawn_onto_foreign_task` was updated to assert the
gate's message and now probes the core's task-claim boundary directly so
that check keeps a kill test. Approval-side behavior (ledger row,
SpecApproved emission, modify→Pending invalidation) is untouched by the
diff.

**Test quality.** No test inflation found: the new tests are
kill-shaped (each fails if its gate is removed — twin/invalidation
scheduler tests assert zero agent records and Backlog status; the API
tests assert no task row plus the exact 4xx/403 and the post-approve
success; the MCP tests assert `isError` + no rows). Existing tests were
adjusted only by seeding the durable approval state the gate resolves
(`mem::seed_approved_spec`, #[cfg(test)]) and pinning fixtures to it —
assertions were strengthened, not weakened (all hunks read).

## Observations (non-blocking, recorded for future tasks)

- `POST /repos/:id/graph/thorough-preview` (api/graph.rs, pre-existing at
  base) still creates an Implementation task without spec_ref and spawns
  its worker via `spawn_agent_core` directly for an external caller. It
  is outside the four surfaces this task assigns (the spec-preview
  feature rides a throwaway branch per system-explorer.md §3, and the
  scheduler itself would skip its task: no spec_ref + non-cascade label).
- MCP `gyre_spawn_worker` (repo-orchestrator tier) delegates to
  `spawn_agent_core` and therefore does not re-run the scheduler's
  approval re-check. Also outside the assigned REST-endpoint gate;
  orchestrators are system-initiated principals, and the candidate
  documents the residual path in its test comments.
- Generated `web/dist/` may match the upstream bundle in this checkpoint
  (verifier-owned); no frontend source is touched by this task.

## Verifier must run

- `cargo nextest run -p gyre-server signal_chain::`
- `cargo test -p gyre-server --test api_integration tasks_create_implementation_requires_approved_spec`
- `cargo test -p gyre-server --lib mcp::tests::mcp_create_task_implementation_requires_approved_spec mcp_decompose_spec_refuses_unpinnable_parent`
- `cargo test -p gyre-server --lib api::spawn::tests::spawn_rejects_implementation_worker_for_external_callers agent_jwt_cannot_spawn_onto_foreign_task`
- the changed-lines rustfmt gate and full checks.sh (the prior run's only
  failure, repaired by `f6a061eb`)
