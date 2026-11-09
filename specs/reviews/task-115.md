# Review: TASK-115 — Implement spec approval signal chain & orchestrators

**Reviewer:** Independent review (pi runtime)
**Date:** 2026-11-07
**Verdict:** approved

**Candidate under review:** `83318af9` (base `fefde2946`, branch
`pipeline/task-115/7b5299803d05490c825510c99c58d2cb-1`)
**Spec:** agent-runtime.md §1 Phases 1–3 (The Model / Signal Chain /
Phase 1 / Phase 2 / Phase 3)
**Evidence:** `/tmp/stage/review-evidence/` (test transcripts, mutation
probes, invariant-check output, README summary)

---

## Verification performed

Read the full base→candidate diff (21 files, ~2.9k insertions): the new
`signal_chain.rs` (all 2245 lines), the approve-handler emission in
`api/specs.rs`, the `OrchestratorRegistry` wiring
(`lib.rs`/`mem.rs`/`middleware.rs`), the task-scheduler job
(`jobs.rs`/`main.rs`), `stale_agents.rs` system-orchestrator handling,
the `spec_ref` column across domain/sqlite/postgres/schema + migration
`2026-10-10-000057_task_spec_ref`, and the LLM latency bounds in
`rig_vertexai.rs`. Ran focused suites with the sandbox cargo wrapper:
`signal_chain` 19/19, `orchestrator` 13/13, `stale_detector` 4/4, the
two approve/reject chain tests in `api::specs` 2/2, `sqlite::task`
11/11. Ran every mechanical invariant check touched by the diff (all
exit 0), `check-arch.sh`, and the rustfmt diff gate. Ran two mutation
probes (restored and re-verified byte-identical afterwards):

1. **Cycle-lock mutation** — replaced the `scheduler_run_once` run-lock
   guard with a non-locking binding →
   `scheduler_cycles_serialize_on_the_run_lock` **failed** ("a second
   cycle must block while the first holds the run lock"). The
   double-processing guard is load-bearing and its test kills its
   removal.
2. **Empty-SHA fail-open mutation** — removed the `bail!` on a
   malformed `path@` spec_ref →
   `delegation_with_empty_sha_spec_ref_stays_blocked` **failed** (Done
   vs Blocked). The fail-open-ref-resolution invariant class is genuinely
   enforced and regression-tested.

## Findings

None blocking. The task contract is met with real implementations:

- **Phase 1**: emission is gated on the ledger transition *into*
  Approved (no duplicate signals on re-approval), routed
  `Destination::Workspace`, full specced payload
  `{repo_id, spec_path, spec_sha, approved_by, approval_id}` — proven by
  an end-to-end handler test (`approve_spec_emits_...`) that asserts the
  bus message shape, destination, orchestrator spawn, and delegation
  task with pinned `spec_ref` `path@sha`.
- **Phase 2**: `OrchestratorRegistry` with per-workspace/per-repo tokio
  mutexes (stable identity across clones, tested); spawn-on-demand via
  the task-093 core (scoped JWT, budget check, durable one-live check);
  delegation task for the spec's repo and coordination tasks for
  dependent repos found via the real `spec_links` store; cross-workspace
  dependencies produce genuine priority-4 `CrossWorkspaceSpecChange`
  notifications (`default_priority()` = 4) to Admin/Owner members with
  tenant resolved from the workspace record (never fabricated); the
  workspace orchestrator run acks only the SpecApproved signal it
  consumed (bulk-ack regression test keeps Escalations unacked).
- **Phase 3**: scheduler cycle lock serializes scan→claim→process
  (mutation-proven); claim state machine releases failed runs to
  Blocked for retry (regression test drives spawn-budget failure →
  Blocked → retry → Done); stale InProgress claims are crash-recovered;
  task re-read after the repo mutex guards double decomposition;
  empty/unresolvable SHA hard-fails (mutation-proven) instead of
  decomposing against an unidentifiable blob; sub-tasks carry
  spec_ref/parent_task_id/order/depends_on chaining and correct
  repo/workspace scoping; Implementation tasks are excluded from the
  orchestrator path (left Backlog for Phase 4); delegation marked Done.
- **Cross-cutting**: `spec_path` stays the bare path so spec-rejection
  cancellation reaches chain-created tasks (regression test:
  approve → reject → delegation Cancelled); migration SQL is portable
  and version-distinct; Vertex AI + token-exchange clients are
  latency-bounded (unbounded-external-http exemptions lowered 5→0, a
  lowering); system orchestrators are heartbeat-refreshed by the stale
  detector instead of churned, with the Active→Dead budget-slot release
  preserved (regression tests on both).

### Non-blocking observations (for the record / future hardening)

1. The bus payload carries an additive `workspace_id` field beyond the
   spec's five, documented in-code as the server-side routing hint (the
   bus destination already carries the scope). Additive; consumers key
   on the five specced fields. No action required.
2. A storage failure *between* sub-task creates inside
   `decompose_delegation`, followed by the designed Blocked-retry,
   would duplicate the partially-created sub-task set (each sub-task id
   is a fresh UUID, so no upsert dedup). The window requires a
   tasks-store failure mid-loop — far narrower than the
   normal-operation races fixed in earlier rounds — and the workspace
   tier recovers on the next signal. Suggested future hardening: skip
   sub-task creation when the delegation already has children
   (`list_by_parent`).

### Not verifiable in this sandbox

`cargo test --all` and listener-bound tests (tty::, ws::,
otlp_receiver::, git_http::, gyre-cli ws_integration) cannot run here
(executor wrapper rejects workspace-wide invocations; transport
restriction per capabilities.json). Infrastructure restriction, not a
code defect — the verification gate must run the full suite and the
exact-head GitHub checks.
