# Review — task-115 (Implement spec approval signal chain & orchestrators)

Spec: `specs/system/agent-runtime.md` §1 Phases 1–3 (The Signal Chain, Spec
Approval Triggers Orchestration, Workspace Orchestrator, Repo Orchestrator).
Candidate: `e54facd4` (base `a1751da1`). Prior assignment outcome: "review
model did not complete; retry independent review on the same candidate" — this
is the fresh independent review of that exact candidate.

Verdict: **not approved** — 3 findings (2 behavioral defects at the chain's
boundaries with pre-existing safety mechanisms, 1 operational risk). The core
Phases 1–3 implementation is genuine, not hollow; the defects are real and
reproducible from source.

## Diff scope

`git diff a1751da1..e54facd4`: `signal_chain.rs` (new, 1414 lines),
`api/specs.rs` (+230/−44: chain hook in `approve_spec`, `reject_spec` tenant
resolution), `jobs.rs`/`lib.rs`/`main.rs`/`mem.rs`/`middleware.rs` wiring,
`api/orchestrator.rs` (`Default` derive on `SpawnOrchestratorRequest`),
`check-fabricated-scope-defaults.sh` 7→6 with the matching exemption entry
removed (legitimate — the exempted code is deleted), `web/dist` rebuild,
`specs/tasks/task-115.md` (frontmatter + end-of-file `## Shipped` only; the
four contract sections remain byte-identical to the assignment template —
verified against the assignment text; the prior round's contract self-edit
was repaired in `1e572089`). Only `79b06a7a` among the five attribution
commits carries the product diff; all five are in candidate ancestry.

## Probes (fresh, at HEAD `e54facd4`, working tree clean; temp probe crate removed)

- `cargo test -p gyre-server --lib signal_chain` → **10 passed, 0 failed**
  (includes full-chain integration tests: approval→orchestrator spawn→
  delegation+coordination tasks→scheduler→repo orchestrator→ordered sub-tasks
  with `depends_on` chaining→delegation Done; exactly-one-orchestrator reuse;
  unscoped-spec no-op; untyped-task no-op).
- `cargo test -p gyre-server --lib api::specs` → **73 passed, 0 failed**
  (incl. `approve_spec_emits_spec_approved_and_triggers_chain` — router-level,
  asserts bus payload fields, `Destination::Workspace`, orchestrator spawn,
  delegation task with `path@sha`).
- `cargo test -p gyre-server --lib api::orchestrator` → 6 passed.
- `cargo test -p gyre-server --lib jobs` → 11 passed.
- `cargo test -p gyre-server --lib mcp` → 71 passed (mcp.rs untouched by the
  branch; confirms prior-round claim).
- `cargo test -p gyre-server --lib budget` → 8 passed.
- `cargo test -p gyre-domain --lib task::` → 10 passed.
- `scripts/check-task-commit-attribution.sh`, `check-fabricated-scope-defaults.sh`,
  `check-in-memory-state-stores.sh` → all OK.
- Domain transition probe (temporary external crate, deleted after):
  `Task` InProgress→Backlog = `Err(InvalidTransition)`; `Agent` Idle→Idle =
  `Err` (benign — second-run skip via `is_ok()` guard).

Transport restriction (unchanged from base, `/tmp/stage/capabilities.json`):
`tcp_listener_probe` unsupported (errno 95) — listener-bound tests and the
full `cargo test --all` cannot run in this sandbox; required host/CI check:
`cargo test --all` at `e54facd4`.

## What is genuinely implemented (verified)

- Phase 1: `approve_spec` records the ledger approval and emits the bus
  `SpecApproved` message with the spec'd payload fields, then invokes
  `signal_chain::on_spec_approved` (server-side interception per the spec's
  "internal server mechanism" wording). The extra `workspace_id` payload key
  is server-side routing data, additive to the spec'd fields.
