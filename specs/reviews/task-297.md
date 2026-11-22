# Review — task-297 (Gate all implementation work on approved specs)

Spec: `specs/system/agent-runtime.md` §1 The Signal Chain ("There is no
other trigger. The `SpecApproved` event is the universal starting
signal.") — task-creation gate, Phase-4 scheduler gate (defense in
depth), direct-spawn gate; approval side to be preserved unchanged.
Candidate under review: `9ee8f1db` (branch
`pipeline/task-297/e3c3fa3fb77d4bd99d65c3bee1d666e7-1`, base
`73c504f2`; product commits `569ea138`, `d5662b9e`, `6c7ce409`,
`52397d16`, `ae87cf5b`, `611bc5ff` plus process/merge commits).
Verdict: **approved** — all three contract gates verified by inspection
against the production code paths, compile and mechanical checks green;
server test execution mechanically deferred to the independent verifier
(build wrapper rejects gyre-server test codegen in the review stage).

## Round 3 (this review)

Probes run (checkout at candidate `9ee8f1db`, clean tree):

- `/tmp/stage/bin/cargo check -p gyre-server --tests` → exit 0 (10m18s).
- `rustfmt --check` on the six touched Rust files → exit 0.
- Mechanical scripts, all exit 0: check-arch, check-inert-enforcement,
  check-mem-port-contracts, check-forged-scope-fields,
  check-in-memory-state-stores, check-fail-open-ref-resolution,
  check-scope-literal-defaults, check-fabricated-scope-defaults,
  check-lossy-secret-conversion, check-mcp-write-tools,
  check-abac-route-registry, check-dead-message-kinds.
- Full diff read; grep sweeps proving every implementation-task test
  fixture now seeds/pins an approved spec, and that
  `UpdateTaskRequest` cannot set `task_type`/`spec_ref` (no
  create-then-PATCH bypass).
- Evidence: `/tmp/stage/review-evidence/task-297-review-round3.md`.

Verified working (no findings):

- **Task-creation gate.** `verify_implementation_spec_ref`
  (api/gates.rs) performs two independent fail-closed checks — the
  durable per-SHA approval row (`verify_spec_ref`, which validates the
  path@sha form and 40-hex SHA) and the registry entry (Approved at
  exactly the pinned `current_sha`, so a later push invalidating the
  approval fails even if a row revocation were missed). Both
  `POST /api/v1/tasks` (api/tasks.rs, after scope validation, before
  any row write) and MCP `gyre_create_task` (mcp.rs, before
  `tasks.create`) reject with errors naming the requirement and the
  approve endpoint. Tested end-to-end against the REAL approve endpoint
  by `tasks_create_implementation_requires_approved_spec`
  (api_integration.rs) and by
  `mcp_create_task_implementation_requires_approved_spec` (no task row
  on rejection; identical request succeeds after approval).
- **Scheduler gate.** `spawn_implementation_workers` (signal_chain.rs)
  verifies each Backlog Implementation task's spec_ref before
  repo/order/budget checks and the claim; on failure the task stays
  Backlog with the observable `spec-approval-missing` skip label
  (freshest-row re-read, same stale-snapshot defense as the budget
  flag) and a logged reason — no agent record, no InProgress; the label
  clears on a later successful spawn. Tested by
  `scheduler_skips_unapproved_spec_and_spawns_approved_twin` (zero
  agents for the unapproved twin, exactly one worker for the approved
  twin) and
  `scheduler_stops_spawn_when_approval_invalidated_by_push`
  (registry moved to Pending + rows revoked).
- **Direct-spawn gate.** REST `spawn_agent` (api/spawn.rs) rejects a
  non-system principal's worker spawn onto an Implementation task with
  403 naming the single-trigger requirement; `agent_type=
  interrogation` spawns remain (Phase-8 system-initiated kind, and
  interrogation agents cannot perform implementation work — the spawn
  core fail-closed-creates their Deny write/delete/spawn/approve/merge
  ABAC policies). `spawn_rejects_implementation_worker_for_external_
  callers` covers the human-JWT 403 (message-asserted, so an ABAC 403
  cannot masquerade), no side effects, and the interrogation 201 for an
  agent principal.
