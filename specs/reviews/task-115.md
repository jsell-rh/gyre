# Review — task-115 (Spec approval signal chain & orchestrators)

Spec: `specs/system/agent-runtime.md` §1 Phases 1–3 (The Model, Agent
Lifecycle, The Signal Chain, Phase 1 Spec Approval Triggers Orchestration,
Phase 2 Workspace Orchestrator, Phase 3 Repo Orchestrator).
Candidate: `5d14b4a3d` (branch pipeline/task-115/…-2), base `774737638`
(rebase repair merge `e0408d527` onto the task-099/task-177 base).
Verdict: **approved** (round 1 for this candidate; prior rounds' findings
F1–F3 verified as fixed with regression tests).

## Evidence

All probe commands, sources, exit codes, and outputs are recorded in
`/tmp/stage/review-evidence/task-115-probes.txt` (mirrored summary):

- Focused tests: `cargo test -p gyre-server --lib signal_chain` → 19/19;
  `api::specs::tests` → 62/62 (both chain tests); `api::orchestrator` →
  12/12 (both conflict-kept tests present); `stale_agents` → 3/3;
  `cargo test -p gyre-adapters sqlite::task` → 11/11 (incl.
  `create_persists_workspace_scope`).
- Mechanical invariants: 17 check scripts + arch + migration-versions +
  rustfmt all pass. The candidate *reduced* the unbounded-external-http
  exemption count 5→0 by bounding the Vertex AI clients (allowed
  direction; never-add rule untouched).
- Mutation probes (all reverted; tree verified pristine at `5d14b4a3d`):
  1. Disabling the `on_spec_approved` hook in the approve handler fails
     both specs.rs chain tests → Phase 1→2 wiring proven, not mirrored.
  2. Making the scheduler skip Delegation/Coordination fails 7 tests →
     Phase 3 proven.
  3. Skipping the per-workspace mutex acquisition passes the existing
     suite (see observation O1) — a direct concurrent probe (20 parallel
     signals, mutation active and inactive) still yields exactly one
     orchestrator and no dropped signals; the mutex is acquired in the
     shipped code and is defense-in-depth over the spawn core's one-live
     and name-collision checks.

## Verified against the acceptance criteria

- **Spec approval emits `SpecApproved` on the bus** — approve handler
  (api/specs.rs) emits workspace-destined `SpecApproved` with the spec'd
  payload fields (`repo_id, spec_path, spec_sha, approved_by,
  approval_id`); test asserts each field, the destination, and the SHA.
- **`OrchestratorRegistry` per-workspace/per-repo mutexes** — real
  process-local tokio mutexes, stable identity across AppState clones,
  plus a whole-cycle scheduler run lock; registry stores no agent state
  (agents table is the source of truth — in-memory-state-stores invariant
  respected).
- **Workspace orchestrator auto-spawned on `SpecApproved`** —
  `on_spec_approved` → `ensure_workspace_orchestrator` reuses the task-093
  spawn core (scoped JWT, keypair, budget, fail-closed approved-persona
  resolution inherited from the base), delivers the message to the
  orchestrator's inbox (Directed tier, persisted), then runs it. Completes
  to Idle after processing ("completes after processing its inbox").
- **Delegation + coordination tasks** — delegation for the spec's repo
  with `spec_ref: path@sha` pinned and bare `spec_path` kept for
  rejection-cancellation reachability (regression test proves the
  rejection path cancels the chain-created task); coordination tasks for
  dependent repos from the real `spec_links` store; cross-workspace
  dependencies produce priority-4 `CrossWorkspaceSpecChange`
  notifications to Admin/Owner members (default_priority 4 verified).
- **Repo orchestrator auto-spawned on delegation; ordered sub-tasks** —
  scheduler claims Backlog/Blocked/stale-InProgress Delegation and
  Coordination tasks under the cycle lock, per-repo mutex, re-reads the
  task after acquiring the repo lock (double-claim defense), spawns the
  repo orchestrator via the task-093 core, decomposes against the pinned
  blob (empty/unresolvable SHA hard-fails to Blocked — no fail-open),
  creates Implementation sub-tasks with `spec_ref`, `parent_task_id`,
  `order`, and `depends_on` chaining, and marks the delegation Done.
  Failed runs release the claim to Blocked for retry (regression test).
- **Task-type discrimination** — scheduler handles only
  Delegation/Coordination; `spawn.rs` rejects Delegation/Coordination (and
  untyped) tasks from worker spawning with explicit errors.
- **Exactly-one-active** — per-workspace mutex serializes concurrent
  signals; per-repo mutex serializes concurrent orchestrator runs; the
  spawn cores keep the durable one-live check; the stale-agent detector
  refreshes system-spawned orchestrators' heartbeats instead of
  churning spawn/kill/restart (regression test), and Dead transitions now
  release the budget slot.
- **Honest degradation** — deployments without `GYRE_VERTEX_PROJECT` use
  a deterministic decomposition derived from the real spec blob (one
  ordered sub-task per `##` section), never fabricated LLM output; LLM
  calls are latency-bounded (120 s `tokio::time::timeout` + bounded
  reqwest clients), and the adapter's HTTP clients now carry total
  timeouts.
- **Rebase repair** — all three conflict sites resolved keeping both
  sides' tests; the signal chain correctly adapted to the base's changed
  spawn-core contract (`&Arc<AppState>`, 3-tuple return, fail-closed
  persona resolution), with production-mirroring persona seeding in
  affected tests. Migration `2026-10-10-000057_task_spec_ref` uses a
  distinct Diesel version (date-prefixed; check-migration-versions OK) and
  is portable SQL (plain `ALTER TABLE ... ADD COLUMN`).

## Non-blocking observations (for the audit/verification record)

- **O1 (test strength):** no test pins the workspace-mutex *acquisition*
  inside `on_spec_approved` (mutation probe 3 passes the suite). The
  cycle-lock acquisition has an analog (`scheduler_cycles_serialize_on_the_run_lock`);
  a sibling test for the workspace lock would close the gap. Production
  behavior is correct (code-verified + concurrent probe), so this is a
  suggestion, not a defect.
- **O2:** the bus `SpecApproved` payload carries one extra field,
  `workspace_id`, beyond the five the spec lists; it is consumed by the
  server-side chain and documented in the handler. Benign extension of
  the spec'd payload shape, not a contradiction.
- **O3:** the orchestrator runs are server-driven: they create tasks via
  the task store and read `spec_links`/git directly rather than calling
  their own MCP tools over the wire. Same effects, documented stance in
  the code; the MCP `task.create` schema gained `spec_ref` so tool parity
  exists.
- **O4:** coverage rows for this task's sections remain `task-assigned`
  pending the audit pipeline (coverage updates flow through audit
  commits, per repo process) — consistent with "the coverage matrix
  reflects reality".
- Full workspace suites, all-target Clippy, and GitHub CI remain for the
  verification gate, as the shipped notes state.
