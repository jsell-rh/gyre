---
title: "Publish reviewed spec backlog for specs/development/dogfooding.md"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: complete
commits: []
---

## Required behavior

Publish exactly the independently reviewed missing-work plan below. Only create the assigned task documents and add their exact section bindings. Preserve every existing task, spec, registry entry, coverage status, product file and gate. Task creation is not implementation. Re-read the canonical spec and challenge the audit, decomposition, dependencies and duplicate-work analysis before approving.

{
  "document": "specs/development/dogfooding.md",
  "plan_id": "9f3a9c5f2c13ec023664c34fb6a43bab3e0536e2f2e4be0391c690724f3f23f1",
  "audit": {
    "base": "88202d67f7a0070e1ef6cfe340ff8313a6475abb",
    "requirements": "644cb4b53ccb97e91fb8e411672202b4b067512863f30338c848280ee052e35a",
    "sections": [
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
    ],
    "gaps": [
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
    ],
    "source_evidence": [
      {
        "file": "docs/dev-pipeline.md",
        "sha256": "fa1def888b27debb0fd240606a67122e62b3e37b3810eade144eda324d229304"
      },
      {
        "file": "scripts/pipeline/execution.py",
        "sha256": "48e0c4125b39550aeda650f2a7488236445a4b7a65dc1d802a263f70961eac7f"
      },
      {
        "file": "scripts/dev-gateway.py",
        "sha256": "29d8046390416f1bba625d3be4f712133d4a863751ed60a7567be25a1ee00757"
      },
      {
        "file": "scripts/dev-pipeline.py",
        "sha256": "8d613ed5da4426282dbf8e984e24a53c7bff1032286b57159440b85107b16cd3"
      },
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "sha256": "f306cf67464f0902d11b3844f425b91a245e6f8726763b3e9360a026286d4c1d"
      },
      {
        "file": "specs/development/ralph-loops.md",
        "sha256": "cf66e52a49ba5f36e560ae7754c73d2261bbabd3878e0ff6946df9cacef6c8e7"
      },
      {
        "file": "specs/milestones/m0-walking-skeleton.md",
        "sha256": "4e1528ab462e4d9170fd7c38d305906c4b053f8713b74e9929b4520f846b860b"
      },
      {
        "file": "specs/prior-art/agent-boss.md",
        "sha256": "7c9aa3aab233c7621f400faf3c68290d373d03e0868a75e5f0643460e223e0f4"
      },
      {
        "file": "specs/tasks/task-298.md",
        "sha256": "4833897026c92eefaf46e1b076fd28f3651e4ad7d67b9ce83b09d60f57da2418"
      }
    ]
  },
  "plan": {
    "audit": "d9a5effa1e4b85cce54ef9f5985a33d02f130c49ffa213510262f127a484a4d5",
    "tasks": [
      {
        "key": "dogfooding-platform-orchestration",
        "title": "Platform-held development task ledger with authoritative sequencing and lifecycle state (dogfooding progression stage 1)",
        "covers": [
          "69fff34a83938a4731034172"
        ],
        "required_behavior": "Close the unimplemented bullet 3 of specs/development/dogfooding.md ('The new platform progressively replaces agent-boss as it matures') by making the Gyre platform product assume the dev-orchestrator's shared-state and sequencing responsibilities for Gyre's own development, as a concrete, load-bearing, observable progression stage — not a mirror. No platform Rust changes are required; the integration consumes the existing production REST task surface (POST/GET /api/v1/tasks, GET/PUT /api/v1/tasks/{id}, PUT /api/v1/tasks/{id}/status, GET /api/v1/workspaces/{id}/tasks — crates/gyre-server/src/api/mod.rs:407-415) through real authenticated HTTP with real platform storage. (1) Bootstrap: a new scripts/dev-platform.py (or equivalent pipeline settings path using the existing store.setting mechanism, as the 'gateway' setting does in scripts/pipeline/execution.py:106) idempotently builds (SKIP_WEB_BUILD=1) and starts a gyre-server instance on a dedicated port with its own SQLite data directory, provisions tenant/workspace/repo records representing the Gyre development context plus a service principal with Developer role and API token, and registers endpoint+token as pipeline settings so stage processes read them. (2) Registration (shared state): catalog sync (scripts/pipeline/catalog.py:43 sync_checkout) upserts every specs/tasks/*.md task into the platform — idempotent, with the task-NNN to platform-task-id mapping persisted in the pipeline store — carrying title, spec_ref, priority, order, and depends_on edges resolved through that mapping. Tasks are registered UNTYPED (no task_type): the mechanical scheduler only spawns agents for task_type=Implementation (crates/gyre-server/src/signal_chain.rs:802), so the ledger records development workflow state without triggering agent spawning and without depending on the approval gate that task-297 places on typed implementation tasks. The bridge must NOT use the orchestrator-spawn routes or the gyre_spawn_worker MCP tool (removed by task-298): 'platform as orchestrator' here means the platform's task coordination surface (ledger, dependency graph, status lifecycle), consistent with the amended agent-runtime contract in which the scheduler is the only task-to-agent path; deferring worker dispatch to the platform scheduler is the explicitly documented next progression stage, not this one. (3) Lifecycle write-back: the pipeline's real state changes transition the platform record — first implement-stage admission of a task sets in_progress; publication (the same code path that rewrites 'progress: complete' in the task document, scripts/pipeline/stages.py:262 and 340-380) sets done; a structural block (structural-fault guard / exhausted repair budget leaving the task held) sets blocked. Sync reconciles records missed by failed transitions. (4) Authoritative sequencing (enforcement): the implement-stage dependency gate at scripts/pipeline/eligibility.py:79 currently evaluates dependency satisfaction purely from local pipeline state; it must be served by the platform — a task is eligible for implement admission only if the platform reports every depends_on task done, queried over the real API. The platform's answer is authoritative over local state: a dependency the platform reports as not done blocks admission even when local task data satisfies it. Platform unavailability fails closed for new admission (durable error event recorded in the pipeline ledger; no new implement admissions) while already-claimed in-flight work proceeds — the platform is load-bearing, not advisory. (5) Documentation: docs/dev-pipeline.md is updated to describe the platform as the development task-state and sequencing authority and to record the staged progression (next stage: scheduler-driven task-to-agent mapping for development work per the amended agent-runtime.md, contingent on platform maturity), replacing its presentation of the bespoke ledger as the permanent sole workflow authority for task state; pipeline.sqlite3 remains the stage/attempt/work telemetry ledger (dispatcher adapter backend). All bridge HTTP goes through the authenticated REST surface under real ABAC policy evaluation (Developer role required for task writes); no direct platform-storage access and no privileged bypass.",
        "acceptance": [
          "Bootstrap + sync: running the bootstrap script and catalog sync against a fresh server, GET /api/v1/workspaces/{id}/tasks with the service token returns a record for every specs/tasks/*.md document with matching title/spec_ref and correctly resolved depends_on edges; re-running sync creates no duplicate records (idempotent upsert).",
          "Lifecycle: admitting implement work for a task transitions its platform record to in_progress, and completing publication transitions it to done — both observable via GET /api/v1/tasks/{id} on the platform, independent of pipeline.sqlite3.",
          "Sequencing enforcement: with a dependency task's platform record not done (e.g. reset via PUT /api/v1/tasks/{id}/status), the dependent task is refused implement-stage admission even though the local pipeline store satisfies the dependency; once the dependency's platform record is done, admission proceeds.",
          "Fail-closed: with the platform unreachable (stopped server or dead endpoint setting), the discover/claim path records a durable error event and admits no new implement work — no silent local-only fallback; already-claimed in-flight work is unaffected.",
          "Authorization: a token lacking the Developer role on the dogfooding workspace receives 403 on task create/transition through the bridge path (real ABAC evaluation, not bypassed).",
          "No spawn surface: the bridge registers tasks untyped and uses no orchestrator-spawn route and no gyre_spawn_worker; after a full sync and admission cycle, zero agent records exist in the platform for development tasks (the mechanical scheduler never spawns for them)."
        ],
        "negative_probe": "Mirror probe: point the pipeline at a running platform instance, sync, then reset a dependency task's platform status to backlog via PUT /api/v1/tasks/{id}/status while the local pipeline store still marks that dependency satisfied — run implement-stage discovery/claim: the dependent task must NOT be admitted. The real defect this must catch is a write-only mirror that updates the platform but never consults it (the fabricated-completion failure the review feedback identified: local state alone would admit the task, exactly as the base behaves today with no platform involvement at all — the base fails this probe by construction). Fail-open probe: stop the platform server and run implement discovery — admission must fail closed with a durable error; an implementation that catches the connection error and proceeds on local state is the fail-open defect.",
        "dependencies": [],
        "existing_task": null
      }
    ]
  },
  "assigned_names": {
    "dogfooding-platform-orchestration": "task-312",
    "_publication": "task-313",
    "_base": "82aed3dbddb5a5be8a195b8ffb789bcb2030c644"
  }
}

## Acceptance Criteria

- [ ] Every planned task faithfully implements its assigned canonical requirements.
- [ ] No duplicate active assignment or missing dependency is introduced.
- [ ] No product behavior, spec requirement, coverage status or enforcement is changed.
- [ ] New tasks remain not-started until this backlog publication merges.

## Shipped

Task definitions and explicit bindings only; no product implementation claimed.