- **Prior review round's finding (coordination-leg stall) genuinely
  repaired.** `run_workspace_orchestrator` pins the Coordination task's
  `spec_ref = spec_path@spec_sha` — the approved SOURCE version, the
  same values the delegation leg pins — and `assess_coordination`
  propagates it to the Implementation review sub-task, so the
  legitimate SpecApproved-derived chain passes the Phase-4 gate instead
  of stalling Backlog forever. `gyre_decompose_spec` resolve-and-pins
  (parent `spec_ref`, else the ledger's current SHA) and refuses
  unpinnable parents with a tool error before writing any sub-task row,
  mirroring `decompose_delegation`'s fallback and empty-SHA hard-fail;
  docs/api-reference.md documents the semantics.
  `scheduler_decomposes_delegation_into_ordered_subtasks` — the test
  that failed CI at the prior candidate 759f075b — now asserts BOTH the
  delegation and coordination review sub-tasks spawn, with finds scoped
  by `parent_task_id` (deterministic against the mem store's HashMap
  ordering); `scheduler_processes_coordination_task` asserts the same
  across two cycles.
- **Approval side preserved.** specs.rs / spec_registry.rs untouched by
  the diff; `approve_spec` still records the durable ledger row,
  resolves the registry status by approval mode, and emits SpecApproved
  only on transition INTO Approved with a workspace scope.
- **No test inflation / no assertion weakening.** Every new gate test
  fails at a candidate with the corresponding production behavior
  disabled; all pre-existing assertions were retained across the
  fixture updates, and the parent-scoped `find` narrowing (ae87cf5b)
  fixes nondeterministic twin selection without dropping any
  delegation-twin assertion.
- **Coverage rows 3/7** (specs/coverage/system/agent-runtime.md) updated
  with accurate source evidence; task record attributes all six product
  commits, all present between base and candidate.

Minor (non-blocking, recorded for completeness):

- The spec approval ledger is keyed globally by (path, sha);
  `verify_implementation_spec_ref` does not bind the approved spec's
  workspace/repo scope to the task's or caller's scope. This mirrors
  the pre-existing merge-gate semantics of `verify_spec_ref` and the
  task contract defines the gate as path@sha resolution; cross-workspace
  references are additionally blunted because worker prompts and
  decomposition read the spec blob from the task's own repo. A future
  hardening task could scope-bind approvals.
- `POST /repos/:id/graph/thorough-preview` still spawns an
  implementation worker for a draft spec without approval — pre-existing
  and spec-sanctioned (system-explorer.md §3 thorough preview; meta-spec
  reconciliation preview ceremony skip; throwaway branch), unchanged by
  this candidate and outside the three enumerated gate surfaces.
- MCP `gyre_spawn_worker` (repo-orchestrator tier) calls
  `spawn_agent_core` directly and is therefore not subject to the REST
  direct-spawn gate; the contract gates the REST surface only, and the
  Phase-4 scheduler gate remains the enforcement backstop for spawns on
  unapproved spec_refs.

## Verifier must run

gyre-server test execution is mechanically deferred (the review-stage
build wrapper rejects server test codegen; compile via
`cargo check -p gyre-server --tests` is green):

1. `cargo nextest run -p gyre-server signal_chain::tests` —
   `scheduler_decomposes_delegation_into_ordered_subtasks`,
   `scheduler_processes_coordination_task`,
   `scheduler_skips_unapproved_spec_and_spawns_approved_twin`,
   `scheduler_stops_spawn_when_approval_invalidated_by_push`, plus the
   pre-existing Phase-4 suite.
2. `cargo nextest run -p gyre-server mcp::tests` —
   `mcp_create_task_implementation_requires_approved_spec`,
   `mcp_decompose_spec_creates_subtasks_in_repo_scope`,
   `mcp_decompose_spec_refuses_unpinnable_parent`.
3. `cargo nextest run -p gyre-server api::spawn::tests` —
   `spawn_rejects_implementation_worker_for_external_callers`,
   `agent_jwt_cannot_spawn_onto_foreign_task`.
4. `cargo nextest run -p gyre-server --test api_integration` —
   `tasks_create_implementation_requires_approved_spec`.
5. Full `cargo nextest run --all` (prior round's CI run was cancelled
   fail-fast).