- Phase 2: `OrchestratorRegistry` per-workspace/per-repo locks with stable
  identity across `AppState` clones (Arc<RwLock<HashMap>>); spawn-on-demand
  through the task-093 core (workspace-scoped JWT, keypair, budget, 409
  one-live guard in the DB path); inbox delivery via Directed-tier persisted
  message; delegation task for the spec's repo with `spec_ref path@sha`;
  coordination tasks for `spec_links` dependents (both same-workspace and
  cross-workspace — coordination task lands in the dependent repo's
  workspace); priority-4 `CrossWorkspaceSpecChange` notifications (priority
  from the type's default — correct per HSI §8) for cross-workspace
  dependents' Admins/Owners with tenant resolved from the workspace record.
- Phase 3: 30 s scheduler loop registered in the job registry (healthz +
  `POST /admin/jobs/task_scheduler/run`), Backlog claim for idempotence,
  per-repo lock, repo orchestrator spawn via task-093 core, delegation
  decomposition reading the real spec blob at the pinned SHA (`git cat-file`
  via `read_git_file`), LLM judgment via `state.llm` with an honest
  deterministic `##`-section fallback (no fabricated model output), sub-tasks
  carrying `spec_ref`/`parent_task_id`/`order`/`depends_on`, delegation
  marked Done. Coordination processed to Done with a conservative sub-task.
  `Implementation`/untyped tasks stay Backlog (Phase 4's path); the worker
  spawn core independently rejects Delegation/Coordination.
- Contract repair verified: acceptance criteria checklist unticked, criterion
  text original, completion recorded only in frontmatter + end-of-file
  Shipped.

## Findings

### F1 — spec-rejection safety regression (medium)

Chain tasks store `spec_path = "path@sha"` (delegation `signal_chain.rs:313`,
sub-tasks `:644`), but `reject_spec`'s mid-flight cancellation
(`api/specs.rs:913-959`) selects tasks via `list_by_spec_path(bare_path)` —
exact equality in every adapter (`mem.rs:702-711`,
`sqlite/task.rs:394-409` `tasks::spec_path.eq(...)`, postgres same). Approve
then reject the same spec: the delegation and its Backlog sub-tasks are NOT
cancelled and remain eligible for Phase-4 worker spawning against a rejected
spec. `get_spec_progress` (`api/specs.rs:1370`) has the same mismatch (linked
tasks/tasks-count silently zero). No existing test combines rejection with
chain-created tasks. Fix direction: store the bare path in `spec_path` (the
pinned SHA belongs in a dedicated `spec_ref` field or the MR-level spec_ref
convention), or make the cancellation/progress consumers match on the
`path@` prefix / split at the last `@`.

### F2 — dead failure-retry path (medium)

`scheduler_run_once` claims Backlog→InProgress (`signal_chain.rs:503-510`)
and on `run_repo_orchestrator` failure "releases the claim" via
`task.transition_status(TaskStatus::Backlog)` (`:519`) — an invalid
transition (probe above; domain table allows only
InProgress→Review/Blocked/Cancelled). The `Err` is discarded (`let _ =`), so
the task stays InProgress and `list_by_status(Backlog)` never returns it: any
transient repo-orchestrator failure permanently strands the task, silently.
Fix direction: release via a valid transition the scan re-selects (e.g.
Blocked with a retry policy, or extend the claim scan to re-claim stale
InProgress tasks), and surface the transition error.

### F3 — unbounded orchestrator churn (low/operational)

Chain-created orchestrators are server-side agent records that never
heartbeat; `is_alive()` falls back to `spawned_at` + 60 s
(`gyre-domain/src/agent.rs:162-168`) while `spawn_orchestrator` sets
`disconnected_behavior=Abort` + `restart_on_failure=true`
(`api/orchestrator.rs:100-104`). The stale detector therefore marks each
chain orchestrator Dead ~60 s after creation and immediately spawns an Active
replacement (`stale_agents.rs:101-103,142-204`) that again never heartbeats —
endless kill/restart churn per workspace with an approved spec (repo tier
also escalates to the workspace orchestrator each death; each restart
increments budget active_agents). Registry one-live semantics are not broken
(the replacement is live, the dead one is not), but the churn is real.
Tests never advance time past the 60 s window, so nothing catches it. Needs
a server-run heartbeat, a liveness exemption for system-driven orchestrator
records, or a documented decision.

## Not findings (checked and fine)

- Budget accounting: `decrement_active_agents` only fires when
  Active→Idle succeeds (first run); the reused orchestrator's second run
  skips it (Idle→Idle invalid) — no double-decrement.
- Exactly-one-active: per-workspace/per-repo mutexes held across the whole
  signal processing; DB-side 409 in the spawn core covers cross-process
  races; the `is_live` predicate is duplicated but identical to the core's.
- `spec_links` dependents query: correct direction (source depends on this
  spec's target), filters on target repo + path, handles missing
  `source_repo_id` and unresolvable dependent repos with skip+log.
- Cross-workspace tenant resolution never fabricates "default" (matches the
  repaired invariant; exemption-file shrink is legitimate).
- Attribution: all five frontmatter commits in candidate ancestry; the
  product surface is carried by `79b06a7a` which is listed.
- Deterministic decomposition fallback is honest degradation, not fake LLM
  output: it derives from the real spec content read at the pinned SHA and
  only runs when `state.llm` is absent or errors.
