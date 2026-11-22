# Review — task-297 (Gate all implementation work on approved specs)

Spec: `specs/system/agent-runtime.md` §1 The Signal Chain — "There is no
other trigger. The `SpecApproved` event is the universal starting signal."
Gap `spec-41a92c20354c6a108e6074e3` / approval-single-trigger.
Candidate under review: `2456086c` (branch
`pipeline/task-297/db39041900d440978fd33ae3dc9f098d-1`, base `0f95668d`;
product commits `d5662b9e` + `569ea138`, clippy repair `6c7ce409`,
verification repair `52397d16`, merge `57485c70`, process/doc commits
`f76f1b89`/`2456086c`).
Verdict: **approved** — no findings. All three required gates are real,
fail-closed, and tested against behavior that fails at the base; the
approval side is untouched; the coverage rows added match the shipped code.

## Probes run (evidence in /tmp/stage/review-evidence/)

- `/tmp/stage/bin/cargo check -p gyre-server --lib` → exit 0 (cold run
  8m18s; `cargo-check-lib.log`).
- `/tmp/stage/bin/cargo check -p gyre-server --tests` → exit 0 (cold run
  4m40s, includes the integration-test targets and the web build;
  `cargo-check-tests.log`).
- Mechanical invariant scripts → all exit 0: check-arch,
  check-inert-enforcement, check-mcp-write-tools, check-dead-message-kinds,
  check-abac-route-registry, check-mem-port-contracts,
  check-forwarded-header-trust, check-unbounded-external-http
  (`mechanical-checks.log`).
- Independent fixture-consistency scan: every `seed_approved_spec`
  (path, sha) pair — literal and constant-derived — checked against every
  `path@sha` use in the same file; all SHAs 40-hex, zero mismatches
  (`fixture-pair-scan.txt`, `fixture-pair-scan-consts.txt`).
- Retained controller verification log inspected: full `cargo test --all`
  on pre-merge candidate `d6f85599` → 2671 passed / 1 failed
  (`api::search::tests::search_emits_analytics_event`, 400 vs 201).
  Root cause independently confirmed as fixture drift (a 46-char SHA
  literal in the request body vs the seeded 40-char SHA — the gate
  correctly rejected it); fix `52397d16` derives the body's `spec_ref`
  from the same constants the seed writes, so the pair cannot drift
  (`retained-verification-note.txt`).
- Full diff read of all 15 touched files; sweep of every task-creating and
  worker-spawning surface for bypasses (see below).

## Verified working

- **Shared verifier** — `verify_implementation_spec_ref` (api/gates.rs)
  resolves a `path@sha` against BOTH the durable per-SHA approval row
  (delegating to the pre-existing `verify_spec_ref`, the same ledger query
  the merge gates use — validates the form and 40-hex SHA, requires an
  active row) AND the registry entry (must exist, still `Approved`, and
  `current_sha` exactly the pinned SHA). Both checks fail-closed with a
  named reason; storage errors are errors, not passes. The result is bound
  and branched (no inert-gate shape; check-inert-enforcement passes).
- **Gate 1, task creation (REST)** — `create_task` (api/tasks.rs) rejects
  `task_type: implementation` unless the gate passes: 400 InvalidInput
  naming "spec approval is the single trigger for implementation work
  (agent-runtime.md §1)" and pointing at the approve endpoint, returned
  BEFORE `state.tasks.create` — no task row on rejection. A missing
  `spec_ref` also rejects (there is nothing to resolve). Delegation /
  Coordination / untyped tasks are untouched by the gate.
- **Gate 1, task creation (MCP)** — `handle_create_task` (mcp.rs) enforces
  the identical gate before `state.tasks.create`, returning a
  `SPEC_APPROVAL_REQUIRED` tool error naming the requirement.
  `gyre_decompose_spec` sub-tasks now pin the parent's `spec_path`/`spec_ref`
  so the legitimate MCP decomposition chain passes the scheduler gate
  (sub-tasks are Implementation type).
- **No creation-gate bypass via update** — `UpdateTaskRequest` (REST) has
  no `task_type`/`spec_ref` fields, and the MCP update tool sets neither;
  task_type cannot be flipped onto an existing task post-creation. (Even if
  it could, gate 2 stops the spawn.)
