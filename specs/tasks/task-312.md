---
title: "Platform-held development task ledger with authoritative sequencing and lifecycle state (dogfooding progression stage 1)"
spec_ref: "Spec gap spec-8986fd94925aed0eaae2d366 / dogfooding-platform-orchestration / 644cb4b53ccb"
depends_on: []
progress: ready-for-review
commits: ["PENDING-COMMIT"]
---

## Required behavior

Close the unimplemented bullet 3 of specs/development/dogfooding.md ('The new platform progressively replaces agent-boss as it matures') by making the Gyre platform product assume the dev-orchestrator's shared-state and sequencing responsibilities for Gyre's own development, as a concrete, load-bearing, observable progression stage — not a mirror. No platform Rust changes are required; the integration consumes the existing production REST task surface (POST/GET /api/v1/tasks, GET/PUT /api/v1/tasks/{id}, PUT /api/v1/tasks/{id}/status, GET /api/v1/workspaces/{id}/tasks — crates/gyre-server/src/api/mod.rs:407-415) through real authenticated HTTP with real platform storage. (1) Bootstrap: a new scripts/dev-platform.py (or equivalent pipeline settings path using the existing store.setting mechanism, as the 'gateway' setting does in scripts/pipeline/execution.py:106) idempotently builds (SKIP_WEB_BUILD=1) and starts a gyre-server instance on a dedicated port with its own SQLite data directory, provisions tenant/workspace/repo records representing the Gyre development context plus a service principal with Developer role and API token, and registers endpoint+token as pipeline settings so stage processes read them. (2) Registration (shared state): catalog sync (scripts/pipeline/catalog.py:43 sync_checkout) upserts every specs/tasks/*.md task into the platform — idempotent, with the task-NNN to platform-task-id mapping persisted in the pipeline store — carrying title, spec_ref, priority, order, and depends_on edges resolved through that mapping. Tasks are registered UNTYPED (no task_type): the mechanical scheduler only spawns agents for task_type=Implementation (crates/gyre-server/src/signal_chain.rs:802), so the ledger records development workflow state without triggering agent spawning and without depending on the approval gate that task-297 places on typed implementation tasks. The bridge must NOT use the orchestrator-spawn routes or the gyre_spawn_worker MCP tool (removed by task-298): 'platform as orchestrator' here means the platform's task coordination surface (ledger, dependency graph, status lifecycle), consistent with the amended agent-runtime contract in which the scheduler is the only task-to-agent path; deferring worker dispatch to the platform scheduler is the explicitly documented next progression stage, not this one. (3) Lifecycle write-back: the pipeline's real state changes transition the platform record — first implement-stage admission of a task sets in_progress; publication (the same code path that rewrites 'progress: complete' in the task document, scripts/pipeline/stages.py:262 and 340-380) sets done; a structural block (structural-fault guard / exhausted repair budget leaving the task held) sets blocked. Sync reconciles records missed by failed transitions. (4) Authoritative sequencing (enforcement): the implement-stage dependency gate at scripts/pipeline/eligibility.py:79 currently evaluates dependency satisfaction purely from local pipeline state; it must be served by the platform — a task is eligible for implement admission only if the platform reports every depends_on task done, queried over the real API. The platform's answer is authoritative over local state: a dependency the platform reports as not done blocks admission even when local task data satisfies it. Platform unavailability fails closed for new admission (durable error event recorded in the pipeline ledger; no new implement admissions) while already-claimed in-flight work proceeds — the platform is load-bearing, not advisory. (5) Documentation: docs/dev-pipeline.md is updated to describe the platform as the development task-state and sequencing authority and to record the staged progression (next stage: scheduler-driven task-to-agent mapping for development work per the amended agent-runtime.md, contingent on platform maturity), replacing its presentation of the bespoke ledger as the permanent sole workflow authority for task state; pipeline.sqlite3 remains the stage/attempt/work telemetry ledger (dispatcher adapter backend). All bridge HTTP goes through the authenticated REST surface under real ABAC policy evaluation (Developer role required for task writes); no direct platform-storage access and no privileged bypass.

## Acceptance Criteria

- [ ] Bootstrap + sync: running the bootstrap script and catalog sync against a fresh server, GET /api/v1/workspaces/{id}/tasks with the service token returns a record for every specs/tasks/*.md document with matching title/spec_ref and correctly resolved depends_on edges; re-running sync creates no duplicate records (idempotent upsert).
- [ ] Lifecycle: admitting implement work for a task transitions its platform record to in_progress, and completing publication transitions it to done — both observable via GET /api/v1/tasks/{id} on the platform, independent of pipeline.sqlite3.
- [ ] Sequencing enforcement: with a dependency task's platform record not done (e.g. reset via PUT /api/v1/tasks/{id}/status), the dependent task is refused implement-stage admission even though the local pipeline store satisfies the dependency; once the dependency's platform record is done, admission proceeds.
- [ ] Fail-closed: with the platform unreachable (stopped server or dead endpoint setting), the discover/claim path records a durable error event and admits no new implement work — no silent local-only fallback; already-claimed in-flight work is unaffected.
- [ ] Authorization: a token lacking the Developer role on the dogfooding workspace receives 403 on task create/transition through the bridge path (real ABAC evaluation, not bypassed).
- [ ] No spawn surface: the bridge registers tasks untyped and uses no orchestrator-spawn route and no gyre_spawn_worker; after a full sync and admission cycle, zero agent records exist in the platform for development tasks (the mechanical scheduler never spawns for them).

## Verification intent

Mirror probe: point the pipeline at a running platform instance, sync, then reset a dependency task's platform status to backlog via PUT /api/v1/tasks/{id}/status while the local pipeline store still marks that dependency satisfied — run implement-stage discovery/claim: the dependent task must NOT be admitted. The real defect this must catch is a write-only mirror that updates the platform but never consults it (the fabricated-completion failure the review feedback identified: local state alone would admit the task, exactly as the base behaves today with no platform involvement at all — the base fails this probe by construction). Fail-open probe: stop the platform server and run implement discovery — admission must fail closed with a durable error; an implementation that catches the connection error and proceeds on local state is the fail-open defect.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Shipped

The platform product now holds the development task ledger and serves authoritative
implement-stage sequencing for Gyre's own development (dogfooding progression stage 1).
All of it is bridge code against the existing production REST task surface; no platform
Rust changes were needed and no orchestrator-spawn route or gyre_spawn_worker is used.

### Behavior

- Bootstrap (`scripts/dev-platform.py start/stop/status`): idempotently builds
  (SKIP_WEB_BUILD=1) and starts a gyre-server on a dedicated port with its own SQLite
  data directory, provisions the `gyre-dev` workspace and repo records plus a
  Developer-role service principal with an API token (workspace membership Developer,
  raw key persisted once via POST /api/v1/users), and registers endpoint/token/workspace/
  repo as pipeline settings using the existing store.setting mechanism. A healthy
  running instance with a valid token is left untouched; a lost token provisions the
  next free principal identity.
- Shared state (`scripts/pipeline/platform.py sync`, called from catalog sync in
  `scripts/pipeline/catalog.py`): upserts every specs/tasks/*.md task into the platform
  as an UNTYPED record (no task_type, so the mechanical scheduler in signal_chain.rs
  never spawns agents for them) carrying title, spec_ref, priority (mapped from the
  pipeline priority class), order (task number), and depends_on edges resolved through
  the task-NNN to platform-id mapping persisted in the `platform_tasks` pipeline
  setting. Creation runs to a dependency-order fixpoint, so edges resolve in one pass;
  re-running sync creates no duplicates. Drifted create-only fields or reopened
  (previously done) work supersede the old record (relabeled dev-ledger-superseded) and
  re-register a fresh one, because the platform's done status is terminal.
- Lifecycle write-back (hooks in scripts/dev-pipeline.py run_claim): first implement
  admission pushes in_progress (under the lease heartbeat), the publication code path
  that records progress: complete pushes done, and a structural runtime hold pushes
  blocked. Transitions walk the platform's legal status machine; each is pushed once
  (an operator resetting a non-terminal platform status is respected), and sync
  reconciles pushes that failed while the platform was unavailable.
- Authoritative sequencing (scripts/pipeline/eligibility.py, gate in
  platform.dependencies_satisfied): a task is eligible for new implement admission only
  if the platform reports every depends_on task done, queried over the real API (one
  authenticated GET per process; the claim transaction never holds per-row HTTP). A
  dependency reported not done blocks admission even when local pipeline state satisfies
  it; an unmapped dependency fails closed. Platform unavailability (stopped server, dead
  endpoint, refused request) records a durable `platform_unavailable` event in the
  pipeline ledger and admits no new implement work, while already-claimed in-flight
  work (post-execution eligibility recheck) proceeds. A checkout that never ran the
  bootstrap keeps its pre-bridge behavior (documented in docs/dev-pipeline.md).
- All bridge HTTP is authenticated REST under real ABAC evaluation with the
  Developer-role service principal; there is no direct platform-storage access and no
  privileged bypass. docs/dev-pipeline.md now describes the platform as the development
  task-state and sequencing authority, keeps pipeline.sqlite3 as the stage/attempt/work
  telemetry ledger (dispatcher adapter backend), and records the staged progression
  (next stage: scheduler-driven task-to-agent mapping per the amended agent-runtime.md).
- Settings surface: platform endpoint/workspace appear as read-only rows in the
  operator configuration (scripts/pipeline/settings.py); the token is never exposed.

### Test evidence

- `scripts/test_pipeline_platform.py` (12 tests): runs the bridge against a real local
  HTTP server implementing the platform task surface (bearer auth, Developer-role
  writes, legal status machine). Covers: registration with resolved edges and
  idempotency; untyped records; lifecycle in_progress/done/blocked observable via GET;
  the mirror probe (local satisfied + platform not done refuses admission, then admits
  once done); unmapped dependency fail-closed; unreachable platform records a durable
  event and admits nothing while claimed work is unaffected; unconfigured inactivity;
  non-Developer token refused on create/transition with durable failure events; reopen
  supersession; operator reset of a non-terminal record respected; catalog sync
  integration. Mutation checks confirm the probes are load bearing: making the gate
  advisory (write-only mirror) fails 3 tests; making unavailability fail open fails the
  fail-closed test.
- `scripts/test_dev_platform.py` (4 tests): provisioning over real HTTP against a
  provisioning-surface server; settings registration; idempotent re-provision without
  duplicates; lost-token re-provisioning; health check.
- Full sweep `python3 -m unittest discover -s scripts -p 'test_pipeline*.py'`: 394
  tests, only the two failures that also fail on the assignment base 52d7b66b with this
  task's changes stashed (sandbox-specific verification-timeout and procfs agent
  identity behavior; pre-existing, unrelated).

### Deferred verification

`cargo build -p gyre-server` is blocked in the implement sandbox by the build wrapper
(exit 64; server builds belong to the independent verifier under the 4 GiB budget), so
the real-server legs of the acceptance criteria (bootstrap against a real gyre-server,
  ABAC 403 from the real policy engine, zero agent records after a sync/admission
  cycle) are recorded as exact probe commands in
  /tmp/stage/review-evidence/task-312/verifier-probes.md for the independent verifier.
No coverage rows exist for specs/development/* (specs/coverage covers system specs
  only), so no coverage rows were updated.

## Discovery evidence

Canonical spec: `specs/development/dogfooding.md`. Source: `88202d67f7a0070e1ef6cfe340ff8313a6475abb`; plan: `9f3a9c5f2c13ec023664c34fb6a43bab3e0536e2f2e4be0391c690724f3f23f1`.

[
  {
    "id": "69fff34a83938a4731034172",
    "decision": "gap",
    "requirement_quote": "- Development of Gyre will use **agent-boss** as the orchestrator - dogfooding agent coordination from day one.\n- Implementation agents run Ralph loops managed by agent-boss, letting us feel the pain points and feed improvements back into the new system.\n- The new platform progressively replaces agent-boss as it matures.",
    "detail": "The section mandates that Gyre's own development dogfood agent coordination: day one orchestrated by agent-boss with implementation agents running Ralph loops under it, and then 'the new platform progressively replaces agent-boss as it matures'. The first two bullets are historical facts that were actually honored: the m0 walking-skeleton milestone records agent-boss's Manager agent dispatching a sub-agent that worked in a worktree and opened a PR the Manager merged (specs/milestones/m0-walking-skeleton.md §4), and agent-boss now survives only as studied prior art (specs/prior-art/agent-boss.md). The third bullet is NOT implemented at the assigned base 88202d6. The orchestrator of Gyre's development is a bespoke pipeline external to the platform product: scripts/dev-pipeline.py plus the Go dispatcher extracted from HyperShell (docs/dev-pipeline.md: the dispatcher 'calls Gyre's existing stage handlers' and Gyre 'retains pipeline.sqlite3 as its single workflow authority'), dispatching implementation/review/triage agents as fullsend/Pi processes inside external OpenShell-gateway sandboxes (scripts/pipeline/execution.py builds every sandbox command as `openshell -g gyre-gyre`; scripts/dev-gateway.py points the gyre-gyre gateway at a hypershell.app endpoint). No pipeline module invokes the platform's coordination machinery: grep for 'gyre-server', 'localhost:3000', and '/api/v1' across all 42 scripts/pipeline/*.py modules and all 19 scripts/dev-*.py scripts returns zero matches — if the platform's orchestrator/spawn/task coordination code were deleted from the repo, Gyre's development process would not change at all. The platform's coordination surface (orchestrator spawn routes, agent spawn/complete Ralph-loop lifecycle) is product code the development workflow never invokes; machinery existing elsewhere in the repo cannot establish dogfooding, since under that logic any repo containing an orchestrator would satisfy this spec regardless of what actually orchestrates its development. The repo's own documents locate the orchestrator role in the platform product: specs/development/ralph-loops.md ('Gyre orchestrates: sequencing, conflict resolution, shared state, escalation') and specs/development/manager-agent.md (superseded into platform-model.md Two-Level Orchestration), so 'the new platform' in bullet 3 means the platform product assuming the dev-orchestrator role — performed by no production code path. docs/dev-pipeline.md presents the bespoke pipeline as the permanent workflow authority with no migration intent toward the platform, which contradicts the canonical requirement; per audit rules this is an implementation gap rather than a spec conflict, because there are no incompatible canonical requirements, the contract can concretely be satisfied (route development orchestration through the platform's coordination machinery or a staged observable progression toward it), and existing-code rationale is not sufficient for a conflict. If the project's actual decision is that the bespoke dev pipeline is the permanent orchestrator, the remedy is amending specs/development/dogfooding.md through the spec lifecycle (goal rule 4), not reinterpreting 'the new platform' to include repo dev scripts. Interaction that any implementing task must reconcile explicitly: the in-flight agent-runtime amendment (specs/tasks/task-298.md, 'Remove orchestrator agent-spawning and carry out the spec amendment table', assigned in the agent-runtime plan) removes the orchestrator agent-spawning surface at specs/system/agent-runtime.md §2 that a naive dogfooding fix would build on — at this base both orchestrator spawn routes still exist (crates/gyre-server/src/api/mod.rs lines 337 and 869) and agent-runtime.md still specces server-spawned orchestrators, so the dogfooding task cannot assume that machinery survives; it must define 'platform as orchestrator' against the amended agent-runtime contract (task-scheduler-driven task→agent mapping) rather than the pre-amendment orchestrator-spawn routes. This corrects the cycle-4 draft: the prior 'satisfied' decision was fabricated completion — its load-bearing claim that development is coordinated through Gyre itself conflated product surface existing in the repo with the platform actually orchestrating development; every product-code citation in that draft (spawn.rs, orchestrator.rs, mod.rs routes, e2e_ralph_loop.rs) is machinery the development workflow never calls.",
    "evidence": [
      {
        "file": "docs/dev-pipeline.md",
        "quote": "The Go dispatcher extracted from HyperShell supplies\nscheduling, worker monitoring, and its terminal cockpit. Its adapter calls Gyre's\nexisting stage handlers; periodic discovery repairs missed events. The previous\nPython scheduling loop has been removed.",
        "detail": "The repo's own development documentation states that the orchestrator of Gyre's development is a bespoke Go dispatcher extracted from HyperShell calling standalone Python stage handlers — not the Gyre platform product. This directly contradicts bullet 3: the role agent-boss held on day one was assumed by external tooling, not by 'the new platform'. Quote verified verbatim at lines 3-6 of the assigned base."
      },
      {
        "file": "docs/dev-pipeline.md",
        "quote": "It supports an adapter backend so Gyre retains\n`pipeline.sqlite3` as its single workflow authority; it does not create a second\njobs database or import HyperShell's state.",
        "detail": "The development workflow's authority is a private pipeline.sqlite3 ledger owned by the bespoke harness, separate from the platform's storage. The document presents this arrangement as the permanent design (no migration intent toward the platform's coordination machinery appears anywhere in its 1027 lines), while the canonical spec requires the platform to progressively take over the orchestrator role. Verified verbatim at lines 10-12 of the assigned base."
      },
      {
        "file": "scripts/pipeline/execution.py",
        "quote": "command = (os.environ.get('OPENSHELL', 'openshell'), '-g', self.store.setting('gateway', 'gyre-gyre'), *args)",
        "detail": "Every development worker (implementation, review, triage) is executed inside an external OpenShell-gateway sandbox via this command constructor — the pipeline's workers do not run on the platform's compute targets, agent runtime, or spawn lifecycle. Verified verbatim at line 106 of the assigned base; this is the production execution path for all model stages of Gyre's development."
      },
      {
        "file": "scripts/dev-gateway.py",
        "quote": "ENDPOINT = \"https://gw-openshell-0ac2a203e0863b91.hyp2-spoke1.infra.hypershell.app:443\"",
        "detail": "The gyre-gyre gateway that all development sandboxes run through points at external hypershell infrastructure, not at any Gyre platform service. Verified verbatim at line 8 of the assigned base; confirms the development orchestration chain (dispatcher → pipeline.sqlite3 → OpenShell gateway → fullsend/Pi workers) never touches the platform product."
      },
      {
        "file": "scripts/dev-pipeline.py",
        "quote": "\"\"\"Independent discover/claim/run development stages and optional supervisor.\"\"\"",
        "detail": "Absence evidence, inspected call paths: this is the entry point of the development orchestration, and I grepped for 'gyre-server', 'localhost:3000', and '/api/v1' across all 42 modules in scripts/pipeline/ and all 19 scripts/dev-*.py scripts — zero matches. The pipeline discovers tasks from specs/tasks/*.md via direct git/sqlite access, claims them in its own ledger, and dispatches fullsend workers; no code path in the development workflow constructs an HTTP request to the platform server or invokes any platform coordination API. Therefore no production code performs the bullet-3 replacement of agent-boss by the platform in the dev-orchestrator role. Quote verified verbatim at line 2 of the assigned base."
      },
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "quote": "        // Orchestrator lifecycle (platform-model.md §3, task-093)\n        .route(\n            \"/api/v1/repos/:id/orchestrator/spawn\",\n            post(orchestrator::spawn_repo_orchestrator),\n        )",
        "detail": "The platform's orchestrator machinery does exist as registered production routes (repo orchestrator here at line 337, workspace orchestrator at line 869) — but these are product surface that the development workflow never invokes (zero references from scripts/pipeline/ and scripts/dev-*.py, per the absence evidence above). Machinery existing in the repo while a bespoke external pipeline actually orchestrates development does not satisfy the dogfooding requirement. Verified verbatim at lines 335-339 of the assigned base."
      },
      {
        "file": "specs/development/ralph-loops.md",
        "quote": "Gyre orchestrates: sequencing, conflict resolution, shared state, escalation, and convergence.",
        "detail": "The repo's own canonical documents define the orchestrator role the dogfooding spec's 'the new platform' refers to as the Gyre platform product ('Gyre's job is twofold... Gyre orchestrates'), superseded into platform-model.md Two-Level Orchestration per specs/development/manager-agent.md. So bullet 3 requires the platform product to assume the dev-orchestrator role — a role performed at this base exclusively by the bespoke pipeline, by no platform code path. Quote verified verbatim at line 13 of the assigned base."
      },
      {
        "file": "specs/milestones/m0-walking-skeleton.md",
        "quote": "- Manager agent (in agent-boss) dispatches a sub-agent",
        "detail": "Historical evidence that bullets 1 and 2 were actually honored: the m0 walking-skeleton milestone (§4 'One Ralph Loop Executes') was executed with agent-boss's Manager agent dispatching a sub-agent that worked in a git worktree, self-reviewed, ran tests, opened a PR, and was merged by the Manager — day-one dogfooding of agent coordination with implementation agents running Ralph loops under agent-boss. Quote verified verbatim at line 36 of the assigned base."
      },
      {
        "file": "specs/prior-art/agent-boss.md",
        "quote": "Studied the `agent-boss` prototype. Not recreating it - rethinking from first principles. But these lessons carry forward:",
        "detail": "agent-boss exists in the repo only as studied prior art whose lessons were carried into the platform — it no longer orchestrates anything. Its replacement in the dev-orchestrator role was the bespoke pipeline, not the platform, which is exactly the unimplemented part of bullet 3. Quote verified verbatim at line 3 of the assigned base."
      },
      {
        "file": "specs/tasks/task-298.md",
        "quote": "title: \"Remove orchestrator agent-spawning and carry out the spec amendment table\"",
        "detail": "Interaction constraint any dogfooding implementation must reconcile: this in-flight task (assigned in the agent-runtime plan) removes the orchestrator agent-spawning surface at specs/system/agent-runtime.md §2 — the very machinery a naive dogfooding fix would route development through. At this base the orchestrator routes still exist and agent-runtime.md still specces server-spawned orchestrators, but the dogfooding task cannot assume they survive; it must define platform-as-dev-orchestrator against the amended task-scheduler contract. Quote verified verbatim at line 2 of the assigned base."
      }
    ]
  }
]
