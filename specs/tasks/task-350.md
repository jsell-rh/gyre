---
title: "Publish reviewed spec backlog for specs/system/agent-runtime.md"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: complete
commits: []
---

## Required behavior

Publish exactly the independently reviewed missing-work plan below. Only create the assigned task documents and add their exact section bindings. Preserve every existing task, spec, registry entry, coverage status, product file and gate. Task creation is not implementation. Re-read the canonical spec and challenge the audit, decomposition, dependencies and duplicate-work analysis before approving.

{
  "document": "specs/system/agent-runtime.md",
  "plan_id": "da093bb0d16da84dcae7a1f8fc0ba4a14ecb9e8275c579ccae32fe282b48fdcc",
  "audit": {
    "base": "0b33cb3a4a586c587e2566ecbcd82c03da94d84b",
    "requirements": "e2624c4730e70e80879acbc050b81404fa9129fc90924610a81c15658ce31de5",
    "sections": [
      {
        "id": "4dd0880659a1055f66316a5d",
        "decision": "context",
        "requirement_quote": "It consolidates and supersedes scattered runtime definitions across `ralph-loop.md`, `platform-model.md` §4, and `agent-protocol.md`.",
        "detail": "Title heading plus an introductory blockquote stating the spec's scope (complete agent lifecycle, compute targets, prompt assembly, Ralph loop, budget enforcement). Rationale/scope only — every implementable requirement lives in the numbered sections below and is audited there. Whether the claimed consolidation/supersession of ralph-loop.md and docs/agent-protocol.md was actually carried out is audited under 'Relationship to Existing Specs' (a40446f46dbb0536f3c30ed0), where it is an open gap.",
        "evidence": []
      },
      {
        "id": "6387a8dfb19b30fa355262b9",
        "decision": "context",
        "requirement_quote": "Agents are work executors. Humans never spawn agents directly — the system spawns them in response to approved specs.",
        "detail": "Conceptual overview of the lifecycle chain (the ASCII diagram). Each arrow in the diagram is an enumerated requirement of a child section and is audited there: approval trigger and single-trigger property (The Signal Chain — gap on the single-trigger enforcement), orchestrator chain (Phases 1–3 — Phase 1 satisfied; Phases 2 and 3 are gaps in substance: the server-driven chains are real, but the principal-level 'never spawns agents' boundary is violated by the live gyre_spawn_repo_orchestrator and gyre_spawn_worker MCP surfaces), mechanical spawn (Phase 4 — satisfied), implementation/MR/gates/Ralph loop (Phases 5–7; Phase 7 has two open gaps), system-initiated agents (Phase 8 — satisfied). The sentence 'Humans never spawn agents directly' is the same requirement counted under The Signal Chain (POST /api/v1/agents/spawn remains a direct human-reachable spawn surface for task-bound workers — see 1bd201625dc5fbe0164ced05); it is not double-counted here.",
        "evidence": []
      },
      {
        "id": "5af6c76eb6471b7cb8ee0014",
        "decision": "context",
        "requirement_quote": "## 1. Agent Lifecycle",
        "detail": "Bare parent heading — no body text of its own; the section consists of The Signal Chain and Phases 1–8, each audited individually. Child dispositions: The Signal Chain gap (single-trigger enforcement incomplete), Phase 1 satisfied, Phases 2 and 3 gap (the server-driven orchestrator chains are real, but the 'never spawns agents' principal boundary is violated by the live gyre_spawn_repo_orchestrator and gyre_spawn_worker MCP surfaces — owned by the open task-298 contract space), Phases 4–6 and 8 satisfied, Phase 7 gap (spec-rejection 60-second grace not implemented; no workspace-level max_iterations tier). No standalone requirement to audit beyond the children.",
        "evidence": []
      },
      {
        "id": "ceca9704c11dbcb1f7b29597",
        "decision": "context",
        "requirement_quote": "## 2. Meta-Spec Prompt Assembly",
        "detail": "Bare parent heading — no body text of its own; the section consists of the ten child headings (Meta-Specs Are Prompts through API), each audited individually. Child dispositions: all ten satisfied at this base (the runtime assembly and spawn injection closed by task-301/task-302 wiring: prompt_assembly.rs is real production code invoked from spawn_agent_core, and the merge attestation is populated from the spawn-time record).",
        "evidence": []
      },
      {
        "id": "93bc0579bcc1fd7c4d419e39",
        "decision": "context",
        "requirement_quote": "## 3. Compute Target Model",
        "detail": "Bare parent heading — no body text of its own; the section consists of Abstraction, Supported Backends, Nix-Based Image Build, and Tenant and Workspace Configuration, each audited individually and each satisfied at this base (real ComputeTarget port, four backend implementations with the specced security defaults, Nix flake image with no Dockerfile, tenant/workspace CRUD with the fallback chain).",
        "evidence": []
      },
      {
        "id": "faa8cf78ab3d221e72297883",
        "decision": "context",
        "requirement_quote": "## 4. Budget Enforcement",
        "detail": "Bare parent heading — no body text of its own; the section consists of Cascade, Enforcement Levels, What's Tracked, and Budget Reset, each audited individually and each satisfied at this base (real cascade validation, 80%/100%/beyond-limit enforcement with a genuine 60-second grace timer and ComputeTarget::kill_process, cred-proxy usage reporting, rolling 24-hour reset job).",
        "evidence": []
      },
      {
        "id": "1bd201625dc5fbe0164ced05",
        "decision": "gap",
        "requirement_quote": "There is no other trigger. The `SpecApproved` event is the universal starting signal.",
        "detail": "Requires that spec approval is the ONLY trigger for agent implementation work. The approval side is real: the approve handler records the ledger row and emits SpecApproved only on the transition into Approved (specs.rs), modified-spec pushes reset approval_status to Pending (auto_invalidate_approvals defaults true) and revoke approvals via the post-receive hook, and untyped push-hook tasks never spawn agents. However the single-trigger property is still mechanically violated at the task layer: (1) POST /api/v1/tasks and the MCP gyre_create_task tool both accept task_type 'implementation' with no precondition that an approved spec exists — the only pre-checks are scope/preview-agent gates; (2) the Phase-4 scheduler spawns a worker for ANY Backlog Implementation task regardless of how the task was created (it checks repo status, ordering, max_agents and budget, never task origin); (3) the spawn-core approval gate added by task-302 only applies when task.spec_path is set AND the spec is in the ledger — a task with no spec_path, or one referencing a spec not in the registry, bypasses the gate entirely (the code comment states this explicitly); (4) POST /api/v1/agents/spawn spawns task-bound implementation workers for any authenticated caller with no scheduler-principal restriction. Two further live production implementation-work paths that never see SpecApproved are NOT covered by any of the four paths above: (5) Thorough preview — POST /api/v1/repos/:id/graph/thorough-preview (route in api/mod.rs, handler graph.rs::thorough_preview) is a human-triggered endpoint that creates a task with task_type Implementation whose description embeds the caller's unapproved spec DRAFT content ('implement the following spec draft on the throwaway branch' — draft_content arrives in the request body) and spawns a worker agent directly in-process via spawn_agent_core with the human as caller: the task is created through the task store (not POST /api/v1/tasks or gyre_create_task), the spawn is issued from its own route (not the /agents/spawn handler), and the scheduler never sees it; the only approval check on this path is the spawn-core ledger gate, which is skipped exactly when the draft spec is not in the registry — the normal case for an un-pushed UI draft. (6) Cascade tests — merge_processor::trigger_cascade_tests, invoked automatically on merge and by the human REST surface POST /api/v1/repos/:id/cascade-tests/trigger (api/mod.rs, dependencies.rs), creates tasks with task_type Implementation, priority High, labels ['cascade-test','auto-created'], NO spec_ref and NO spec_path, in dependent repos; the Phase-4 scheduler spawns workers for them (the in-code comment states task_type Implementation is chosen precisely so the scheduler spawns; untyped tasks are never spawned). CANONICAL CONFLICT: both surfaces are canonical requirements of sibling specs — system-explorer.md §3 ('the thorough preview (minutes via agent implementation on throwaway branches)') and dependency-graph.md §Cascade Testing ('the forge triggers a test run against Repo B's new version') — so this section's 'There is no other trigger. The SpecApproved event is the universal starting signal.' is in canonical conflict with them. Per the goal rules the conflict must be recorded and routed through the spec lifecycle — a proposed agent-runtime.md §1 amendment scoping the single-trigger property to spec-implementation work while explicitly acknowledging merge-triggered cascade verification and human-initiated draft previews — never silently ignored and never resolved by deleting the sibling specs' specced features. Consequence for any remediation contract: a scheduler approval gate implemented strictly ('the task stays Backlog unless its spec_ref resolves to an Approved ledger entry at the pinned SHA') strands every cascade-test task in Backlog forever (they carry no spec_ref), breaking dependency-graph.md's specced cascade testing together with its REST trigger and result-reporting surfaces; implemented as spec-less pass-through it leaves the single-trigger property violated by a specced path. A closure claim of 'close every task→agent path that bypasses the approval chain' is therefore false as written — the claim must name the specced exceptions and the conflict resolution. This is the open task-297 gap ('Gate all implementation work on approved specs', progress not-started).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/tasks.rs",
            "quote": "task.task_type = Some(parse_task_type(&tt)?);",
            "detail": "The HTTP task-create handler accepts task_type directly from the request body with no approval precondition (the surrounding code only gates preview agents and validates caller scope); a task created here with type 'implementation' is picked up by the Phase-4 scheduler and spawns a worker agent."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"enum\": [\"implementation\", \"delegation\", \"coordination\"],",
            "detail": "The gyre_create_task MCP tool schema lets any authorized caller create a task with task_type 'implementation'; the handler maps it to TaskType::Implementation with target-scope validation but no spec-approval precondition, so the universal-approval gate is bypassable at task creation."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "continue; // Delegation/Coordination → orchestrators; untyped → never spawned",
            "detail": "The Phase-4 scheduler scan (spawn_implementation_workers) spawns workers for every Backlog task with task_type Implementation regardless of how the task was created; the scan checks repo status, ordering, max_agents and budget but never checks that the task derives from an approved spec."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "// registry cannot be evaluated and are not gated here.",
            "detail": "The spawn-core approval gate (added by task-302 for bindings invalidation) only applies when task.spec_path is set and the spec is in the ledger; tasks with no spec_path or an unregistered spec are explicitly not gated, so they spawn without any approval."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "let resp = spawn_agent_core(&state, req, &auth, \"POST /api/v1/agents/spawn\").await?;",
            "detail": "A direct agent-spawn REST surface exists for any authenticated caller with no scheduler-principal restriction: it spawns task-bound implementation worker agents (only the internal task_type and registered-spec_path approval gates apply), so implementation agents can be created without any SpecApproved event."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "crate::signal_chain::on_spec_approved(&state, &payload).await;",
            "detail": "Positive wiring on the approval side: after recording the ledger row and emitting the SpecApproved message (only on transition into Approved), the handler routes the signal into the orchestrator chain — the legitimate chain itself is real."
          },
          {
            "file": "crates/gyre-server/src/api/graph.rs",
            "quote": "Thorough preview (system-explorer.md §3): implement the following ",
            "detail": "The thorough-preview handler builds the preview task's description from the caller-supplied unapproved spec draft (draft_content arrives in the request body, so the draft is normally not in the spec registry) and sets task_type = Some(TaskType::Implementation) — the task embeds unapproved spec content and is destined for a worker agent."
          },
          {
            "file": "crates/gyre-server/src/api/graph.rs",
            "quote": "\"POST /api/v1/repos/:id/graph/thorough-preview\",",
            "detail": "The same handler spawns the worker agent directly in-process via crate::api::spawn::spawn_agent_core with the human caller as principal: the task is created through the task store (not POST /api/v1/tasks or gyre_create_task), the spawn is issued from its own route (not the /agents/spawn handler), and the Phase-4 scheduler never sees the task — none of the four previously enumerated bypass-gate surfaces touch this path."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "if let Some(entry) = entry {",
            "detail": "Inside the spawn-core approval gate, enforcement fires only when the ledger returns an entry (if let Some); a spec_path not present in the registry — the thorough-preview draft case, since draft_content arrives in the request body and the spec was never pushed — falls through ungated, so the preview path spawns a worker for unapproved draft content exactly when the draft is unregistered."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "task_type = Implementation is the discriminator the worker",
            "detail": "trigger_cascade_tests (invoked automatically on merge and by the human REST surface POST /api/v1/repos/:id/cascade-tests/trigger) creates cascade-test tasks in dependent repos with task_type Implementation, priority High, labels ['cascade-test','auto-created'], and no spec_ref/spec_path precisely so spawn_implementation_workers dispatches a worker agent for them — an implementation-work path with no SpecApproved event anywhere in the chain, and a class a strict spec_ref-approval scheduler gate would strand in Backlog forever."
          },
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "\"/api/v1/repos/:id/cascade-tests/trigger\",",
            "detail": "The human-triggered REST surface for cascade tests; the handler (dependencies.rs::trigger_cascade_tests) delegates to merge_processor::trigger_cascade_tests, so cascade implementation work can also start from a direct human REST call rather than a merge."
          },
          {
            "file": "specs/system/system-explorer.md",
            "quote": "the thorough preview (minutes via agent implementation on throwaway branches)",
            "detail": "Canonical sibling requirement (system-explorer.md §3): the thorough preview is specced as agent implementation on throwaway branches — the endpoint's specced purpose is exactly the unapproved-draft implementation work that agent-runtime.md §1's 'There is no other trigger' wording forbids. Incompatible canonical requirements: the conflict must be routed through the spec lifecycle, not resolved by deleting either feature."
          },
          {
            "file": "specs/system/dependency-graph.md",
            "quote": "For each dependent repo, the forge triggers a test run against Repo B's new version",
            "detail": "Canonical sibling requirement (dependency-graph.md §Cascade Testing): merge-triggered test runs in dependent repos, executed in code by Implementation-type tasks the scheduler spawns; these tasks carry no spec_ref, so they cannot pass an approval-ledger gate on spec_ref — canonical conflict with agent-runtime.md §1's universal single-trigger wording."
          }
        ]
      },
      {
        "id": "f7f7d487735bb8a3495a5ef4",
        "decision": "satisfied",
        "requirement_quote": "2. Creates a `SpecApproved` message on the message bus (**new message kind** — amends `message-bus.md`). Payload: `{repo_id, spec_path, spec_sha, approved_by, approval_id}`. This is distinct from `SpecChanged` (which fires on push, before approval).",
        "detail": "All three steps are real production behavior. (1) The approve handler records the approval in the ledger (SpecApproval row with spec SHA, approver id/type derived from the verified token not the body, timestamp) and persists the transition; (2) it emits a SpecApproved message (MessageKind::SpecApproved, Event tier, server_only) with Destination::Workspace and the full specced payload — plus a documented sixth field workspace_id consumed by the server-side signal chain, noted in-code as not part of the spec'd bus payload; emission happens only on the transition INTO Approved (prior_status != Approved), so re-approving an already-approved spec does not re-fire the chain; ledger entries with no workspace scope skip the event with a warning rather than broadcasting; (3) the handler routes the payload into signal_chain::on_spec_approved, which scopes the registry, holds the per-workspace mutex, ensures a live workspace orchestrator, delivers the message to its inbox, and drives its run. SpecChanged remains a distinct push-time kind that never triggers agent spawning.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "let ledger_entry = gyre_domain::SpecApproval {",
            "detail": "Step 1: the approval handler constructs and persists the ledger entry (spec_path, spec_sha, approver type/id from the verified token, approved_at) before any event is emitted."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "\"approved_by\": event.approver_id,",
            "detail": "Step 2: the SpecApproved payload carries the specced fields (repo_id, spec_path, spec_sha, approved_by, approval_id appear together in the json! block; a documented extra workspace_id field is consumed server-side)."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "crate::signal_chain::on_spec_approved(&state, &payload).await;",
            "detail": "Step 3: the approval handler feeds the emitted payload into the server-side signal chain that ensures a live workspace orchestrator and delivers the message to its inbox."
          },
          {
            "file": "crates/gyre-common/src/message.rs",
            "quote": "/// Distinct from `SpecChanged` (which fires on push, before approval).",
            "detail": "MessageKind::SpecApproved is a real variant (serde 'spec_approved', Event tier, server_only) distinct from SpecChanged, matching the amendment row's semantics in code."
          }
        ]
      },
      {
        "id": "6d7302e3c6a23af8c4020c20",
        "decision": "gap",
        "requirement_quote": "The workspace orchestrator **only creates tasks** (via `task.create` MCP tool). It never spawns agents, never touches code, never interacts with compute targets. It thinks in tasks.",
        "detail": "The server-driven chain itself is real: on_spec_approved serializes concurrent signals on a per-workspace mutex (exactly-one-active semantics) and calls ensure_workspace_orchestrator, which reuses a live orchestrator or spawns one on demand through the real spawn core (api/orchestrator.rs) with fail-closed workspace-orchestrator persona validation, a workspace-scoped JWT, and the workspace's configured compute target (workspace assignment → tenant default → local). run_workspace_orchestrator reads the approved spec content at the pinned SHA, queries spec_links for dependents, creates the Delegation task with task_type Delegation and spec_ref 'path@sha' referencing the approved SHA, creates Coordination tasks in dependent repos, and creates priority-4 CrossWorkspaceSpecChange notifications for cross-workspace dependent workspace admins. The server-driven run only creates tasks (no agent spawns, no code writes), acks only the consumed SpecApproved message, and transitions Idle after processing (on demand, not long-lived). Documented deviations: tasks are created through the task store rather than by calling the task.create MCP tool (documented in-code as equivalent effect for a server-driven run), and the cross-repo impact analysis is deterministic spec_links querying. GAP (principal-level boundary): 'It never spawns agents' is violated at this base by the live gyre_spawn_repo_orchestrator MCP surface — a workspace-orchestrator-tier JWT (holding workspace:{ws}:spawn) can spawn the repo orchestrator agent directly through tools/call, bypassing the delegation-task trigger that Phase 3 assigns to the task scheduler ('For delegation tasks, the scheduler spawns the repo orchestrator'). The handler performs a real agent spawn via spawn_repo_orchestrator_core and returns the agent record plus token. The amendment-table row 'Remove agent.spawn MCP tool — orchestrators do not spawn agents' covers this surface. Ownership note: the open task-298 contract removes gyre_spawn_worker and its acceptance criterion is scoped to 'a repo-orchestrator JWT has no MCP tool whose handler creates agent records' — a workspace-orchestrator JWT calling gyre_spawn_repo_orchestrator would still create agent records after task-298 as currently specced, so the second tool's removal needs explicit ownership (extension of task-298's contract or separate assignment); recorded for the controller.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"name\": \"gyre_spawn_repo_orchestrator\",",
            "detail": "The gyre_spawn_repo_orchestrator MCP tool is live production code at this base — a catalog entry advertised in tools/list."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "Spawn the single repo orchestrator for a repo in the caller's workspace. Requires a workspace-orchestrator JWT.",
            "detail": "The tool's own description states its contract: a workspace-orchestrator JWT spawns the repo orchestrator — the workspace-orchestrator principal retains a direct agent-spawning capability the spec's 'never spawns agents' boundary removes."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "None => return tool_error(\"requires a workspace-orchestrator token\"),",
            "detail": "The handler's tier check admits exactly the workspace-orchestrator principal (workspace_orchestrator_scope), confirming the surface is scoped to the persona the Phase-2 boundary constrains."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "match crate::api::orchestrator::spawn_repo_orchestrator_core(",
            "detail": "The handler performs a real agent spawn (spawn_repo_orchestrator_core: agent record, token, launch on the compute target) and returns the agent record plus token — an agent spawn reachable at the workspace-orchestrator tier without any delegation task."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "pub async fn on_spec_approved(state: &Arc<AppState>, payload: &serde_json::Value) {",
            "detail": "Positive wiring retained: server-side interception of the workspace-destined SpecApproved signal — scopes by workspace, holds the per-workspace mutex (concurrent approvals serialize), ensures a live workspace orchestrator before delivering the message to its inbox."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "task.task_type = Some(TaskType::Delegation);",
            "detail": "Positive wiring retained: the delegation task is created with task_type Delegation, referencing the approved spec via spec_ref and assigned to the orchestrator."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "let spec_ref = format!(\"{spec_path}@{spec_sha}\");",
            "detail": "Positive wiring retained: the delegation task's spec_ref is the specced spec_path@spec_sha form pinning the approved blob."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "task.task_type = Some(TaskType::Coordination);",
            "detail": "Positive wiring retained: coordination tasks are created in dependent repos (discovered from spec_links) for the same repo-orchestrator mechanism to process."
          },
          {
            "file": "crates/gyre-common/src/notification.rs",
            "quote": "Self::CrossWorkspaceSpecChange => 4,",
            "detail": "Positive wiring retained: cross-workspace dependents get priority-4 notifications (CrossWorkspaceSpecChange maps to rank 4 in the unified notification taxonomy), addressed to the dependent workspace's Admin/Owner members."
          }
        ]
      },
      {
        "id": "15aec83b58b3494602d31ce8",
        "decision": "gap",
        "requirement_quote": "The repo orchestrator **only creates tasks**. It never spawns agents. The system handles the task→agent mapping mechanically.",
        "detail": "The decomposition machinery is real: the task scheduler (startup loop plus operator trigger, cycles serialized on a run lock) claims only Delegation/Coordination tasks — Implementation and untyped tasks are skipped — and holds the per-repo mutex with a re-read after the lock, so a second delegation for the same repo waits while one repo orchestrator is active; stale InProgress claims are recovered crash-safely. run_repo_orchestrator reuses/creates the repo-orchestrator agent (repo-orchestrator persona, repo-scoped JWT). decompose_delegation reads the delegation task and the spec blob at the pinned spec_ref, failing closed on a malformed or empty SHA or an unresolvable ledger SHA; decomposition uses the real LLM port (latency-bounded at 120s) with persona prompt and repo tree, and degrades to an explicitly-labeled deterministic decomposition derived from real spec content when no LLM is configured or the call fails — honest degradation, not fabricated model output. Sub-tasks carry task_type Implementation, spec_ref to the approved SHA, parent_task_id linking the delegation task, order, and depends_on chaining; the delegation task is marked Done. Documented deviations: the repo codebase state is read via git ls-tree rather than MCP tools, and tasks are created through the task store rather than the task.create MCP tool. GAP in substance: 'The repo orchestrator only creates tasks. It never spawns agents.' is violated at this base by the live gyre_spawn_worker MCP surface — catalog entry, tools/call dispatch, scope validation requiring only a repo-orchestrator token holding a repo spawn scope, and a handler that delegates to spawn_agent_core with the full worktree/scoped-JWT/compute-target provisioning. The tool's restriction TO repo-orchestrator JWTs describes the opposite of the spec's requirement ('Remove agent.spawn MCP tool — orchestrators do not spawn agents', amendment table): it exists precisely so a repo-orchestrator JWT can spawn worker agents directly, bypassing the mechanical task→agent mapping of Phase 4. This is the open task-298 gap ('Remove orchestrator agent-spawning and carry out the spec amendment table', progress not-started).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"name\": \"gyre_spawn_worker\",",
            "detail": "The gyre_spawn_worker MCP tool is live production code at this base — a catalog entry advertised in tools/list, not a removed or stubbed surface."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "Spawn a worker agent for an implementation task in the caller's repo. Requires a repo-orchestrator JWT.",
            "detail": "The tool's own description states its contract: a repo-orchestrator JWT spawns a worker agent for an implementation task — exactly the orchestrator-spawns-agents capability the spec removes."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"gyre_spawn_worker\" => handle_spawn_worker(&state, &args, &auth).await,",
            "detail": "The tools/call dispatch routes gyre_spawn_worker to a real handler — the surface is reachable by any caller holding a repo-orchestrator token."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"gyre_spawn_worker\" => {",
            "detail": "Scope validation for the tool requires only a repo-scoped orchestrator token (check_repo_scope with write/spawn on the token's single repo scope) — no scheduler-principal or delegation-task-origin check stands between an orchestrator and a worker spawn."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "match crate::api::spawn::spawn_agent_core(",
            "detail": "handle_spawn_worker delegates to the full production spawn core (worktree creation, scoped JWT minting, compute target resolution) — a complete agent spawn performed at an orchestrator's discretion, violating 'It never spawns agents'."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "if !matches!(task_type, TaskType::Delegation | TaskType::Coordination) {",
            "detail": "Positive wiring retained: the scheduler claims only Delegation/Coordination tasks (Implementation goes to the worker-spawn path; untyped pre-approval push-hook tasks are skipped), matching the task_type discriminator requirement."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "let lock = state.orchestrator_registry.repo_lock(&repo.id).await;",
            "detail": "Positive wiring retained: per-repo mutex — only one repo orchestrator is active at a time; a delegation task for a repo with a live orchestrator waits (re-scanned on a later cycle)."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "sub.parent_task_id = Some(task.id.clone());",
            "detail": "Positive wiring retained: sub-tasks carry parent_task_id linking to the delegation task, plus order and depends_on chaining and spec_ref pinning the approved SHA."
          }
        ]
      },
      {
        "id": "eb6edb7ecfd0f65611163d8c",
        "decision": "satisfied",
        "requirement_quote": "3. **Preconditions:** Checks repo status (rejects if `Archived` per `repo-lifecycle.md` §4), checks `max_agents` limit (queues task — the scheduler re-checks periodically as agents complete, does not reject or return 429), checks workspace budget (rejects if exhausted — task stays `Backlog` with a `BudgetExhausted` flag)",
        "detail": "Every enumerated step of mechanical spawning is implemented. The scheduler detects only Backlog tasks with task_type Implementation (Delegation/Coordination trigger orchestrators; untyped push-hook tasks never spawn — the task_type discriminator is additionally enforced inside spawn_agent_core, which rejects task spawning for tasks without task_type). Ordering: depends_on must be resolved (Done/Cancelled; deleted dependency treated resolved) and checked before order; order_is_next blocks only on strictly lower incomplete orders, so same-order siblings run in parallel. Preconditions: archived repos defer the task (it stays Backlog; unarchive resumes), the effective max_agents chain (Repository.max_agents → Workspace.max_agents_per_repo → unlimited) queues the task with a logged 'repo at max_agents' skip (no rejection, no 429), and workspace/repo budget exhaustion keeps the task Backlog with the 'budget-exhausted' label flag (cleared on the later successful spawn). The claim transitions Backlog → InProgress with a freshest-row re-read defense. The spawn core creates the agent record (Active, bound to the task), mints an EdDSA repo-scoped worker JWT, assembles the prompt set via prompt_assembly::assemble_prompt_set, creates the git worktree on the task's branch, provisions on the workspace's compute target, and injects GYRE_SERVER_URL/AUTH_TOKEN/BRANCH/TASK_ID/REPO_ID (plus workspace/agent ids, meta-spec and protocol prompts, iteration). Spawn failure retries 3 times at 1s/5s/30s with the claim kept fresh, tears down each failed attempt's agent, and on final failure blocks the task, notifies the repo orchestrator (TaskBlocked, spawning one if none is active) and creates the priority-1 'Compute target unavailable' notification for workspace Owner/Admin members, with no fallback to a different compute target. AgentStatus enum (Active/Idle/Failed/Stopped/Dead) exists on the domain Agent. The spec-approval precondition is not among Phase 4's enumerated preconditions — the single-trigger gap is counted under The Signal Chain (1bd201625dc5fbe0164ced05); note the spawn core does check ledger approval for tasks with a registered spec_path. The repo max_agents ≤ workspace max_agents_per_repo validation (400) is real (audited under §4 Cascade). Note also that the scheduler's spawn set includes the specced cascade-test class of spec-less Implementation tasks (merge_processor::trigger_cascade_tests creates them with no spec_ref and no spec_path precisely so this scheduler spawns workers, per dependency-graph.md §Cascade Testing): Phase 4's own enumerated preconditions include no spec-approval check, but any approval gate added under The Signal Chain (1bd201625dc5fbe0164ced05) must define its behavior for these spec-less tasks or it strands them in Backlog.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "if task.task_type != Some(TaskType::Implementation) {",
            "detail": "Step 1: the scheduler spawns workers only for Backlog Implementation tasks — Delegation/Coordination trigger orchestrator spawning and untyped pre-approval push-hook tasks are never spawned."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "// task_type intentionally left as None: push-hook tasks are informational",
            "detail": "The spec-lifecycle push hook creates its initial tasks without task_type, so they cannot trigger agent spawning — the discriminator the spec requires against pre-approval tasks."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "const BUDGET_EXHAUSTED_LABEL: &str = \"budget-exhausted\";",
            "detail": "Budget precondition: an exhausted workspace/repo budget keeps the task Backlog with the budget-exhausted label flag (set on the pre-check failure and on mid-flight races, cleared on the later successful claim)."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "/// Effective per-repo agent limit: `Repository.max_agents` →",
            "detail": "The max_agents fallback chain (Repository.max_agents → Workspace.max_agents_per_repo → unlimited) is enforced at spawn by counting Active agents scoped to the repo; at the limit the task queues for a later cycle — no 429."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "pub(crate) const SPAWN_RETRY_BACKOFF_SECS: [u64; 3] = [1, 5, 30];",
            "detail": "Spawn-failure handling: 3 retries with the specced 1s/5s/30s backoff (claim kept fresh against the 60s stale recovery), then task Blocked + TaskBlocked to the repo orchestrator + priority-1 compute-target-unavailable notification."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "let prompt_set = crate::prompt_assembly::assemble_prompt_set(state, &task).await;",
            "detail": "Step 7: the spawn core assembles the prompt set from the meta-spec registry (§2 wiring) and stores the pinned record for later attestation and MCP prompt serving."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "container_env.insert(\"GYRE_SERVER_URL\".to_string(), state.base_url.clone());",
            "detail": "Step 10: environment injection — server URL, auth token, branch, task id and repo id (plus agent/workspace ids) are injected into the launch context for the compute-target backend."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": ".create_worktree(&repo_root, &worktree_path, &req.branch)",
            "detail": "Step 8: a git worktree is created on the task's branch (jj workspace preferred, plain-git fallback), recorded in storage linked to the agent and task."
          },
          {
            "file": "crates/gyre-server/src/auth.rs",
            "quote": "pub fn mint_worker_scoped(",
            "detail": "Step 6: the worker JWT is minted scoped to the agent's repo (EdDSA encoding with a real Ed25519 key pair; repo:{name}:write scope), per platform-model.md §1 Token Scoping."
          }
        ]
      },
      {
        "id": "de73e06638559fcca67b3c5d",
        "decision": "satisfied",
        "requirement_quote": "The agent has no knowledge of other agents, the merge queue, or the broader system state. It sees its task, its spec, and its repo.",
        "detail": "The agent loop is real: the agent reads its task and spec refs via MCP tools, edits/commits/pushes via git, and signals completion with the gyre_agent_complete MCP tool, which (identically to the REST POST /agents/:id/complete surface) reuses the shared completion core — MR from the agent's worktree branch to the repo default branch, token revoked, budget slot released, task-scoped secrets revoked — and starts the Ralph loop. The knowledge-boundary sentence is enforced mechanically (task-299): worker-tier principals (JWTs carrying a task claim) get agents:// and queue:// removed from their resources/list advertisement and their resources/read is refused with a Forbidden naming the Phase-5 boundary; principals permitted to read them get per-handler caller-scope resolution (workspace memberships, signed workspace/repo claims) rather than unfiltered listings.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"gyre_agent_complete\" => handle_agent_complete(&state, &args).await,",
            "detail": "The agent signals completion via the gyre_agent_complete MCP tool (in the needs_write RBAC gate, self-only identity check), which creates the completion MR through the shared core and starts the Ralph loop — the same behavior as the REST surface."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "fn is_worker_tier(auth: &AuthenticatedAgent) -> bool {",
            "detail": "Worker-tier detection (JWT task claim) drives the knowledge boundary: workers cannot discover or read other agents or the merge queue."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "uri != \"agents://\" && uri != \"queue://\"",
            "detail": "The worker-tier resources/list advertisement removes agents:// and queue://, so a worker agent has no knowledge of other agents or the merge queue; resources/read additionally refuses them with a Forbidden naming the agent-runtime.md §1 Phase 5 boundary."
          }
        ]
      },
      {
        "id": "72fc3b6af856353035268add",
        "decision": "satisfied",
        "requirement_quote": "2. Gates run automatically (configured per repo in `agent-gates.md`):",
        "detail": "Both completion surfaces (REST complete and MCP gyre_agent_complete) create the MR from the agent's branch to the repo's default branch via the shared create_mr_for_completion core and then call ralph_loop::on_agent_mr_created, which enqueues the MR through merge_queue::enqueue_core (mergeable-status guard) — enqueueing triggers gate_executor::trigger_gates_for_mr, so gates run automatically on agent completion with no human step. Gate types match the spec: TestCommand/LintCommand gates run the configured command and pass on exit 0; the AgentReview (spec review) gate spawns a real reviewer agent process with the reviewer persona, serving the MR diff together with the spec content at the MR's pinned spec_ref SHA (not HEAD); the TraceCapture gate captures OTel spans during test execution via a real OTLP receiver and is observational (always passes). All gate results are recorded on the MR through the port-backed gate_results store (queried by check_gates_for_mr), with advisory failures additionally attached as MR comments.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/ralph_loop.rs",
            "quote": "pub async fn on_agent_mr_created(state: &Arc<AppState>, agent: &Agent, mr: &MergeRequest) {",
            "detail": "The Ralph-loop entry point called by BOTH completion surfaces after MR creation; it enqueues the MR and starts the bounded gate watcher."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "crate::ralph_loop::on_agent_mr_created(&state, &agent, &mr).await;",
            "detail": "REST completion surface: after creating the MR via the shared core, the handler hands off to the Ralph loop so gates run automatically (the MCP surface makes the identical call)."
          },
          {
            "file": "crates/gyre-server/src/api/merge_queue.rs",
            "quote": "crate::gate_executor::trigger_gates_for_mr(state.clone(), mr_id.clone(), mr.repository_id)",
            "detail": "Entering the merge queue triggers the repo's configured gate set — automatic gate execution on agent completion."
          },
          {
            "file": "crates/gyre-server/src/gate_executor.rs",
            "quote": "GateType::TraceCapture => run_trace_capture_gate(&state, &gate, &mr_id, &result_id).await,",
            "detail": "TraceCapture is a real gate type that captures OTel spans during test execution and is observational (always passes), matching the spec row; test/lint gates run commands with exit-0 verdicts and the AgentReview gate spawns a reviewer agent evaluating the diff against the spec at its pinned SHA."
          }
        ]
      },
      {
        "id": "e60f62cd06826e5e4de9decf",
        "decision": "gap",
        "requirement_quote": "3. Agents have 60 seconds to commit current work (same grace period as budget exhaustion), then are killed and marked `Stopped`",
        "detail": "Most of Phase 7 is real (task-118): pass path merges and completes the task; fail path returns the task to InProgress, writes one GateResult message per failed gate carrying the gate's real recorded output into the agent's inbox, closes the failed MR, and re-spawns the agent via relaunch_agent_session with a fresh EdDSA JWT, a fresh context (prompt set re-assembled at pinned versions; no conversation history loaded) on the same worktree/branch, incrementing the iteration and injecting GYRE_ITERATION; max iterations transition the agent to Failed, the task to Blocked, revoke the token and route a TaskBlocked message to the repo orchestrator's inbox (spawning one if none is active); spawn failure retries 3× at 1s/5s/30s then blocks with priority-1 notification. Two specific requirements are NOT implemented (the open task-300 gap, progress not-started): (1) Spec-rejection grace period — the reject handler cancels in-flight tasks (including the delegation task), sends a shutdown message whose payload merely carries \"grace_period_secs\": 60, creates the priority-2 SpecRejected notification, but marks the agent Stopped synchronously and cleans up its worktrees immediately; there is no 60-second timer and no ComputeTarget::kill_process() call on this path (in contrast, the budget-exhaustion path implements a real 60s grace timer with kill_process — so the 'same grace period as budget exhaustion' equivalence the spec asserts is false in code). (2) max_iterations workspace tier — the spec says the Repository's max_agent_iterations 'falls back to workspace default, then server default of 10', but no workspace-level iteration setting exists anywhere (the Workspace struct has max_agents_per_repo only); the resolution goes straight from the repo override to the server default 10, and the constraints prompt documents 'repo override, else the server default (10)'.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "\"grace_period_secs\": 60,",
            "detail": "The spec-rejection shutdown message carries the 60-second grace only as a payload field — no timer is armed on this path."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "let _ = agent.transition_status(gyre_domain::AgentStatus::Stopped);",
            "detail": "The reject handler marks active agents Stopped synchronously; there is no ComputeTarget::kill_process() call and no grace window in which the agent could commit current work (the process is left running while its worktree is destroyed)."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "crate::api::spawn::cleanup_worktrees_for_task(&state, &task).await;",
            "detail": "The cancelled task's worktrees are torn down immediately during rejection — the in-process grace the spec requires for committing current work does not exist on this path."
          },
          {
            "file": "crates/gyre-server/src/api/budget.rs",
            "quote": "pub const BUDGET_GRACE_PERIOD_SECS: u64 = 60;",
            "detail": "Contrast: the budget-exhaustion path DOES implement the real 60-second grace (start_budget_grace_timer → kill_workspace_agents_for_budget via ComputeTarget::kill_process, Idle for agents completing in grace, Stopped for killed ones) — the same mechanism the spec says spec rejection shares, but the rejection path never invokes it."
          },
          {
            "file": "crates/gyre-server/src/ralph_loop.rs",
            "quote": "let max_iterations = repo\n        .as_ref()\n        .and_then(|r| r.max_agent_iterations)\n        .unwrap_or(DEFAULT_MAX_ITERATIONS);",
            "detail": "The iteration ceiling resolves from the Repository override directly to the server default (10) — the specced workspace-default tier does not exist (the Workspace entity has no max_agent_iterations field; a repo-wide grep finds none)."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "// Iteration ceiling: repo override, else the server default (10).",
            "detail": "The served constraints prompt itself documents the two-tier resolution (repo override, else server default 10), confirming the workspace tier required by the spec is absent."
          },
          {
            "file": "crates/gyre-server/src/ralph_loop.rs",
            "quote": "pub(crate) async fn relaunch_agent_session(",
            "detail": "Positive wiring for the loop itself: gate failure re-spawns the agent with a fresh JWT and fresh context (no conversation history), incrementing the iteration and injecting GYRE_ITERATION, exactly as the fresh-session key property requires."
          }
        ]
      },
      {
        "id": "90c6b120df4e54f175cc08de",
        "decision": "satisfied",
        "requirement_quote": "System-initiated agents use the same compute target as their workspace. They receive the same meta-spec prompt assembly (§2) — the distinction is in their persona and scope, not their runtime.",
        "detail": "All three system-initiated agent types are real. Gate reviewer: the AgentReview gate spawns a reviewer process (GYRE_REVIEW_TOKEN, reviewer persona) that evaluates the MR diff against the spec at the pinned SHA and is single-shot — the scoped token is revoked after the process exits regardless of outcome; no-command gates fail closed rather than fabricating approval. Interrogation: the spawn endpoint's agent_type 'interrogation' path is read-only (no worktree; three fail-closed ABAC Deny restriction policies; policies torn down on completion) with a 30-minute JWT TTL. Reconciliation: the reconciliation sweep is database-only, but its Immediate strategy creates Delegation tasks (and Rolling/Background defer to the orchestrator), which flow through the same signal-chain scheduler and spawn machinery as worker agents — same workspace compute target, same §2 prompt assembly. All run on the workspace's compute target with the same runtime as workers.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/gate_executor.rs",
            "quote": "// regardless of outcome — single-minded agents get one verdict.",
            "detail": "Gate reviewer lifecycle: the scoped review token is revoked after the verdict regardless of outcome (revoke_gate_token), implementing the single-shot submit-verdict-then-teardown row."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "const INTERROGATION_JWT_TTL_SECS: u64 = 1800;",
            "detail": "Interrogation agents get a 30-minute JWT TTL, are read-only via fail-closed ABAC Deny policies, spawn no worktree, and have their restriction policies cleaned up when the conversation ends."
          },
          {
            "file": "crates/gyre-server/src/reconciliation.rs",
            "quote": "task.task_type = if delegation {",
            "detail": "Reconciliation creates Delegation-signal tasks that route through the same task scheduler and spawn chain as worker agents (same compute target and prompt assembly), rather than a separate runtime."
          }
        ]
      },
      {
        "id": "a00a6b5082295555260842b2",
        "decision": "satisfied",
        "requirement_quote": "There is no separate \"prompt configuration.\" The meta-spec registry IS the prompt configuration.",
        "detail": "The core property holds in production: there is no separate prompt configuration — the meta-spec registry drives what agents receive. prompt_assembly::assemble_prompt_set builds the agent's system prompt from the registry (required tenant/workspace bands + spec-level bindings), spawn_agent_core invokes it and injects the rendered prompt as GYRE_META_SPEC_PROMPT, and the pinned record is stored for attestation and system://persona serving, so editing a meta-spec changes what spawned agents receive. The MetaSpec struct carries exactly the specced extension fields (prompt, version auto-incremented on edit, content_hash SHA-256, required) plus id/kind/name/scope/scope_id/created_by/created_at/updated_at, covering all four kinds including meta:persona. The approval lifecycle is enforced fail-closed: editing content bumps the version and resets approval_status to Pending, and assembly excludes non-Approved meta-specs (required bands via filter_approved, bindings gated on the entity's current approval status) — 'cannot be used by agents until re-approved' is real. PersonaScope (Tenant/Workspace/Repo) is preserved on the Persona entity for nearest-scope-wins resolution, while the meta-spec registry's MetaSpecScope (Global/Tenant/Workspace) has no Repo variant, so repo-scoped entries cannot be marked required — matching 'only tenant/workspace admins can enforce meta-specs across all agents'. Persona operational fields (model, temperature, max_tokens, budget) remain separate from prompt content. Documented modeling deviation: the spec says the Persona struct gains the MetaSpec fields (using existing names prompt/content_hash) — instead, all four kinds including meta:persona run through the unified MetaSpec struct, and the legacy Persona entity retains system_prompt (not prompt) and gains no required flag. Every §2 runtime behavior (injection, versioning, approval gating, required enforcement) is delivered through the registry; the retention of the parallel /api/v1/personas surface that the amendment table says to remove is counted under a40446f46dbb0536f3c30ed0.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/prompt_assembly.rs",
            "quote": "pub async fn assemble_prompt_set(state: &Arc<AppState>, task: &Task) -> PromptSet {",
            "detail": "The registry IS the prompt configuration: prompt assembly reads the meta-spec registry (required bands + spec-level bindings at pinned versions) — there is no separate prompt configuration path."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "container_env.insert(\"GYRE_META_SPEC_PROMPT\".to_string(), meta_prompt);",
            "detail": "The assembled meta-spec prompt is injected into the spawned agent's environment (with GYRE_META_SPEC_SET_SHA), so editing a meta-spec changes what agents receive — the meta-specs-as-prompts property is runtime behavior, not storage only."
          },
          {
            "file": "crates/gyre-domain/src/meta_spec.rs",
            "quote": "pub required: bool,",
            "detail": "The MetaSpec struct carries the specced extension fields (prompt, version, content_hash, required) for all four kinds including meta:persona — the registry model the section defines."
          },
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "ms.approval_status = MetaSpecApprovalStatus::Pending;",
            "detail": "Editing a meta-spec's content resets approval_status to Pending (with approved_by/approved_at cleared), so the new version cannot be used by agents until re-approved by a human."
          },
          {
            "file": "crates/gyre-server/src/prompt_assembly.rs",
            "quote": "if ms.approval_status != MetaSpecApprovalStatus::Approved {",
            "detail": "Fail-closed approval gate: assembly only ever injects Approved meta-specs — Pending/Rejected entries (e.g. after an edit) are excluded from both required bands and bindings."
          },
          {
            "file": "crates/gyre-domain/src/workspace.rs",
            "quote": "    Repo(Id),",
            "detail": "PersonaScope retains the Repo variant on the Persona entity (nearest-scope-wins overrides per platform-model.md §2), while the meta-spec registry's scope enum has no Repo variant — repo-scoped entries cannot be marked required, as the section requires."
          }
        ]
      },
      {
        "id": "6caa3e55951ef9950cbd621f",
        "decision": "satisfied",
        "requirement_quote": "**DB-backed, not repo-backed.** Meta-specs span repos and workspaces. They use content-addressable SHA-256 hashes (computed from `content`) for versioning, not git SHAs. All edits are stored in the database. Full version history is retained in a `meta_spec_versions` table.",
        "detail": "The registry is DB-backed exactly as specced: migration 000032 creates meta_specs, meta_spec_versions (UNIQUE(meta_spec_id, version)) and meta_spec_bindings in portable SQL (widened to the Global/Tenant/Workspace scope set by migration 000059 with a portable table rebuild; bindings_sha added to the spec ledger by 000071, which also drops the retired KV meta_spec_sets tables). The MetaSpec struct carries prompt/version/content_hash/required plus the specced identity fields; content_hash is a SHA-256 over the prompt text recomputed on edit; canonical_bindings_sha gives bindings a content-addressable identity. Both the SQLite and PostgreSQL adapters implement MetaSpecRepository with the version-archive semantics (update bumps the version and archives the prior row in meta_spec_versions immutably; get_version resolves from archive then live row), and the mem adapter guards the same contracts in code with a shared binding store. Spec-level bindings storage exists with a production writer (the bindings PUT endpoint, task-302).",
        "evidence": [
          {
            "file": "crates/gyre-adapters/migrations/2026-03-26-000032_meta_specs/up.sql",
            "quote": "CREATE TABLE meta_spec_versions (",
            "detail": "The meta_spec_versions table retains full version history in the database (UNIQUE(meta_spec_id, version)), alongside meta_specs and meta_spec_bindings — the DB-backed registry the spec requires, in portable SQL that runs on both SQLite and PostgreSQL."
          },
          {
            "file": "crates/gyre-adapters/src/sqlite/meta_spec.rs",
            "quote": "if current_row.version != version {",
            "detail": "SQLite adapter: an update that changes the version archives the prior row into meta_spec_versions before writing the new one — immutable history retention."
          },
          {
            "file": "crates/gyre-adapters/src/postgres/meta_spec.rs",
            "quote": "if current_row.version != version {",
            "detail": "PostgreSQL adapter implements the identical version-archive semantics, so the port contract (history retention) is enforced by every adapter, not just SQLite."
          },
          {
            "file": "crates/gyre-domain/src/meta_spec.rs",
            "quote": "pub fn canonical_bindings_sha(bindings: &[MetaSpecBinding]) -> String {",
            "detail": "Bindings are content-addressed (SHA-256 over the sorted meta_spec_id/pinned_version lines) — the DB-backed, hash-addressed versioning model, not git SHAs."
          }
        ]
      },
      {
        "id": "54a514da8df9bf4d236e09ff",
        "decision": "satisfied",
        "requirement_quote": "**Who can set `required`:** Only admins at the meta-spec's own scope level — tenant admins for tenant meta-specs, workspace admins (Owner/Admin role) for workspace meta-specs.",
        "detail": "Required meta-specs are always injected and cannot be opted out: prompt_assembly pushes every Approved required meta-spec in the required-tenant and required-workspace bands unconditionally (bypassing nearest-scope resolution), regardless of spec-level bindings. The privilege boundary is real (task-301, including the cross-tenant repair): authorize_registry_write gates POST and PUT — including flipping required in either direction — by the meta-spec's OWN scope: tenant Admin for Global/Tenant entries (Tenant additionally requires the caller to be an Admin of the scope tenant), and the workspace's Owner/Admin members for Workspace entries, with the Workspace branch loading the workspace and enforcing ws.tenant_id == auth.tenant_id unconditionally so an Admin of tenant A can never write inside tenant B's workspaces; scope containment runs first so a membership grant can never cross tenants. A workspace Owner/Admin PUT on a Global/Tenant entry is 403 — the escalation direction the spec warns about. Repo scope cannot be marked required (MetaSpecScope has no Repo variant). Same-name collisions: dedup is by meta_spec_id, so a required tenant meta-spec and a same-named repo-scoped persona are separate prompt sections — the agent receives both, as the spec's collision rule states.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "async fn authorize_registry_write(",
            "detail": "The scope-level grant gates registry writes (POST/PUT, including required flips in either direction) by the meta-spec's own scope — tenant Admin for Global/Tenant, workspace Owner/Admin membership for Workspace, with unconditional cross-tenant containment on the Workspace branch."
          },
          {
            "file": "crates/gyre-server/src/prompt_assembly.rs",
            "quote": "push_required(ms, \"required-tenant\");",
            "detail": "Required meta-specs are injected unconditionally in the required bands (required-tenant then required-workspace) — always included, never opt-out, bypassing scope resolution."
          },
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "ms.approval_status = MetaSpecApprovalStatus::Pending;",
            "detail": "Editing a meta-spec resets approval to Pending under the same admin-gated write path, preserving the human-oversight property for prompt changes (required-flag flips are PUTs through the same gate)."
          }
        ]
      },
      {
        "id": "e97ea9a092758dc93c245e6e",
        "decision": "satisfied",
        "requirement_quote": "Stored in the spec registry alongside other spec metadata. The spec author selects these via the UI (Specs view → Edit → Meta-spec bindings panel). The UI shows available meta-specs from the tenant and workspace registries, with required ones pre-selected and locked (cannot be removed).",
        "detail": "Spec-level bindings are stored in the spec registry as metadata covered by the registry SHA the approval ledger records (task-302): SpecLedgerEntry.bindings_sha holds the canonical SHA over the sorted {meta_spec_id}\\t{pinned_version} lines, and registry_sha derives the composite identity (content SHA + bindings SHA, equal to current_sha when no bindings, so existing approvals stay valid). The API surface is real: PUT/GET /api/v1/specs/:path/meta-spec-bindings — a declarative full-set replace with pin-version-exists validation (history lookup for pins below current), a cross-workspace leak guard (a Workspace-scoped meta-spec cannot bind outside its workspace), role gating (global Admin or Developer membership in the workspace owning the spec; agent tokens refused), and a fail-closed ledger-first write that resets approval_status to Pending on any binding-set change (re-approval cascade). The UI panel exists in the Specs view Edit tab (DetailPanel.svelte) — available meta-specs from the registries, required ones pre-selected and locked, optional ones selectable with a pinned-version select, saved via the bindings endpoints (web/src/lib/api.js putSpecMetaSpecBindings). Consumption at pinned versions happens in assembly band 3 with same-id dedup against the required bands.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "\"/api/v1/specs/:path/meta-spec-bindings\",",
            "detail": "The spec-level binding endpoints (GET + PUT) are registered at the specced path — the author-facing surface for selecting meta-specs at pinned versions."
          },
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "let new_bindings_sha = gyre_domain::meta_spec::canonical_bindings_sha(&created);",
            "detail": "The binding set is content-addressed and stored as registry metadata (bindings_sha on the ledger entry), giving the spec's identity a component the approval ledger records."
          },
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "ledger_entry.approval_status = gyre_domain::spec_ledger::ApprovalStatus::Pending;",
            "detail": "Changing the binding set resets the spec's approval to Pending (new registry SHA), so pins cannot take effect until a human re-approves — the re-approval cascade the stale-pin flow relies on."
          },
          {
            "file": "web/src/lib/DetailPanel.svelte",
            "quote": "── Meta-spec bindings panel (agent-runtime.md §2 Spec-Level Binding) ──",
            "detail": "The UI panel exists in the Specs view Edit tab: available meta-specs from the registries with required ones pre-selected and locked, optional ones selectable with pinned-version selects, saved through the bindings endpoints."
          }
        ]
      },
      {
        "id": "ea9967fc3c67f4c5ee136e41",
        "decision": "satisfied",
        "requirement_quote": "1. All REQUIRED tenant meta-specs (ordered by kind: persona → principle → standard → process)",
        "detail": "The three-band injection order is implemented in prompt_assembly: band 1 pushes all Approved required tenant meta-specs (Global entries plus Tenant-scoped entries for the workspace's own tenant), band 2 all Approved required workspace meta-specs, band 3 spec-level bindings at their pinned versions — each band sorted by kind_rank (persona=0, principle=1, standard=2, process=3), matching the specced persona → principle → standard → process ordering. Bindings resolve at pinned versions via prompt_at_version, which reads immutable version history (a pinned old version serves that version's content). Deduplication is by meta_spec_id: a required meta-spec that also appears in spec-level bindings is included only once (in the required section) — the binding is treated as redundant and skipped via the seen_ids set maintained across bands. No concern matching — ordered inclusion, as specced.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/prompt_assembly.rs",
            "quote": "MetaSpecKind::Persona => 0,",
            "detail": "Kind ordering within each band: persona → principle → standard → process via kind_rank, exactly the specced ordering."
          },
          {
            "file": "crates/gyre-server/src/prompt_assembly.rs",
            "quote": "push_required(ms, \"required-workspace\");",
            "detail": "Band order: required-tenant is pushed first, then required-workspace, then spec-level bindings — the three-band injection order the spec defines."
          },
          {
            "file": "crates/gyre-server/src/prompt_assembly.rs",
            "quote": "async fn prompt_at_version(",
            "detail": "Spec-level bindings resolve at their pinned versions from immutable version history — the author's explicit selections at pinned versions, band 3."
          },
          {
            "file": "crates/gyre-server/src/prompt_assembly.rs",
            "quote": "if seen_ids.contains(&binding.meta_spec_id.to_string()) {",
            "detail": "Deduplication: a required meta-spec that also appears in spec-level bindings (same meta_spec_id) is included only once, in the required section — the redundant binding is skipped."
          }
        ]
      },
      {
        "id": "b0a63ae17c4db552f4dfab90",
        "decision": "satisfied",
        "requirement_quote": "**Attestation:** The merge attestation bundle records the full set of meta-spec SHAs used:",
        "detail": "Versioning: editing a meta-spec assigns a new version number (auto-increment), recomputes content_hash from the content, and archives the prior row immutably in meta_spec_versions (both SQLite and PostgreSQL adapters; the mem adapter guards the same contract); specs pinning old versions are not automatically updated — pins resolve through version history. Attestation: MergeAttestation carries the meta_specs_used field (Vec<MetaSpecUsed> with id, kind, content_hash, version, required, scope — the specced JSON shape), and the sole production writer (merge_processor.rs process_next) populates it with real data by loading the agent's spawn-time prompt-set record (load_prompt_set_record) and mapping its meta_specs_used into the bundle before signature. The meta_specs_used: vec![] occurrences in api/merge_requests.rs and retention.rs are inside #[cfg(test)] modules (test fixtures), not production callsites — the 2026-09-30 coverage note describing them as the production state predates the task-116 repair, which merge_processor's own regression test guards ('Reverting the wiring in process_next to meta_specs_used: vec![] ... fails this').",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "ms.version += 1;",
            "detail": "Editing a meta-spec assigns the next version number (with content_hash recomputed and approval reset), per the versioning steps."
          },
          {
            "file": "crates/gyre-adapters/src/sqlite/meta_spec.rs",
            "quote": "if current_row.version != version {",
            "detail": "The old version is retained immutably in meta_spec_versions (the archive happens on every version change), and pinned specs keep resolving the old content through that history — not auto-updated."
          },
          {
            "file": "crates/gyre-domain/src/attestation.rs",
            "quote": "pub meta_specs_used: Vec<MetaSpecUsed>,",
            "detail": "MergeAttestation carries the meta_specs_used array (MetaSpecUsed: id, kind, content_hash, version, required, scope), amending the bundle as the spec requires."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "let meta_specs_used = match updated_mr.author_agent_id.as_ref() {",
            "detail": "Production population with real data: at merge time the processor loads the author agent's spawn-time prompt-set record and records the meta-specs actually used — not a hardcoded empty array."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "crate::prompt_assembly::load_prompt_set_record(state, agent_id.as_str())",
            "detail": "The attestation's meta_specs_used comes from the pinned spawn-time record (the set the agent actually ran with), making the bundle an accurate record of the meta-spec SHAs used."
          }
        ]
      },
      {
        "id": "9f95c49cf209205750a0177a",
        "decision": "satisfied",
        "requirement_quote": "System creates a soft notification (Inbox priority 6, \"Meta-spec drift alert\"):",
        "detail": "Stale pin detection is a live hourly job plus an on-edit trigger (task-302): the JobRegistry registers 'meta_spec_stale_pin_check' at interval_secs 3600 (and the meta-spec update path triggers a detection run), and detect_stale_pins compares every binding's pinned_version against the meta-spec's current version, deduplicating via stale_pin:{spec}:{ms} KV keys. A stale pin creates the priority-6 MetaSpecDrift notification with the specced wording, addressed to the workspace's Admin/Owner members and fanned out through the notification dispatcher. Acting on the alert — updating the pin via the bindings PUT — records the new binding set (bindings_sha), producing a new registry SHA and resetting approval_status to Pending exactly as a content edit does; the spawn gate then refuses worker agents for the task's spec until a human re-approves, and re-approval fires the existing SpecApproved → orchestrator → delegation chain, re-implementing under the new pins. The invalidation cascades through the existing spec lifecycle — no new mechanism, as the spec states.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/jobs.rs",
            "quote": "name: \"meta_spec_stale_pin_check\".to_string(),",
            "detail": "The background job is registered and runs hourly (interval_secs: 3600), satisfying 'a background job (or check on spec access) detects when a spec pins an old meta-spec version'."
          },
          {
            "file": "crates/gyre-server/src/prompt_assembly.rs",
            "quote": "\"Meta-spec drift alert: '{}' uses {} v{}, but v{} is available. Review and update pin.\",",
            "detail": "The soft notification carries the specced drift-alert wording, created per stale binding and deduplicated so it re-notifies only after a real change."
          },
          {
            "file": "crates/gyre-common/src/notification.rs",
            "quote": "Self::MetaSpecDrift => 6,",
            "detail": "MetaSpecDrift maps to Inbox priority 6 — the soft-notification rank the spec assigns to the drift alert."
          },
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "ledger_entry.approval_status = gyre_domain::spec_ledger::ApprovalStatus::Pending;",
            "detail": "Updating the pin changes the spec's registry SHA and resets approval to Pending, cascading through the existing lifecycle (re-approval → SpecApproved → re-implementation) rather than a new mechanism."
          }
        ]
      },
      {
        "id": "a787945d4c0f4330d64e8aca",
        "decision": "satisfied",
        "requirement_quote": "Gyre ships with default meta-specs seeded at first startup:",
        "detail": "seed_builtin_meta_specs seeds all nine specced entries at first startup — seven personas (default-worker, workspace-orchestrator, repo-orchestrator, spec-reviewer, accountability, security, reconciliation), the conventional-commits principle (Global, required=true), and the test-coverage standard — all Global scope, all with approval_status Approved (pre-approved, with approved_by system), idempotent (skips when the registry is non-empty), and invoked from main at server start. Entries can be edited (PUT, which version-bumps and re-approves), replaced (DELETE + POST, with DELETE guarded while bindings reference), and their required flag changed by admins through the scope-gated write path. Minor note: 'cloned' has no dedicated clone endpoint — cloning is achievable by composing GET (content) + POST (create), which the API surface fully supports; the seeded prompt bodies are one-liners, which the spec does not constrain.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "pub async fn seed_builtin_meta_specs(state: &Arc<AppState>) {",
            "detail": "The bootstrap seeding function seeds the default meta-spec registry at first startup."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "\"conventional-commits\",",
            "detail": "The specced conventional-commits principle is seeded (Global, required=true — the one required bootstrap entry)."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "\"test-coverage\",",
            "detail": "The specced test-coverage standard is seeded (Global, not required), completing the nine-entry bootstrap table with the seven personas."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "if !existing.is_empty() {",
            "detail": "Seeding is idempotent — it runs only when the registry is empty, so user edits are never overwritten on restart."
          },
          {
            "file": "crates/gyre-server/src/main.rs",
            "quote": "seed_builtin_meta_specs(&state).await;",
            "detail": "The seeding is invoked at server startup, so a fresh deployment ships the defaults."
          }
        ]
      },
      {
        "id": "917eb99ea432ee472c349487",
        "decision": "satisfied",
        "requirement_quote": "| `GET /api/v1/meta-specs` | GET | List meta-specs (filtered by `?scope=`, `?scope_id=`, `?kind=`, `?required=`). ABAC: scope_id extracted from query param for tenant/workspace authorization. |",
        "detail": "All seven specced endpoints exist at the flat routes (task-301): GET/POST /api/v1/meta-specs, GET/PUT/DELETE /api/v1/meta-specs/:id, GET /api/v1/meta-specs/:id/versions and /versions/:version, with ABAC RouteResourceMapping entries for the meta_spec resource. The list endpoint supports the specced ?scope=, ?scope_id=, ?kind=, ?required= filters (case-insensitive scope/kind parsing at the API edge); POST creates from body scope/scope_id with the row-15 scope-level write gate; PUT bumps the version with prior versions retained and content edits resetting approval; DELETE returns 409 while spec bindings reference the meta-spec; the version endpoints serve version content. One divergence, stricter than spec rather than weaker: the flat GET /api/v1/meta-specs list is Admin-only rather than query-param-scope ABAC, with the member-facing listing (including the bindings-panel discovery need) served by the membership-gated GET /api/v1/workspaces/:id/meta-specs — a functional superset for the specced use case. The old /meta-specs-registry prefix was removed with no silent alias.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "\"/api/v1/meta-specs\",",
            "detail": "The flat registry routes are registered at the specced path (GET list + POST create), with :id CRUD, versions and versions/:version routes alongside — all seven endpoints of the spec's table."
          },
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "\"/api/v1/meta-specs/:id/versions/:version\",",
            "detail": "The specific-version content endpoint exists, completing the version-history surface."
          },
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "pub struct ListMetaSpecsQuery {",
            "detail": "The list endpoint carries the specced query filters (scope, scope_id, kind, required)."
          },
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "\"cannot delete meta-spec '{id}': active bindings reference it\"",
            "detail": "DELETE fails (409 Conflict) while any spec bindings reference the meta-spec, as the spec's table requires."
          },
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "async fn authorize_registry_write(",
            "detail": "POST/PUT are gated by the scope-level admin authorization (the section's ABAC note), with create resolving scope/scope_id from the request body."
          }
        ]
      },
      {
        "id": "82e215b9d8c3847f600ff3ab",
        "decision": "satisfied",
        "requirement_quote": "async fn spawn_process(&self, config: &SpawnConfig) -> Result<ProcessHandle>;",
        "detail": "The ComputeTarget port trait exists with the specced shape: name(), target_type(), async spawn_process(&SpawnConfig) -> ProcessHandle, kill_process(&ProcessHandle), is_alive(&ProcessHandle). target_type() returns a &'static str ('local'/'container'/'ssh'/'kubernetes') rather than an enum — shape-equivalent discrimination. Production backends implement the trait in gyre-adapters/src/compute/: LocalTarget (tokio::process), ContainerTarget (docker/podman CLI), SshTarget/SshDockerTarget (ssh CLI), KubernetesTarget (kubectl CLI). The spawn flow records both the handle and its owning SpawnBackend so admin kills route to the owning backend's kill_process (docker rm / kubectl delete / remote docker rm) instead of a pid-based local kill.",
        "evidence": [
          {
            "file": "crates/gyre-ports/src/compute.rs",
            "quote": "pub trait ComputeTarget: Send + Sync {",
            "detail": "The pluggable ComputeTarget port trait — the single abstraction behind all backends."
          },
          {
            "file": "crates/gyre-ports/src/compute.rs",
            "quote": "async fn spawn_process(&self, config: &SpawnConfig) -> Result<ProcessHandle>;",
            "detail": "The specced spawn signature, with SpawnConfig/ProcessHandle shared by all backends."
          },
          {
            "file": "crates/gyre-ports/src/compute.rs",
            "quote": "async fn kill_process(&self, handle: &ProcessHandle) -> Result<()>;",
            "detail": "The specced kill and is_alive signatures — the full trait surface the spec's code block defines."
          }
        ]
      },
      {
        "id": "ebd16b0ebf8506b9c84eef25",
        "decision": "satisfied",
        "requirement_quote": "Network isolation | `--network=none` | platform-created default-deny NetworkPolicy applied alongside the Pod (same `kubectl apply` stream): both policy types, empty rule lists, `podSelector` on the spawned Pod's `gyre-agent` label; deleted with the Pod",
        "detail": "Every security default is enforced by every specced backend. Container: docker/podman run with --network=none (bridge opt-in via target config), --memory=2g, --pids-limit=512, --user=65534:65534, and the worktree mounted as a volume; kill via rm --force, is_alive via inspect. SSH: SshDockerTarget runs docker on the remote host with the same defaults (network/memory/pids/user), single-quote shell-escaped remote args, bounded ConnectTimeout, remote docker rm/inspect for kill/liveness. Kubernetes: the Pod manifest carries memory 2Gi and pids 512 resource limits plus runAsUser/runAsGroup 65534, runAsNonRoot and allowPrivilegeEscalation: false; a platform-created default-deny NetworkPolicy (both policy types, empty rule lists, podSelector on the spawned Pod's gyre-agent label) is applied in the SAME kubectl apply stream as the Pod and deleted with it; the 'network': 'bridge' opt-in skips the NetworkPolicy. Per the task-304 amendment recorded in this spec, the remote backends (SSH remote-docker, Kubernetes) provision the working copy via the entrypoint clone instead of mounting the server-local worktree (a git worktree's .git linkage cannot be mounted cross-host) — the server-local Container backend keeps the volume mount. Note: a legacy DockerTarget in compute/docker.rs enforces none of the defaults, but it is dead code — exported, yet referenced by no production spawn path (only its own tests), so no code path spawns an agent through it.",
        "evidence": [
          {
            "file": "crates/gyre-adapters/src/compute/container.rs",
            "quote": "let network = self.network.as_deref().unwrap_or(\"none\");",
            "detail": "Container backend: network isolation defaults to --network=none (bridge only via explicit target config), with --memory=2g, --pids-limit=512 and --user=65534:65534 defaults alongside, and the worktree volume mount."
          },
          {
            "file": "crates/gyre-adapters/src/compute/ssh.rs",
            "quote": "format!(\"--pids-limit={}\", self.pids_limit.unwrap_or(512)),",
            "detail": "SSH backend: docker run on the remote host with the same security defaults (network none, memory 2g, pids 512, user 65534:65534), shell-escaped remote args and bounded connect timeout."
          },
          {
            "file": "crates/gyre-adapters/src/compute/kubernetes.rs",
            "quote": "\"runAsUser\": 65534,",
            "detail": "Kubernetes backend: the Pod's container security context enforces runAsUser/runAsGroup 65534, runAsNonRoot and allowPrivilegeEscalation: false, with memory 2Gi and pids 512 resource limits."
          },
          {
            "file": "crates/gyre-adapters/src/compute/kubernetes.rs",
            "quote": "\"policyTypes\": [\"Ingress\", \"Egress\"],",
            "detail": "The default-deny NetworkPolicy carries both policy types with empty rule lists, applied in the same kubectl apply stream as the Pod, podSelector-scoped to the spawned Pod's gyre-agent label, and deleted with the Pod; the bridge opt-in skips it."
          },
          {
            "file": "crates/gyre-adapters/src/compute/kubernetes.rs",
            "quote": "\"matchLabels\": {\"gyre-agent\": pod_name},",
            "detail": "The NetworkPolicy's podSelector targets exactly the spawned Pod's gyre-agent label — the per-Pod scoping the spec's amendment table requires."
          }
        ]
      },
      {
        "id": "3ee23b90e9236c59b010822f",
        "decision": "satisfied",
        "requirement_quote": "One definition, all targets. The `Dockerfile` is replaced by the Nix flake — no `Dockerfile` ships in the tree (the legacy pre-flake Dockerfile was removed by task-304), and no code path, script, or doc instructs building the agent image any other way.",
        "detail": "docker/gyre-agent/ contains all five specced files — flake.nix (single definition), flake.lock, agent-runner.mjs, entrypoint.sh, cred-proxy.mjs (plus test/support files: package.json, package-lock.json, README.md, setup-wg.sh, *.test.mjs — none of which the spec forbids) — and NO Dockerfile (verified by directory listing; the repo-root Dockerfile is the server image and docker/dev-worker/ is the CI worker, both unrelated). The flake builds all three targets: agent-image (buildLayeredImage, OCI), agent-image-docker (Docker-loadable), agent-image-tar (air-gapped tarball), from pinned nixpkgs with the committed package lock. No code path or doc instructs building the agent image any other way — the spawn-failure guidance in api/spawn.rs points only at the Nix targets. Image-hash attestation: the spawn flow resolves the image digest BEFORE minting the agent JWT (docker image inspect needs no running container) for the container/SSH backends and mints the wl_image_hash claim via the workload mint; Kubernetes records the cluster-pulled imageID (pod imageID) in the post-spawn workload attestation record served by GET /api/v1/agents/:id/workload — the backend split the spec's amendment describes.",
        "evidence": [
          {
            "file": "docker/gyre-agent/flake.nix",
            "quote": "agent-image = pkgs.dockerTools.buildLayeredImage ({",
            "detail": "The OCI image target builds from the single Nix definition with pinned inputs — one of the three specced build targets."
          },
          {
            "file": "docker/gyre-agent/flake.nix",
            "quote": "agent-image-tar = pkgs.stdenv.mkDerivation {",
            "detail": "The air-gapped tarball target exists alongside agent-image-docker — 'one definition, all targets' with no Dockerfile in docker/gyre-agent/."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "docker/gyre-agent/: `nix build .#agent-image-docker`",
            "detail": "Deployment guidance points only at the Nix targets (docker load from agent-image-docker, or agent-image-tar) — no code path instructs building the agent image any other way."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "let wl_image_hash: Option<String> = match resolved_ct_entity.as_ref() {",
            "detail": "The spawn flow resolves the image digest before minting the agent JWT so the wl_image_hash claim carries the exact local image for container/SSH backends; Kubernetes records the cluster-pulled imageID post-spawn."
          },
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": ".route(\"/api/v1/agents/:id/workload\", get(workload::get_workload))",
            "detail": "The post-spawn workload attestation record (including the Kubernetes imageID) is served at GET /api/v1/agents/:id/workload, as the spec's K8s amendment requires."
          }
        ]
      },
      {
        "id": "7e80932897fe7fd051bf3188",
        "decision": "satisfied",
        "requirement_quote": "The workspace's `compute_target_id` field determines where all agents in that workspace run. If unset, falls back to the tenant's default target. If no tenant default, falls back to local container auto-detection (Docker or Podman).",
        "detail": "ComputeTargetEntity is persisted behind ComputeTargetRepository (SQLite + PostgreSQL + mem: tenant-scoped list, default flag, duplicate-name guard) with the specced fields (id, tenant_id, name, target_type, config, is_default, created_at). Full CRUD exists at /api/v1/compute-targets and /api/v1/compute-targets/:id with tenant context taken from auth (tenant_id_from_auth), ABAC resolver mappings for the compute_target resource, and a delete guard that fails while workspaces reference the target. The Workspace struct carries compute_target_id: Option<Id> with PUT /api/v1/workspaces/:id validation (rejects targets outside the workspace's tenant). The spawn fallback chain is exactly the specced one: request-supplied id → workspace assignment → tenant default (get_default_for_tenant) → local container auto-detection (Runtime::detect: Docker then Podman); cross-tenant targets are rejected at both assignment (4xx) and spawn (404 without existence confirmation). No per-agent configuration — the workspace decides and agents inherit.",
        "evidence": [
          {
            "file": "crates/gyre-domain/src/compute_target.rs",
            "quote": "pub struct ComputeTargetEntity {",
            "detail": "The ComputeTargetConfig entity (id, tenant_id, name, target_type, config, is_default, created_at) is persisted behind a port repository with SQLite, PostgreSQL and mem adapters."
          },
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "\"/api/v1/compute-targets\",",
            "detail": "The specced CRUD routes exist (list/create plus :id get/update/delete), tenant-scoped."
          },
          {
            "file": "crates/gyre-server/src/api/compute_targets.rs",
            "quote": "fn tenant_id_from_auth(auth: &crate::auth::AuthenticatedAgent) -> Id {",
            "detail": "Tenant context is extracted from auth (not a path param), matching the spec's API note."
          },
          {
            "file": "crates/gyre-server/src/api/compute_targets.rs",
            "quote": "\"compute target {id} is referenced by one or more workspaces\"",
            "detail": "DELETE fails while workspaces reference the target — the spec's remove-guard."
          },
          {
            "file": "crates/gyre-domain/src/workspace.rs",
            "quote": "pub compute_target_id: Option<Id>,",
            "detail": "The Workspace struct carries compute_target_id (the platform-model §1 amendment), validated on workspace update."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": ".get_default_for_tenant(&ws.tenant_id)",
            "detail": "The spawn fallback chain resolves the workspace's assignment, then the tenant default, then local container auto-detection — exactly the specced order, with cross-tenant ids rejected."
          }
        ]
      },
      {
        "id": "68331317c747aa4a1798c8f0",
        "decision": "satisfied",
        "requirement_quote": "A repo's `max_agents` cannot exceed the workspace's `max_agents_per_repo` — the server validates this on `PUT /api/v1/repos/:id` and rejects values that exceed the workspace limit (400 error).",
        "detail": "The full cascade is real. Workspace ≤ tenant: PUT /api/v1/workspaces/:id/budget validates every BudgetConfig field against the tenant ceiling (400 InvalidInput per field). Repo ≤ workspace: Repository.budget override (migration 000061, both SQLite and PG adapters) validated field-by-field on PUT /api/v1/repos/:id against the EFFECTIVE workspace limit (min of workspace config and tenant ceiling — a dimension the workspace leaves unset inherits the tenant). max_agents ≤ workspace max_agents_per_repo is validated on PUT /api/v1/repos/:id with a 400 error naming the violation. Per-agent usage is charged to the repo (repo:{id} usage entries, resolved from agent.repo_id or the current task) and rolled up to workspace and tenant; spawn-time enforcement uses the effective config (effective_workspace_config takes the tighter of workspace/tenant per field) via check_spawn_budget plus check_repo_spawn_budget on both spawn surfaces; the repo override enforces all its fields including max_concurrent_agents (live Active-agent count) and max_agent_lifetime_secs (enforced continuously by enforce_agent_lifetimes with the effective limit = min(repo override, effective workspace limit) — the override can tighten but never loosen the cascade). The max_agents fallback chain (repo → workspace → unlimited) with queuing-not-rejection is enforced at spawn (audited under Phase 4).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/budget.rs",
            "quote": "// Cascade: workspace limits cannot exceed tenant limits.",
            "detail": "Workspace ≤ tenant: the workspace budget PUT validates every field against the tenant ceiling and rejects violations (400)."
          },
          {
            "file": "crates/gyre-server/src/api/repos.rs",
            "quote": "\"max_agents {max_agents} exceeds the workspace limit of {ws_max}\"",
            "detail": "Repo max_agents ≤ workspace max_agents_per_repo is validated on PUT /api/v1/repos/:id and rejected with a 400 error naming the violation — the exact validation sentence of this section."
          },
          {
            "file": "crates/gyre-server/src/api/repos.rs",
            "quote": "\"repo max_tokens_per_day ({repo_max}) exceeds the workspace limit ({ws_max})\"",
            "detail": "The repo budget override is validated field-by-field against the effective workspace limit (the other cascade dimensions: cost, concurrent agents, agent lifetime)."
          },
          {
            "file": "crates/gyre-server/src/api/budget.rs",
            "quote": "pub(crate) async fn effective_workspace_config(state: &AppState, ws_key: &str) -> BudgetConfig {",
            "detail": "Spawn-time enforcement uses the effective config — the tighter of workspace and tenant per field — so a dimension either leaves unset still binds via the tenant ceiling."
          }
        ]
      },
      {
        "id": "c8aab7322dffd3789790d78c",
        "decision": "satisfied",
        "requirement_quote": "**100%** | Graceful stop | All active agents in the workspace receive a `BudgetExhausted` message in their inbox. Agents have 60 seconds to commit current work and call `agent.complete`. After 60 seconds, agents are killed via `ComputeTarget::kill_process()`. Agents that complete within the grace period are marked `Idle`. Agents killed after the grace period are marked `Stopped`.",
        "detail": "All three thresholds are real production behavior in enforce_workspace_budget, which runs after every usage report and after every workspace budget PUT. 80%: AgentBudgetWarning notification (rank 4, High band — the unified taxonomy the spec's amendment references) plus a BudgetWarning bus event on the crossing, agents continue. 100%: the default action is GracefulStop; every Active agent in the workspace receives a BudgetExhausted message delivered to its agent inbox (Destination::Agent — the surface list_unacked and the MCP inbox tool serve); a single 60-second grace timer per workspace (budget_grace_in_progress guard) re-evaluates before killing, then calls ComputeTarget::kill_process() on the owning backend for agents still Active — agents that completed within grace go Idle via the normal path, killed agents are marked Stopped. Beyond limit (usage > limit, e.g. the budget was reduced below current usage): immediate synchronous kill (kill_workspace_agents_for_budget with 'budget beyond limit', grace 0). Budget kills preserve the worktree, leave the task InProgress for human review, revoke the agent token, and emit the AgentKilled audit event. Note: the ≥100% action is configurable per platform-model.md §5 (Warn/HardKill/QueuePause overrides exist) with GracefulStop as the specced default — a documented superset, not a violation.",
        "evidence": [
          {
            "file": "crates/gyre-common/src/notification.rs",
            "quote": "Self::AgentBudgetWarning => 4,",
            "detail": "80% warn: the AgentBudgetWarning notification maps to rank 4 (High) in the unified notification taxonomy — the amended ranking the spec cites; agents continue running."
          },
          {
            "file": "crates/gyre-server/src/api/budget.rs",
            "quote": "async fn start_budget_grace_timer(state: Arc<AppState>, workspace_id: Id, grace_secs: u64) {",
            "detail": "100% graceful stop: a real 60-second grace timer (one per workspace, re-evaluating before the kill) — agents have the window to commit and complete."
          },
          {
            "file": "crates/gyre-server/src/api/budget.rs",
            "quote": "Some(b) => gyre_ports::ComputeTarget::kill_process(b, &handle).await,",
            "detail": "After the grace period, agents still Active are killed via the owning backend's ComputeTarget::kill_process (docker rm / kubectl delete / remote docker rm), and marked Stopped; agents completing in grace go Idle."
          },
          {
            "file": "crates/gyre-server/src/api/budget.rs",
            "quote": "kill_workspace_agents_for_budget(state, &workspace, \"budget beyond limit\").await;",
            "detail": "Beyond limit (budget reduced below current usage): immediate hard kill with grace 0 — worktree preserved, task left InProgress for human review, no MR."
          }
        ]
      },
      {
        "id": "d6a13e4b2effb55dd1283f96",
        "decision": "satisfied",
        "requirement_quote": "The credential proxy (`cred-proxy.mjs`) reports token usage back to the server after each LLM call via `POST /api/v1/agents/:id/usage`. The server aggregates usage per workspace and checks against workspace limits on every report.",
        "detail": "All four cost types are tracked. llm_tokens/llm_cost_usd: cred-proxy.mjs reports {tokens_input, tokens_output, cost_usd} to POST /api/v1/agents/:id/usage after every proxied LLM call (JSON and buffered-SSE usage parsing, a model-specific pricing table overridable via GYRE_PRICING_JSON, Bearer auth, best-effort and latency-bounded at 10s so a failed report never fails the LLM call); the server endpoint enforces self-report identity and calls enforce_workspace_budget on every report — the aggregation and per-report limit check the spec requires. compute_minutes: BudgetUsage.compute_minutes charged on terminal transitions (complete REST+MCP, fail, stop, admin kill, stale-detector death, budget kill) from spawned_at wall clock via record_compute_minutes_for_agent, backed by the add_compute_minutes port method with SQLite/PG/mem adapters. llm_query: user-initiated queries (specs/assist, explorer/generate, graph, MCP spec_assist) charged to the workspace directly with no repo entry (usage_type llm_query is explicitly filtered out of the repo charge). Minor cosmetic note: the usage endpoint's doc comment still mentions a 429 response it no longer returns — enforcement is via the kill/notify path, which does satisfy the spec.",
        "evidence": [
          {
            "file": "docker/gyre-agent/cred-proxy.mjs",
            "quote": "tokens_input: usage.tokens_input,",
            "detail": "Per-call usage reporting: the credential proxy reports tokens_input/tokens_output/cost_usd to POST /api/v1/agents/:id/usage after every proxied LLM call."
          },
          {
            "file": "docker/gyre-agent/cred-proxy.mjs",
            "quote": "const DEFAULT_PRICING = [",
            "detail": "Model-specific pricing computes llm_cost_usd at the proxy (overridable via GYRE_PRICING_JSON), the dollar-amount cost type the spec tracks."
          },
          {
            "file": "docker/gyre-agent/cred-proxy.mjs",
            "quote": "const USAGE_REPORT_TIMEOUT_MS = 10_000;",
            "detail": "The report is latency-bounded (10s) so a failed report never fails the LLM call — bounded external HTTP on a path with a fallback guarantee."
          },
          {
            "file": "crates/gyre-server/src/api/budget.rs",
            "quote": "enforce_workspace_budget(state, &ws, agent, repo, BUDGET_GRACE_PERIOD_SECS, cause).await;",
            "detail": "The server checks workspace limits on every usage report (the enforcement entry point runs at the end of record_llm_call_usage), aggregating per workspace/repo/tenant."
          },
          {
            "file": "crates/gyre-server/src/api/budget.rs",
            "quote": "pub async fn record_compute_minutes_for_agent(state: &AppState, agent: &gyre_domain::Agent) {",
            "detail": "compute_minutes: wall-clock spawn-to-terminal time charged on every terminal transition (complete, fail, stop, kill, stale death), the third tracked cost type."
          }
        ]
      },
      {
        "id": "5d31ae0e730c80ccd5596348",
        "decision": "satisfied",
        "requirement_quote": "Budgets with time-based limits (e.g., `max_tokens_per_day`, `max_cost_per_day`) reset on a rolling 24-hour window. The reset is handled by a background job that runs every hour and clears expired usage entries.",
        "detail": "The reset is a rolling 24-hour window, not a calendar day: apply_rolling_24h_window recomputes every entity's token/cost counters from the per-call BudgetCallRecords with timestamp ≥ now − 24h (the new BudgetCallRepository::list_since port method, implemented by the SQLite, PostgreSQL and mem adapters), zeroes entities with no in-window records, and sets period_start to the cutoff. It runs hourly via the spawn_budget_reset job (interval_secs: 3600), replacing the old midnight-UTC full reset in both the lib.rs background task and the JobRegistry registration. compute_minutes stays cumulative (no time-based limit consumes it) and active_agents remains a live gauge.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/budget.rs",
            "quote": "let cutoff = now.saturating_sub(24 * 60 * 60);",
            "detail": "The rolling 24-hour window: counters are recomputed from per-call records with timestamp ≥ now−24h, so time-based limits reset on a rolling basis."
          },
          {
            "file": "crates/gyre-server/src/jobs.rs",
            "quote": "name: \"spawn_budget_reset\".to_string(),",
            "detail": "The reset is a registered background job (the hourly budget job), replacing the old midnight-UTC reset."
          },
          {
            "file": "crates/gyre-server/src/jobs.rs",
            "quote": "interval_secs: 3600,",
            "detail": "The job runs every hour, clearing expired usage entries as the spec requires."
          }
        ]
      },
      {
        "id": "47d309e09aac1e2e04d334d0",
        "decision": "gap",
        "requirement_quote": "`system://constraints` delivers budget limits and operational parameters from the persona's operational fields. The four MCP prompts remain the delivery mechanism — the three-section structure above describes what goes INTO them.",
        "detail": "The three-section structure is largely real: the system prompt is the assembled meta-spec set (GYRE_META_SPEC_PROMPT plus GYRE_META_SPEC_SET_SHA at spawn; system://persona serves the pinned spawn-time record verbatim); the protocol section is system-managed and injected as GYRE_PROTOCOL_PROMPT and served by system://protocol — it carries the Ralph loop rules, MCP tool usage, the 60-second heartbeat requirement, escalation rules and the gyre_agent_complete completion signaling; task context (task/spec ref/branch/gate failures/inbox) is delivered via system://context; all four MCP prompts are implemented and dispatched in prompts/list + prompts/get. GAP: system://constraints does NOT deliver operational parameters from the persona's operational fields — prompt_constraints assembles only scope boundaries, trust level, workspace budget, lifetime budget and the iteration ceiling; the Persona operational fields (model, temperature, max_tokens, budget) are consumed by no execution path anywhere in the server (only CRUD DTOs reference them), so an agent spawned with a persona carrying distinctive operational values never sees them in its constraints. This is the open task-303 gap ('Deliver persona operational fields in constraints prompt', progress not-started, blocked on the task-298 registry replacement). Minor naming deviation: the spec's `agent.escalate` tool does not exist — the protocol's escalation rules direct agents to escalate via gyre_message_send to their orchestrator (a functional equivalent, documented in the protocol text).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"system://constraints\" => prompt_constraints(state, auth).await,",
            "detail": "The system://constraints MCP prompt is implemented and dispatched (all four specced prompts exist) — but its content does not include the persona's operational fields."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "text.push_str(\"\\n## Operational parameters\\n\");",
            "detail": "The constraints prompt's 'Operational parameters' section begins after the workspace-budget section — its contents are only the lifetime budget and the iteration ceiling, not the persona's model/temperature/max_tokens/budget."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "Some(secs) => text.push_str(&format!(\"- Lifetime budget: {secs}s\\n\")),",
            "detail": "What the operational-parameters section actually contains: agent lifetime budget and (below it) the Ralph-loop iteration ceiling — sourced from the agent and repo, never from the bound persona's operational fields, which no execution path reads."
          },
          {
            "file": "crates/gyre-server/src/api/prompts.rs",
            "quote": "- Send a heartbeat every 60 seconds; missing heartbeats mark you Dead and trigger re-spawn or cleanup.",
            "detail": "Positive wiring for the protocol section: the system-managed protocol (served by GYRE_PROTOCOL_PROMPT and system://protocol) carries the heartbeat requirement, Ralph rules, tool usage and completion signaling."
          },
          {
            "file": "crates/gyre-server/src/api/prompts.rs",
            "quote": "- `gyre_agent_complete` is the only completion signal: it creates the MR, revokes your token, and hands off to the gates.",
            "detail": "Completion signaling rules are delivered in the protocol section — the specced protocol content is real (escalation rules direct agents to gyre_message_send rather than the spec's agent.escalate name)."
          }
        ]
      },
      {
        "id": "a40446f46dbb0536f3c30ed0",
        "decision": "gap",
        "requirement_quote": "All cross-references to `ralph-loop.md` in other specs (e.g., `agent-gates.md`) should be updated to reference `agent-runtime.md` §1.",
        "detail": "The amendment table is partially carried out. SATISFIED rows: Task gains task_type (Implementation/Delegation/Coordination), order: Option<u32>, depends_on: Vec<Id>, parent_task_id: Option<Id>; MessageKind::SpecApproved exists in code (Event tier, server-only, specced payload) though message-bus.md was never amended; spec-lifecycle.md terminology cleaned (no 'CEO agent' remains; push-hook tasks informational); Workspace.compute_target_id added; the meta-spec-set endpoints retired in code (404, KV tables dropped by migration 000071) with MetaSpecSnapshot using content_hash tuples and drift detection comparing merge-attestation meta_specs_used against current required versions; MergeAttestation.meta_specs_used populated in production; the abac resolver carries meta_spec and compute_target resource mappings. UNMET rows (the open task-298 amendment-table gap, progress not-started): (1) the 'Remove agent.spawn MCP tool — orchestrators do not spawn agents' row (platform-model.md §4) is NOT carried out: two orchestrator-tier spawn surfaces ship as live production code — gyre_spawn_worker (a repo-orchestrator JWT spawns a worker agent through the full spawn core; catalog, dispatch, scope validation and handler all present) and gyre_spawn_repo_orchestrator (a workspace-orchestrator JWT spawns the repo orchestrator agent directly via spawn_repo_orchestrator_core). task-298's current acceptance criterion names only the repo-orchestrator/worker surface ('a repo-orchestrator JWT has no MCP tool whose handler creates agent records'), so the workspace-orchestrator surface needs explicit ownership — an extension of task-298's contract or a separate assignment — recorded for the controller. (2) ralph-loop.md cross-references remain in canonical specs — authorization-provenance.md still cites ralph-loop.md for the Ralph Loop re-spawn semantics and meta-spec-reconciliation.md still carries a ralph-loop.md@sha example path (agent-gates.md itself was updated). (3) docs/agent-protocol.md still exists and is actively maintained ('consolidated into this spec' not carried out) — and the retirement is uncoordinated with active plans: the assigned task-311 (websocket-agent-transport) has a deliverable to update docs/agent-protocol.md, the assigned task-272 (terminal-worktree-cleanup) must update its worktree-cleanup claims, and task-321's docs site builds one page per docs/ file including agent-protocol, so deleting the file requires sequencing after those deliverables or explicitly retargeting them; recorded for the controller. (4) the /api/v1/personas endpoints were NOT removed — four routes remain live in api/mod.rs alongside the replacement registry. (5) the Persona struct gains no required: bool field and its prompt field is named system_prompt, not prompt (the required flag lives on the unified MetaSpec struct instead — see a00a6b5082295555260842b2). (6) message-bus.md contains no SpecApproved kind (the doc amendment was never applied). (7) the abac-policy-engine.md resource-attribute amendment is half-done in code — meta_spec and compute_target resources exist in the resolver, but the 'archive' action was never added (the archive/unarchive routes sit in the frozen ABAC exemption file with per-handler enforcement instead) and the abac-policy-engine.md doc has none of the three additions. (8) hierarchy-enforcement.md was never amended — no compute-target or meta-spec CRUD routes in its route table and meta_spec_versions/meta_spec_bindings/compute_targets are not in its tenant-filter configuration. (9) human-system-interface.md §5 was never amended to include meta_specs_used in the attestation bundle schema. (10) the Agent entity's task binding is current_task_id: Option<Id> rather than the specced non-optional task_id: Id. (11) meta-spec-reconciliation.md still specces the retired PUT /api/v1/workspaces/{id}/meta-spec-set endpoint, contradicting this spec's replacement of it.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"name\": \"gyre_spawn_worker\",",
            "detail": "Unmet amendment row (platform-model.md §4, 'Remove agent.spawn MCP tool — orchestrators do not spawn agents'): the gyre_spawn_worker tool is live production code — a repo-orchestrator JWT can spawn a worker agent through the full spawn core (see 15aec83b58b3494602d31ce8)."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"name\": \"gyre_spawn_repo_orchestrator\",",
            "detail": "Unmet amendment row (same 'orchestrators do not spawn agents' rationale): a second live spawn surface lets a workspace-orchestrator JWT spawn the repo orchestrator agent directly (see 6d7302e3c6a23af8c4020c20) — not covered by task-298's current acceptance criterion, which scopes only the repo-orchestrator tier."
          },
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "\"/api/v1/personas\",",
            "detail": "Unmet amendment row (platform-model.md §2): the /api/v1/personas endpoints were NOT removed — the route block (list/create, resolve, :id CRUD, :id/approve) remains live alongside the replacement /api/v1/meta-specs?kind=meta:persona registry."
          },
          {
            "file": "specs/system/message-bus.md",
            "quote": "SpecChanged,",
            "detail": "Unmet amendment row (message-bus.md MessageKind): the canonical message-bus.md enumeration still lists SpecChanged but contains no SpecApproved anywhere (grep for SpecApproved/spec_approved returns nothing) — the kind exists only in code; the doc amendment was never applied."
          },
          {
            "file": "specs/system/authorization-provenance.md",
            "quote": "- **Ralph Loop** (`ralph-loop.md`): Re-spawn produces `DerivedInput` from same parent, preserving constraint chain (§4.2).",
            "detail": "Unmet supersede row: a canonical spec still cross-references ralph-loop.md (meta-spec-reconciliation.md likewise carries a meta/process/ralph-loop.md@sha example) — 'all cross-references should be updated' was not carried out (agent-gates.md itself was updated)."
          },
          {
            "file": "docs/agent-protocol.md",
            "quote": "This document covers the full agent lifecycle: spawn, operation, and completion.",
            "detail": "Unmet supersede row: docs/agent-protocol.md still exists and documents the full agent lifecycle this spec claims to consolidate — never removed or folded in; the assigned task-311 and task-272 deliverables edit this exact file, so retirement needs sequencing or retargeting."
          },
          {
            "file": "scripts/abac-route-registry-exemptions.txt",
            "quote": "/api/v1/repos/:id/archive",
            "detail": "Unmet amendment row (abac-policy-engine.md): the 'archive' action was never added to the ABAC action set — the archive/unarchive routes sit in the frozen exemption file (per-handler enforcement) and 'archive' appears nowhere in abac_middleware.rs."
          },
          {
            "file": "crates/gyre-domain/src/agent.rs",
            "quote": "pub current_task_id: Option<Id>,",
            "detail": "Deviation from the amendment row (Agent entity): the spec says 'Add task_id: Id field — every agent is bound to exactly one task'; the code uses a nullable current_task_id: Option<Id> (assigned at spawn, but not the non-optional binding the row defines)."
          },
          {
            "file": "specs/system/hierarchy-enforcement.md",
            "quote": "### In-Memory meta_spec_sets",
            "detail": "Unmet amendment row (hierarchy-enforcement.md §4): the doc was never amended — it still describes the retired in-memory meta_spec_sets model and its route table / tenant-filter configuration contain no compute-target or meta-spec CRUD routes or the meta_spec_versions/meta_spec_bindings/compute_targets tables (the tables exist in the adapters)."
          },
          {
            "file": "specs/system/meta-spec-reconciliation.md",
            "quote": "PUT /api/v1/workspaces/{id}/meta-spec-set",
            "detail": "Contradiction left in place: meta-spec-reconciliation.md still specces the meta-spec-set endpoint that this spec's amendment table replaces (the code retired it — the endpoints 404 — but the canonical doc was never amended), and human-system-interface.md §5 likewise lacks the meta_specs_used array this spec says to add."
          },
          {
            "file": "crates/gyre-domain/src/task.rs",
            "quote": "pub enum TaskType {",
            "detail": "Satisfied amendment row (platform-model.md §3 Task): TaskType (Implementation/Delegation/Coordination) plus order, depends_on and parent_task_id all exist on the Task entity and drive the scheduler discriminators."
          },
          {
            "file": "crates/gyre-common/src/message.rs",
            "quote": "MessageKind::SpecApproved,",
            "detail": "Satisfied amendment row (code side): the SpecApproved kind exists with Event tier, server-only flag and the specced payload — only the message-bus.md doc amendment is missing."
          },
          {
            "file": "crates/gyre-domain/src/workspace.rs",
            "quote": "pub system_prompt: String,",
            "detail": "The Persona struct's prompt field is named system_prompt (not prompt) and the struct carries no required flag — the row's 'Persona struct gains required: bool' was implemented instead on the unified MetaSpec registry (see a00a6b5082295555260842b2)."
          }
        ]
      }
    ],
    "gaps": [
      {
        "id": "1bd201625dc5fbe0164ced05",
        "decision": "gap",
        "requirement_quote": "There is no other trigger. The `SpecApproved` event is the universal starting signal.",
        "detail": "Requires that spec approval is the ONLY trigger for agent implementation work. The approval side is real: the approve handler records the ledger row and emits SpecApproved only on the transition into Approved (specs.rs), modified-spec pushes reset approval_status to Pending (auto_invalidate_approvals defaults true) and revoke approvals via the post-receive hook, and untyped push-hook tasks never spawn agents. However the single-trigger property is still mechanically violated at the task layer: (1) POST /api/v1/tasks and the MCP gyre_create_task tool both accept task_type 'implementation' with no precondition that an approved spec exists — the only pre-checks are scope/preview-agent gates; (2) the Phase-4 scheduler spawns a worker for ANY Backlog Implementation task regardless of how the task was created (it checks repo status, ordering, max_agents and budget, never task origin); (3) the spawn-core approval gate added by task-302 only applies when task.spec_path is set AND the spec is in the ledger — a task with no spec_path, or one referencing a spec not in the registry, bypasses the gate entirely (the code comment states this explicitly); (4) POST /api/v1/agents/spawn spawns task-bound implementation workers for any authenticated caller with no scheduler-principal restriction. Two further live production implementation-work paths that never see SpecApproved are NOT covered by any of the four paths above: (5) Thorough preview — POST /api/v1/repos/:id/graph/thorough-preview (route in api/mod.rs, handler graph.rs::thorough_preview) is a human-triggered endpoint that creates a task with task_type Implementation whose description embeds the caller's unapproved spec DRAFT content ('implement the following spec draft on the throwaway branch' — draft_content arrives in the request body) and spawns a worker agent directly in-process via spawn_agent_core with the human as caller: the task is created through the task store (not POST /api/v1/tasks or gyre_create_task), the spawn is issued from its own route (not the /agents/spawn handler), and the scheduler never sees it; the only approval check on this path is the spawn-core ledger gate, which is skipped exactly when the draft spec is not in the registry — the normal case for an un-pushed UI draft. (6) Cascade tests — merge_processor::trigger_cascade_tests, invoked automatically on merge and by the human REST surface POST /api/v1/repos/:id/cascade-tests/trigger (api/mod.rs, dependencies.rs), creates tasks with task_type Implementation, priority High, labels ['cascade-test','auto-created'], NO spec_ref and NO spec_path, in dependent repos; the Phase-4 scheduler spawns workers for them (the in-code comment states task_type Implementation is chosen precisely so the scheduler spawns; untyped tasks are never spawned). CANONICAL CONFLICT: both surfaces are canonical requirements of sibling specs — system-explorer.md §3 ('the thorough preview (minutes via agent implementation on throwaway branches)') and dependency-graph.md §Cascade Testing ('the forge triggers a test run against Repo B's new version') — so this section's 'There is no other trigger. The SpecApproved event is the universal starting signal.' is in canonical conflict with them. Per the goal rules the conflict must be recorded and routed through the spec lifecycle — a proposed agent-runtime.md §1 amendment scoping the single-trigger property to spec-implementation work while explicitly acknowledging merge-triggered cascade verification and human-initiated draft previews — never silently ignored and never resolved by deleting the sibling specs' specced features. Consequence for any remediation contract: a scheduler approval gate implemented strictly ('the task stays Backlog unless its spec_ref resolves to an Approved ledger entry at the pinned SHA') strands every cascade-test task in Backlog forever (they carry no spec_ref), breaking dependency-graph.md's specced cascade testing together with its REST trigger and result-reporting surfaces; implemented as spec-less pass-through it leaves the single-trigger property violated by a specced path. A closure claim of 'close every task→agent path that bypasses the approval chain' is therefore false as written — the claim must name the specced exceptions and the conflict resolution. This is the open task-297 gap ('Gate all implementation work on approved specs', progress not-started).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/tasks.rs",
            "quote": "task.task_type = Some(parse_task_type(&tt)?);",
            "detail": "The HTTP task-create handler accepts task_type directly from the request body with no approval precondition (the surrounding code only gates preview agents and validates caller scope); a task created here with type 'implementation' is picked up by the Phase-4 scheduler and spawns a worker agent."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"enum\": [\"implementation\", \"delegation\", \"coordination\"],",
            "detail": "The gyre_create_task MCP tool schema lets any authorized caller create a task with task_type 'implementation'; the handler maps it to TaskType::Implementation with target-scope validation but no spec-approval precondition, so the universal-approval gate is bypassable at task creation."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "continue; // Delegation/Coordination → orchestrators; untyped → never spawned",
            "detail": "The Phase-4 scheduler scan (spawn_implementation_workers) spawns workers for every Backlog task with task_type Implementation regardless of how the task was created; the scan checks repo status, ordering, max_agents and budget but never checks that the task derives from an approved spec."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "// registry cannot be evaluated and are not gated here.",
            "detail": "The spawn-core approval gate (added by task-302 for bindings invalidation) only applies when task.spec_path is set and the spec is in the ledger; tasks with no spec_path or an unregistered spec are explicitly not gated, so they spawn without any approval."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "let resp = spawn_agent_core(&state, req, &auth, \"POST /api/v1/agents/spawn\").await?;",
            "detail": "A direct agent-spawn REST surface exists for any authenticated caller with no scheduler-principal restriction: it spawns task-bound implementation worker agents (only the internal task_type and registered-spec_path approval gates apply), so implementation agents can be created without any SpecApproved event."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "crate::signal_chain::on_spec_approved(&state, &payload).await;",
            "detail": "Positive wiring on the approval side: after recording the ledger row and emitting the SpecApproved message (only on transition into Approved), the handler routes the signal into the orchestrator chain — the legitimate chain itself is real."
          },
          {
            "file": "crates/gyre-server/src/api/graph.rs",
            "quote": "Thorough preview (system-explorer.md §3): implement the following ",
            "detail": "The thorough-preview handler builds the preview task's description from the caller-supplied unapproved spec draft (draft_content arrives in the request body, so the draft is normally not in the spec registry) and sets task_type = Some(TaskType::Implementation) — the task embeds unapproved spec content and is destined for a worker agent."
          },
          {
            "file": "crates/gyre-server/src/api/graph.rs",
            "quote": "\"POST /api/v1/repos/:id/graph/thorough-preview\",",
            "detail": "The same handler spawns the worker agent directly in-process via crate::api::spawn::spawn_agent_core with the human caller as principal: the task is created through the task store (not POST /api/v1/tasks or gyre_create_task), the spawn is issued from its own route (not the /agents/spawn handler), and the Phase-4 scheduler never sees the task — none of the four previously enumerated bypass-gate surfaces touch this path."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "if let Some(entry) = entry {",
            "detail": "Inside the spawn-core approval gate, enforcement fires only when the ledger returns an entry (if let Some); a spec_path not present in the registry — the thorough-preview draft case, since draft_content arrives in the request body and the spec was never pushed — falls through ungated, so the preview path spawns a worker for unapproved draft content exactly when the draft is unregistered."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "task_type = Implementation is the discriminator the worker",
            "detail": "trigger_cascade_tests (invoked automatically on merge and by the human REST surface POST /api/v1/repos/:id/cascade-tests/trigger) creates cascade-test tasks in dependent repos with task_type Implementation, priority High, labels ['cascade-test','auto-created'], and no spec_ref/spec_path precisely so spawn_implementation_workers dispatches a worker agent for them — an implementation-work path with no SpecApproved event anywhere in the chain, and a class a strict spec_ref-approval scheduler gate would strand in Backlog forever."
          },
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "\"/api/v1/repos/:id/cascade-tests/trigger\",",
            "detail": "The human-triggered REST surface for cascade tests; the handler (dependencies.rs::trigger_cascade_tests) delegates to merge_processor::trigger_cascade_tests, so cascade implementation work can also start from a direct human REST call rather than a merge."
          },
          {
            "file": "specs/system/system-explorer.md",
            "quote": "the thorough preview (minutes via agent implementation on throwaway branches)",
            "detail": "Canonical sibling requirement (system-explorer.md §3): the thorough preview is specced as agent implementation on throwaway branches — the endpoint's specced purpose is exactly the unapproved-draft implementation work that agent-runtime.md §1's 'There is no other trigger' wording forbids. Incompatible canonical requirements: the conflict must be routed through the spec lifecycle, not resolved by deleting either feature."
          },
          {
            "file": "specs/system/dependency-graph.md",
            "quote": "For each dependent repo, the forge triggers a test run against Repo B's new version",
            "detail": "Canonical sibling requirement (dependency-graph.md §Cascade Testing): merge-triggered test runs in dependent repos, executed in code by Implementation-type tasks the scheduler spawns; these tasks carry no spec_ref, so they cannot pass an approval-ledger gate on spec_ref — canonical conflict with agent-runtime.md §1's universal single-trigger wording."
          }
        ]
      },
      {
        "id": "6d7302e3c6a23af8c4020c20",
        "decision": "gap",
        "requirement_quote": "The workspace orchestrator **only creates tasks** (via `task.create` MCP tool). It never spawns agents, never touches code, never interacts with compute targets. It thinks in tasks.",
        "detail": "The server-driven chain itself is real: on_spec_approved serializes concurrent signals on a per-workspace mutex (exactly-one-active semantics) and calls ensure_workspace_orchestrator, which reuses a live orchestrator or spawns one on demand through the real spawn core (api/orchestrator.rs) with fail-closed workspace-orchestrator persona validation, a workspace-scoped JWT, and the workspace's configured compute target (workspace assignment → tenant default → local). run_workspace_orchestrator reads the approved spec content at the pinned SHA, queries spec_links for dependents, creates the Delegation task with task_type Delegation and spec_ref 'path@sha' referencing the approved SHA, creates Coordination tasks in dependent repos, and creates priority-4 CrossWorkspaceSpecChange notifications for cross-workspace dependent workspace admins. The server-driven run only creates tasks (no agent spawns, no code writes), acks only the consumed SpecApproved message, and transitions Idle after processing (on demand, not long-lived). Documented deviations: tasks are created through the task store rather than by calling the task.create MCP tool (documented in-code as equivalent effect for a server-driven run), and the cross-repo impact analysis is deterministic spec_links querying. GAP (principal-level boundary): 'It never spawns agents' is violated at this base by the live gyre_spawn_repo_orchestrator MCP surface — a workspace-orchestrator-tier JWT (holding workspace:{ws}:spawn) can spawn the repo orchestrator agent directly through tools/call, bypassing the delegation-task trigger that Phase 3 assigns to the task scheduler ('For delegation tasks, the scheduler spawns the repo orchestrator'). The handler performs a real agent spawn via spawn_repo_orchestrator_core and returns the agent record plus token. The amendment-table row 'Remove agent.spawn MCP tool — orchestrators do not spawn agents' covers this surface. Ownership note: the open task-298 contract removes gyre_spawn_worker and its acceptance criterion is scoped to 'a repo-orchestrator JWT has no MCP tool whose handler creates agent records' — a workspace-orchestrator JWT calling gyre_spawn_repo_orchestrator would still create agent records after task-298 as currently specced, so the second tool's removal needs explicit ownership (extension of task-298's contract or separate assignment); recorded for the controller.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"name\": \"gyre_spawn_repo_orchestrator\",",
            "detail": "The gyre_spawn_repo_orchestrator MCP tool is live production code at this base — a catalog entry advertised in tools/list."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "Spawn the single repo orchestrator for a repo in the caller's workspace. Requires a workspace-orchestrator JWT.",
            "detail": "The tool's own description states its contract: a workspace-orchestrator JWT spawns the repo orchestrator — the workspace-orchestrator principal retains a direct agent-spawning capability the spec's 'never spawns agents' boundary removes."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "None => return tool_error(\"requires a workspace-orchestrator token\"),",
            "detail": "The handler's tier check admits exactly the workspace-orchestrator principal (workspace_orchestrator_scope), confirming the surface is scoped to the persona the Phase-2 boundary constrains."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "match crate::api::orchestrator::spawn_repo_orchestrator_core(",
            "detail": "The handler performs a real agent spawn (spawn_repo_orchestrator_core: agent record, token, launch on the compute target) and returns the agent record plus token — an agent spawn reachable at the workspace-orchestrator tier without any delegation task."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "pub async fn on_spec_approved(state: &Arc<AppState>, payload: &serde_json::Value) {",
            "detail": "Positive wiring retained: server-side interception of the workspace-destined SpecApproved signal — scopes by workspace, holds the per-workspace mutex (concurrent approvals serialize), ensures a live workspace orchestrator before delivering the message to its inbox."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "task.task_type = Some(TaskType::Delegation);",
            "detail": "Positive wiring retained: the delegation task is created with task_type Delegation, referencing the approved spec via spec_ref and assigned to the orchestrator."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "let spec_ref = format!(\"{spec_path}@{spec_sha}\");",
            "detail": "Positive wiring retained: the delegation task's spec_ref is the specced spec_path@spec_sha form pinning the approved blob."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "task.task_type = Some(TaskType::Coordination);",
            "detail": "Positive wiring retained: coordination tasks are created in dependent repos (discovered from spec_links) for the same repo-orchestrator mechanism to process."
          },
          {
            "file": "crates/gyre-common/src/notification.rs",
            "quote": "Self::CrossWorkspaceSpecChange => 4,",
            "detail": "Positive wiring retained: cross-workspace dependents get priority-4 notifications (CrossWorkspaceSpecChange maps to rank 4 in the unified notification taxonomy), addressed to the dependent workspace's Admin/Owner members."
          }
        ]
      },
      {
        "id": "15aec83b58b3494602d31ce8",
        "decision": "gap",
        "requirement_quote": "The repo orchestrator **only creates tasks**. It never spawns agents. The system handles the task→agent mapping mechanically.",
        "detail": "The decomposition machinery is real: the task scheduler (startup loop plus operator trigger, cycles serialized on a run lock) claims only Delegation/Coordination tasks — Implementation and untyped tasks are skipped — and holds the per-repo mutex with a re-read after the lock, so a second delegation for the same repo waits while one repo orchestrator is active; stale InProgress claims are recovered crash-safely. run_repo_orchestrator reuses/creates the repo-orchestrator agent (repo-orchestrator persona, repo-scoped JWT). decompose_delegation reads the delegation task and the spec blob at the pinned spec_ref, failing closed on a malformed or empty SHA or an unresolvable ledger SHA; decomposition uses the real LLM port (latency-bounded at 120s) with persona prompt and repo tree, and degrades to an explicitly-labeled deterministic decomposition derived from real spec content when no LLM is configured or the call fails — honest degradation, not fabricated model output. Sub-tasks carry task_type Implementation, spec_ref to the approved SHA, parent_task_id linking the delegation task, order, and depends_on chaining; the delegation task is marked Done. Documented deviations: the repo codebase state is read via git ls-tree rather than MCP tools, and tasks are created through the task store rather than the task.create MCP tool. GAP in substance: 'The repo orchestrator only creates tasks. It never spawns agents.' is violated at this base by the live gyre_spawn_worker MCP surface — catalog entry, tools/call dispatch, scope validation requiring only a repo-orchestrator token holding a repo spawn scope, and a handler that delegates to spawn_agent_core with the full worktree/scoped-JWT/compute-target provisioning. The tool's restriction TO repo-orchestrator JWTs describes the opposite of the spec's requirement ('Remove agent.spawn MCP tool — orchestrators do not spawn agents', amendment table): it exists precisely so a repo-orchestrator JWT can spawn worker agents directly, bypassing the mechanical task→agent mapping of Phase 4. This is the open task-298 gap ('Remove orchestrator agent-spawning and carry out the spec amendment table', progress not-started).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"name\": \"gyre_spawn_worker\",",
            "detail": "The gyre_spawn_worker MCP tool is live production code at this base — a catalog entry advertised in tools/list, not a removed or stubbed surface."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "Spawn a worker agent for an implementation task in the caller's repo. Requires a repo-orchestrator JWT.",
            "detail": "The tool's own description states its contract: a repo-orchestrator JWT spawns a worker agent for an implementation task — exactly the orchestrator-spawns-agents capability the spec removes."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"gyre_spawn_worker\" => handle_spawn_worker(&state, &args, &auth).await,",
            "detail": "The tools/call dispatch routes gyre_spawn_worker to a real handler — the surface is reachable by any caller holding a repo-orchestrator token."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"gyre_spawn_worker\" => {",
            "detail": "Scope validation for the tool requires only a repo-scoped orchestrator token (check_repo_scope with write/spawn on the token's single repo scope) — no scheduler-principal or delegation-task-origin check stands between an orchestrator and a worker spawn."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "match crate::api::spawn::spawn_agent_core(",
            "detail": "handle_spawn_worker delegates to the full production spawn core (worktree creation, scoped JWT minting, compute target resolution) — a complete agent spawn performed at an orchestrator's discretion, violating 'It never spawns agents'."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "if !matches!(task_type, TaskType::Delegation | TaskType::Coordination) {",
            "detail": "Positive wiring retained: the scheduler claims only Delegation/Coordination tasks (Implementation goes to the worker-spawn path; untyped pre-approval push-hook tasks are skipped), matching the task_type discriminator requirement."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "let lock = state.orchestrator_registry.repo_lock(&repo.id).await;",
            "detail": "Positive wiring retained: per-repo mutex — only one repo orchestrator is active at a time; a delegation task for a repo with a live orchestrator waits (re-scanned on a later cycle)."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "sub.parent_task_id = Some(task.id.clone());",
            "detail": "Positive wiring retained: sub-tasks carry parent_task_id linking to the delegation task, plus order and depends_on chaining and spec_ref pinning the approved SHA."
          }
        ]
      },
      {
        "id": "e60f62cd06826e5e4de9decf",
        "decision": "gap",
        "requirement_quote": "3. Agents have 60 seconds to commit current work (same grace period as budget exhaustion), then are killed and marked `Stopped`",
        "detail": "Most of Phase 7 is real (task-118): pass path merges and completes the task; fail path returns the task to InProgress, writes one GateResult message per failed gate carrying the gate's real recorded output into the agent's inbox, closes the failed MR, and re-spawns the agent via relaunch_agent_session with a fresh EdDSA JWT, a fresh context (prompt set re-assembled at pinned versions; no conversation history loaded) on the same worktree/branch, incrementing the iteration and injecting GYRE_ITERATION; max iterations transition the agent to Failed, the task to Blocked, revoke the token and route a TaskBlocked message to the repo orchestrator's inbox (spawning one if none is active); spawn failure retries 3× at 1s/5s/30s then blocks with priority-1 notification. Two specific requirements are NOT implemented (the open task-300 gap, progress not-started): (1) Spec-rejection grace period — the reject handler cancels in-flight tasks (including the delegation task), sends a shutdown message whose payload merely carries \"grace_period_secs\": 60, creates the priority-2 SpecRejected notification, but marks the agent Stopped synchronously and cleans up its worktrees immediately; there is no 60-second timer and no ComputeTarget::kill_process() call on this path (in contrast, the budget-exhaustion path implements a real 60s grace timer with kill_process — so the 'same grace period as budget exhaustion' equivalence the spec asserts is false in code). (2) max_iterations workspace tier — the spec says the Repository's max_agent_iterations 'falls back to workspace default, then server default of 10', but no workspace-level iteration setting exists anywhere (the Workspace struct has max_agents_per_repo only); the resolution goes straight from the repo override to the server default 10, and the constraints prompt documents 'repo override, else the server default (10)'.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "\"grace_period_secs\": 60,",
            "detail": "The spec-rejection shutdown message carries the 60-second grace only as a payload field — no timer is armed on this path."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "let _ = agent.transition_status(gyre_domain::AgentStatus::Stopped);",
            "detail": "The reject handler marks active agents Stopped synchronously; there is no ComputeTarget::kill_process() call and no grace window in which the agent could commit current work (the process is left running while its worktree is destroyed)."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "crate::api::spawn::cleanup_worktrees_for_task(&state, &task).await;",
            "detail": "The cancelled task's worktrees are torn down immediately during rejection — the in-process grace the spec requires for committing current work does not exist on this path."
          },
          {
            "file": "crates/gyre-server/src/api/budget.rs",
            "quote": "pub const BUDGET_GRACE_PERIOD_SECS: u64 = 60;",
            "detail": "Contrast: the budget-exhaustion path DOES implement the real 60-second grace (start_budget_grace_timer → kill_workspace_agents_for_budget via ComputeTarget::kill_process, Idle for agents completing in grace, Stopped for killed ones) — the same mechanism the spec says spec rejection shares, but the rejection path never invokes it."
          },
          {
            "file": "crates/gyre-server/src/ralph_loop.rs",
            "quote": "let max_iterations = repo\n        .as_ref()\n        .and_then(|r| r.max_agent_iterations)\n        .unwrap_or(DEFAULT_MAX_ITERATIONS);",
            "detail": "The iteration ceiling resolves from the Repository override directly to the server default (10) — the specced workspace-default tier does not exist (the Workspace entity has no max_agent_iterations field; a repo-wide grep finds none)."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "// Iteration ceiling: repo override, else the server default (10).",
            "detail": "The served constraints prompt itself documents the two-tier resolution (repo override, else server default 10), confirming the workspace tier required by the spec is absent."
          },
          {
            "file": "crates/gyre-server/src/ralph_loop.rs",
            "quote": "pub(crate) async fn relaunch_agent_session(",
            "detail": "Positive wiring for the loop itself: gate failure re-spawns the agent with a fresh JWT and fresh context (no conversation history), incrementing the iteration and injecting GYRE_ITERATION, exactly as the fresh-session key property requires."
          }
        ]
      },
      {
        "id": "47d309e09aac1e2e04d334d0",
        "decision": "gap",
        "requirement_quote": "`system://constraints` delivers budget limits and operational parameters from the persona's operational fields. The four MCP prompts remain the delivery mechanism — the three-section structure above describes what goes INTO them.",
        "detail": "The three-section structure is largely real: the system prompt is the assembled meta-spec set (GYRE_META_SPEC_PROMPT plus GYRE_META_SPEC_SET_SHA at spawn; system://persona serves the pinned spawn-time record verbatim); the protocol section is system-managed and injected as GYRE_PROTOCOL_PROMPT and served by system://protocol — it carries the Ralph loop rules, MCP tool usage, the 60-second heartbeat requirement, escalation rules and the gyre_agent_complete completion signaling; task context (task/spec ref/branch/gate failures/inbox) is delivered via system://context; all four MCP prompts are implemented and dispatched in prompts/list + prompts/get. GAP: system://constraints does NOT deliver operational parameters from the persona's operational fields — prompt_constraints assembles only scope boundaries, trust level, workspace budget, lifetime budget and the iteration ceiling; the Persona operational fields (model, temperature, max_tokens, budget) are consumed by no execution path anywhere in the server (only CRUD DTOs reference them), so an agent spawned with a persona carrying distinctive operational values never sees them in its constraints. This is the open task-303 gap ('Deliver persona operational fields in constraints prompt', progress not-started, blocked on the task-298 registry replacement). Minor naming deviation: the spec's `agent.escalate` tool does not exist — the protocol's escalation rules direct agents to escalate via gyre_message_send to their orchestrator (a functional equivalent, documented in the protocol text).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"system://constraints\" => prompt_constraints(state, auth).await,",
            "detail": "The system://constraints MCP prompt is implemented and dispatched (all four specced prompts exist) — but its content does not include the persona's operational fields."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "text.push_str(\"\\n## Operational parameters\\n\");",
            "detail": "The constraints prompt's 'Operational parameters' section begins after the workspace-budget section — its contents are only the lifetime budget and the iteration ceiling, not the persona's model/temperature/max_tokens/budget."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "Some(secs) => text.push_str(&format!(\"- Lifetime budget: {secs}s\\n\")),",
            "detail": "What the operational-parameters section actually contains: agent lifetime budget and (below it) the Ralph-loop iteration ceiling — sourced from the agent and repo, never from the bound persona's operational fields, which no execution path reads."
          },
          {
            "file": "crates/gyre-server/src/api/prompts.rs",
            "quote": "- Send a heartbeat every 60 seconds; missing heartbeats mark you Dead and trigger re-spawn or cleanup.",
            "detail": "Positive wiring for the protocol section: the system-managed protocol (served by GYRE_PROTOCOL_PROMPT and system://protocol) carries the heartbeat requirement, Ralph rules, tool usage and completion signaling."
          },
          {
            "file": "crates/gyre-server/src/api/prompts.rs",
            "quote": "- `gyre_agent_complete` is the only completion signal: it creates the MR, revokes your token, and hands off to the gates.",
            "detail": "Completion signaling rules are delivered in the protocol section — the specced protocol content is real (escalation rules direct agents to gyre_message_send rather than the spec's agent.escalate name)."
          }
        ]
      },
      {
        "id": "a40446f46dbb0536f3c30ed0",
        "decision": "gap",
        "requirement_quote": "All cross-references to `ralph-loop.md` in other specs (e.g., `agent-gates.md`) should be updated to reference `agent-runtime.md` §1.",
        "detail": "The amendment table is partially carried out. SATISFIED rows: Task gains task_type (Implementation/Delegation/Coordination), order: Option<u32>, depends_on: Vec<Id>, parent_task_id: Option<Id>; MessageKind::SpecApproved exists in code (Event tier, server-only, specced payload) though message-bus.md was never amended; spec-lifecycle.md terminology cleaned (no 'CEO agent' remains; push-hook tasks informational); Workspace.compute_target_id added; the meta-spec-set endpoints retired in code (404, KV tables dropped by migration 000071) with MetaSpecSnapshot using content_hash tuples and drift detection comparing merge-attestation meta_specs_used against current required versions; MergeAttestation.meta_specs_used populated in production; the abac resolver carries meta_spec and compute_target resource mappings. UNMET rows (the open task-298 amendment-table gap, progress not-started): (1) the 'Remove agent.spawn MCP tool — orchestrators do not spawn agents' row (platform-model.md §4) is NOT carried out: two orchestrator-tier spawn surfaces ship as live production code — gyre_spawn_worker (a repo-orchestrator JWT spawns a worker agent through the full spawn core; catalog, dispatch, scope validation and handler all present) and gyre_spawn_repo_orchestrator (a workspace-orchestrator JWT spawns the repo orchestrator agent directly via spawn_repo_orchestrator_core). task-298's current acceptance criterion names only the repo-orchestrator/worker surface ('a repo-orchestrator JWT has no MCP tool whose handler creates agent records'), so the workspace-orchestrator surface needs explicit ownership — an extension of task-298's contract or a separate assignment — recorded for the controller. (2) ralph-loop.md cross-references remain in canonical specs — authorization-provenance.md still cites ralph-loop.md for the Ralph Loop re-spawn semantics and meta-spec-reconciliation.md still carries a ralph-loop.md@sha example path (agent-gates.md itself was updated). (3) docs/agent-protocol.md still exists and is actively maintained ('consolidated into this spec' not carried out) — and the retirement is uncoordinated with active plans: the assigned task-311 (websocket-agent-transport) has a deliverable to update docs/agent-protocol.md, the assigned task-272 (terminal-worktree-cleanup) must update its worktree-cleanup claims, and task-321's docs site builds one page per docs/ file including agent-protocol, so deleting the file requires sequencing after those deliverables or explicitly retargeting them; recorded for the controller. (4) the /api/v1/personas endpoints were NOT removed — four routes remain live in api/mod.rs alongside the replacement registry. (5) the Persona struct gains no required: bool field and its prompt field is named system_prompt, not prompt (the required flag lives on the unified MetaSpec struct instead — see a00a6b5082295555260842b2). (6) message-bus.md contains no SpecApproved kind (the doc amendment was never applied). (7) the abac-policy-engine.md resource-attribute amendment is half-done in code — meta_spec and compute_target resources exist in the resolver, but the 'archive' action was never added (the archive/unarchive routes sit in the frozen ABAC exemption file with per-handler enforcement instead) and the abac-policy-engine.md doc has none of the three additions. (8) hierarchy-enforcement.md was never amended — no compute-target or meta-spec CRUD routes in its route table and meta_spec_versions/meta_spec_bindings/compute_targets are not in its tenant-filter configuration. (9) human-system-interface.md §5 was never amended to include meta_specs_used in the attestation bundle schema. (10) the Agent entity's task binding is current_task_id: Option<Id> rather than the specced non-optional task_id: Id. (11) meta-spec-reconciliation.md still specces the retired PUT /api/v1/workspaces/{id}/meta-spec-set endpoint, contradicting this spec's replacement of it.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"name\": \"gyre_spawn_worker\",",
            "detail": "Unmet amendment row (platform-model.md §4, 'Remove agent.spawn MCP tool — orchestrators do not spawn agents'): the gyre_spawn_worker tool is live production code — a repo-orchestrator JWT can spawn a worker agent through the full spawn core (see 15aec83b58b3494602d31ce8)."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"name\": \"gyre_spawn_repo_orchestrator\",",
            "detail": "Unmet amendment row (same 'orchestrators do not spawn agents' rationale): a second live spawn surface lets a workspace-orchestrator JWT spawn the repo orchestrator agent directly (see 6d7302e3c6a23af8c4020c20) — not covered by task-298's current acceptance criterion, which scopes only the repo-orchestrator tier."
          },
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "\"/api/v1/personas\",",
            "detail": "Unmet amendment row (platform-model.md §2): the /api/v1/personas endpoints were NOT removed — the route block (list/create, resolve, :id CRUD, :id/approve) remains live alongside the replacement /api/v1/meta-specs?kind=meta:persona registry."
          },
          {
            "file": "specs/system/message-bus.md",
            "quote": "SpecChanged,",
            "detail": "Unmet amendment row (message-bus.md MessageKind): the canonical message-bus.md enumeration still lists SpecChanged but contains no SpecApproved anywhere (grep for SpecApproved/spec_approved returns nothing) — the kind exists only in code; the doc amendment was never applied."
          },
          {
            "file": "specs/system/authorization-provenance.md",
            "quote": "- **Ralph Loop** (`ralph-loop.md`): Re-spawn produces `DerivedInput` from same parent, preserving constraint chain (§4.2).",
            "detail": "Unmet supersede row: a canonical spec still cross-references ralph-loop.md (meta-spec-reconciliation.md likewise carries a meta/process/ralph-loop.md@sha example) — 'all cross-references should be updated' was not carried out (agent-gates.md itself was updated)."
          },
          {
            "file": "docs/agent-protocol.md",
            "quote": "This document covers the full agent lifecycle: spawn, operation, and completion.",
            "detail": "Unmet supersede row: docs/agent-protocol.md still exists and documents the full agent lifecycle this spec claims to consolidate — never removed or folded in; the assigned task-311 and task-272 deliverables edit this exact file, so retirement needs sequencing or retargeting."
          },
          {
            "file": "scripts/abac-route-registry-exemptions.txt",
            "quote": "/api/v1/repos/:id/archive",
            "detail": "Unmet amendment row (abac-policy-engine.md): the 'archive' action was never added to the ABAC action set — the archive/unarchive routes sit in the frozen exemption file (per-handler enforcement) and 'archive' appears nowhere in abac_middleware.rs."
          },
          {
            "file": "crates/gyre-domain/src/agent.rs",
            "quote": "pub current_task_id: Option<Id>,",
            "detail": "Deviation from the amendment row (Agent entity): the spec says 'Add task_id: Id field — every agent is bound to exactly one task'; the code uses a nullable current_task_id: Option<Id> (assigned at spawn, but not the non-optional binding the row defines)."
          },
          {
            "file": "specs/system/hierarchy-enforcement.md",
            "quote": "### In-Memory meta_spec_sets",
            "detail": "Unmet amendment row (hierarchy-enforcement.md §4): the doc was never amended — it still describes the retired in-memory meta_spec_sets model and its route table / tenant-filter configuration contain no compute-target or meta-spec CRUD routes or the meta_spec_versions/meta_spec_bindings/compute_targets tables (the tables exist in the adapters)."
          },
          {
            "file": "specs/system/meta-spec-reconciliation.md",
            "quote": "PUT /api/v1/workspaces/{id}/meta-spec-set",
            "detail": "Contradiction left in place: meta-spec-reconciliation.md still specces the meta-spec-set endpoint that this spec's amendment table replaces (the code retired it — the endpoints 404 — but the canonical doc was never amended), and human-system-interface.md §5 likewise lacks the meta_specs_used array this spec says to add."
          },
          {
            "file": "crates/gyre-domain/src/task.rs",
            "quote": "pub enum TaskType {",
            "detail": "Satisfied amendment row (platform-model.md §3 Task): TaskType (Implementation/Delegation/Coordination) plus order, depends_on and parent_task_id all exist on the Task entity and drive the scheduler discriminators."
          },
          {
            "file": "crates/gyre-common/src/message.rs",
            "quote": "MessageKind::SpecApproved,",
            "detail": "Satisfied amendment row (code side): the SpecApproved kind exists with Event tier, server-only flag and the specced payload — only the message-bus.md doc amendment is missing."
          },
          {
            "file": "crates/gyre-domain/src/workspace.rs",
            "quote": "pub system_prompt: String,",
            "detail": "The Persona struct's prompt field is named system_prompt (not prompt) and the struct carries no required flag — the row's 'Persona struct gains required: bool' was implemented instead on the unified MetaSpec registry (see a00a6b5082295555260842b2)."
          }
        ]
      }
    ],
    "source_evidence": [
      {
        "file": "crates/gyre-server/src/api/tasks.rs",
        "sha256": "bd62eeaca091ddddf920b78a5dc708750f76e1485baac31def92a6a3c128f81e"
      },
      {
        "file": "crates/gyre-server/src/mcp.rs",
        "sha256": "0b268bd680b9e5429ca191debfb2451f2841beb7b383cd1719a59dd70683dd68"
      },
      {
        "file": "crates/gyre-server/src/signal_chain.rs",
        "sha256": "ddb20b46f8fe9d671a8b11f570aebdc715b19371d64b7ace878099dcae4377f2"
      },
      {
        "file": "crates/gyre-server/src/api/spawn.rs",
        "sha256": "4dc88c1516f2135573d94cf255c246ef1b73ce7b3849683669a6543b21823e60"
      },
      {
        "file": "crates/gyre-server/src/api/specs.rs",
        "sha256": "701749a457338c0bff05f4c0da748f94759958d1140c9a68edfcf26aa5e0ab41"
      },
      {
        "file": "crates/gyre-server/src/api/graph.rs",
        "sha256": "5a62cb4730260dfbd9dd78335fa17aba78264c8abf58350ed0cdd78988a87d77"
      },
      {
        "file": "crates/gyre-server/src/merge_processor.rs",
        "sha256": "5a5676ba50a366b0ed75b1297e3eb95f1490d18d5983758b869da2201e53e34d"
      },
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "sha256": "012f5b3fc89e4f82182168f7c32bb966544d4cfd90201147deb0b6a7971d6d40"
      },
      {
        "file": "specs/system/system-explorer.md",
        "sha256": "5cbe239d7fd30ecb36b4f3268f6c3e897509403a3db68fe154fa7a4d1ee70c9b"
      },
      {
        "file": "specs/system/dependency-graph.md",
        "sha256": "ecd5a4c73c517d23014bd4846d7554f1a42fbf55ff5326849940f1b13e5adbd3"
      },
      {
        "file": "crates/gyre-common/src/message.rs",
        "sha256": "dea344a7ab04b3f2bd754e1335f9143184c5a02508f0763ebd7638f17d735558"
      },
      {
        "file": "crates/gyre-common/src/notification.rs",
        "sha256": "d5406306ddad1dcb230ab5ab6b4280be45dcc29f46ab03efc6d13c951facdc9c"
      },
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "sha256": "fd5036bd1272469fc55a2b304ac10a7db63c95a6d5f546b37a3c2c6c86307149"
      },
      {
        "file": "crates/gyre-server/src/auth.rs",
        "sha256": "457a4f1d2cb8605ae871a353943f77c8387b2e5f8c9b7d326eed97231024c0e7"
      },
      {
        "file": "crates/gyre-server/src/ralph_loop.rs",
        "sha256": "94d90084cb6a3072ab49b6b02959f7afae7d833713f87c3c09d214f56818996f"
      },
      {
        "file": "crates/gyre-server/src/api/merge_queue.rs",
        "sha256": "e555c04913f5a50e244c20efe5070922f67a9871561559bb2d14af92819e173e"
      },
      {
        "file": "crates/gyre-server/src/gate_executor.rs",
        "sha256": "c4b7e05809c8be5cd04d754fbcd53619bb8832ac29c355222a5c031972ac1ef5"
      },
      {
        "file": "crates/gyre-server/src/api/budget.rs",
        "sha256": "427482dce0027b0410e0607cfac5d77294b5be097c27880cd32c1f92282237cf"
      },
      {
        "file": "crates/gyre-server/src/reconciliation.rs",
        "sha256": "09ceb5008680b3517763d5b365e4b547d0e0118d10e078421a8d535c6a50ccdf"
      },
      {
        "file": "crates/gyre-server/src/prompt_assembly.rs",
        "sha256": "f02c2ec7b852145682f0e9c356de0be3098e336954d48aa7c368cfb98b6b8000"
      },
      {
        "file": "crates/gyre-domain/src/meta_spec.rs",
        "sha256": "1109c30b01232bc0c413db708bd52fa22f6a5507d1660506d409f036fcae7335"
      },
      {
        "file": "crates/gyre-server/src/api/meta_specs.rs",
        "sha256": "9ac2473fcd3a488f662f0aa38d4a15fc46880c6ddd5b6f43560fc58a9a0017c1"
      },
      {
        "file": "crates/gyre-domain/src/workspace.rs",
        "sha256": "ec60899067451e201133c32606309f1f8e4db8cc9d1a154723cb05981de6d379"
      },
      {
        "file": "crates/gyre-adapters/migrations/2026-03-26-000032_meta_specs/up.sql",
        "sha256": "17798fe21887f0e21c48fcc2bdb4af53b7778b89f35fff38598c53bba869cf88"
      },
      {
        "file": "crates/gyre-adapters/src/sqlite/meta_spec.rs",
        "sha256": "ba368f5dad20a798ad2f39b0b2d1dfd38a8c66ff783864bdf6b1da0102dbf112"
      },
      {
        "file": "crates/gyre-adapters/src/postgres/meta_spec.rs",
        "sha256": "8ecd163ab458c04153321489040d1a6553c53791b5f8f2106966c2396b4748ea"
      },
      {
        "file": "web/src/lib/DetailPanel.svelte",
        "sha256": "c98732e1f616fbc93fa1e0f6039bcc4b11112e50e723f8210454c55491cfbdad"
      },
      {
        "file": "crates/gyre-domain/src/attestation.rs",
        "sha256": "a8954208e773442481f6fd3d774571ac77a9e5316b77e0bb34ea38d1072b5e23"
      },
      {
        "file": "crates/gyre-server/src/jobs.rs",
        "sha256": "922f3de73788564bd7c4b4ca48b065f2bba6149c7b651aaf108c02b667c559b4"
      },
      {
        "file": "crates/gyre-server/src/lib.rs",
        "sha256": "46d398754fa7d7921c396160262899d430e24de71bdd53c32e0e7f89c0329afb"
      },
      {
        "file": "crates/gyre-server/src/main.rs",
        "sha256": "32b8a97dd0b5faf9be7428d713a6678c5fadb0c55d6b2467f014b21f489e5087"
      },
      {
        "file": "crates/gyre-ports/src/compute.rs",
        "sha256": "c67c91d31ee7fdd6332bf2d36b9fade006eb805d7be86817d9f4e546498ec725"
      },
      {
        "file": "crates/gyre-adapters/src/compute/container.rs",
        "sha256": "d1d3728d3e77f9a4668a4d3cc10a7500d0fcf8e86408b0b84c5a1bbd6b3c6f84"
      },
      {
        "file": "crates/gyre-adapters/src/compute/ssh.rs",
        "sha256": "7a286dbaafdb9690fd98b2cd45743ee796f022e39b1ab873d475c84fa760e3c9"
      },
      {
        "file": "crates/gyre-adapters/src/compute/kubernetes.rs",
        "sha256": "41b6b1d99c737816f145cb7998091c65d310f024a47070a1f995d711d81b3023"
      },
      {
        "file": "docker/gyre-agent/flake.nix",
        "sha256": "d714ef1b0493604998e58b1865fd68de2875105f0521dc63798229f248351b10"
      },
      {
        "file": "crates/gyre-domain/src/compute_target.rs",
        "sha256": "33765e531f3f4e7c733ad2c308d079b4bea9ac837a07967f0bfa22287feed6b6"
      },
      {
        "file": "crates/gyre-server/src/api/compute_targets.rs",
        "sha256": "e129e133ca66eca5ece72edaf86dae5c4c33d6047df325cfe3c1d1f6a34c1750"
      },
      {
        "file": "crates/gyre-server/src/api/repos.rs",
        "sha256": "29546768ffe40cb08f036e665ffc0e93ab784f8791d89aa2bb7ae4102fd7e6bf"
      },
      {
        "file": "docker/gyre-agent/cred-proxy.mjs",
        "sha256": "37506661eb1504c4ea1681a94490937a9c3ddd05202e6cf4e8119f3d989dcfe2"
      },
      {
        "file": "crates/gyre-server/src/api/prompts.rs",
        "sha256": "99bb012d73b1cdf33db352022e2c2ec2855838a0730ab15fb2e59e38f8d984ce"
      },
      {
        "file": "specs/system/message-bus.md",
        "sha256": "28e9e1e7a050805bfd61961f0a6a099139f23cf00cceabb14d0757ada9062cf9"
      },
      {
        "file": "specs/system/authorization-provenance.md",
        "sha256": "4174f9f24c3d5833307158d87af6f779e864acb8a24d73b6aeff90bf5db2134c"
      },
      {
        "file": "docs/agent-protocol.md",
        "sha256": "6c9507a5af3f5f8ddbba99852652f9bf71717da0a08e4f7df07dde104a310124"
      },
      {
        "file": "scripts/abac-route-registry-exemptions.txt",
        "sha256": "ca0f12c94e9b80b65a86483f97547c7db22a75bc335fabc1066c54e72a1c24b4"
      },
      {
        "file": "crates/gyre-domain/src/agent.rs",
        "sha256": "3b5a49231eece5b6dbfdd468c25be9f00e2b38341cb21997b1afefbe4d53d519"
      },
      {
        "file": "specs/system/hierarchy-enforcement.md",
        "sha256": "e458f8c1230e2860a249a222c8bf6481c38aaaea91b0d36ebce128d32225872b"
      },
      {
        "file": "specs/system/meta-spec-reconciliation.md",
        "sha256": "d2c11f086b81bf5f3ce118edea75323762d2dda63e22340e7f6e46404e93a024"
      },
      {
        "file": "crates/gyre-domain/src/task.rs",
        "sha256": "87455f0263528e7d60ba3cb07c0d48684c30ca171445d0b2f22182e6f13929b6"
      }
    ]
  },
  "plan": {
    "audit": "7db16b5d9dc226dc1a99fbcf46f11e946fa694ba58d53b3e122ad2f572e9ae28",
    "tasks": [
      {
        "key": "approval-single-trigger",
        "title": "Gate all spec-implementation work on approved specs; classify the two specced non-approval triggers",
        "covers": [
          "1bd201625dc5fbe0164ced05"
        ],
        "required_behavior": "Existing task-297 contract, revised per controller review finding unapproved-implementation-paths-omitted so that every implementation-work surface named by the revision-4 audit is owned (the revision-3 plan omitted the thorough-preview and cascade-test surfaces and over-claimed complete closure). Per agent-runtime.md §1 The Signal Chain ('There is no other trigger. The SpecApproved event is the universal starting signal.'): (1) Task-creation gate — POST /api/v1/tasks (crates/gyre-server/src/api/tasks.rs, task_type accepted from the body) and the MCP gyre_create_task tool (crates/gyre-server/src/mcp.rs) reject creating a task with task_type 'implementation' unless its spec_ref resolves to a spec currently Approved in the spec approval ledger at exactly the pinned SHA (path@sha); unapproved, Pending, invalidated, unresolvable, or missing spec_ref yields a 4xx error (HTTP) / tool error (MCP) naming the requirement and no task is created. Delegation and Coordination tasks and untyped (pre-approval push-hook) tasks are unaffected — the orchestrator chain creates them from SpecApproved and pins the approved SHA. (2) Scheduler gate (defense in depth) — the Phase-4 scan in crates/gyre-server/src/signal_chain.rs (spawn_implementation_workers) must, before claiming a Backlog Implementation task and spawning a worker, verify the task's spec_ref still resolves to an Approved ledger entry at the pinned SHA (an approval invalidated by a later push stops the spawn); on failure the task stays Backlog with an observable skip label and a logged reason — no agent record, no InProgress transition. BEHAVIOR FOR spec_ref-LESS IMPLEMENTATION TASKS (review correction (a), the cascade-test class): the gate must not strand dependency-graph.md §Cascade Testing — merge_processor::trigger_cascade_tests (invoked automatically on merge and via POST /api/v1/repos/:id/cascade-tests/trigger) creates Implementation tasks with labels ['cascade-test','auto-created'] and no spec_ref/spec_path in dependent repos precisely so the scheduler dispatches workers, and dependency-graph.md specces these merge-triggered test runs together with their REST trigger and result-reporting surfaces. The scheduler therefore distinguishes spec-derived implementation tasks (carrying spec_ref/spec_path — gated on approval) from the named system-created cascade-verification class (recognized by its labels/creator and dispatched as before): the exception is explicit, labeled, and documented in code — never a silent pass-through of every spec_ref-less task — and is recorded as pending the §1 spec-lifecycle amendment in (4). No active plan owns cascade testing (the assigned dependency-graph plan owns only orphaned-edge transitions), so this task records the exception itself. (3) Direct-spawn gate, moved into the spawn core (review correction (b)) — the non-scheduler-principal rejection for task-bound implementation worker spawns must live in spawn_agent_core (crates/gyre-server/src/api/spawn.rs), keyed on principal class rather than only in the POST /api/v1/agents/spawn handler, so no in-process route can bypass it by calling the core directly: a human or agent principal requesting a worker bound to an ordinary Implementation task is rejected 403; the scheduler path (signal_chain.rs) is the only worker-spawn route for ordinary implementation tasks. The endpoint remains for Phase-8 system-initiated kinds (interrogation via agent_type). THOROUGH-PREVIEW DISPOSITION: POST /api/v1/repos/:id/graph/thorough-preview (api/mod.rs:1332, handler graph.rs:2601) is a canonical system-explorer.md §3 requirement — a human-initiated implementation of an unapproved spec DRAFT (draft_content arrives in the request body, so the draft is normally not in the spec registry) on a throwaway branch, spawned in-process via spawn_agent_core with the human as caller. It must keep working (never break the sibling spec's specced feature to satisfy an unamended wording), and it must not survive the core-level principal gate by accident: the preview spawn is explicitly classified as the human-initiated preview exception (the preview task already carries the 'spec-preview' label and a preview/ throwaway branch; the classification is a named code path in the spawn core, not an unlabelled bypass), so the core admits exactly two worker-spawn classes for non-scheduler principals — none for ordinary implementation tasks, and the explicitly-classified preview exception — with the preview path's spawn otherwise subject to the same repo/budget checks as today. Note the existing task-302 spawn-core ledger gate does not cover this path: it applies only when task.spec_path resolves in the ledger, and an un-pushed UI draft has no ledger entry (the code comment states unregistered specs 'are not gated here'). (4) Conflict resolution, not silent contradiction (review correction (c)) — agent-runtime.md §1's 'There is no other trigger' is in canonical conflict with system-explorer.md §3 (thorough preview via agent implementation on throwaway branches) and dependency-graph.md §Cascade Testing (the forge triggers a test run against Repo B's new version). This task proposes, via the spec lifecycle (a spec-edit branch requiring human approval — never editing the canonical spec's contract in place), an agent-runtime.md §1 amendment scoping the single-trigger property to spec-implementation work while explicitly acknowledging merge-triggered cascade verification and human-initiated draft previews as named non-spec-implementation triggers. The closure claim is correspondingly accurate: after this task, every path that starts SPEC-IMPLEMENTATION work requires an Approved spec at the pinned SHA (task creation, scheduler, spawn core); the two specced exception surfaces remain functional, explicitly classified, and pending the amendment's human approval — never silently ignored, never closed by deleting the sibling specs' features. The already-satisfied approval side (ledger row, SpecApproved emission only on transition into Approved, modified-spec invalidation to Pending) is preserved unchanged. The orchestrator-tier MCP spawn surfaces (gyre_spawn_worker, gyre_spawn_repo_orchestrator) are owned by task-298 in this plan, not by this task.",
        "acceptance": [
          "POST /api/v1/tasks (and MCP gyre_create_task) with task_type=implementation and a spec_ref whose spec has no Approved ledger entry at that SHA returns 4xx and creates no task; the identical request after POST /api/v1/specs/:path/approve succeeds and the created task enters the normal chain. A task_type=implementation request with no spec_ref at all is likewise rejected on these external surfaces.",
          "With an Implementation/Backlog task referencing an unapproved spec inserted directly into storage, one scheduler cycle creates zero agent records and leaves the task Backlog with the skip label, while a twin task referencing an approved spec spawns exactly one worker.",
          "Cascade testing still works end-to-end: a merge (or POST /api/v1/repos/:id/cascade-tests/trigger) that creates cascade-test tasks in a dependent repo still results in worker agents being spawned for them and results reported — the scheduler dispatches the named cascade-verification class while gating spec-derived tasks (no task stranded in Backlog).",
          "POST /api/v1/agents/spawn with the task_id of an ordinary Implementation task from a human or agent principal returns 403, and the same rejection fires for any in-process spawn_agent_core caller with a non-scheduler principal that is not the explicitly-classified preview path; spawning an interrogation agent (agent_type=interrogation) still succeeds; POST /api/v1/repos/:id/graph/thorough-preview still creates the preview task ('spec-preview' label, preview/ throwaway branch) and spawns the preview worker.",
          "An agent-runtime.md §1 amendment proposal exists on a spec-edit branch (spec lifecycle) scoping the single-trigger property to spec-implementation work and naming both exception surfaces, awaiting human approval; the canonical spec file is not edited in place."
        ],
        "negative_probe": "Three probes, all failing at the base. (1) Insert directly into task storage a Backlog task with task_type=implementation and spec_ref 'specs/x.md@<sha>' where the spec ledger shows approval_status Pending; run one scheduler cycle and assert zero agent records — at the base the Phase-4 scan (signal_chain.rs spawn_implementation_workers) spawns a worker for ANY Backlog Implementation task: it checks repo status, ordering, max_agents and budget but never the approval ledger, and gyre_create_task / POST /api/v1/tasks accept task_type=implementation with no approval precondition, so implementation work starts with no human spec approval anywhere in the chain. (2) POST /api/v1/repos/:id/graph/thorough-preview with a spec_path that was never pushed (no ledger entry): at the base the spawn-core approval gate is skipped exactly when the spec is not in the registry (spawn.rs: 'Tasks whose spec is not in the registry cannot be evaluated and are not gated here' — the if-let-Some(entry) fall-through), so a worker agent is spawned for unapproved draft content with the human as caller and no explicit classification anywhere; after the change the spawn must survive only via the named preview classification (probe asserts the 'spec-preview' label and throwaway branch are present and the spawn succeeds), while an ordinary implementation-task spawn from the same human principal is rejected. (3) Drive a cascade test (merge in a repo with a dependent repo, or the REST trigger): assert the cascade-test task still spawns its worker AND that the exception is explicit — a named, labeled dispatch class in the scheduler, not an unlabeled fall-through. A gate implemented as blanket spec_ref enforcement fails this probe by stranding the cascade-test task in Backlog forever (breaking dependency-graph.md's cascade testing); a gate implemented as blanket pass-through of spec_ref-less tasks fails it by leaving the exception unclassified and unowned — both failure modes are the real defects the probe must distinguish.",
        "dependencies": [],
        "existing_task": "task-297"
      },
      {
        "key": "orchestrator-spawn-surface-removal",
        "title": "Remove both orchestrator-tier MCP spawn tools and carry out the code-side amendment table",
        "covers": [
          "15aec83b58b3494602d31ce8",
          "6d7302e3c6a23af8c4020c20"
        ],
        "required_behavior": "Existing task-298 contract ('Remove orchestrator agent-spawning and carry out the spec amendment table'), extended openly per controller review findings agent-spawn-amendment-row-falsely-satisfied and workspace-orchestrator-spawn-surface-omitted so that BOTH orchestrator-tier spawn surfaces named by the revision-4 audit are owned here; its dependency set is unchanged (task-302). Per agent-runtime.md §1 Phase 3 ('The repo orchestrator only creates tasks. It never spawns agents. The system handles the task→agent mapping mechanically.'), §1 Phase 2 ('The workspace orchestrator only creates tasks... It never spawns agents, never touches code, never interacts with compute targets.'), and the amendment-table row 'Remove agent.spawn MCP tool — orchestrators do not spawn agents' (platform-model.md §4). (1) Remove the gyre_spawn_worker MCP tool entirely — catalog entry (mcp.rs:605), tools/call dispatch (mcp.rs:5497), scope validation (mcp.rs:842), handler handle_spawn_worker (mcp.rs:1702), prompt references (api/prompts.rs, api/spawn.rs) and tests — so no repo-orchestrator principal can create agent records through any MCP surface; the mechanical scheduler is the only worker-spawn path. (2) EXPLICIT CONTRACT EXTENSION (not a silent reuse of the task-298 wording): remove the gyre_spawn_repo_orchestrator MCP tool entirely as well — catalog entry (mcp.rs:545), tools/call dispatch (mcp.rs:5489), scope validation (mcp.rs:863), handler handle_spawn_repo_orchestrator (mcp.rs:1525) and tests — so no workspace-orchestrator principal can create agent records through any MCP surface either (the revision-4 audit records that task-298's original acceptance criterion names only the repo-orchestrator tier; this plan-carried extension broadens it). The scheduler's Delegation/Coordination detection in signal_chain.rs remains the ONLY repo-orchestrator spawn path (Phase 3: 'For delegation tasks, the scheduler spawns the repo orchestrator'); the server-side ensure_repo_orchestrator / spawn_repo_orchestrator_core chain is preserved unchanged for that path, and likewise the server-side ensure_workspace_orchestrator chain for Phase 2. The task document's first acceptance criterion is broadened correspondingly — from 'a repo-orchestrator JWT has no MCP tool whose handler creates agent records' to 'no orchestrator-tier JWT (repo or workspace) has an MCP tool whose handler creates agent records' — a strengthening carried by this plan's text so the executing worker updates the task-298 contract rather than replaying the narrower original. (3) Remove the /api/v1/personas routes (CRUD, resolve, approve — live at api/mod.rs:1054 ff. at the base) — 'replaced entirely by /api/v1/meta-specs?kind=meta:persona'; the meta-spec registry persona kind must carry the Persona operational fields (model, temperature, max_tokens, budget) so they remain editable after the personas routes are removed, and spawn-time persona resolution/binding keeps working against registry-backed persona data; all consumers (web client, CLI, tests, ABAC/RBAC registrations for the removed routes) are updated, and the frozen exemption-file count only shrinks. (4) Add the 'archive' action to the ABAC action vocabulary and register the repo archive/unarchive routes with the middleware resolver (removing their entries from scripts/abac-route-registry-exemptions.txt) so archive/unarchive get real policy evaluation, keeping scripts/check-abac-route-registry.sh green. (5) Supersession cleanup per the spec's own directive: update every remaining live cross-reference to ralph-loop.md in specs/ and docs/ to reference agent-runtime.md §1 (specs/system/authorization-provenance.md:853, the ralph-loop.md@sha example in specs/system/meta-spec-reconciliation.md:87, the milestone example in specs/milestones/m32-meta-spec-reconciliation.md) and retire specs/system/ralph-loop.md as fully superseded. (6) hierarchy-enforcement.md §4 amendment carry-out: add the compute-target and meta-spec CRUD routes to its route table with their auth-context notes, add meta_spec_versions/meta_spec_bindings/compute_targets to its tenant-filter configuration, and correct the stale KvJsonStore note that still lists compute_targets among tenant-less KV namespaces. (7) The meta-spec-reconciliation.md row's registry-replacement clauses are delivered by task-302 (dependency); this task's close-out verifies those clauses are carried out. The existing decomposition machinery (Delegation detection, per-repo mutex one-live semantics, sub-tasks carrying spec_ref/parent_task_id/order/depends_on, LLM judgment with deterministic fallback) and the workspace-orchestrator chain (per-workspace mutex, on-demand spawn, Delegation/Coordination task creation) are preserved unchanged — only the MCP spawn surfaces are removed. The needs_write RBAC gate membership derives from handler effects, so removing the handlers removes the membership (scripts/check-mcp-write-tools.sh stays green).",
        "acceptance": [
          "The MCP tools catalog lists neither gyre_spawn_worker nor gyre_spawn_repo_orchestrator, and tools/call with either name returns an error; no orchestrator-tier JWT (repo-orchestrator or workspace-orchestrator) has any MCP tool whose handler creates agent records.",
          "Creating a Delegation task in a repo still results in exactly one repo-orchestrator spawn through the scheduler path (signal_chain.rs Delegation/Coordination detection → ensure_repo_orchestrator), proving the removal eliminated the orchestrator-initiated bypass, not the scheduler-initiated capability; likewise a SpecApproved event still spawns/uses the workspace orchestrator through the server-side chain.",
          "GET/POST/PUT/DELETE /api/v1/personas and /api/v1/personas/resolve return 404; persona creation, editing (including operational fields and approval), and spawn-time persona resolution work via the meta-spec registry endpoints with kind=meta:persona.",
          "A Deny policy with action 'archive' on a repo resource blocks repo archive for the affected principal through the middleware (real policy evaluation), and the repo archive/unarchive routes are covered by RouteResourceMapping entries with no exemption-file entries.",
          "grep for 'ralph-loop.md' across specs/ and docs/ returns no live normative cross-references; specs/system/ralph-loop.md no longer exists as a live spec.",
          "hierarchy-enforcement.md's route table and tenant-filter configuration name the compute-target and meta-spec CRUD routes/tables with their auth-context notes, and its KvJsonStore note no longer misdescribes compute_targets storage.",
          "Carried from the dependency: PUT/GET /api/v1/workspaces/:id/meta-spec-set and PUT/GET /api/v1/tenants/:id/meta-specs/set return 404/405 with no production writer to the meta_spec_sets/tenant_meta_spec_sets stores remaining, and merge-time drift enforcement compares merge-attestation meta_specs_used against current required meta-spec versions."
        ],
        "negative_probe": "Two probes, one per spawn surface, both failing at the base. (1) Mint a repo-orchestrator JWT (holding repo:{name}:spawn) and call MCP tools/call gyre_spawn_worker with a valid implementation task id — at the base this performs a real agent spawn through spawn_agent_core (worktree creation, scoped JWT minting, compute-target resolution) and returns the agent record, exactly the 'repo orchestrator spawns agents' capability Phase 3 removes. (2) Mint a workspace-orchestrator JWT (holding workspace:{ws}:spawn) and call MCP tools/call gyre_spawn_repo_orchestrator with a valid repo id — at the base this performs a real agent spawn through spawn_repo_orchestrator_core (agent record, repo-scoped JWT, compute-target launch) and returns the agent record plus token, exactly the 'workspace orchestrator spawns agents' capability Phase 2 removes; no delegation task exists anywhere in this second flow. After the change both probes must fail with the tool absent from the catalog and no agent record creatable by an orchestrator principal, while the scheduler-driven probes (delegation task → repo orchestrator; SpecApproved → workspace orchestrator) still pass — proving real behavior was removed at the principal boundary, not the capability itself.",
        "dependencies": [
          "task-302"
        ],
        "existing_task": "task-298"
      },
      {
        "key": "spec-rejection-grace-iteration-tier",
        "title": "Real spec-rejection 60-second grace period and workspace max_iterations tier",
        "covers": [
          "e60f62cd06826e5e4de9decf"
        ],
        "required_behavior": "Existing task-300 contract, unchanged. Per agent-runtime.md §1 Phase 7 'Spec rejection mid-flight' ('Agents have 60 seconds to commit current work (same grace period as budget exhaustion), then are killed and marked Stopped'). (1) The reject handler (crates/gyre-server/src/api/specs.rs) must implement the specced sequence instead of the current synchronous stop (transition_status(Stopped) at specs.rs:1567, cleanup_worktrees_for_task at specs.rs:1626, with 'grace_period_secs': 60 existing only as payload text at specs.rs:1610): cancel the in-flight tasks and delegation task, send the shutdown message, then start a real 60-second grace timer using the same mechanism as the budget path (start_budget_grace_timer / BUDGET_GRACE_PERIOD_SECS in api/budget.rs, which already implements the real kill_process grace); agents that commit and call agent.complete within the grace period are marked Idle; at expiry the server kills remaining agents via ComputeTarget::kill_process(), marks them Stopped, and revokes their JWTs; the terminal worktree cleanup runs only after the kill, preserving the agent-workflow terminal-cleanup contract (the terminal-worktree-cleanup task is complete — its semantics live in docs/agent-protocol.md's 'Worktree teardown is deferred to terminal completion' section, which this change must keep accurate: spec rejection remains a listed terminal point, with teardown after the grace kill rather than synchronously). The existing priority-2 'Spec rejected' notification fanout and task cancellation are preserved. (2) Iteration-ceiling middle tier: the spec says the Repository's max_agent_iterations 'falls back to workspace default, then server default of 10', but no workspace-level iteration setting exists (the Workspace struct has max_agents_per_repo only; ralph_loop.rs:410-411 resolves repo → DEFAULT_MAX_ITERATIONS=10 directly). Add max_agent_iterations: Option<u32> to the Workspace entity (next free migration number, port contract enforced in EVERY adapter including the mem adapter per check-mem-port-contracts), settable via the workspace update API; resolution becomes repo.max_agent_iterations → workspace.max_agent_iterations → server default 10, applied consistently in the Ralph-loop ceiling check (ralph_loop.rs) and every surface that renders the ceiling (the system://constraints operational-parameters section in mcp.rs, spawn).",
        "acceptance": [
          "On POST reject with an active implementing agent that does not complete: the agent is still Active within the grace window and has the shutdown message in its inbox; after 60 seconds its process is killed (kill_process invoked on the compute target), its status is Stopped, its JWT is rejected on next use, and its worktree teardown happens after the kill. An agent that completes within the grace window is marked Idle.",
          "A repo with max_agent_iterations unset in a workspace with max_agent_iterations=3 reaches the ceiling (task Blocked, agent Failed, TaskBlocked message to the repo orchestrator) after 3 iterations, not 10; with both unset the ceiling remains 10.",
          "Workspace max_agent_iterations is settable and readable via the workspace API and persisted durably (survives restart)."
        ],
        "negative_probe": "Spawn an agent on a task whose spec is then rejected, keep the agent from completing, and inspect state 5 seconds after reject. At the base this fails: the handler has already synchronously transitioned the agent to Stopped (api/specs.rs:1567 transition_status(Stopped) inside the cancellation loop) and torn down its worktree (cleanup_worktrees_for_task at specs.rs:1626) — 'grace_period_secs': 60 exists only as payload text sent to an already-Stopped agent, no timer, no kill_process, no JWT revocation runs on this path (unlike the real budget grace timer at api/budget.rs BUDGET_GRACE_PERIOD_SECS), so 'still Active within the grace window, killed at expiry, token revoked' cannot hold. Second probe: set workspace max_agent_iterations=3 with the repo field unset and drive gate failures — at the base the ceiling resolves repo → server default 10 (ralph_loop.rs:410-411 has no workspace tier; the Workspace entity has no max_agent_iterations field), so the task is not Blocked at 3 iterations.",
        "dependencies": [],
        "existing_task": "task-300"
      },
      {
        "key": "persona-constraints-delivery",
        "title": "Deliver persona operational fields in the system://constraints prompt",
        "covers": [
          "47d309e09aac1e2e04d334d0"
        ],
        "required_behavior": "Existing task-303 contract, unchanged. Per agent-runtime.md §5: 'system://constraints delivers budget limits and operational parameters from the persona's operational fields.' prompt_constraints (crates/gyre-server/src/mcp.rs) and the spawn-time constraints injection must include a persona operational-parameters section sourced from the persona bound to the agent — model, temperature, max_tokens, and budget — read from real storage via the canonical persona surface established by the registry replacement (the meta-spec registry persona kind carrying the Persona operational fields, once /api/v1/personas is removed by task-298, on which this task depends). The existing sections (workspace budget, trust level, scope boundaries, agent lifetime, iteration ceiling) remain; agents with no bound persona report that explicitly rather than fabricating defaults. The values must be the stored persona's values — a change to the persona's operational fields is reflected on the next constraints read or spawn.",
        "acceptance": [
          "For an agent spawned with a persona whose operational fields are set (e.g. model X, temperature 0.2, max_tokens 8192, budget 500000), the system://constraints MCP prompt contains those exact values read from storage.",
          "Editing the bound persona's operational fields changes the served constraints on the next read/spawn (no cached or hardcoded stand-ins).",
          "An agent with no bound persona receives an explicit 'no persona bound' statement in the operational-parameters section, not fabricated defaults."
        ],
        "negative_probe": "Set a bound persona's max_tokens to a distinctive value (e.g. 12345) in storage, then read system://constraints for that agent and assert the value appears. At the base this fails: prompt_constraints (mcp.rs) assembles only scope boundaries, trust level, workspace budget, agent lifetime budget and the iteration ceiling — the Persona operational fields (model, temperature, max_tokens, budget) are consumed by no execution path anywhere in the server (only the CRUD DTOs reference them), so the distinctive value is absent.",
        "dependencies": [
          "task-298"
        ],
        "existing_task": "task-303"
      },
      {
        "key": "amendment-table-docs-and-binding",
        "title": "Carry out the remaining amendment-table rows, the Agent task-binding row, and the section close-out",
        "covers": [
          "a40446f46dbb0536f3c30ed0"
        ],
        "required_behavior": "New delta task, the single owner of the agent-runtime.md 'Relationship to Existing Specs' amendment-table gap. Every amendment row is owned exactly once: the code-side rows belong to existing task-298 'Remove orchestrator agent-spawning and carry out the spec amendment table' (this task's declared dependency — gyre_spawn_worker AND gyre_spawn_repo_orchestrator removal per that task's plan-carried extension, /api/v1/personas route removal with registry-carried operational fields, the ABAC 'archive' action, ralph-loop.md supersession cleanup, hierarchy-enforcement.md §4 route-table/tenant-filter amendments); the meta-spec-set registry-replacement clauses belong to satisfied task-302 (also a declared dependency — endpoint retirement, meta_specs_used-based drift detection, content_hash snapshot tuples); the rows the revision-4 audit dispositions satisfied need no work (TaskType/order/depends_on/parent_task_id; MessageKind::SpecApproved in code; Workspace.compute_target_id; AgentStatus enum; spec-lifecycle terminology; MergeAttestation.meta_specs_used population; meta_spec/compute_target resolver resources; the Persona required/prompt/content_hash fields via the unified MetaSpec registry). The Phase 2 and Phase 3 boundary violations (the two orchestrator-tier MCP spawn tools) are owned by task-298 in this plan, not by this task — no overlap. This task's own rows: (1) Supersede row 'docs/agent-protocol.md — consolidated into this spec': retire docs/agent-protocol.md — verify every requirement it documents is owned by the canonical spec (agent-runtime.md is the single lifecycle authority); relocate still-accurate operational detail that belongs to no canonical spec (exact container env vars, gate protocol mechanics, and the WS-transport and terminal-worktree semantics landed by the now-complete tasks task-310 (websocket-agent-transport) and task-269 (terminal-worktree-cleanup) — both already present in the file: the 'Worktree teardown is deferred to terminal completion' section and the 'WebSocket primary transport' section) to the appropriate existing docs (docs/server-config.md / docs/development.md), and update every reference (AGENTS.md Documentation Index, docs/api-reference.md cross-references, web/CLI help) with no dead links and no second lifecycle narrative; never edit agent-runtime.md's normative content. RETIREMENT SEQUENCING (correction of the revision-3 draft, which cited the misnumbered tasks task-311/task-272 as unfinished prerequisites): the two tasks formerly contracted to edit docs/agent-protocol.md are task-310 and task-269, and both are complete at this base with their content landed in the file — no unfinished plan edits it, so the retirement needs no external dependency beyond this plan's own task-298 and task-302; this task relocates the landed content rather than deleting content yet to be written. The assigned ci-docs-release plan's docs-site task (task-321) builds one rendered page per file currently in docs/ — its acceptance is source-dynamic by contract, so the retirement shrinks its page set by design; record the page-list change in this task's close-out so the task-321 verification is not surprised by the delta (no dependency edge — the docs-site task consumes docs/ dynamically and needs no change for the retirement). (2) message-bus.md MessageKind amendment: document SpecApproved in specs/system/message-bus.md — Event tier, server-only, payload {repo_id, spec_path, spec_sha, approved_by, approval_id}, fired by the spec approval handler, distinct from SpecChanged (which fires on push, before approval); the workspace orchestrator subscribes to SpecApproved — matching the existing code (gyre-common/src/message.rs). Doc-only; the active message-bus plan (task-255) owns record-activity-fail-closed and dead-inbox-sweep in different sections of that document — no overlap. (3) abac-policy-engine.md §Resource attributes: add meta_spec (attributes scope, scope_id, kind) and compute_target (attribute tenant_id) to the resource type list and add archive to the action attribute table, matching the resolver and the post-task-298 code state. (4) human-system-interface.md §5: amend the attestation bundle schema to include the meta_specs_used array with full content SHAs, matching the production MergeAttestation. (5) meta-spec-reconciliation.md: replace the retired PUT/GET /api/v1/workspaces/{id}/meta-spec-set endpoint text (and the tenant twin) with the registry model this spec mandates — required flags plus spec-level bindings via the meta-spec API, reconciliation triggered on required-meta-spec approval, MetaSpecSnapshot as {meta_spec_id, kind, content_hash, version} tuples — removing the doc's contradiction with the retired code (the endpoints 404 in production while the doc still specces them). Doc-only and limited to the retired-endpoint rows; the assigned meta-spec-reconciliation plan (task-332) owns the registry-parity/repo-scope/blast-radius/candidate-scoping/drift-policy/sweep/observability code surfaces of that document — no overlap with its deliverables. (6) Agent-entity task-binding row ('Add task_id: Id field — every agent is bound to exactly one task (the system creates the binding mechanically)'): make the binding non-optional and mechanically enforced for every task-bound agent kind — the spawn core fails closed (hard error, no agent record persisted) when creating a worker (bound to its Implementation task), repo orchestrator (bound to the Delegation/Coordination task being processed), gate reviewer (bound to the MR's source task), or interrogation agent (bound to the target agent's task) without a resolvable binding; schema migration at the next free 6-digit sequence number with the port contract enforced in every adapter including the mem adapter. The workspace orchestrator is taskless by §1 Phase 2's own definition (an inbox-processing agent that creates tasks and is bound to none), so the row's 'every agent' wording conflicts with the canonical spec's own Phase 2 — resolve that conflict through the spec lifecycle (propose the one-line amendment scoping the row to task-bound agent kinds, requiring human approval), never by fabricating a binding or silently shipping the contradiction; this proposal targets the amendment-table row, a different section from the §1 single-trigger amendment proposed by task-297 in this plan — the two proposals must be coordinated (one spec-edit branch or two non-conflicting branches) but do not overlap. (7) Section close-out, scoped exactly to this task's declared dependencies (task-298, task-302) and its own rows (1)-(6): verify every amendment-table row is carried out — the task-298 deliverables (gyre_spawn_worker and gyre_spawn_repo_orchestrator absent with no orchestrator-creatable agent records via MCP; /api/v1/personas 404 with persona CRUD/resolution via /api/v1/meta-specs?kind=meta:persona carrying the operational fields and required/prompt/content_hash/version; archive action live in the ABAC middleware with routes registered and exemption entries removed; ralph-loop.md retired with zero live cross-references; hierarchy-enforcement.md route-table/tenant-filter/storage-note amendments), the task-302 deliverables (workspace/tenant meta-spec-set endpoints 404/405 with no production writer; merge-time drift enforcement comparing meta_specs_used against current required versions), this task's rows (1)-(6), and the already-satisfied audit rows listed above — recording the task-321 docs-site page-list change as a close-out note.",
        "acceptance": [
          "docs/agent-protocol.md no longer exists; no file in specs/ or docs/ duplicates the agent lifecycle narrative; every former reference (AGENTS.md docs index, docs/api-reference.md cross-references, web/CLI help) resolves to agent-runtime.md or the relocated operational doc (grep 'agent-protocol.md' returns no live references); the WS-transport content landed by task-310 and the terminal-worktree-cleanup semantics landed by task-269 are present in the relocated docs, not lost by the retirement (both tasks are complete at the base, so their content is in the file being relocated).",
          "specs/system/message-bus.md documents SpecApproved with tier/payload/origin matching gyre-common/src/message.rs; specs/system/abac-policy-engine.md names the meta_spec and compute_target resource types and the archive action; specs/system/human-system-interface.md §5 includes the meta_specs_used array; specs/system/meta-spec-reconciliation.md no longer specces the retired meta-spec-set endpoints.",
          "Creating an agent record for a task-bound kind (worker, repo orchestrator, gate reviewer, interrogation) without a resolvable task binding fails closed with no agent record persisted; interrogation agents carry the target agent's task binding; the workspace-orchestrator exception is recorded as a spec-lifecycle amendment pending human approval, not a silent deviation.",
          "The close-out checklist passes for every amendment-table row, each row traceable to task-298, task-302, this task's rows (1)-(6), or an already-satisfied audit row — no row unaccounted — and the close-out notes the task-321 docs-site page-list change caused by the retirement."
        ],
        "negative_probe": "At the base every close-out assertion fails provably: grep SpecApproved specs/system/message-bus.md returns nothing (the kind exists only in code, gyre-common/src/message.rs) while the doc still lists only SpecChanged; docs/agent-protocol.md exists and documents the full lifecycle this spec claims to consolidate (304 lines, including the task-310 WS-transport and task-269 worktree-teardown content to be relocated); specs/system/human-system-interface.md §5 lacks meta_specs_used while the production MergeAttestation carries it; specs/system/meta-spec-reconciliation.md still specces PUT /api/v1/workspaces/{id}/meta-spec-set (line 246) while the endpoint 404s in production — the doc contradicts the deployed code. For the binding row: spawn an interrogation agent (POST /api/v1/agents/spawn, agent_type=interrogation) and read its record — current_task_id is None (gyre-domain/src/agent.rs:105, a Nullable column) and orchestrator.rs explicitly asserts task_id null for orchestrators, so no non-optional mechanical binding exists anywhere; the fail-closed taskless-spawn probe cannot pass at the base.",
        "dependencies": [
          "task-298",
          "task-302"
        ],
        "existing_task": null
      }
    ]
  },
  "assigned_names": {
    "approval-single-trigger": "task-297",
    "orchestrator-spawn-surface-removal": "task-298",
    "spec-rejection-grace-iteration-tier": "task-300",
    "persona-constraints-delivery": "task-303",
    "amendment-table-docs-and-binding": "task-349",
    "_publication": "task-350",
    "_base": "973068a86ab3154b6cba448fda9369125326cab7"
  }
}