- **Gate 2, scheduler (defense in depth)** — `spawn_implementation_workers`
  (signal_chain.rs) re-verifies the spec_ref BEFORE `resolve_task_repo`,
  budget, max_agents, or the claim: on failure the task stays Backlog, an
  observable `spec-approval-missing` label is added via a freshest-row
  re-read (same stale-snapshot defense as the budget flag), a
  `tracing::info!` reason is logged, and no agent record or InProgress
  transition occurs. The label is cleared on a later successful spawn.
  Tests cover both AC scenarios: the unapproved twin (zero agent records,
  Backlog, label) vs the approved twin (exactly one worker, InProgress),
  and the approval-invalidated-by-later-push case (registry moved to
  Pending at a new SHA + `revoke_all_for_path`).
- **Gate 3, direct spawn** — REST `spawn_agent` (api/spawn.rs) rejects 403
  ("single trigger" named) when a non-system principal would create a
  worker bound to an Implementation task; `agent_type: interrogation` and
  system principals (`is_system_principal()`: agent_id "system" AND
  user_id none — the spoof-resistant discriminator, so a user *named*
  "system" cannot ride the exemption) are exempt per the Phase-8
  system-initiated kinds. The in-process scheduler
  (`scheduler_system_auth()`) and the repo-orchestrator-only MCP
  `gyre_spawn_worker` path reach `spawn_agent_core` directly and keep the
  pre-existing task-claim boundary — both are system/chain-initiated, so
  the single-trigger property holds. The task-claim boundary test was
  re-pointed at a direct core probe so it still fails if the boundary is
  removed.
- **Approval side preserved** — the diff does not touch the approve
  handler, `spec_registry.rs` sync, or the post-receive hook; base behavior
  (ledger row, SpecApproved emission, modified-spec invalidation) is
  unchanged.
- **Tests are kill-capable, not inflated** — each new test asserts the
  rejection (4xx / tool error / 403) AND the absence of side effects (no
  task row, no agent record, task untouched) AND the positive counterpart
  (identical request succeeds after a real `POST /api/v1/specs/:path/approve`;
  approved twin spawns; interrogation spawn still 201). At the base these
  all fail (no gate existed). Existing tests were amended only by seeding
  the durable approval state and pinning the spec_ref — no assertion
  weakened (all fixture pairs verified consistent).
- **Coverage rows** — agent-runtime.md rows 3 and 7 were updated with
  concrete, accurate source evidence for the new gates; no unrelated rows
  touched.

## Non-blocking observations (recorded for completeness)

- The thorough-preview surface (api/graph.rs, unchanged by this candidate)
  creates a spec-preview Implementation task and spawns a preview worker
  without spec approval. This is a specced exception
  (meta-spec-reconciliation.md §Preview Loop: "The draft doesn't need to be
  committed or approved. It's ephemeral", throwaway `preview/*` branch,
  publish requires the commit + approve flow), outside this task's three
  assigned gates. Side effect of gate 2 that is an improvement: a leftover
  preview task stranded in Backlog (no spec_ref) can no longer be picked up
  by the production scheduler.
- The direct-spawn gate exempts the global `GYRE_AUTH_TOKEN` holder (a
  system principal per `is_system_principal`). This matches the AC
  ("from a human or agent principal returns 403") and the codebase-wide
  treatment of the operator token as the system authority.
- Server test execution exceeds this sandbox's build budget (per the
  stage constraints); the retained full-suite run covered the pre-merge
  tree and the merged tree is compile-verified here. The verifier's full
  `cargo test --all` on the final tree remains the deterministic gate.

## Verifier must run

- `cargo test -p gyre-server --lib` — at minimum:
  `api::tasks` (create-gate tests), `api::gates`, `mcp::tests`
  (`mcp_create_task_implementation_requires_approved_spec`,
  `mcp_create_task_with_signal_chain_fields`, search-fixture tests),
  `api::search::tests::search_emits_analytics_event` (the repaired
  fixture), `signal_chain` (`scheduler_skips_unapproved_spec_and_spawns_approved_twin`,
  `scheduler_stops_spawn_when_approval_invalidated_by_push`,
  `scheduler_spawns_worker_for_implementation_task`,
  `scheduler_decomposes_delegation_into_ordered_subtasks`),
  `api::spawn::tests` (`spawn_rejects_implementation_worker_for_external_callers`,
  `agent_jwt_cannot_spawn_onto_foreign_task`).
- `cargo test -p gyre-server --test api_integration -- tasks_create_implementation_requires_approved_spec`
  (real approve-flow AC), plus the e2e/git/auth/m18 integration suites
  amended with seeded approvals.
- Full `cargo test --all` plus the standard check scripts (rustfmt,
  clippy changed-lines, all `scripts/check-*.sh`).
