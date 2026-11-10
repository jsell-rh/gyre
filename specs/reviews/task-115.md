# Review — task-115 (Spec approval signal chain & orchestrators)

Spec: `specs/system/agent-runtime.md` §1 Phases 1–3 (The Model, Agent Lifecycle,
Signal Chain, Phase 1 Spec Approval, Phase 2 Workspace Orchestrator, Phase 3
Repo Orchestrator).
Candidate: `f159e527af19a9bf0ebf80c02f9b9f62fc87f1c7` (base `b5a872087`,
task-108 admin impersonation; rebase merge `dac3c7e65`).
Verdict: **approved** — no findings.

## Round 1 (2026-11-10)

Diff scope (22 files): new `crates/gyre-server/src/signal_chain.rs` (2260
lines incl. tests), approve-handler hook + chain tests in `api/specs.rs`,
task_scheduler job registration (`jobs.rs`, `main.rs`), `OrchestratorRegistry`
wiring (`lib.rs`, `mem.rs`, `middleware.rs`), `Task.spec_ref` field
(`gyre-domain`, SQLite/PG adapters, diesel schema, migration
`2026-10-10-000057_task_spec_ref`), `spec_ref` passthrough on
REST `POST /tasks` + MCP `task.create`/`task.list`, stale-agent detector
refresh for system orchestrators + budget-slot release on Dead, Vertex AI
client latency bounds, e2e runner-pin comment merge.

### Probes (evidence under /tmp/stage/review-evidence/)