## Acceptance Criteria

- [ ] Every planned task faithfully implements its assigned canonical requirements.
- [ ] No duplicate active assignment or missing dependency is introduced.
- [ ] No product behavior, spec requirement, coverage status or enforcement is changed.
- [ ] New tasks remain not-started until this backlog publication merges.

## Shipped

Backlog publication only: task-349 (amendment-table-docs-and-binding) and this task-350 document carrying the reviewed plan; the amendment-table-docs-and-binding section binding added to specs/task-spec-inputs.json. Existing tasks (297, 298, 300, 303) and every prior binding are preserved untouched — their contract revisions are carried by this document's plan text for the executing workers. No product behavior, spec, coverage status, or gate changed.

### Merge reconciliation

The published candidate (d59d94b9) conflicted with current main (f3ca7db3, which merged the dependency-graph backlog task-341/task-342). Resolved specs/task-spec-inputs.json as a union of both sides' appended backlog bindings — the agent-runtime amendment-table-docs-and-binding entry and the dependency-graph dep-edge-orphaned-on-target-archive-delete entry are both retained; JSON validity verified with python3 json.load. task-349.md is byte-identical to the published candidate; task-350.md differs only in the progress field (reset to ready-for-review for this review round). No Rust or web artifacts were affected, so no rebuild was needed.