- `cargo test -p gyre-server --lib` → **1636 passed, 0 failed** (includes all
  19 `signal_chain` tests, `api::specs` chain/rejection-cancellation tests,
  `api::orchestrator` + `stale_agents` regressions, base's impersonation tests).
- `cargo test -p gyre-server --lib signal_chain` → 19/19 (full-chain
  integration: approve → orchestrator spawn → delegation + coordination →
  scheduler → decomposition with order/depends_on, exactly-one-active,
  claim-release retry, stale-claim crash recovery, targeted inbox ack,
  empty-SHA hard-fail).
- `cargo test -p gyre-adapters --lib sqlite::` → 294 passed, 0 failed
  (incl. `sqlite::task::create_persists_workspace_scope` upsert branch).
- `cargo test -p gyre-domain` → 451 passed, 0 failed.
- All 20 mechanical invariant scripts → exit 0 (arch, ABAC route registry +
  exempt handlers, MCP write tools, migration versions + SQL portability,
  dead message kinds, byte-slice truncation, relative-path defaults,
  fail-open ref resolution, mem port contracts, fabricated/lossy/scope
  defaults, inert enforcement, forged scope fields, forwarded headers,
  in-memory state stores, unbounded external HTTP, task-commit attribution).
- **Mutation probe** (restored after): disabling the
  `signal_chain::on_spec_approved` call in the approve handler makes
  `approve_spec_emits_spec_approved_and_triggers_chain` **FAIL** (panics at
  "workspace orchestrator spawned by the approve handler"); re-enabled it
  passes. The chain tests are behavior-backed, not self-confirming.

### Acceptance criteria — verified

- **Spec approval emits `SpecApproved` on the bus.** `approve_spec` emits
  only on transition *into* Approved (mode-based resolution preserved;
  partial human_and_agent approvals don't fire the chain), payload carries
  the five spec'd fields plus an internal `workspace_id`, destination
  `Workspace(ws)` when the ledger entry is workspace-scoped. Verified by the
  router-driven test + mutation probe.
- **`OrchestratorRegistry` per-workspace/per-repo mutexes.** Real tokio
  `Mutex` per scope behind an `Arc<RwLock<…>>` map with stable identity
  across `AppState` clones, plus a whole-cycle scheduler run lock. Registry
  unit tests assert scope isolation and serialization.
- **Workspace orchestrator auto-spawned on `SpecApproved`.**
  `on_spec_approved` holds the per-workspace lock, ensures a live
  orchestrator via the task-093 spawn core (budget check, fail-closed
  persona resolution, durable one-live DB check), delivers the message to
  its Directed-tier inbox, then runs processing.
- **Delegation task for the spec's repo** with `spec_ref: path@sha` pinning
  the approved blob, bare `spec_path` (so `reject_spec` cancellation via
  `list_by_spec_path` reaches it — regression-tested), repo/workspace
  scoping from the ledger entry.
- **Coordination tasks for dependent repos** from a real `spec_links` query
  (target repo + path match), plus priority-4 `CrossWorkspaceSpecChange`
  notifications (default_priority 4) to Admin/Owner members of dependent
  workspaces, tenant resolved from the workspace record (never fabricated).
- **Repo orchestrator auto-spawned on delegation creation.** The 30 s
  scheduler loop (job-registry-recorded, `/healthz`-visible, admin-trigger
  capable) claims Backlog Delegation/Coordination tasks under the cycle
  lock, spawns the repo orchestrator via the spawn core (repo-scoped JWT),
  and runs decomposition under the per-repo mutex.
- **Decomposition into ordered sub-tasks**: spec blob read via real
  `git show sha:path` at the pinned SHA (empty/unresolvable SHA is a hard
  fail → Blocked, never an empty-blob decomposition); sub-tasks carry
  spec_ref, parent_task_id, order, and depends_on chaining; LLM judgment
  when configured (latency-bounded by `LLM_CALL_TIMEOUT` + adapter HTTP
  timeouts) with an honest deterministic per-`##`-section fallback that
  fabricates no model output; delegation marked Done after.
- **Scheduler distinguishes task types**: only Delegation/Coordination are
  claimed; Implementation and untyped pre-approval push-hook tasks stay
  Backlog (tested).
- **Exactly-one-active**: per-scope mutexes + spawn core's DB one-live
  check; concurrent-cycle and stale-claim recovery are regression-tested.
- **Tests pass**: package-scoped suites above all green. Full-workspace
  `cargo test --all` remains verification-gate scope (the executor wrapper
  rejects workspace-wide cargo invocations in this sandbox).

### Rebase integrity (base `b5a872087`)

The merge `dac3c7e65` resolved one content conflict in
`.github/workflows/e2e.yml` (comment merge only; `runs-on: ubuntu-24.04`
unchanged on both sides). No source conflicts; the signal chain merged
cleanly over task-108's auth/middleware/spawn changes — confirmed by the
full lib suite including the base's impersonation tests, and by the
mechanical gates.

### Non-blocking observations (recorded, not findings)

- If the workspace-orchestrator spawn fails (e.g. budget exhausted at
  approval time), the Phase-1 signal is dropped until the next approval or
  operator spawn (the bus message persists). The Phase-3 side does retry
  (failed runs release the claim to Blocked, re-selected by the scan). A
  documented, logged degradation — resilience asymmetry, not a spec break.
- A completed (Idle) workspace orchestrator is reused by the next signal
  rather than a fresh session spawned; the spec's parenthetical allows
  either reading, and exactly-one-live + inbox delivery + serialized
  processing hold either way.
- Registry/cycle locks are process-local; multi-process deployments rely on
  the spawn core's DB one-live check (documented in code). Task double-claim
  across processes is out of the single-server model this codebase assumes.
- The dependents query does not filter `spec_links.status` — a stale link
  still yields a coordination task (arguably correct: the dependent repo
  should update its link).

Test-inflation check: none found — every new test asserts real state
through port-backed stores or a real git repo (tempdir), and the mutation
probe demonstrates the headline test fails when the production hook is
disabled.

Scope check: no out-of-scope edits. The Vertex AI timeout additions close
the exemption debt the new scheduler path would otherwise violate
(exemption count honestly reduced 5→0 with real `.timeout(...)` bounds);
`check-unbounded-external-http.sh` passes at the new count.
