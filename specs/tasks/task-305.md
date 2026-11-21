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
  "plan_id": "6a0d0dc0bb142af9f5ac4d1be1c142f795b9b34555ddd9a114ef1f10f0a59e6d",
  "audit": {
    "base": "678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce",
    "requirements": "4a3f940495a85ceb75c7101b9ed34331a65dd5a1b2a9c726787027d5af06fdd4",
    "sections": [
      {
        "id": "4dd0880659a1055f66316a5d",
        "decision": "context",
        "requirement_quote": "This spec defines the complete agent lifecycle — from spec approval through task completion — including compute target selection, prompt assembly from meta-specs, the Ralph loop, and budget enforcement.",
        "detail": "Preamble: scope and consolidation statement (supersedes ralph-loop.md, platform-model.md §4, agent-protocol.md). No implementable requirement of its own; the normative content lives in §1–§5 and the amendments table, each audited under its own heading.",
        "evidence": []
      },
      {
        "id": "6387a8dfb19b30fa355262b9",
        "decision": "context",
        "requirement_quote": "Agents are work executors. Humans never spawn agents directly — the system spawns them in response to approved specs.",
        "detail": "Descriptive overview plus the end-to-end flow diagram. Every step of the diagram is specified normatively in §1 child headings (The Signal Chain, Phases 1–8) and audited there; the single-trigger property is accounted under 'The Signal Chain' (where its gap is recorded). No independent requirement to audit at this level.",
        "evidence": []
      },
      {
        "id": "5af6c76eb6471b7cb8ee0014",
        "decision": "context",
        "requirement_quote": "1. Agent Lifecycle",
        "detail": "Structural parent heading with no body text of its own; its requirements are the child headings 'The Signal Chain' and 'Phase 1' through 'Phase 8', each audited and decided individually below. Counting them here would double-account the same requirements.",
        "evidence": []
      },
      {
        "id": "1bd201625dc5fbe0164ced05",
        "decision": "gap",
        "requirement_quote": "Spec approval is the **single trigger** for all agent work. Every path to implementation goes through human approval:",
        "detail": "Requires that spec approval is the ONLY trigger for agent implementation work ('There is no other trigger. The SpecApproved event is the universal starting signal.'). The approval side is real: the approve handler records the ledger row and emits SpecApproved, modified-spec pushes reset approval_status to Pending (spec_registry sync) and revoke approvals via the post-receive hook, and untyped push-hook tasks never spawn agents. However, the single-trigger property is mechanically violated at the task layer: the MCP tool gyre_create_task and the HTTP task-create handler both accept task_type \"implementation\" with no precondition that an approved spec exists, and the Phase-4 scheduler spawns worker agents for ANY Backlog task carrying task_type Implementation — it never checks that the task derives from an approved spec. POST /api/v1/agents/spawn additionally spawns worker agents directly for authenticated callers. Implementation work can therefore start with no human spec approval anywhere in the chain.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"enum\": [\"implementation\", \"delegation\", \"coordination\"],",
            "detail": "The gyre_create_task MCP tool schema lets any authorized caller create a task with task_type \"implementation\"; the handler maps it to TaskType::Implementation with no spec-approval precondition, so the universal-approval gate is bypassable at task creation."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "continue; // Delegation/Coordination → orchestrators; untyped → never spawned",
            "detail": "The Phase-4 scheduler scan spawns workers for every Backlog task with task_type Implementation regardless of how the task was created; the scan checks repo status, ordering, max_agents and budget but never checks that the task references an approved spec."
          },
          {
            "file": "crates/gyre-server/src/api/tasks.rs",
            "quote": "if let Some(tt) = req.task_type {\n        task.task_type = Some(parse_task_type(&tt)?);\n    }",
            "detail": "The HTTP task-create handler accepts task_type directly from the request body with no approval precondition; a task created this way with type \"implementation\" is picked up by the scheduler and spawns an agent."
          },
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": ".route(\"/api/v1/agents/spawn\", post(spawn::spawn_agent))",
            "detail": "A direct agent-spawn REST surface exists for authenticated callers (used legitimately for interrogation agents per Phase 8, but it also spawns task-bound worker agents), so implementation agents can be created without any spec approval event."
          },
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "existing.approval_status = ApprovalStatus::Pending;",
            "detail": "Positive wiring for the modified-spec rule: when a spec push changes the blob SHA, sync_spec_ledger resets approval_status to Pending (auto_invalidate_on_change default true), so a modified spec does need re-approval — that part of the signal chain is enforced."
          }
        ]
      },
      {
        "id": "f7f7d487735bb8a3495a5ef4",
        "decision": "satisfied",
        "requirement_quote": "1. Records the approval in the spec approval ledger (SHA + approver + timestamp)",
        "detail": "The approve handler (POST /api/v1/specs/:path/approve) records a durable SpecApproval ledger row (spec_sha, approver_id, signature, approved_at set via spec_approvals.approve) plus an approval-history event, then — only on the transition INTO Approved — emits a MessageKind::SpecApproved message on the bus with exactly the specced payload {repo_id, spec_path, spec_sha, approved_by, approval_id} (plus workspace_id for server-side routing) to Destination::Workspace, and invokes signal_chain::on_spec_approved. MessageKind::SpecApproved is defined server-only and documented as distinct from SpecChanged (which fires on push).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "\"failed to record spec approval in ledger: {e}\"",
            "detail": "The approval handler creates the SpecApproval ledger entry (spec_sha + approver + timestamp via approve()) before any signal is emitted; failure to persist the ledger aborts the approval."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "crate::signal_chain::on_spec_approved(&state, &payload).await;",
            "detail": "After emitting the bus message, the handler routes the signal into the server-side orchestrator chain — Phase 1 step 3 (workspace orchestrator receives SpecApproved) is wired from the approval handler itself."
          },
          {
            "file": "crates/gyre-common/src/message.rs",
            "quote": "/// Emitted when a human approves a spec (`POST /api/v1/specs/:path/approve`).",
            "detail": "MessageKind::SpecApproved is a defined message kind, documented server_only with the exact specced payload and explicitly distinct from SpecChanged (fires on push, before approval)."
          }
        ]
      },
      {
        "id": "6d7302e3c6a23af8c4020c20",
        "decision": "satisfied",
        "requirement_quote": "The workspace orchestrator is an **LLM agent** with the `workspace-orchestrator` persona. It is spawned on demand — not long-lived.",
        "detail": "On SpecApproved the server takes the per-workspace registry mutex (concurrent approvals serialize), ensures exactly one live workspace orchestrator — spawned on demand via spawn_workspace_orchestrator_core with the workspace-orchestrator persona resolved fail-closed, a workspace-scoped JWT, and a process launch on the workspace compute target (409 if one is already live) — delivers the SpecApproved message to its inbox (Destination::Agent after the Workspace-destined bus emission), then performs the specced Phase-2 job: reads the approved spec content at the pinned SHA, queries spec_links for cross-repo/cross-workspace dependents, creates the Delegation task with spec_ref path@sha for the spec's own repo, Coordination tasks per dependent repo, and priority-4 CrossWorkspaceSpecChange notifications for dependent workspace Admin/Owner members. It only creates tasks and notifications — never spawns agents. Deviations (noted, not disqualifying): the Phase-2 analysis/delegation is driven by deterministic server code (run_workspace_orchestrator) rather than an LLM judgment call — the specced steps 1–5 are mechanical spec_links queries and task creation, and the LLM is consulted for judgment in Phase 3 decompose/coordinate; and tasks are created via the tasks port (state.tasks.create) rather than the task.create MCP tool an external orchestrator process would use.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "let orchestrator = match ensure_workspace_orchestrator(state, &workspace_id).await {",
            "detail": "SpecApproved handling spawns a workspace orchestrator on demand when none is live — the spec's 'spawned on demand — not long-lived' model — under the per-workspace registry lock that serializes concurrent approvals."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "/// The orchestrator only creates tasks — never spawns agents.",
            "detail": "The workspace orchestrator run creates Delegation/Coordination tasks and notifications only; inspected call paths confirm no agent spawning, no code writes, no compute-target interaction from the orchestrator run."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "/// - a Delegation task for the spec's own repo (`spec_ref: path@sha`)",
            "detail": "The delegation task is created with the spec reference pinned to the approved SHA (task.spec_ref = Some(spec_ref) at the creation site), and Coordination tasks plus priority-4 CrossWorkspaceSpecChange notifications for cross-workspace dependents are produced by the same run."
          },
          {
            "file": "crates/gyre-server/src/api/orchestrator.rs",
            "quote": "\"a workspace orchestrator is already active for this workspace\".to_string(),",
            "detail": "Exactly-one-active semantics: the spawn core returns 409 when a live workspace orchestrator exists, and the persona is resolved fail-closed (validate_persona + resolve_orchestrator_persona) with a workspace-scoped JWT minted for the agent."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "Some(TaskType::Coordination) => assess_coordination(state, repo, task).await?,",
            "detail": "Coordination tasks are handled by the same repo-orchestrator mechanism as delegation tasks — the scheduler detects them and the repo orchestrator assesses the dependency-change impact, per the spec's coordination-processing paragraph."
          }
        ]
      },
      {
        "id": "15aec83b58b3494602d31ce8",
        "decision": "gap",
        "requirement_quote": "The repo orchestrator **only creates tasks**. It never spawns agents. The system handles the task→agent mapping mechanically.",
        "detail": "The decomposition machinery is implemented: the task scheduler detects Delegation tasks by task_type, spawns the repo orchestrator under a per-repo mutex (repo-orchestrator persona, repo-scoped JWT, one-live 409); decompose_delegation fails hard on an unresolvable spec_ref, reads the spec at the pinned SHA, uses LLM decomposition judgment when an LLM is configured (deterministic fallback otherwise), creates Implementation sub-tasks carrying spec_ref, parent_task_id, order and depends_on, and marks the delegation task Done. GAP: the 'never spawns agents' boundary is contradicted by a live production mechanism — the dispatched MCP tool gyre_spawn_worker lets any repo-orchestrator JWT spawn a worker agent directly through spawn_agent_core (worktree, scoped JWT, compute-target resolution). That is exactly the orchestrator-spawns-agents capability the spec removes (see also the amendments table: 'Remove agent.spawn MCP tool — orchestrators do not spawn agents'); the mechanical scheduler is not the only task→agent path available to an orchestrator. Secondary mechanism deviation: sub-tasks are created via the tasks port (state.tasks.create) rather than the task.create MCP tool.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"description\": \"Spawn a worker agent for an implementation task in the caller's repo. Requires a repo-orchestrator JWT. Delegates to the standard agent spawn flow (worktree, scoped JWT, compute target resolution).\",",
            "detail": "gyre_spawn_worker is advertised in the MCP tool catalog specifically to repo-orchestrator JWT holders — the repo orchestrator can spawn worker agents directly, contradicting 'It never spawns agents. The system handles the task→agent mapping mechanically.'"
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"gyre_spawn_worker\" => handle_spawn_worker(&state, &args, &auth).await,",
            "detail": "The tool is dispatched in the production tools/call path (not dead catalog text); handle_spawn_worker builds a SpawnAgentRequest and calls spawn_agent_core under the repo-orchestrator's own authentication."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "Some(TaskType::Delegation) => decompose_delegation(state, repo, task).await?,",
            "detail": "Positive wiring: the scheduler routes Delegation tasks to the repo orchestrator's decomposition, which creates ordered Implementation sub-tasks and completes the delegation task — the specced Phase-3 flow itself is real."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "sub.task_type = Some(TaskType::Implementation);",
            "detail": "Sub-tasks are created with the Implementation discriminator together with spec_ref, parent_task_id, order and depends_on (the same creation block sets sub.spec_ref/sub.parent_task_id/sub.order/sub.depends_on), via the tasks port rather than the task.create MCP tool — a mechanism deviation in how sub-tasks are created, with all specced fields present."
          }
        ]
      },
      {
        "id": "eb6edb7ecfd0f65611163d8c",
        "decision": "satisfied",
        "requirement_quote": "**Preconditions:** Checks repo status (rejects if `Archived` per `repo-lifecycle.md` §4), checks `max_agents` limit (queues task — the scheduler re-checks periodically as agents complete, does not reject or return 429), checks workspace budget (rejects if exhausted — task stays `Backlog` with a `BudgetExhausted` flag)",
        "detail": "All ten mechanical spawn steps are production code in the scheduler cycle and the shared spawn core: the scan filters to Backlog tasks with task_type Implementation (untyped push-hook tasks never spawn); archived repos defer the task (stays Backlog); depends_on and order are checked before claiming; max_agents is enforced by counting Active agents with repo → workspace fallback and the task is QUEUED (stays Backlog, no 429); workspace-budget exhaustion leaves the task Backlog with the 'budget-exhausted' label (cleared on successful claim); the claim sets InProgress; the agent record is created Active; an EdDSA-signed worker JWT scoped to the repo is minted (mint_worker_scoped → shared EdDSA encode with kid, JWKS-published); the meta-spec prompt set is assembled and injected (GYRE_META_SPEC_PROMPT + set SHA); a git worktree is created on the task branch; the agent is provisioned on the workspace compute target; env vars GYRE_SERVER_URL/GYRE_AUTH_TOKEN/GYRE_BRANCH/GYRE_TASK_ID/GYRE_REPO_ID are injected. AgentStatus is a five-variant enum (Active/Idle/Failed/Stopped/Dead) and the stale-agent detector marks heartbeat-expired agents Dead.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "\"signal-chain: repo at max_agents; task queued\"",
            "detail": "At the max_agents limit the scheduler queues the task (stays Backlog, re-checked on later cycles) rather than rejecting — exactly the specced no-429 queuing behavior."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "const BUDGET_EXHAUSTED_LABEL: &str = \"budget-exhausted\";",
            "detail": "Budget exhaustion leaves the task in Backlog with an observable BudgetExhausted flag (a task label), cleared when a later cycle successfully claims and spawns it."
          },
          {
            "file": "crates/gyre-server/src/auth.rs",
            "quote": "let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::EdDSA);",
            "detail": "Agent JWTs (worker and orchestrator tiers share this encode path) are EdDSA-signed; mint_worker_scoped mints the repo-scoped worker token used by the scheduler spawn."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "container_env.insert(\"GYRE_META_SPEC_PROMPT\".to_string(), meta_prompt);",
            "detail": "Step 7 (assemble the prompt set per §2) is wired into spawn: the assembled meta-spec prompt and its set SHA are injected into the agent environment."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "container_env.insert(\"GYRE_SERVER_URL\".to_string(), state.base_url.clone());",
            "detail": "Step 10 (inject environment variables): GYRE_SERVER_URL, GYRE_AUTH_TOKEN, GYRE_BRANCH, GYRE_TASK_ID and GYRE_REPO_ID are all injected into the container environment at spawn."
          },
          {
            "file": "crates/gyre-domain/src/agent.rs",
            "quote": "/// Agent status enum per agent-runtime.md §1 Phase 4.",
            "detail": "AgentStatus is defined with the five specced variants (Active, Idle, Failed, Stopped, Dead) and a guarded transition state machine."
          },
          {
            "file": "crates/gyre-server/src/stale_agents.rs",
            "quote": "const HEARTBEAT_TIMEOUT_SECS: u64 = 60;",
            "detail": "The stale-agent detector reaps heartbeat-expired agents to Dead (system orchestrators are heartbeat-refreshed server-side instead — a documented nuance), started from main alongside the scheduler."
          }
        ]
      },
      {
        "id": "de73e06638559fcca67b3c5d",
        "decision": "gap",
        "requirement_quote": "The agent has no knowledge of other agents, the merge queue, or the broader system state. It sees its task, its spec, and its repo.",
        "detail": "The worker workflow itself is real: the agent runner reads its task via MCP, edits/commits/pushes, and signals completion (gyre_agent_complete MCP tool and the REST twin both create the completion MR). But the normative knowledge boundary is mechanically violated, not just prompt-level: the MCP resources/read layer advertises and serves 'Workspace Agents' (agents://) and 'Merge Queue' (queue://) resources to every authenticated MCP session with no per-role or per-scope gate at the dispatch. An unscoped agents:// read lists ALL Active/Idle agents across ALL workspaces (state.agents.list() with no caller-scope filter — a cross-workspace enumeration leak), and queue://{repo_id} returns any repository's merge-queue entries. A worker agent can therefore acquire exactly the knowledge of other agents and the merge queue that this section forbids. No spec amendment authorizes prompt-level-only isolation. (This was blocking review feedback at the prior audit base; the cited code is unchanged at this base.)",
        "evidence": [
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"description\": \"List active agents in a workspace. URI: agents://{workspace_id} or agents:// for all.\",",
            "detail": "resources/list advertises the agents:// and queue:// resources to every MCP session including worker agent tokens — the resource layer itself offers the forbidden knowledge."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "let agents_result = if workspace_id.is_empty() || workspace_id == \"*\" {",
            "detail": "The unscoped agents:// read falls through to state.agents.list() with no caller-scope filter — a worker JWT can enumerate Active/Idle agents across every workspace, far beyond 'its task, its spec, and its repo'."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "let result = handle_resource_read(&state, &auth, uri).await;",
            "detail": "The resources/read dispatch calls handle_resource_read with no role gate, so worker-scoped JWTs are served the agents:// and queue:// resources like any other caller."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "} else if let Some(repo_id) = uri.strip_prefix(\"queue://\") {",
            "detail": "queue://{repo_id} returns merge-queue entries for any repository the caller names — the merge-queue state the spec says a worker agent must not see."
          }
        ]
      },
      {
        "id": "72fc3b6af856353035268add",
        "decision": "satisfied",
        "requirement_quote": "Server creates a MR from the agent's branch to the default branch",
        "detail": "Both completion surfaces (MCP gyre_agent_complete and REST POST /agents/:id/complete) route to create_mr_for_completion, which propagates the task's pinned spec_ref (path@sha from the spec ledger SHA) and targets the repo's default branch, then call ralph_loop::on_agent_mr_created which enqueues the MR — triggering gate execution automatically. Gate types are real: TestCommand/LintCommand gates run commands with exit-0 pass semantics; the AgentReview gate spawns a reviewer agent with the review persona that evaluates the MR diff against the spec at the pinned SHA (fail-closed when spec_ref does not resolve); the TraceCapture gate is observational and always passes; results are persisted as GateResult rows recorded on the MR.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "crate::ralph_loop::on_agent_mr_created(&state, &agent, &mr).await;",
            "detail": "Completion creates the MR and immediately routes it into the merge queue, which triggers gate execution — 'gates run automatically' is wired from both completion surfaces."
          },
          {
            "file": "crates/gyre-server/src/gate_executor.rs",
            "quote": "GateType::TestCommand | GateType::LintCommand => {",
            "detail": "Test and lint gates dispatch to run_command, which maps exit status 0 to GateStatus::Passed and non-zero to Failed — the specced exit-0 pass semantics."
          },
          {
            "file": "crates/gyre-server/src/gate_executor.rs",
            "quote": "\"spec_ref '{spec_ref}' does not resolve: '{path}' absent at SHA {sha}\"",
            "detail": "The spec review gate evaluates the MR diff against the spec at the SHA pinned in spec_ref and fails closed when the pin cannot be resolved — the reviewer agent path runs with the review persona's system prompt."
          },
          {
            "file": "crates/gyre-server/src/gate_executor.rs",
            "quote": "// TraceCapture always passes — capture failure is observational.",
            "detail": "The TraceCapture gate captures OTel spans during test execution and always passes, exactly as specced ('observational, always passes')."
          }
        ]
      },
      {
        "id": "e60f62cd06826e5e4de9decf",
        "decision": "gap",
        "requirement_quote": "Agents have 60 seconds to commit current work (same grace period as budget exhaustion), then are killed and marked `Stopped`",
        "detail": "The Ralph loop core is implemented: pass path merges and marks the task Done, agent Idle, token revoked; fail path writes per-gate failure messages to the agent's inbox, closes the failed MR, and re-spawns the same agent entity with the same persona/task and a FRESH session (no conversation history — new JWT, re-assembled prompt set, reused worktree); iteration ceiling with repo-level max_agent_iterations falling back to a server default of 10; at the ceiling the task is Blocked, the agent Failed, the token removed, and a TaskBlocked message is routed to the repo orchestrator (spawning one when none is live). Spawn-failure policy: 3 retries at 1s/5s/30s, then Blocked plus a priority-1 'Compute target unavailable: {target_name}' notification, no fallback to a different target. TWO gaps: (1) Spec rejection mid-flight — the specced 60-second grace is decorative: the reject handler synchronously marks active agents Stopped and tears down their worktrees immediately; the 'grace_period_secs': 60 value exists only as a payload field in a shutdown message sent to an already-Stopped agent; there is no timer and no ComputeTarget::kill_process() call on this path (the budget path implements a real grace timer + kill via start_budget_grace_timer — the contrast proves the mechanism exists but is not applied here), and the agent's JWT is not revoked. (2) The iteration-default tier 'falls back to workspace default, then server default of 10' is missing its middle tier: resolution is repo.max_agent_iterations → DEFAULT_MAX_ITERATIONS (10); the Workspace entity has no max_agent_iterations field anywhere.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "let _ = agent.transition_status(gyre_domain::AgentStatus::Stopped);",
            "detail": "On spec rejection the agent is marked Stopped synchronously inside the task-cancellation loop — before any grace period could elapse — so 'agents have 60 seconds to commit current work, then are killed and marked Stopped' is not the implemented order of operations."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "\"grace_period_secs\": 60,",
            "detail": "The 60-second grace exists only as a payload field of the shutdown message delivered to the already-Stopped agent; no timer, no deferred kill via ComputeTarget::kill_process(), and no JWT revocation run on this path."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "// helper). The 60-second shutdown grace in the spec is for",
            "detail": "The code itself documents the deviation: worktrees are torn down immediately because 'the agents are stopped above' — an intentional code-versus-spec contradiction with no amending spec change."
          },
          {
            "file": "crates/gyre-server/src/api/budget.rs",
            "quote": "async fn start_budget_grace_timer(state: Arc<AppState>, workspace_id: Id, grace_secs: u64) {",
            "detail": "The budget-exhaustion path implements a real 60-second grace timer with deferred kill — the same grace the spec requires on spec rejection — proving the mechanism exists but is not applied to the rejection path."
          },
          {
            "file": "crates/gyre-server/src/ralph_loop.rs",
            "quote": ".unwrap_or(DEFAULT_MAX_ITERATIONS);",
            "detail": "Iteration resolution is repo.max_agent_iterations → DEFAULT_MAX_ITERATIONS (10) with no workspace tier; the Workspace struct has no max_agent_iterations field, so the specced 'falls back to workspace default, then server default of 10' middle tier does not exist."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "pub(crate) const SPAWN_RETRY_BACKOFF_SECS: [u64; 3] = [1, 5, 30];",
            "detail": "Positive wiring for the spawn-failure policy: 3 retries at 1s/5s/30s, then task Blocked, orchestrator notified, priority-1 'Compute target unavailable' notification, and no fallback to a different compute target."
          }
        ]
      },
      {
        "id": "90c6b120df4e54f175cc08de",
        "decision": "satisfied",
        "requirement_quote": "System-initiated agents use the same compute target as their workspace. They receive the same meta-spec prompt assembly (§2) — the distinction is in their persona and scope, not their runtime.",
        "detail": "All three system-initiated agent types are real: (1) Gate reviewer — manifest-declared AgentReview gates mint a scoped review:submit-only JWT for a real reviewer agent record and launch its runtime through the compute-target machinery, torn down after the verdict is submitted (single-shot). (2) Interrogation — spawned via POST /api/v1/agents/spawn with agent_type 'interrogation', a 30-minute JWT TTL (1800s), read-only (no worktree), with conversation context served to the spawned agent via the conversation:// MCP resource; policies cleaned up on completion. (3) Reconciliation — meta-spec approval (PUT with approval_status Approved, required meta-specs) triggers reconciliation sweeps that create Delegation tasks, which flow through the normal signal chain to worker agents and therefore the same Ralph loop, compute target and prompt assembly as worker agents.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/gate_executor.rs",
            "quote": "crate::auth::REVIEW_SUBMIT_SCOPE,",
            "detail": "Gate reviewers get a scoped review:submit-only JWT minted for a real agent record whose runtime is launched on compute-target machinery and torn down after the verdict — single-shot per the spec's lifecycle table."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "const INTERROGATION_JWT_TTL_SECS: u64 = 1800;",
            "detail": "Interrogation agents get the specced 30-minute JWT TTL; they are read-only (no worktree) and their conversation context is served via the conversation:// MCP resource, torn down when the conversation ends."
          },
          {
            "file": "crates/gyre-server/src/reconciliation.rs",
            "quote": "task.task_type = if delegation {",
            "detail": "Reconciliation creates Delegation tasks (Immediate strategy) that enter the normal signal chain — repo orchestrator → worker agents → Ralph loop — so reconciliation agents share the worker runtime, compute target and prompt assembly rather than being a separate mechanism."
          }
        ]
      },
      {
        "id": "ceca9704c11dbcb1f7b29597",
        "decision": "context",
        "requirement_quote": "2. Meta-Spec Prompt Assembly",
        "detail": "Structural parent heading with no body text of its own; its requirements are the child headings 'Meta-Specs Are Prompts' through 'API', each audited and decided individually below. Counting them here would double-account the same requirements.",
        "evidence": []
      },
      {
        "id": "a00a6b5082295555260842b2",
        "decision": "satisfied",
        "requirement_quote": "There is no separate \"prompt configuration.\" The meta-spec registry IS the prompt configuration.",
        "detail": "The meta-spec registry is the actual prompt source: MetaSpec entries carry prompt/version/content_hash (SHA-256 of prompt)/required for all four kinds (persona, principle, standard, process — the non-persona kinds being pure prompt text with no operational fields), full version history is retained in the meta_spec_versions table (DB-backed, content-addressable, not git-backed), and the assembled registry output is what every spawned agent receives as its system prompt (GYRE_META_SPEC_PROMPT + set SHA at spawn, re-assembled on re-spawn). Deviation noted (accounted under 'Relationship to Existing Specs'): the pre-existing Persona entity survives as a parallel registry with its own fields (system_prompt, model, temperature, max_tokens, budget) still bound to agents via Agent.persona_id — the spec's single-struct extension model is realized as two coexisting registries rather than Persona gaining the MetaSpec fields.",
        "evidence": [
          {
            "file": "crates/gyre-domain/src/meta_spec.rs",
            "quote": "pub prompt: String,",
            "detail": "The MetaSpec struct carries the prompt text plus version, content_hash (SHA-256) and required for every kind — the registry entry IS the prompt configuration."
          },
          {
            "file": "crates/gyre-adapters/migrations/2026-03-26-000032_meta_specs/up.sql",
            "quote": "UNIQUE(meta_spec_id, version)",
            "detail": "The meta_spec_versions table retains full immutable version history in the database — the DB-backed, content-addressable model the section requires (superseding git-backed meta-specs)."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "container_env.insert(\"GYRE_META_SPEC_PROMPT\".to_string(), meta_prompt);",
            "detail": "Editing a meta-spec genuinely changes what agents receive: the registry output is rendered into the agent system prompt at spawn (and re-rendered on Ralph-loop re-spawn), proving the registry is the prompt configuration in production."
          }
        ]
      },
      {
        "id": "6caa3e55951ef9950cbd621f",
        "decision": "satisfied",
        "requirement_quote": "Full version history is retained in a `meta_spec_versions` table.",
        "detail": "Registry levels are implemented as specced: Global/Tenant (org-wide) and Workspace scopes on MetaSpec entries, plus spec-level bindings as the third level; the MetaSpec struct carries id, kind, name, scope, scope_id, created_by, created_at, updated_at alongside prompt/version/content_hash/required; meta_spec_versions retains full history with UNIQUE(meta_spec_id, version); meta_spec_bindings stores spec-level selections with UNIQUE(spec_id, meta_spec_id); both SQLite and PostgreSQL adapters implement the stores; content_hash is SHA-256 computed from the prompt text (content-addressable, not git SHAs).",
        "evidence": [
          {
            "file": "crates/gyre-domain/src/meta_spec.rs",
            "quote": "pub prompt: String,",
            "detail": "The MetaSpec struct carries the extension fields (prompt, version, content_hash, required) plus id/kind/name/scope/scope_id/created_by/created_at/updated_at — the specced entity model for the non-Persona kinds."
          },
          {
            "file": "crates/gyre-adapters/migrations/2026-03-26-000032_meta_specs/up.sql",
            "quote": "CREATE TABLE meta_spec_versions (",
            "detail": "Full version history is a real DB table with immutable (meta_spec_id, version) uniqueness, populated by the adapter update path on every edit."
          },
          {
            "file": "crates/gyre-adapters/migrations/2026-03-26-000032_meta_specs/up.sql",
            "quote": "CREATE TABLE meta_spec_bindings (",
            "detail": "Spec-level bindings are stored as the third registry level, keyed by spec_id + meta_spec_id with pin columns."
          }
        ]
      },
      {
        "id": "54a514da8df9bf4d236e09ff",
        "decision": "gap",
        "requirement_quote": "**Who can set `required`:** Only admins at the meta-spec's own scope level — tenant admins for tenant meta-specs, workspace admins (Owner/Admin role) for workspace meta-specs.",
        "detail": "The injection semantics are implemented: required meta-specs are always injected (both tenant and workspace bands filter on required: Some(true)), cannot be opted out, and bypass nearest-scope resolution; Repo scope structurally cannot be required (the scope parser accepts only Global/Tenant/Workspace, and the Persona entity has no required flag at all); same-name/different-id collisions inject both, required first, as separate prompt sections. GAP: the scope-level grant is not implemented as specced — creating and updating registry entries (including flipping the required flag) is gated to the platform tenant Admin role for EVERY scope, so a workspace Owner/Admin cannot set required on a workspace-scoped meta-spec; only DELETE carries a full scope-aware authorization gate. The code comment claims 'The outer write gate already enforces this for every PUT', but that outer gate is tenant-Admin for all scopes — stricter than the spec for workspace meta-specs, i.e. the specced workspace-admin authority simply does not exist. (The privilege-escalation direction the spec warns about — a workspace admin marking a tenant meta-spec required — is prevented, but by over-restriction, not by scope-level enforcement.)",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "\"only Admin role may modify the meta-spec registry\".to_string(),",
            "detail": "The require_registry_admin helper gates POST and PUT on the tenant Admin role regardless of the meta-spec's scope — workspace Owner/Admin members cannot set required on workspace meta-specs, so the specced scope-level grant is absent for the update/create path."
          },
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "// The outer write gate already enforces this for every PUT.",
            "detail": "The required-flag handling cites an outer scope-level gate that does not exist as described: the outer gate is the tenant-Admin-only role check, so the comment's claim is inaccurate for workspace-scoped meta-specs."
          },
          {
            "file": "crates/gyre-server/src/prompt_assembly.rs",
            "quote": "required: Some(true),",
            "detail": "Positive wiring: required meta-specs are unconditionally injected in both the tenant band (Global + caller's Tenant) and the workspace band — the always-injected, cannot-opt-out semantics are real."
          }
        ]
      },
      {
        "id": "e97ea9a092758dc93c245e6e",
        "decision": "gap",
        "requirement_quote": "The spec author selects these via the UI (Specs view → Edit → Meta-spec bindings panel). The UI shows available meta-specs from the tenant and workspace registries, with required ones pre-selected and locked (cannot be removed).",
        "detail": "The binding model is real as an API: PUT/GET /api/v1/specs/:path/meta-spec-bindings performs a declarative full-set replace with pin-version-exists validation, a cross-workscope binding leak guard, and role gating; band 3 of prompt assembly consumes the bindings at pinned versions via version-history lookup. Two gaps: (1) Storage location — bindings live in a separate meta_spec_bindings DB table keyed by the bare spec path string, NOT 'stored in the spec registry alongside other spec metadata'; the spec ledger entry's SHA is the git blob SHA of the spec markdown file and does not cover bindings (this is the root cause of the broken stale-pin cascade, accounted under 'Stale Pin Detection'). (2) The specced UI bindings panel does not exist: no web component or API client references the bindings endpoint (the web meta-specs surface covers registry CRUD only), and the shipped locale string admits workspace-level drift tracking is 'available after the spec-bindings API is extended' — so spec authors have no UI to select bindings, pre-selected required entries, or locked required rows.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "spec_id: spec_path.clone(),",
            "detail": "Bindings are keyed by the bare spec path string in the side table — the ledger entry, its blob SHA and its approval status are separate structures, so bindings are not spec-registry metadata and pin changes have no effect on spec identity."
          },
          {
            "file": "web/src/locales/en.json",
            "quote": "\"drift_text_suffix\": \"Workspace-level drift tracking (repos built under older versions) is available after the spec-bindings API is extended.\",",
            "detail": "The shipped UI itself concedes the bindings surface is not integrated: no Specs-view bindings panel exists, and the drift text defers workspace-level tracking until the bindings API is 'extended'."
          },
          {
            "file": "crates/gyre-server/src/prompt_assembly.rs",
            "quote": "// Band 3: spec-level bindings at pinned versions. A binding that",
            "detail": "Positive wiring: the assembly consumes bindings at their pinned versions via the version-history lookup, so the binding model itself works end-to-end for prompt injection."
          }
        ]
      },
      {
        "id": "ea9967fc3c67f4c5ee136e41",
        "decision": "satisfied",
        "requirement_quote": "1. All REQUIRED tenant meta-specs (ordered by kind: persona → principle → standard → process)",
        "detail": "assemble_prompt_set implements the three bands in order — band 1: required tenant-level meta-specs (Global plus the caller tenant's Tenant-scoped entries), band 2: required workspace meta-specs, band 3: spec-level bindings at pinned versions — with kind_rank ordering persona → principle → standard → process inside each band and a deterministic tiebreak. Deduplication is by meta-spec id: a required meta-spec that also appears in spec-level bindings is included once, in the required band (the binding is treated as redundant). Unresolvable pins are skipped rather than silently rendered at the wrong version. All concatenated into the agent's system prompt context and injected at spawn (GYRE_META_SPEC_PROMPT), re-assembled on Ralph-loop re-spawn.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/prompt_assembly.rs",
            "quote": "MetaSpecKind::Persona => 0,",
            "detail": "kind_rank orders persona(0) → principle(1) → standard(2) → process(3) inside each band, with a deterministic (workspace, repo, title) tiebreak — the specced kind ordering."
          },
          {
            "file": "crates/gyre-server/src/prompt_assembly.rs",
            "quote": "// Band 2: required meta-specs scoped to this workspace.",
            "detail": "The three-band structure is explicit: required tenant band, required workspace band, then spec-level bindings — in the specced order."
          },
          {
            "file": "crates/gyre-server/src/prompt_assembly.rs",
            "quote": "if seen_ids.contains(&binding.meta_spec_id.to_string()) {",
            "detail": "Dedup by meta_spec_id: a binding whose meta-spec was already injected in a required band is skipped, so a required meta-spec re-listed in spec-level bindings is included exactly once, in the required section."
          }
        ]
      },
      {
        "id": "b0a63ae17c4db552f4dfab90",
        "decision": "satisfied",
        "requirement_quote": "**Attestation:** The merge attestation bundle records the full set of meta-spec SHAs used:",
        "detail": "Versioning: editing a meta-spec's content assigns ms.version += 1, recomputes content_hash as SHA-256 of the prompt, and resets approval_status to Pending (old versions retained in meta_spec_versions; specs pinning old versions are not auto-updated; metadata-only edits deliberately do not bump the version). Attestation: MergeAttestation carries meta_specs_used: Vec<MetaSpecUsed> with {id, kind, content_hash, version, required, scope}, populated at merge time from the spawn-time prompt-set record (the pinned set the agent actually ran under) and covered by the EdDSA attestation signature.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "ms.version += 1;",
            "detail": "Each content edit assigns a new version number and a new content_hash (SHA-256 of the prompt), resetting approval to Pending until re-approved — the specced versioning model."
          },
          {
            "file": "crates/gyre-domain/src/attestation.rs",
            "quote": "pub meta_specs_used: Vec<MetaSpecUsed>,",
            "detail": "MergeAttestation is amended with the meta_specs_used array carrying id/kind/content_hash/version/required/scope per entry."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": ".map(|rec| rec.meta_specs_used)",
            "detail": "The merge-time bundle is populated from the agent's spawn-time prompt-set record — the attested set is what the agent actually ran under, and the bundle is signed with the agent signing key."
          }
        ]
      },
      {
        "id": "9f95c49cf209205750a0177a",
        "decision": "gap",
        "requirement_quote": "Human updates the pin → spec metadata changes → spec SHA changes → spec needs re-approval",
        "detail": "Detection and notification are real: an hourly background job (meta_spec_stale_pin_check, also triggered on meta-spec edits) runs detect_stale_pins, compares every binding's pinned_version against the current version, dedups via KV keys, and creates priority-6 MetaSpecDrift notifications ('Meta-spec drift alert: ... Review and update pin.') routed to the workspace's Admin/Owner members. But the specced cascade never fires: updating a pin (put_spec_meta_spec_bindings) writes only the meta_spec_bindings side table and clears the stale-pin dedup keys — it never touches the spec ledger entry, its blob SHA, or its approval status. Because bindings are not part of the spec registry metadata (the ledger SHA is the git blob SHA of the spec markdown file), a pin update does NOT change the spec SHA, does NOT require re-approval, emits no SpecApproved event, and triggers no delegation task or re-implementation; the new pin takes effect only for agents spawned later under tasks that happen to reference the spec. The section's outcome — 'agents re-implement with new prompts' after a human acts on a drift alert — is therefore unreachable through the implemented mechanisms. (This was blocking review feedback at the prior audit base; the cited code is unchanged at this base.)",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "let prefix = format!(\"stale_pin:{}:\", spec_path);",
            "detail": "The pin-update handler's only side effects are the meta_spec_bindings side table and stale-pin dedup-key clearing — it never writes the spec ledger, so the pin update cannot change the spec SHA or reset its approval."
          },
          {
            "file": "crates/gyre-domain/src/spec_ledger.rs",
            "quote": "/// Git blob SHA of the spec file at HEAD.",
            "detail": "The ledger SHA is the git blob SHA of the spec markdown file — bindings are not part of it, so the specced 'spec metadata changes → spec SHA changes → spec needs re-approval' chain cannot fire on a pin update."
          },
          {
            "file": "crates/gyre-server/src/prompt_assembly.rs",
            "quote": "\"Meta-spec drift alert: '{}' uses {} v{}, but v{} is available. Review and update pin.\",",
            "detail": "Positive wiring for the detection half: the priority-6 drift notification with the specced message body is created for stale pins — the alert itself works; it is the post-alert cascade that is missing."
          },
          {
            "file": "crates/gyre-server/src/jobs.rs",
            "quote": "name: \"meta_spec_stale_pin_check\".to_string(),",
            "detail": "The background job runs the detection hourly, satisfying the 'background job (or check on spec access)' trigger for detection."
          }
        ]
      },
      {
        "id": "a787945d4c0f4330d64e8aca",
        "decision": "satisfied",
        "requirement_quote": "Gyre ships with default meta-specs seeded at first startup:",
        "detail": "seed_builtin_meta_specs runs at server startup (and defensively from the spawn/orchestrator/MCP cores), is idempotent (early return when the registry is non-empty), and seeds all nine specced entries: the seven personas (default-worker, workspace-orchestrator, repo-orchestrator, spec-reviewer, accountability, security, reconciliation), the conventional-commits Principle with required: true, and the test-coverage Standard — every one seeded with approval_status Approved (approved_by 'system'), matching the spec's pre-approved bootstrap semantics. The entries are ordinary registry rows, editable/clonable/replaceable by users.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "pub async fn seed_builtin_meta_specs(state: &Arc<AppState>) {",
            "detail": "The bootstrap seeding is invoked at startup (and defensively from the spawn cores), idempotent on a non-empty registry."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "\"conventional-commits\",",
            "detail": "The conventional-commits Principle is seeded with required = true (the seed tuple's third element), matching the bootstrap table's only required entry."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "approval_status: MetaSpecApprovalStatus::Approved,",
            "detail": "All bootstrap meta-specs are seeded pre-Approved (approved_by 'system'), including the test-coverage Standard and the seven personas."
          }
        ]
      },
      {
        "id": "917eb99ea432ee472c349487",
        "decision": "gap",
        "requirement_quote": "| `GET /api/v1/meta-specs` | GET | List meta-specs (filtered by `?scope=`, `?scope_id=`, `?kind=`, `?required=`). ABAC: scope_id extracted from query param for tenant/workspace authorization. |",
        "detail": "Every specced endpoint semantics exists with real wiring: list with scope/scope_id/kind/required filters, create, get-with-content, update (new version, old retained), delete (409 when spec bindings reference it), versions list, and specific-version content — all with ABAC resolver entries and per-handler role gates. However, none of them is served at the specced path: the CRUD surface lives at /api/v1/meta-specs-registry (the /api/v1/meta-specs prefix is occupied by the blast-radius route /api/v1/meta-specs/:path/blast-radius), so a client written against this spec's API table receives 404 on every listed route. The flat-route rationale in the spec's note is honored (single entity type, scope via query/body), but the route names themselves — part of the API contract — are not.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "\"/api/v1/meta-specs-registry\",",
            "detail": "The meta-spec CRUD endpoints (list/create, get/put/delete, versions) are registered under the meta-specs-registry prefix, not the specced /api/v1/meta-specs path."
          },
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "\"/api/v1/meta-specs/:path/blast-radius\",",
            "detail": "The specced /api/v1/meta-specs prefix is occupied by a different route (blast-radius), so GET /api/v1/meta-specs — the spec's list endpoint — is not served at all."
          },
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "\"cannot delete meta-spec '{id}': active bindings reference it\"",
            "detail": "Positive wiring: DELETE fails (409) when spec bindings reference the meta-spec, one of the specced endpoint behaviors."
          },
          {
            "file": "web/src/lib/api.js",
            "quote": "return request(`/meta-specs-registry${qs ? '?' + qs : ''}`);",
            "detail": "The web client also targets the renamed prefix with the specced query filters (scope/scope_id/kind/required), confirming the functional parity of the surface at the divergent path."
          }
        ]
      },
      {
        "id": "93bc0579bcc1fd7c4d419e39",
        "decision": "context",
        "requirement_quote": "3. Compute Target Model",
        "detail": "Structural parent heading with no body text of its own; its requirements are the child headings 'Abstraction', 'Supported Backends', 'Nix-Based Image Build' and 'Tenant and Workspace Configuration', each audited and decided individually below. Counting them here would double-account the same requirements.",
        "evidence": []
      },
      {
        "id": "82e215b9d8c3847f600ff3ab",
        "decision": "satisfied",
        "requirement_quote": "Compute targets are pluggable backends for agent execution. The system supports multiple backends behind a single trait:",
        "detail": "The ComputeTarget port trait defines name(), target_type(), async spawn_process(&SpawnConfig) -> Result<ProcessHandle>, kill_process, and is_alive — the specced surface — and is implemented by four real backends: LocalTarget, ContainerTarget (docker/podman), SshTarget and SshDockerTarget (docker on a remote host), and KubernetesTarget (Pod via kubectl apply); SpawnConfig/ProcessHandle are the shared config/handle types; the server's spawn dispatch routes to the concrete target. Cosmetic deviation: target_type() returns &'static str backend identifiers rather than the ComputeTargetType enum from the spec's trait sketch (the enum exists in gyre-domain and is used for DB rows and the API) — behaviorally equivalent, all three specced backend types are represented.",
        "evidence": [
          {
            "file": "crates/gyre-ports/src/compute.rs",
            "quote": "async fn spawn_process(&self, config: &SpawnConfig) -> Result<ProcessHandle>;",
            "detail": "The port trait carries the specced five-method surface (name, target_type, spawn_process, kill_process, is_alive) behind which all backends sit."
          },
          {
            "file": "crates/gyre-ports/src/compute.rs",
            "quote": "fn target_type(&self) -> &'static str;",
            "detail": "The one signature deviation: target_type returns a static string identifier instead of the ComputeTargetType enum (which exists in gyre-domain for rows/API) — cosmetic, behaviorally equivalent."
          },
          {
            "file": "crates/gyre-adapters/src/compute/ssh.rs",
            "quote": "pub struct SshDockerTarget {",
            "detail": "Real backend implementations exist for every specced type — container (docker run), SSH (docker run on a remote host), Kubernetes (Pod creation), plus a local fallback — all behind the single trait."
          }
        ]
      },
      {
        "id": "ebd16b0ebf8506b9c84eef25",
        "decision": "gap",
        "requirement_quote": "**Container security defaults** (all backends):\n- `--network=none` (opt-in to bridge via compute target config)\n- `--memory=2g`, `--pids-limit=512`\n- `--user=65534:65534` (non-root)\n- Worktree mounted as volume",
        "detail": "All three backends are real: ContainerTarget runs docker/podman with the full default set including the worktree volume mount; SshDockerTarget runs docker on the remote host with the same four flags (--network=none, --memory=2g, --pids-limit=512, --user=65534:65534); KubernetesTarget creates a Pod with memory 2Gi and runAsUser/runAsGroup 65534, runAsNonRoot, no privilege escalation. Gap against the '(all backends)' claim: (1) the Kubernetes backend sets no pids-limit (the code defers to kubelet-level cgroup limits the manifest cannot express) and has no --network=none equivalent (DNS egress is allowed; isolation is deferred to cluster-side network policies the platform does not create); (2) the worktree volume mount exists only on the local ContainerTarget — SshDockerTarget and KubernetesTarget mount no volume (the remote worktree cannot be mounted, and the agent image clones its own checkout). So two of the five specced defaults are absent on the Kubernetes backend and the volume mount is absent on two of three backends, with no spec amendment authorizing the deviations — the code comments document them as platform constraints, which per the audit rules is not a spec conflict without concrete evidence the contract cannot be satisfied.",
        "evidence": [
          {
            "file": "crates/gyre-adapters/src/compute/container.rs",
            "quote": "format!(\"--volume={}:{}\", config.work_dir, config.work_dir));",
            "detail": "The local ContainerTarget mounts the worktree as a volume and applies all four docker security defaults — the reference implementation of the specced default set."
          },
          {
            "file": "crates/gyre-adapters/src/compute/ssh.rs",
            "quote": "// Spec §3 security defaults (same as ContainerTarget).",
            "detail": "The SSH-docker backend carries the same four docker flags (network=none, memory=2g, pids-limit=512, user=65534:65534) — but its docker_run_args pushes no worktree volume mount, so 'Worktree mounted as volume' does not hold on this backend."
          },
          {
            "file": "crates/gyre-adapters/src/compute/kubernetes.rs",
            "quote": "Kubernetes has no direct pids-limit",
            "detail": "The Kubernetes backend's manifest sets no per-Pod pids-limit — the code defers to kubelet-derived cgroup limits it does not configure — so one of the specced '(all backends)' security defaults is absent on this backend."
          },
          {
            "file": "crates/gyre-adapters/src/compute/kubernetes.rs",
            "quote": "// Spec: --network=none equivalent — no egress except what the",
            "detail": "The Pod spec has no network isolation: DNS egress remains available and network-policy enforcement is deferred to the cluster admin, so the --network=none default does not hold on the Kubernetes backend."
          },
          {
            "file": "crates/gyre-adapters/src/compute/kubernetes.rs",
            "quote": "\"runAsUser\": 65534,",
            "detail": "Positive wiring: the Pod manifest does enforce the non-root user (65534:65534, runAsNonRoot, no privilege escalation) and the 2Gi memory limit — the defaults that can be expressed per-Pod are set."
          }
        ]
      },
      {
        "id": "3ee23b90e9236c59b010822f",
        "decision": "gap",
        "requirement_quote": "One definition, all targets. The `Dockerfile` is replaced by the Nix flake.",
        "detail": "The Nix side is fully implemented: docker/gyre-agent/flake.nix defines agent-image (buildLayeredImage, default), agent-image-docker (docker-loadable) and agent-image-tar (gzipped, air-gapped transfer) with pinned nixpkgs and a committed lockfile; the spawn flow resolves the image digest before minting the agent JWT (docker image inspect / remote inspect) into the wl_image_hash claim, and the Kubernetes backend records the actually-pulled pod imageID in the post-spawn workload attestation served by GET /api/v1/agents/:id/workload. GAP: the Dockerfile is NOT replaced — docker/gyre-agent/Dockerfile still exists and builds a materially different image (FROM node:22-alpine with unpinned packages, versus the bit-reproducible pinned flake), and the production spawn failure path actively directs operators to build it ('If the image is missing, build it first: docker build -t gyre-agent:latest docker/gyre-agent/'). A default deployment following the server's own error guidance therefore runs agents on a non-reproducible image that can diverge from the attested one — undermining both 'The Dockerfile is replaced by the Nix flake' and 'The image is the unit of consistency'. The spec already provides the no-Nix deployment path (nix build .#agent-image-tar for air-gapped transfer), so a live Dockerfile fallback is not required to satisfy the build-command set. (This was blocking review feedback at the prior audit base; the cited code is unchanged at this base.)",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "`docker build -t gyre-agent:latest docker/gyre-agent/`.",
            "detail": "The production container-spawn failure path directs operators to build the legacy Dockerfile — the Dockerfile is a live deployment path, not vestigial, contradicting 'The Dockerfile is replaced by the Nix flake'."
          },
          {
            "file": "docker/gyre-agent/Dockerfile",
            "quote": "FROM node:22-alpine",
            "detail": "The legacy Dockerfile still exists and builds a materially different, non-pinned image than the Nix flake — two image definitions ship, so there is no single 'unit of consistency'."
          },
          {
            "file": "docker/gyre-agent/flake.nix",
            "quote": "#   nix build .#agent-image-tar      # gzipped tarball for air-gapped transfer",
            "detail": "Positive wiring: the flake defines all three specced targets (agent-image, agent-image-docker, agent-image-tar) with pinned dependencies — and the tar target already covers no-Nix environments, so the Dockerfile fallback is not needed for that case."
          },
          {
            "file": "crates/gyre-server/src/auth.rs",
            "quote": "pub wl_image_hash: Option<String>,",
            "detail": "Positive wiring: the wl_image_hash JWT claim carries the pre-resolved image digest for container/SSH backends; the Kubernetes backend records the pulled pod imageID in the post-spawn workload attestation."
          },
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": ".route(\"/api/v1/agents/:id/workload\", get(workload::get_workload))",
            "detail": "The post-spawn workload attestation record is served at the specced GET /api/v1/agents/:id/workload endpoint."
          }
        ]
      },
      {
        "id": "7e80932897fe7fd051bf3188",
        "decision": "satisfied",
        "requirement_quote": "**Workspace level:** Selects ONE compute target from the tenant's list.",
        "detail": "The tenant-level entity (ComputeTargetEntity: id, tenant_id, name, target_type, config JSON, is_default, created_at) is persisted via the ComputeTargetRepository; Workspace.compute_target_id selects exactly one target, validated on PUT for tenant containment (cross-tenant targets rejected); the spawn resolution chain is: request-specified target → workspace target → tenant default (get_default_for_tenant) → local container auto-detection (Runtime::detect() Docker or Podman); the API (GET/POST/GET/PUT/DELETE /api/v1/compute-targets) gates create on tenant Admin, and DELETE fails with 409 when workspaces reference the target. Entity is named ComputeTargetEntity rather than ComputeTargetConfig — a naming deviation with all specced fields present.",
        "evidence": [
          {
            "file": "crates/gyre-domain/src/workspace.rs",
            "quote": "pub compute_target_id: Option<Id>,",
            "detail": "The Workspace struct carries the single compute-target selection; PUT validates it resolves to an existing target in the workspace's own tenant."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": ".get_default_for_tenant(&ws.tenant_id)",
            "detail": "The fallback chain is real: workspace selection falls back to the tenant's default target, and beyond that to local Docker/Podman auto-detection (Runtime::detect())."
          },
          {
            "file": "crates/gyre-server/src/api/compute_targets.rs",
            "quote": "\"compute target {id} is referenced by one or more workspaces\"",
            "detail": "DELETE fails with a conflict when workspaces still reference the target — the specced delete guard."
          },
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "\"/api/v1/compute-targets\",",
            "detail": "The specced flat compute-target routes (list/create and id-scoped get/put/delete) are registered at the specced path."
          }
        ]
      },
      {
        "id": "faa8cf78ab3d221e72297883",
        "decision": "context",
        "requirement_quote": "4. Budget Enforcement",
        "detail": "Structural parent heading with no body text of its own; its requirements are the child headings 'Cascade', 'Enforcement Levels', 'What's Tracked' and 'Budget Reset', each audited and decided individually below. Counting them here would double-account the same requirements.",
        "evidence": []
      },
      {
        "id": "68331317c747aa4a1798c8f0",
        "decision": "satisfied",
        "requirement_quote": "A repo's `max_agents` cannot exceed the workspace's `max_agents_per_repo` — the server validates this on `PUT /api/v1/repos/:id` and rejects values that exceed the workspace limit (400 error).",
        "detail": "The cascade is enforced at every level: effective_workspace_config takes each field as the tighter of the workspace config and the tenant ceiling (the tenant budget is the absolute ceiling, validated on workspace PUT too); repo budget fields are validated against the effective workspace config on PUT with 400 errors; repo max_agents is validated against the workspace's max_agents_per_repo with a 400 error; the scheduler enforces max_agents by counting Active agents per repo with the fallback chain Repository.max_agents → Workspace.max_agents_per_repo → unlimited, queueing (never 429) at the limit; agent usage is charged to the repo and rolls up to workspace and tenant entries.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/repos.rs",
            "quote": "\"max_agents {max_agents} exceeds the workspace limit of {ws_max}\"",
            "detail": "PUT /api/v1/repos/:id rejects a repo max_agents exceeding the workspace's max_agents_per_repo with a 400 — the specced validation."
          },
          {
            "file": "crates/gyre-server/src/api/budget.rs",
            "quote": "/// Effective workspace limits: each field is the tighter of the workspace",
            "detail": "The tenant budget is the absolute ceiling: effective workspace limits take the tighter of the workspace config and the tenant ceiling, and workspace PUT validates the same cascade."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "\"signal-chain: repo at max_agents; task queued\"",
            "detail": "Spawn-time max_agents enforcement counts Active agents scoped to the repo (repo override → workspace fallback → unlimited) and queues rather than rejecting — the scheduler-side half of the cascade."
          }
        ]
      },
      {
        "id": "c8aab7322dffd3789790d78c",
        "decision": "satisfied",
        "requirement_quote": "| **100%** | Graceful stop | All active agents in the workspace receive a `BudgetExhausted` message in their inbox. Agents have 60 seconds to commit current work and call `agent.complete`. After 60 seconds, agents are killed via `ComputeTarget::kill_process()`. Agents that complete within the grace period are marked `Idle`. Agents killed after the grace period are marked `Stopped`. |",
        "detail": "enforce_workspace_budget runs after every usage report and after every workspace budget update. 80%: AgentBudgetWarning-class notification (High priority) plus BudgetWarning bus events; agents continue. 100% (default GracefulStop action; the action is configurable per platform-model §5 with GracefulStop as the default): BudgetExhausted messages delivered to every active agent's inbox with the grace window, a single live 60-second grace timer per workspace that re-evaluates limits before killing, kill via the spawn-recorded backend (docker rm --force / kubectl delete pod / remote docker rm — the ComputeTarget kill path), grace-period completers left Idle, survivors transitioned to Stopped. Beyond limit (budget reduced below current usage, BudgetChanged cause): immediate hard kill with grace_secs 0, no timer, worktree preserved, no MR, task left InProgress for human review, plus a reassignment task per stopped agent.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/budget.rs",
            "quote": "const BUDGET_WARNING_THRESHOLD_PCT: f64 = 80.0;",
            "detail": "The 80% warn threshold emits the budget-warning notification and bus events; agents continue running."
          },
          {
            "file": "crates/gyre-server/src/api/budget.rs",
            "quote": "pub const BUDGET_GRACE_PERIOD_SECS: u64 = 60;",
            "detail": "The 100% graceful stop uses the specced 60-second grace period before killing via the recorded compute-target backend."
          },
          {
            "file": "crates/gyre-server/src/api/budget.rs",
            "quote": "async fn start_budget_grace_timer(state: Arc<AppState>, workspace_id: Id, grace_secs: u64) {",
            "detail": "A real grace timer runs (single live timer per workspace, re-evaluated before the kill), with agents completing in the window left Idle and survivors Stopped — the specced graceful-stop semantics."
          },
          {
            "file": "crates/gyre-server/src/api/budget.rs",
            "quote": "/// Deliberately unlike the admin kill route: the worktree is preserved",
            "detail": "The beyond-limit hard kill (budget reduced below usage) kills immediately with no timer, preserves the worktree, leaves the task InProgress for human review and creates no MR — the specced beyond-limit behavior."
          }
        ]
      },
      {
        "id": "d6a13e4b2effb55dd1283f96",
        "decision": "satisfied",
        "requirement_quote": "The credential proxy (`cred-proxy.mjs`) reports token usage back to the server after each LLM call via `POST /api/v1/agents/:id/usage`. The server aggregates usage per workspace and checks against workspace limits on every report.",
        "detail": "All four cost types are tracked: llm_tokens and llm_cost_usd from agent LLM calls (cred-proxy usage reports), compute_minutes as wall-clock time from spawn to the terminal event (charged on complete/fail/stop/admin kill/budget stop/stale reap via record_compute_minutes_for_agent), and llm_query for user-initiated LLM calls charged to the workspace directly with no repo attribution (the repo_id filter is explicitly skipped for llm_query usage). cred-proxy.mjs reports usage after each proxied LLM call to POST /api/v1/agents/:id/usage with a bounded timeout; the server funnels every report through record_llm_call_usage into the workspace counters and runs enforce_workspace_budget on every report.",
        "evidence": [
          {
            "file": "docker/gyre-agent/cred-proxy.mjs",
            "quote": "*   server via POST $GYRE_SERVER_URL/api/v1/agents/$GYRE_AGENT_ID/usage",
            "detail": "The credential proxy reports token usage back to the server after each proxied LLM call, with a bounded report timeout — the specced reporting channel."
          },
          {
            "file": "crates/gyre-server/src/api/budget.rs",
            "quote": ".filter(|_| usage.usage_type != \"llm_query\")",
            "detail": "llm_query usage is charged to the workspace directly with no repo-level tracking (user queries have no repo context), exactly as the table requires."
          },
          {
            "file": "crates/gyre-server/src/api/budget.rs",
            "quote": "let minutes = now.saturating_sub(agent.spawned_at) / 60;",
            "detail": "compute_minutes is the agent's wall-clock time from spawn to the terminal event, charged to workspace, tenant and repo entries on every terminal path."
          }
        ]
      },
      {
        "id": "5d31ae0e730c80ccd5596348",
        "decision": "satisfied",
        "requirement_quote": "The reset is handled by a background job that runs every hour and clears expired usage entries.",
        "detail": "An hourly background loop (plus a registered job-registry entry with interval_secs 3600) calls apply_rolling_24h_window, which computes the 24-hour cutoff, re-sums the per-call budget call records within the window, and resyncs the workspace/repo/tenant counters (period_start reset to the cutoff) so expired usage no longer counts against time-based limits. Mechanism refinement with the same effect: the per-call audit records are retained as the rolling window's source of truth rather than literally deleted, while the counters they feed are cleared of expired usage; compute_minutes (cumulative) and active_agents (live gauge) are deliberately not windowed, matching their semantics.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;",
            "detail": "The background reset loop runs every hour, invoking the rolling-window resync."
          },
          {
            "file": "crates/gyre-server/src/api/budget.rs",
            "quote": "pub async fn apply_rolling_24h_window(state: &AppState) -> anyhow::Result<()> {",
            "detail": "The rolling 24-hour window recomputes counters from the per-call records since the cutoff and resyncs workspace/repo/tenant usage — expired usage stops counting, the specced reset effect."
          }
        ]
      },
      {
        "id": "47d309e09aac1e2e04d334d0",
        "decision": "gap",
        "requirement_quote": "`system://constraints` delivers budget limits and operational parameters from the persona's operational fields.",
        "detail": "The three-section prompt structure and the four MCP prompts are implemented and wired end-to-end: the meta-spec system prompt is assembled and injected at spawn (GYRE_META_SPEC_PROMPT + set SHA, re-assembled on re-spawn); the protocol section is a single system-managed source (Ralph loop rules, MCP tool catalog, the 60-second heartbeat requirement, escalation rules, completion signaling — 'users don't edit this'), injected at spawn and served via system://protocol; task context (task details, pinned spec ref, branch/repo, gate-failure messages from previous iterations and unacked inbox messages) is delivered via the inbox and the system://context MCP prompt; heartbeat is enforced by the gyre_agent_heartbeat tool plus the 60-second stale-agent detector. GAP: the specced mapping of system://constraints to the persona's operational fields is not implemented — prompt_constraints delivers workspace budget configuration, trust level, scope boundaries, agent lifetime and the repo iteration ceiling, while the Persona entity's operational fields (model, temperature, max_tokens, budget) are stored and editable via /api/v1/personas but consumed by no execution path anywhere (only the CRUD DTOs reference them). 'Operational parameters from the persona's operational fields' therefore have no delivery mechanism; the constraints prompt draws from different sources than the spec mandates.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"system://constraints\" => prompt_constraints(state, auth).await,",
            "detail": "All four specced MCP prompts are dispatched (persona/protocol/context/constraints); the constraints prompt is served — but from workspace budget config, trust level and repo ceilings, not from the persona's operational fields."
          },
          {
            "file": "crates/gyre-server/src/api/prompts.rs",
            "quote": "- Send a heartbeat every 60 seconds; missing heartbeats mark you Dead and trigger re-spawn or cleanup.",
            "detail": "Positive wiring: the system-managed protocol section carries the Ralph loop rules, tool usage, the 60-second heartbeat requirement, escalation and completion signaling, and is not part of the meta-spec registry."
          },
          {
            "file": "crates/gyre-domain/src/workspace.rs",
            "quote": "pub model: Option<String>,",
            "detail": "The Persona entity's operational fields (model, temperature, max_tokens, budget) exist on the struct — the source the spec says system://constraints must draw from."
          },
          {
            "file": "crates/gyre-server/src/api/personas.rs",
            "quote": "persona.temperature = req.temperature;",
            "detail": "The persona operational fields are only ever written/read by the personas CRUD DTOs — no spawn, prompt-assembly or constraints path consumes them, so the specced delivery from persona operational fields does not exist."
          }
        ]
      },
      {
        "id": "a40446f46dbb0536f3c30ed0",
        "decision": "gap",
        "requirement_quote": "Remove `agent.spawn` MCP tool — orchestrators do not spawn agents. They create tasks via `task.create`. The system spawns agents mechanically in response to new implementation tasks.",
        "detail": "The amendments table is dispositioned row by row. Implemented: platform-model.md §3 Task (task_type Implementation/Delegation/Coordination, order, depends_on, parent_task_id); message-bus.md MessageKind::SpecApproved (server-only Event tier, distinct from SpecChanged); platform-model.md §1 Workspace compute_target_id; the Agent entity amendment (AgentStatus enum + status field + task binding as current_task_id: Option<Id> — workers are bound mechanically by the scheduler; orchestrators are deliberately taskless, a documented Option deviation from the specced 'task_id: Id'); agent-gates.md (the approve handler emits SpecApproved after recording the ledger row; MergeAttestation gains meta_specs_used, covered by the EdDSA signature and populated from the spawn-time prompt-set record); abac-policy-engine.md meta_spec and compute_target resource types; human-system-interface.md §1 (the UI meta-specs surface fetches the registry API); and the spec-lifecycle.md stale-terminology replacement ('CEO agent' no longer occurs anywhere in the specs tree). Contradicted or missing: (1) 'Remove agent.spawn MCP tool — orchestrators do not spawn agents' — the live gyre_spawn_worker tool lets a repo-orchestrator JWT spawn worker agents directly (see Phase 3); (2) 'The /api/v1/personas endpoints ... are removed — replaced entirely by /api/v1/meta-specs?kind=meta:persona' — the full personas CRUD plus approve routes remain registered and the Persona entity is still bound to agents at spawn, coexisting with the meta-spec registry (whose CRUD lives at /api/v1/meta-specs-registry, not the specced path — see API); (3) 'Add archive to the action attribute table' — the ABAC action vocabulary contains no 'archive' action and the repo archive/unarchive routes run without middleware ABAC (frozen exemption-file entries with per-handler authorization only); (4) the meta-spec-reconciliation.md row's registry-replacement clauses are not carried out: the old path@sha-pinned meta-spec-set model is fully alive production code — PUT/GET /api/v1/workspaces/:id/meta-spec-set and PUT/GET /api/v1/tenants/:id/meta-specs/set upsert MetaSpecSet{personas: BTreeMap<String, MetaSpecPinnedEntry{path, sha}>, principles, standards, process} into the meta_spec_sets/tenant_meta_spec_sets KV stores, trigger old-model reconciliation (diff_meta_spec_set → run_reconciliation, emit_meta_spec_set_updated) and feed resolve_effective_meta_spec_set, the §10 conformance sweep (run_conformance_sweep compares compute_meta_spec_set_sha of the path@sha set against the provenance-recorded meta_spec_set_sha) and merge-time drift enforcement (mr_meta_spec_drift compares the old-model set SHA, not meta_specs_used); MetaSpecSnapshot does not exist in the amended {meta_spec_id, kind, content_hash, version} form anywhere (grep across crates/: zero matches — the operative pin type remains MetaSpecPinnedEntry{path, sha}), and reconciliation.rs never references meta_specs_used — the specced replacement surface (required flags + spec-level bindings via the meta-spec API) coexists with the old endpoint instead of replacing it, the same amendment class as agent.spawn and /api/v1/personas; (5) the hierarchy-enforcement.md §4 row is not carried out in the amended document: it contains no compute-target or meta-spec CRUD route-table entries and no meta_spec_versions/meta_spec_bindings/compute_targets entries in its tenant-filter configuration, while its KvJsonStore note still lists compute_targets among tenant-less KV namespaces, contradicting the migrated tenant_id-bearing table (migration 000033) — the production substance largely exists, making the un-amended spec text stale rather than pending. Additionally the supersession directive that all cross-references to ralph-loop.md in other specs be updated to agent-runtime.md §1 is not carried out (e.g. specs/system/authorization-provenance.md still cites ralph-loop.md, and specs/system/ralph-loop.md still exists).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"gyre_spawn_worker\" => handle_spawn_worker(&state, &args, &auth).await,",
            "detail": "Amendment (a) contradicted: an agent-spawning MCP tool exists and is available to repo-orchestrator JWTs — orchestrators can spawn agents, which the amendment removes."
          },
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "\"/api/v1/personas\",",
            "detail": "Amendment (h) contradicted: the /api/v1/personas endpoints (create/list/resolve/get/put/delete/approve) are still routed rather than removed — both persona surfaces coexist."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"read\", \"write\", \"delete\", \"approve\", \"spawn\", \"push\", \"merge\", \"escalate\", \"generate\",",
            "detail": "Amendment (g) missing: the ABAC action vocabulary has no 'archive' action, and the archive/unarchive routes bypass middleware ABAC via the frozen exemption file, so the amendment's action-table addition is absent."
          },
          {
            "file": "scripts/abac-route-registry-exemptions.txt",
            "quote": "/api/v1/repos/:id/archive",
            "detail": "The archive route runs without middleware ABAC (exemption-file entry, per-handler authorization only) — the concrete state behind the missing 'archive' action."
          },
          {
            "file": "specs/system/authorization-provenance.md",
            "quote": "- **Ralph Loop** (`ralph-loop.md`): Re-spawn produces `DerivedInput` from same parent, preserving constraint chain (§4.2).",
            "detail": "The supersession directive 'All cross-references to ralph-loop.md in other specs ... should be updated to reference agent-runtime.md §1' is not carried out — other canonical specs still cite the superseded document."
          },
          {
            "file": "crates/gyre-common/src/message.rs",
            "quote": "SpecApproved,",
            "detail": "Positive wiring for the message-bus amendment: MessageKind::SpecApproved exists (server-only Event tier) with the specced payload, distinct from SpecChanged."
          },
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "\"/api/v1/workspaces/:id/meta-spec-set\",",
            "detail": "Amendment (4) — the row's 'PUT /api/v1/workspaces/{id}/meta-spec-set endpoint is replaced by updating required flags and spec-level bindings via the meta-spec API' is not enacted: the old PUT/GET meta-spec-set route (and the tenant twin '/api/v1/tenants/:id/meta-specs/set' at the adjacent registration) is still live production routing alongside the new registry/bindings surface."
          },
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "pub struct MetaSpecPinnedEntry {",
            "detail": "Amendment (4) — 'The MetaSpecSnapshot from reconciliation spec uses content_hash ... struct fields become {meta_spec_id, kind, content_hash, version} tuples instead of path@sha strings': MetaSpecSnapshot does not exist in that form anywhere in crates/ (grep: zero matches); the operative pin type remains MetaSpecPinnedEntry{path, sha} keyed by git-spec path."
          },
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "let summary = crate::reconciliation::run_reconciliation(",
            "detail": "Amendment (4) — a meta-spec-set PUT still drives the old-model reconciliation machinery (diff_meta_spec_set over path@sha pins → emit_meta_spec_set_updated → run_reconciliation), the exact flow the amendment replaces with required-flag changes plus reconciliation triggered on meta-spec approval."
          },
          {
            "file": "crates/gyre-server/src/reconciliation.rs",
            "quote": "let current_set_sha = crate::compute_meta_spec_set_sha(state, &mr.workspace_id).await;",
            "detail": "Amendment (4) — 'Drift detection compares the meta_specs_used array in merge attestations against current required meta-spec versions': merge-time drift (mr_meta_spec_drift) and the §10 conformance sweep compare the old-model set SHA (compute_meta_spec_set_sha vs the provenance-recorded meta_spec_set_sha), never the meta_specs_used array; reconciliation.rs contains no reference to meta_specs_used."
          },
          {
            "file": "specs/system/hierarchy-enforcement.md",
            "quote": "**KvJsonStore gap:** The generic `KvJsonStore` (used for agent_tokens, compute_targets, abac_policies, etc.)",
            "detail": "Amendment (5) — the hierarchy-enforcement.md §4 directives are absent from the amended document: its route table contains no compute-target or meta-spec CRUD entries (no 'compute-target'/'meta-spec' string occurs in the file outside this note), its §3 tenant-filter configuration lists no meta_spec_versions/meta_spec_bindings/compute_targets tables, and this KvJsonStore note still lists compute_targets among tenant-less KV namespaces — contradicting the migrated tenant_id-bearing table."
          },
          {
            "file": "crates/gyre-adapters/migrations/2026-03-26-000033_compute_targets/up.sql",
            "quote": "CREATE TABLE compute_targets (",
            "detail": "Amendment (5) — the production substance exists: compute_targets is a real tenant_id-bearing table (UNIQUE(tenant_id, name), with workspaces.compute_target_id referencing it), so hierarchy-enforcement.md's un-updated route table and tenant-less-KV note are stale spec text, not pending work."
          }
        ]
      }
    ],
    "gaps": [
      {
        "id": "1bd201625dc5fbe0164ced05",
        "decision": "gap",
        "requirement_quote": "Spec approval is the **single trigger** for all agent work. Every path to implementation goes through human approval:",
        "detail": "Requires that spec approval is the ONLY trigger for agent implementation work ('There is no other trigger. The SpecApproved event is the universal starting signal.'). The approval side is real: the approve handler records the ledger row and emits SpecApproved, modified-spec pushes reset approval_status to Pending (spec_registry sync) and revoke approvals via the post-receive hook, and untyped push-hook tasks never spawn agents. However, the single-trigger property is mechanically violated at the task layer: the MCP tool gyre_create_task and the HTTP task-create handler both accept task_type \"implementation\" with no precondition that an approved spec exists, and the Phase-4 scheduler spawns worker agents for ANY Backlog task carrying task_type Implementation — it never checks that the task derives from an approved spec. POST /api/v1/agents/spawn additionally spawns worker agents directly for authenticated callers. Implementation work can therefore start with no human spec approval anywhere in the chain.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"enum\": [\"implementation\", \"delegation\", \"coordination\"],",
            "detail": "The gyre_create_task MCP tool schema lets any authorized caller create a task with task_type \"implementation\"; the handler maps it to TaskType::Implementation with no spec-approval precondition, so the universal-approval gate is bypassable at task creation."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "continue; // Delegation/Coordination → orchestrators; untyped → never spawned",
            "detail": "The Phase-4 scheduler scan spawns workers for every Backlog task with task_type Implementation regardless of how the task was created; the scan checks repo status, ordering, max_agents and budget but never checks that the task references an approved spec."
          },
          {
            "file": "crates/gyre-server/src/api/tasks.rs",
            "quote": "if let Some(tt) = req.task_type {\n        task.task_type = Some(parse_task_type(&tt)?);\n    }",
            "detail": "The HTTP task-create handler accepts task_type directly from the request body with no approval precondition; a task created this way with type \"implementation\" is picked up by the scheduler and spawns an agent."
          },
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": ".route(\"/api/v1/agents/spawn\", post(spawn::spawn_agent))",
            "detail": "A direct agent-spawn REST surface exists for authenticated callers (used legitimately for interrogation agents per Phase 8, but it also spawns task-bound worker agents), so implementation agents can be created without any spec approval event."
          },
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "existing.approval_status = ApprovalStatus::Pending;",
            "detail": "Positive wiring for the modified-spec rule: when a spec push changes the blob SHA, sync_spec_ledger resets approval_status to Pending (auto_invalidate_on_change default true), so a modified spec does need re-approval — that part of the signal chain is enforced."
          }
        ]
      },
      {
        "id": "15aec83b58b3494602d31ce8",
        "decision": "gap",
        "requirement_quote": "The repo orchestrator **only creates tasks**. It never spawns agents. The system handles the task→agent mapping mechanically.",
        "detail": "The decomposition machinery is implemented: the task scheduler detects Delegation tasks by task_type, spawns the repo orchestrator under a per-repo mutex (repo-orchestrator persona, repo-scoped JWT, one-live 409); decompose_delegation fails hard on an unresolvable spec_ref, reads the spec at the pinned SHA, uses LLM decomposition judgment when an LLM is configured (deterministic fallback otherwise), creates Implementation sub-tasks carrying spec_ref, parent_task_id, order and depends_on, and marks the delegation task Done. GAP: the 'never spawns agents' boundary is contradicted by a live production mechanism — the dispatched MCP tool gyre_spawn_worker lets any repo-orchestrator JWT spawn a worker agent directly through spawn_agent_core (worktree, scoped JWT, compute-target resolution). That is exactly the orchestrator-spawns-agents capability the spec removes (see also the amendments table: 'Remove agent.spawn MCP tool — orchestrators do not spawn agents'); the mechanical scheduler is not the only task→agent path available to an orchestrator. Secondary mechanism deviation: sub-tasks are created via the tasks port (state.tasks.create) rather than the task.create MCP tool.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"description\": \"Spawn a worker agent for an implementation task in the caller's repo. Requires a repo-orchestrator JWT. Delegates to the standard agent spawn flow (worktree, scoped JWT, compute target resolution).\",",
            "detail": "gyre_spawn_worker is advertised in the MCP tool catalog specifically to repo-orchestrator JWT holders — the repo orchestrator can spawn worker agents directly, contradicting 'It never spawns agents. The system handles the task→agent mapping mechanically.'"
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"gyre_spawn_worker\" => handle_spawn_worker(&state, &args, &auth).await,",
            "detail": "The tool is dispatched in the production tools/call path (not dead catalog text); handle_spawn_worker builds a SpawnAgentRequest and calls spawn_agent_core under the repo-orchestrator's own authentication."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "Some(TaskType::Delegation) => decompose_delegation(state, repo, task).await?,",
            "detail": "Positive wiring: the scheduler routes Delegation tasks to the repo orchestrator's decomposition, which creates ordered Implementation sub-tasks and completes the delegation task — the specced Phase-3 flow itself is real."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "sub.task_type = Some(TaskType::Implementation);",
            "detail": "Sub-tasks are created with the Implementation discriminator together with spec_ref, parent_task_id, order and depends_on (the same creation block sets sub.spec_ref/sub.parent_task_id/sub.order/sub.depends_on), via the tasks port rather than the task.create MCP tool — a mechanism deviation in how sub-tasks are created, with all specced fields present."
          }
        ]
      },
      {
        "id": "de73e06638559fcca67b3c5d",
        "decision": "gap",
        "requirement_quote": "The agent has no knowledge of other agents, the merge queue, or the broader system state. It sees its task, its spec, and its repo.",
        "detail": "The worker workflow itself is real: the agent runner reads its task via MCP, edits/commits/pushes, and signals completion (gyre_agent_complete MCP tool and the REST twin both create the completion MR). But the normative knowledge boundary is mechanically violated, not just prompt-level: the MCP resources/read layer advertises and serves 'Workspace Agents' (agents://) and 'Merge Queue' (queue://) resources to every authenticated MCP session with no per-role or per-scope gate at the dispatch. An unscoped agents:// read lists ALL Active/Idle agents across ALL workspaces (state.agents.list() with no caller-scope filter — a cross-workspace enumeration leak), and queue://{repo_id} returns any repository's merge-queue entries. A worker agent can therefore acquire exactly the knowledge of other agents and the merge queue that this section forbids. No spec amendment authorizes prompt-level-only isolation. (This was blocking review feedback at the prior audit base; the cited code is unchanged at this base.)",
        "evidence": [
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"description\": \"List active agents in a workspace. URI: agents://{workspace_id} or agents:// for all.\",",
            "detail": "resources/list advertises the agents:// and queue:// resources to every MCP session including worker agent tokens — the resource layer itself offers the forbidden knowledge."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "let agents_result = if workspace_id.is_empty() || workspace_id == \"*\" {",
            "detail": "The unscoped agents:// read falls through to state.agents.list() with no caller-scope filter — a worker JWT can enumerate Active/Idle agents across every workspace, far beyond 'its task, its spec, and its repo'."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "let result = handle_resource_read(&state, &auth, uri).await;",
            "detail": "The resources/read dispatch calls handle_resource_read with no role gate, so worker-scoped JWTs are served the agents:// and queue:// resources like any other caller."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "} else if let Some(repo_id) = uri.strip_prefix(\"queue://\") {",
            "detail": "queue://{repo_id} returns merge-queue entries for any repository the caller names — the merge-queue state the spec says a worker agent must not see."
          }
        ]
      },
      {
        "id": "e60f62cd06826e5e4de9decf",
        "decision": "gap",
        "requirement_quote": "Agents have 60 seconds to commit current work (same grace period as budget exhaustion), then are killed and marked `Stopped`",
        "detail": "The Ralph loop core is implemented: pass path merges and marks the task Done, agent Idle, token revoked; fail path writes per-gate failure messages to the agent's inbox, closes the failed MR, and re-spawns the same agent entity with the same persona/task and a FRESH session (no conversation history — new JWT, re-assembled prompt set, reused worktree); iteration ceiling with repo-level max_agent_iterations falling back to a server default of 10; at the ceiling the task is Blocked, the agent Failed, the token removed, and a TaskBlocked message is routed to the repo orchestrator (spawning one when none is live). Spawn-failure policy: 3 retries at 1s/5s/30s, then Blocked plus a priority-1 'Compute target unavailable: {target_name}' notification, no fallback to a different target. TWO gaps: (1) Spec rejection mid-flight — the specced 60-second grace is decorative: the reject handler synchronously marks active agents Stopped and tears down their worktrees immediately; the 'grace_period_secs': 60 value exists only as a payload field in a shutdown message sent to an already-Stopped agent; there is no timer and no ComputeTarget::kill_process() call on this path (the budget path implements a real grace timer + kill via start_budget_grace_timer — the contrast proves the mechanism exists but is not applied here), and the agent's JWT is not revoked. (2) The iteration-default tier 'falls back to workspace default, then server default of 10' is missing its middle tier: resolution is repo.max_agent_iterations → DEFAULT_MAX_ITERATIONS (10); the Workspace entity has no max_agent_iterations field anywhere.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "let _ = agent.transition_status(gyre_domain::AgentStatus::Stopped);",
            "detail": "On spec rejection the agent is marked Stopped synchronously inside the task-cancellation loop — before any grace period could elapse — so 'agents have 60 seconds to commit current work, then are killed and marked Stopped' is not the implemented order of operations."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "\"grace_period_secs\": 60,",
            "detail": "The 60-second grace exists only as a payload field of the shutdown message delivered to the already-Stopped agent; no timer, no deferred kill via ComputeTarget::kill_process(), and no JWT revocation run on this path."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "// helper). The 60-second shutdown grace in the spec is for",
            "detail": "The code itself documents the deviation: worktrees are torn down immediately because 'the agents are stopped above' — an intentional code-versus-spec contradiction with no amending spec change."
          },
          {
            "file": "crates/gyre-server/src/api/budget.rs",
            "quote": "async fn start_budget_grace_timer(state: Arc<AppState>, workspace_id: Id, grace_secs: u64) {",
            "detail": "The budget-exhaustion path implements a real 60-second grace timer with deferred kill — the same grace the spec requires on spec rejection — proving the mechanism exists but is not applied to the rejection path."
          },
          {
            "file": "crates/gyre-server/src/ralph_loop.rs",
            "quote": ".unwrap_or(DEFAULT_MAX_ITERATIONS);",
            "detail": "Iteration resolution is repo.max_agent_iterations → DEFAULT_MAX_ITERATIONS (10) with no workspace tier; the Workspace struct has no max_agent_iterations field, so the specced 'falls back to workspace default, then server default of 10' middle tier does not exist."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "pub(crate) const SPAWN_RETRY_BACKOFF_SECS: [u64; 3] = [1, 5, 30];",
            "detail": "Positive wiring for the spawn-failure policy: 3 retries at 1s/5s/30s, then task Blocked, orchestrator notified, priority-1 'Compute target unavailable' notification, and no fallback to a different compute target."
          }
        ]
      },
      {
        "id": "54a514da8df9bf4d236e09ff",
        "decision": "gap",
        "requirement_quote": "**Who can set `required`:** Only admins at the meta-spec's own scope level — tenant admins for tenant meta-specs, workspace admins (Owner/Admin role) for workspace meta-specs.",
        "detail": "The injection semantics are implemented: required meta-specs are always injected (both tenant and workspace bands filter on required: Some(true)), cannot be opted out, and bypass nearest-scope resolution; Repo scope structurally cannot be required (the scope parser accepts only Global/Tenant/Workspace, and the Persona entity has no required flag at all); same-name/different-id collisions inject both, required first, as separate prompt sections. GAP: the scope-level grant is not implemented as specced — creating and updating registry entries (including flipping the required flag) is gated to the platform tenant Admin role for EVERY scope, so a workspace Owner/Admin cannot set required on a workspace-scoped meta-spec; only DELETE carries a full scope-aware authorization gate. The code comment claims 'The outer write gate already enforces this for every PUT', but that outer gate is tenant-Admin for all scopes — stricter than the spec for workspace meta-specs, i.e. the specced workspace-admin authority simply does not exist. (The privilege-escalation direction the spec warns about — a workspace admin marking a tenant meta-spec required — is prevented, but by over-restriction, not by scope-level enforcement.)",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "\"only Admin role may modify the meta-spec registry\".to_string(),",
            "detail": "The require_registry_admin helper gates POST and PUT on the tenant Admin role regardless of the meta-spec's scope — workspace Owner/Admin members cannot set required on workspace meta-specs, so the specced scope-level grant is absent for the update/create path."
          },
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "// The outer write gate already enforces this for every PUT.",
            "detail": "The required-flag handling cites an outer scope-level gate that does not exist as described: the outer gate is the tenant-Admin-only role check, so the comment's claim is inaccurate for workspace-scoped meta-specs."
          },
          {
            "file": "crates/gyre-server/src/prompt_assembly.rs",
            "quote": "required: Some(true),",
            "detail": "Positive wiring: required meta-specs are unconditionally injected in both the tenant band (Global + caller's Tenant) and the workspace band — the always-injected, cannot-opt-out semantics are real."
          }
        ]
      },
      {
        "id": "e97ea9a092758dc93c245e6e",
        "decision": "gap",
        "requirement_quote": "The spec author selects these via the UI (Specs view → Edit → Meta-spec bindings panel). The UI shows available meta-specs from the tenant and workspace registries, with required ones pre-selected and locked (cannot be removed).",
        "detail": "The binding model is real as an API: PUT/GET /api/v1/specs/:path/meta-spec-bindings performs a declarative full-set replace with pin-version-exists validation, a cross-workscope binding leak guard, and role gating; band 3 of prompt assembly consumes the bindings at pinned versions via version-history lookup. Two gaps: (1) Storage location — bindings live in a separate meta_spec_bindings DB table keyed by the bare spec path string, NOT 'stored in the spec registry alongside other spec metadata'; the spec ledger entry's SHA is the git blob SHA of the spec markdown file and does not cover bindings (this is the root cause of the broken stale-pin cascade, accounted under 'Stale Pin Detection'). (2) The specced UI bindings panel does not exist: no web component or API client references the bindings endpoint (the web meta-specs surface covers registry CRUD only), and the shipped locale string admits workspace-level drift tracking is 'available after the spec-bindings API is extended' — so spec authors have no UI to select bindings, pre-selected required entries, or locked required rows.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "spec_id: spec_path.clone(),",
            "detail": "Bindings are keyed by the bare spec path string in the side table — the ledger entry, its blob SHA and its approval status are separate structures, so bindings are not spec-registry metadata and pin changes have no effect on spec identity."
          },
          {
            "file": "web/src/locales/en.json",
            "quote": "\"drift_text_suffix\": \"Workspace-level drift tracking (repos built under older versions) is available after the spec-bindings API is extended.\",",
            "detail": "The shipped UI itself concedes the bindings surface is not integrated: no Specs-view bindings panel exists, and the drift text defers workspace-level tracking until the bindings API is 'extended'."
          },
          {
            "file": "crates/gyre-server/src/prompt_assembly.rs",
            "quote": "// Band 3: spec-level bindings at pinned versions. A binding that",
            "detail": "Positive wiring: the assembly consumes bindings at their pinned versions via the version-history lookup, so the binding model itself works end-to-end for prompt injection."
          }
        ]
      },
      {
        "id": "9f95c49cf209205750a0177a",
        "decision": "gap",
        "requirement_quote": "Human updates the pin → spec metadata changes → spec SHA changes → spec needs re-approval",
        "detail": "Detection and notification are real: an hourly background job (meta_spec_stale_pin_check, also triggered on meta-spec edits) runs detect_stale_pins, compares every binding's pinned_version against the current version, dedups via KV keys, and creates priority-6 MetaSpecDrift notifications ('Meta-spec drift alert: ... Review and update pin.') routed to the workspace's Admin/Owner members. But the specced cascade never fires: updating a pin (put_spec_meta_spec_bindings) writes only the meta_spec_bindings side table and clears the stale-pin dedup keys — it never touches the spec ledger entry, its blob SHA, or its approval status. Because bindings are not part of the spec registry metadata (the ledger SHA is the git blob SHA of the spec markdown file), a pin update does NOT change the spec SHA, does NOT require re-approval, emits no SpecApproved event, and triggers no delegation task or re-implementation; the new pin takes effect only for agents spawned later under tasks that happen to reference the spec. The section's outcome — 'agents re-implement with new prompts' after a human acts on a drift alert — is therefore unreachable through the implemented mechanisms. (This was blocking review feedback at the prior audit base; the cited code is unchanged at this base.)",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "let prefix = format!(\"stale_pin:{}:\", spec_path);",
            "detail": "The pin-update handler's only side effects are the meta_spec_bindings side table and stale-pin dedup-key clearing — it never writes the spec ledger, so the pin update cannot change the spec SHA or reset its approval."
          },
          {
            "file": "crates/gyre-domain/src/spec_ledger.rs",
            "quote": "/// Git blob SHA of the spec file at HEAD.",
            "detail": "The ledger SHA is the git blob SHA of the spec markdown file — bindings are not part of it, so the specced 'spec metadata changes → spec SHA changes → spec needs re-approval' chain cannot fire on a pin update."
          },
          {
            "file": "crates/gyre-server/src/prompt_assembly.rs",
            "quote": "\"Meta-spec drift alert: '{}' uses {} v{}, but v{} is available. Review and update pin.\",",
            "detail": "Positive wiring for the detection half: the priority-6 drift notification with the specced message body is created for stale pins — the alert itself works; it is the post-alert cascade that is missing."
          },
          {
            "file": "crates/gyre-server/src/jobs.rs",
            "quote": "name: \"meta_spec_stale_pin_check\".to_string(),",
            "detail": "The background job runs the detection hourly, satisfying the 'background job (or check on spec access)' trigger for detection."
          }
        ]
      },
      {
        "id": "917eb99ea432ee472c349487",
        "decision": "gap",
        "requirement_quote": "| `GET /api/v1/meta-specs` | GET | List meta-specs (filtered by `?scope=`, `?scope_id=`, `?kind=`, `?required=`). ABAC: scope_id extracted from query param for tenant/workspace authorization. |",
        "detail": "Every specced endpoint semantics exists with real wiring: list with scope/scope_id/kind/required filters, create, get-with-content, update (new version, old retained), delete (409 when spec bindings reference it), versions list, and specific-version content — all with ABAC resolver entries and per-handler role gates. However, none of them is served at the specced path: the CRUD surface lives at /api/v1/meta-specs-registry (the /api/v1/meta-specs prefix is occupied by the blast-radius route /api/v1/meta-specs/:path/blast-radius), so a client written against this spec's API table receives 404 on every listed route. The flat-route rationale in the spec's note is honored (single entity type, scope via query/body), but the route names themselves — part of the API contract — are not.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "\"/api/v1/meta-specs-registry\",",
            "detail": "The meta-spec CRUD endpoints (list/create, get/put/delete, versions) are registered under the meta-specs-registry prefix, not the specced /api/v1/meta-specs path."
          },
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "\"/api/v1/meta-specs/:path/blast-radius\",",
            "detail": "The specced /api/v1/meta-specs prefix is occupied by a different route (blast-radius), so GET /api/v1/meta-specs — the spec's list endpoint — is not served at all."
          },
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "\"cannot delete meta-spec '{id}': active bindings reference it\"",
            "detail": "Positive wiring: DELETE fails (409) when spec bindings reference the meta-spec, one of the specced endpoint behaviors."
          },
          {
            "file": "web/src/lib/api.js",
            "quote": "return request(`/meta-specs-registry${qs ? '?' + qs : ''}`);",
            "detail": "The web client also targets the renamed prefix with the specced query filters (scope/scope_id/kind/required), confirming the functional parity of the surface at the divergent path."
          }
        ]
      },
      {
        "id": "ebd16b0ebf8506b9c84eef25",
        "decision": "gap",
        "requirement_quote": "**Container security defaults** (all backends):\n- `--network=none` (opt-in to bridge via compute target config)\n- `--memory=2g`, `--pids-limit=512`\n- `--user=65534:65534` (non-root)\n- Worktree mounted as volume",
        "detail": "All three backends are real: ContainerTarget runs docker/podman with the full default set including the worktree volume mount; SshDockerTarget runs docker on the remote host with the same four flags (--network=none, --memory=2g, --pids-limit=512, --user=65534:65534); KubernetesTarget creates a Pod with memory 2Gi and runAsUser/runAsGroup 65534, runAsNonRoot, no privilege escalation. Gap against the '(all backends)' claim: (1) the Kubernetes backend sets no pids-limit (the code defers to kubelet-level cgroup limits the manifest cannot express) and has no --network=none equivalent (DNS egress is allowed; isolation is deferred to cluster-side network policies the platform does not create); (2) the worktree volume mount exists only on the local ContainerTarget — SshDockerTarget and KubernetesTarget mount no volume (the remote worktree cannot be mounted, and the agent image clones its own checkout). So two of the five specced defaults are absent on the Kubernetes backend and the volume mount is absent on two of three backends, with no spec amendment authorizing the deviations — the code comments document them as platform constraints, which per the audit rules is not a spec conflict without concrete evidence the contract cannot be satisfied.",
        "evidence": [
          {
            "file": "crates/gyre-adapters/src/compute/container.rs",
            "quote": "format!(\"--volume={}:{}\", config.work_dir, config.work_dir));",
            "detail": "The local ContainerTarget mounts the worktree as a volume and applies all four docker security defaults — the reference implementation of the specced default set."
          },
          {
            "file": "crates/gyre-adapters/src/compute/ssh.rs",
            "quote": "// Spec §3 security defaults (same as ContainerTarget).",
            "detail": "The SSH-docker backend carries the same four docker flags (network=none, memory=2g, pids-limit=512, user=65534:65534) — but its docker_run_args pushes no worktree volume mount, so 'Worktree mounted as volume' does not hold on this backend."
          },
          {
            "file": "crates/gyre-adapters/src/compute/kubernetes.rs",
            "quote": "Kubernetes has no direct pids-limit",
            "detail": "The Kubernetes backend's manifest sets no per-Pod pids-limit — the code defers to kubelet-derived cgroup limits it does not configure — so one of the specced '(all backends)' security defaults is absent on this backend."
          },
          {
            "file": "crates/gyre-adapters/src/compute/kubernetes.rs",
            "quote": "// Spec: --network=none equivalent — no egress except what the",
            "detail": "The Pod spec has no network isolation: DNS egress remains available and network-policy enforcement is deferred to the cluster admin, so the --network=none default does not hold on the Kubernetes backend."
          },
          {
            "file": "crates/gyre-adapters/src/compute/kubernetes.rs",
            "quote": "\"runAsUser\": 65534,",
            "detail": "Positive wiring: the Pod manifest does enforce the non-root user (65534:65534, runAsNonRoot, no privilege escalation) and the 2Gi memory limit — the defaults that can be expressed per-Pod are set."
          }
        ]
      },
      {
        "id": "3ee23b90e9236c59b010822f",
        "decision": "gap",
        "requirement_quote": "One definition, all targets. The `Dockerfile` is replaced by the Nix flake.",
        "detail": "The Nix side is fully implemented: docker/gyre-agent/flake.nix defines agent-image (buildLayeredImage, default), agent-image-docker (docker-loadable) and agent-image-tar (gzipped, air-gapped transfer) with pinned nixpkgs and a committed lockfile; the spawn flow resolves the image digest before minting the agent JWT (docker image inspect / remote inspect) into the wl_image_hash claim, and the Kubernetes backend records the actually-pulled pod imageID in the post-spawn workload attestation served by GET /api/v1/agents/:id/workload. GAP: the Dockerfile is NOT replaced — docker/gyre-agent/Dockerfile still exists and builds a materially different image (FROM node:22-alpine with unpinned packages, versus the bit-reproducible pinned flake), and the production spawn failure path actively directs operators to build it ('If the image is missing, build it first: docker build -t gyre-agent:latest docker/gyre-agent/'). A default deployment following the server's own error guidance therefore runs agents on a non-reproducible image that can diverge from the attested one — undermining both 'The Dockerfile is replaced by the Nix flake' and 'The image is the unit of consistency'. The spec already provides the no-Nix deployment path (nix build .#agent-image-tar for air-gapped transfer), so a live Dockerfile fallback is not required to satisfy the build-command set. (This was blocking review feedback at the prior audit base; the cited code is unchanged at this base.)",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "`docker build -t gyre-agent:latest docker/gyre-agent/`.",
            "detail": "The production container-spawn failure path directs operators to build the legacy Dockerfile — the Dockerfile is a live deployment path, not vestigial, contradicting 'The Dockerfile is replaced by the Nix flake'."
          },
          {
            "file": "docker/gyre-agent/Dockerfile",
            "quote": "FROM node:22-alpine",
            "detail": "The legacy Dockerfile still exists and builds a materially different, non-pinned image than the Nix flake — two image definitions ship, so there is no single 'unit of consistency'."
          },
          {
            "file": "docker/gyre-agent/flake.nix",
            "quote": "#   nix build .#agent-image-tar      # gzipped tarball for air-gapped transfer",
            "detail": "Positive wiring: the flake defines all three specced targets (agent-image, agent-image-docker, agent-image-tar) with pinned dependencies — and the tar target already covers no-Nix environments, so the Dockerfile fallback is not needed for that case."
          },
          {
            "file": "crates/gyre-server/src/auth.rs",
            "quote": "pub wl_image_hash: Option<String>,",
            "detail": "Positive wiring: the wl_image_hash JWT claim carries the pre-resolved image digest for container/SSH backends; the Kubernetes backend records the pulled pod imageID in the post-spawn workload attestation."
          },
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": ".route(\"/api/v1/agents/:id/workload\", get(workload::get_workload))",
            "detail": "The post-spawn workload attestation record is served at the specced GET /api/v1/agents/:id/workload endpoint."
          }
        ]
      },
      {
        "id": "47d309e09aac1e2e04d334d0",
        "decision": "gap",
        "requirement_quote": "`system://constraints` delivers budget limits and operational parameters from the persona's operational fields.",
        "detail": "The three-section prompt structure and the four MCP prompts are implemented and wired end-to-end: the meta-spec system prompt is assembled and injected at spawn (GYRE_META_SPEC_PROMPT + set SHA, re-assembled on re-spawn); the protocol section is a single system-managed source (Ralph loop rules, MCP tool catalog, the 60-second heartbeat requirement, escalation rules, completion signaling — 'users don't edit this'), injected at spawn and served via system://protocol; task context (task details, pinned spec ref, branch/repo, gate-failure messages from previous iterations and unacked inbox messages) is delivered via the inbox and the system://context MCP prompt; heartbeat is enforced by the gyre_agent_heartbeat tool plus the 60-second stale-agent detector. GAP: the specced mapping of system://constraints to the persona's operational fields is not implemented — prompt_constraints delivers workspace budget configuration, trust level, scope boundaries, agent lifetime and the repo iteration ceiling, while the Persona entity's operational fields (model, temperature, max_tokens, budget) are stored and editable via /api/v1/personas but consumed by no execution path anywhere (only the CRUD DTOs reference them). 'Operational parameters from the persona's operational fields' therefore have no delivery mechanism; the constraints prompt draws from different sources than the spec mandates.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"system://constraints\" => prompt_constraints(state, auth).await,",
            "detail": "All four specced MCP prompts are dispatched (persona/protocol/context/constraints); the constraints prompt is served — but from workspace budget config, trust level and repo ceilings, not from the persona's operational fields."
          },
          {
            "file": "crates/gyre-server/src/api/prompts.rs",
            "quote": "- Send a heartbeat every 60 seconds; missing heartbeats mark you Dead and trigger re-spawn or cleanup.",
            "detail": "Positive wiring: the system-managed protocol section carries the Ralph loop rules, tool usage, the 60-second heartbeat requirement, escalation and completion signaling, and is not part of the meta-spec registry."
          },
          {
            "file": "crates/gyre-domain/src/workspace.rs",
            "quote": "pub model: Option<String>,",
            "detail": "The Persona entity's operational fields (model, temperature, max_tokens, budget) exist on the struct — the source the spec says system://constraints must draw from."
          },
          {
            "file": "crates/gyre-server/src/api/personas.rs",
            "quote": "persona.temperature = req.temperature;",
            "detail": "The persona operational fields are only ever written/read by the personas CRUD DTOs — no spawn, prompt-assembly or constraints path consumes them, so the specced delivery from persona operational fields does not exist."
          }
        ]
      },
      {
        "id": "a40446f46dbb0536f3c30ed0",
        "decision": "gap",
        "requirement_quote": "Remove `agent.spawn` MCP tool — orchestrators do not spawn agents. They create tasks via `task.create`. The system spawns agents mechanically in response to new implementation tasks.",
        "detail": "The amendments table is dispositioned row by row. Implemented: platform-model.md §3 Task (task_type Implementation/Delegation/Coordination, order, depends_on, parent_task_id); message-bus.md MessageKind::SpecApproved (server-only Event tier, distinct from SpecChanged); platform-model.md §1 Workspace compute_target_id; the Agent entity amendment (AgentStatus enum + status field + task binding as current_task_id: Option<Id> — workers are bound mechanically by the scheduler; orchestrators are deliberately taskless, a documented Option deviation from the specced 'task_id: Id'); agent-gates.md (the approve handler emits SpecApproved after recording the ledger row; MergeAttestation gains meta_specs_used, covered by the EdDSA signature and populated from the spawn-time prompt-set record); abac-policy-engine.md meta_spec and compute_target resource types; human-system-interface.md §1 (the UI meta-specs surface fetches the registry API); and the spec-lifecycle.md stale-terminology replacement ('CEO agent' no longer occurs anywhere in the specs tree). Contradicted or missing: (1) 'Remove agent.spawn MCP tool — orchestrators do not spawn agents' — the live gyre_spawn_worker tool lets a repo-orchestrator JWT spawn worker agents directly (see Phase 3); (2) 'The /api/v1/personas endpoints ... are removed — replaced entirely by /api/v1/meta-specs?kind=meta:persona' — the full personas CRUD plus approve routes remain registered and the Persona entity is still bound to agents at spawn, coexisting with the meta-spec registry (whose CRUD lives at /api/v1/meta-specs-registry, not the specced path — see API); (3) 'Add archive to the action attribute table' — the ABAC action vocabulary contains no 'archive' action and the repo archive/unarchive routes run without middleware ABAC (frozen exemption-file entries with per-handler authorization only); (4) the meta-spec-reconciliation.md row's registry-replacement clauses are not carried out: the old path@sha-pinned meta-spec-set model is fully alive production code — PUT/GET /api/v1/workspaces/:id/meta-spec-set and PUT/GET /api/v1/tenants/:id/meta-specs/set upsert MetaSpecSet{personas: BTreeMap<String, MetaSpecPinnedEntry{path, sha}>, principles, standards, process} into the meta_spec_sets/tenant_meta_spec_sets KV stores, trigger old-model reconciliation (diff_meta_spec_set → run_reconciliation, emit_meta_spec_set_updated) and feed resolve_effective_meta_spec_set, the §10 conformance sweep (run_conformance_sweep compares compute_meta_spec_set_sha of the path@sha set against the provenance-recorded meta_spec_set_sha) and merge-time drift enforcement (mr_meta_spec_drift compares the old-model set SHA, not meta_specs_used); MetaSpecSnapshot does not exist in the amended {meta_spec_id, kind, content_hash, version} form anywhere (grep across crates/: zero matches — the operative pin type remains MetaSpecPinnedEntry{path, sha}), and reconciliation.rs never references meta_specs_used — the specced replacement surface (required flags + spec-level bindings via the meta-spec API) coexists with the old endpoint instead of replacing it, the same amendment class as agent.spawn and /api/v1/personas; (5) the hierarchy-enforcement.md §4 row is not carried out in the amended document: it contains no compute-target or meta-spec CRUD route-table entries and no meta_spec_versions/meta_spec_bindings/compute_targets entries in its tenant-filter configuration, while its KvJsonStore note still lists compute_targets among tenant-less KV namespaces, contradicting the migrated tenant_id-bearing table (migration 000033) — the production substance largely exists, making the un-amended spec text stale rather than pending. Additionally the supersession directive that all cross-references to ralph-loop.md in other specs be updated to agent-runtime.md §1 is not carried out (e.g. specs/system/authorization-provenance.md still cites ralph-loop.md, and specs/system/ralph-loop.md still exists).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"gyre_spawn_worker\" => handle_spawn_worker(&state, &args, &auth).await,",
            "detail": "Amendment (a) contradicted: an agent-spawning MCP tool exists and is available to repo-orchestrator JWTs — orchestrators can spawn agents, which the amendment removes."
          },
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "\"/api/v1/personas\",",
            "detail": "Amendment (h) contradicted: the /api/v1/personas endpoints (create/list/resolve/get/put/delete/approve) are still routed rather than removed — both persona surfaces coexist."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"read\", \"write\", \"delete\", \"approve\", \"spawn\", \"push\", \"merge\", \"escalate\", \"generate\",",
            "detail": "Amendment (g) missing: the ABAC action vocabulary has no 'archive' action, and the archive/unarchive routes bypass middleware ABAC via the frozen exemption file, so the amendment's action-table addition is absent."
          },
          {
            "file": "scripts/abac-route-registry-exemptions.txt",
            "quote": "/api/v1/repos/:id/archive",
            "detail": "The archive route runs without middleware ABAC (exemption-file entry, per-handler authorization only) — the concrete state behind the missing 'archive' action."
          },
          {
            "file": "specs/system/authorization-provenance.md",
            "quote": "- **Ralph Loop** (`ralph-loop.md`): Re-spawn produces `DerivedInput` from same parent, preserving constraint chain (§4.2).",
            "detail": "The supersession directive 'All cross-references to ralph-loop.md in other specs ... should be updated to reference agent-runtime.md §1' is not carried out — other canonical specs still cite the superseded document."
          },
          {
            "file": "crates/gyre-common/src/message.rs",
            "quote": "SpecApproved,",
            "detail": "Positive wiring for the message-bus amendment: MessageKind::SpecApproved exists (server-only Event tier) with the specced payload, distinct from SpecChanged."
          },
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "\"/api/v1/workspaces/:id/meta-spec-set\",",
            "detail": "Amendment (4) — the row's 'PUT /api/v1/workspaces/{id}/meta-spec-set endpoint is replaced by updating required flags and spec-level bindings via the meta-spec API' is not enacted: the old PUT/GET meta-spec-set route (and the tenant twin '/api/v1/tenants/:id/meta-specs/set' at the adjacent registration) is still live production routing alongside the new registry/bindings surface."
          },
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "pub struct MetaSpecPinnedEntry {",
            "detail": "Amendment (4) — 'The MetaSpecSnapshot from reconciliation spec uses content_hash ... struct fields become {meta_spec_id, kind, content_hash, version} tuples instead of path@sha strings': MetaSpecSnapshot does not exist in that form anywhere in crates/ (grep: zero matches); the operative pin type remains MetaSpecPinnedEntry{path, sha} keyed by git-spec path."
          },
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "let summary = crate::reconciliation::run_reconciliation(",
            "detail": "Amendment (4) — a meta-spec-set PUT still drives the old-model reconciliation machinery (diff_meta_spec_set over path@sha pins → emit_meta_spec_set_updated → run_reconciliation), the exact flow the amendment replaces with required-flag changes plus reconciliation triggered on meta-spec approval."
          },
          {
            "file": "crates/gyre-server/src/reconciliation.rs",
            "quote": "let current_set_sha = crate::compute_meta_spec_set_sha(state, &mr.workspace_id).await;",
            "detail": "Amendment (4) — 'Drift detection compares the meta_specs_used array in merge attestations against current required meta-spec versions': merge-time drift (mr_meta_spec_drift) and the §10 conformance sweep compare the old-model set SHA (compute_meta_spec_set_sha vs the provenance-recorded meta_spec_set_sha), never the meta_specs_used array; reconciliation.rs contains no reference to meta_specs_used."
          },
          {
            "file": "specs/system/hierarchy-enforcement.md",
            "quote": "**KvJsonStore gap:** The generic `KvJsonStore` (used for agent_tokens, compute_targets, abac_policies, etc.)",
            "detail": "Amendment (5) — the hierarchy-enforcement.md §4 directives are absent from the amended document: its route table contains no compute-target or meta-spec CRUD entries (no 'compute-target'/'meta-spec' string occurs in the file outside this note), its §3 tenant-filter configuration lists no meta_spec_versions/meta_spec_bindings/compute_targets tables, and this KvJsonStore note still lists compute_targets among tenant-less KV namespaces — contradicting the migrated tenant_id-bearing table."
          },
          {
            "file": "crates/gyre-adapters/migrations/2026-03-26-000033_compute_targets/up.sql",
            "quote": "CREATE TABLE compute_targets (",
            "detail": "Amendment (5) — the production substance exists: compute_targets is a real tenant_id-bearing table (UNIQUE(tenant_id, name), with workspaces.compute_target_id referencing it), so hierarchy-enforcement.md's un-updated route table and tenant-less-KV note are stale spec text, not pending work."
          }
        ]
      }
    ],
    "source_evidence": [
      {
        "file": "crates/gyre-server/src/mcp.rs",
        "sha256": "00a5625653694977198dede81025bc248022b768f17959dc92eb05b6f8fc6733"
      },
      {
        "file": "crates/gyre-server/src/signal_chain.rs",
        "sha256": "fe55371b80328824dee46be8e931fdd1b4a10a2e48da686b19557f81ea810a82"
      },
      {
        "file": "crates/gyre-server/src/api/tasks.rs",
        "sha256": "bd62eeaca091ddddf920b78a5dc708750f76e1485baac31def92a6a3c128f81e"
      },
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "sha256": "036b32975f4110d6aa35cef408441c0be5ecb4cef4a9a7504eee9caf08a8bc35"
      },
      {
        "file": "crates/gyre-server/src/spec_registry.rs",
        "sha256": "187a311bccfff72faea7b86d5225210383ab7031f1c87762fdb1d677c60b5cec"
      },
      {
        "file": "crates/gyre-server/src/api/specs.rs",
        "sha256": "b6391c5208b1cfe511c0d41b69f7e14e08472d2b597655e299dfcb7e0eac8ca0"
      },
      {
        "file": "crates/gyre-common/src/message.rs",
        "sha256": "dea344a7ab04b3f2bd754e1335f9143184c5a02508f0763ebd7638f17d735558"
      },
      {
        "file": "crates/gyre-server/src/api/orchestrator.rs",
        "sha256": "7057b033d8208c4575a1c2c30915a4ee18bffb47af0033f10e88f98165cf45e1"
      },
      {
        "file": "crates/gyre-server/src/auth.rs",
        "sha256": "457a4f1d2cb8605ae871a353943f77c8387b2e5f8c9b7d326eed97231024c0e7"
      },
      {
        "file": "crates/gyre-server/src/api/spawn.rs",
        "sha256": "80fad25139b5d3c24ea43fd4cfced0c896cf9fa92b65a4fd8f10bd35b197b783"
      },
      {
        "file": "crates/gyre-domain/src/agent.rs",
        "sha256": "3b5a49231eece5b6dbfdd468c25be9f00e2b38341cb21997b1afefbe4d53d519"
      },
      {
        "file": "crates/gyre-server/src/stale_agents.rs",
        "sha256": "8994395ae9ebdfcb0d610b99aac063615eac8bd323a5200750aa13345089b15f"
      },
      {
        "file": "crates/gyre-server/src/gate_executor.rs",
        "sha256": "38d34c49f5e0afbc401f4e4d239cbbe0e82d3f9f062e1a9391b7c185c37fe4ab"
      },
      {
        "file": "crates/gyre-server/src/api/budget.rs",
        "sha256": "5cb094810ac18ea6844575d8a4aba183dba5568d784a5425349b7762fd153e06"
      },
      {
        "file": "crates/gyre-server/src/ralph_loop.rs",
        "sha256": "94d90084cb6a3072ab49b6b02959f7afae7d833713f87c3c09d214f56818996f"
      },
      {
        "file": "crates/gyre-server/src/reconciliation.rs",
        "sha256": "ba4e0274450ee7685196b8695c0994600ac332a14c733be5165e8933336a5e21"
      },
      {
        "file": "crates/gyre-domain/src/meta_spec.rs",
        "sha256": "354f5db5c971cd6b59f1c543126d10fd5412f88e0f7e55b74d59546cfb678da4"
      },
      {
        "file": "crates/gyre-adapters/migrations/2026-03-26-000032_meta_specs/up.sql",
        "sha256": "17798fe21887f0e21c48fcc2bdb4af53b7778b89f35fff38598c53bba869cf88"
      },
      {
        "file": "crates/gyre-server/src/api/meta_specs.rs",
        "sha256": "9dc0e4539a1f21da70590a1249acb5814babb8a893d942e486c248367eb11d08"
      },
      {
        "file": "crates/gyre-server/src/prompt_assembly.rs",
        "sha256": "f02c2ec7b852145682f0e9c356de0be3098e336954d48aa7c368cfb98b6b8000"
      },
      {
        "file": "web/src/locales/en.json",
        "sha256": "ab882ab5f8fe3fd33a4eb8bcb6b81c2e9820cf2eaf75af86a2bc75a576d14c44"
      },
      {
        "file": "crates/gyre-domain/src/attestation.rs",
        "sha256": "a8954208e773442481f6fd3d774571ac77a9e5316b77e0bb34ea38d1072b5e23"
      },
      {
        "file": "crates/gyre-server/src/merge_processor.rs",
        "sha256": "42ad3821e16bc5e58a55a8edc2dfd8714b901fe397466b79b78a4213980dbbce"
      },
      {
        "file": "crates/gyre-domain/src/spec_ledger.rs",
        "sha256": "e3605fde2bdc006a1301d336d38743777cdf2834d3c483141c6c662f024aee67"
      },
      {
        "file": "crates/gyre-server/src/jobs.rs",
        "sha256": "922f3de73788564bd7c4b4ca48b065f2bba6149c7b651aaf108c02b667c559b4"
      },
      {
        "file": "crates/gyre-server/src/lib.rs",
        "sha256": "a23a71630b3302002b50d4dbc1f256cf95c2deeed11f69df4a02b083410f2771"
      },
      {
        "file": "web/src/lib/api.js",
        "sha256": "de3d8147c3ebaad027847809ebcf72a5ad6286e6983544f715931c307c927740"
      },
      {
        "file": "crates/gyre-ports/src/compute.rs",
        "sha256": "c67c91d31ee7fdd6332bf2d36b9fade006eb805d7be86817d9f4e546498ec725"
      },
      {
        "file": "crates/gyre-adapters/src/compute/ssh.rs",
        "sha256": "7a286dbaafdb9690fd98b2cd45743ee796f022e39b1ab873d475c84fa760e3c9"
      },
      {
        "file": "crates/gyre-adapters/src/compute/container.rs",
        "sha256": "d1d3728d3e77f9a4668a4d3cc10a7500d0fcf8e86408b0b84c5a1bbd6b3c6f84"
      },
      {
        "file": "crates/gyre-adapters/src/compute/kubernetes.rs",
        "sha256": "a0da3808c0a79790e512822df3c9632dc0fed1ddadd37cd641ef60122307a777"
      },
      {
        "file": "docker/gyre-agent/Dockerfile",
        "sha256": "ca5c2bc181070157ef829fde753b5ea9688b03ce4a010cee2b4cbb4ed75eec45"
      },
      {
        "file": "docker/gyre-agent/flake.nix",
        "sha256": "213f5ceb95b9159511667325cb5fd89d89917c0f0433134485993f2393ab635c"
      },
      {
        "file": "crates/gyre-domain/src/workspace.rs",
        "sha256": "ec60899067451e201133c32606309f1f8e4db8cc9d1a154723cb05981de6d379"
      },
      {
        "file": "crates/gyre-server/src/api/compute_targets.rs",
        "sha256": "e129e133ca66eca5ece72edaf86dae5c4c33d6047df325cfe3c1d1f6a34c1750"
      },
      {
        "file": "crates/gyre-server/src/api/repos.rs",
        "sha256": "0dc64ee357c339c93e3ee19b279f9e9e0268251e1edbf3680848f863acc48568"
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
        "file": "crates/gyre-server/src/api/personas.rs",
        "sha256": "c8fd41db880afa8f6608e122687838cbceec6c6c6a140e8b80f0e8eab3b31e12"
      },
      {
        "file": "scripts/abac-route-registry-exemptions.txt",
        "sha256": "0789c85cf261c0f0c39c9e22bd2711581436792e6686d2ddb0187c9f10d07b06"
      },
      {
        "file": "specs/system/authorization-provenance.md",
        "sha256": "5632422e51d664309b59a255ca308b20cca69bd12d9c874dc3fe3941208e97a3"
      },
      {
        "file": "specs/system/hierarchy-enforcement.md",
        "sha256": "e458f8c1230e2860a249a222c8bf6481c38aaaea91b0d36ebce128d32225872b"
      },
      {
        "file": "crates/gyre-adapters/migrations/2026-03-26-000033_compute_targets/up.sql",
        "sha256": "44cfc15844df475be2a05eb34fc8f8205cc35393eed5cd62a2d8193aa0875b81"
      }
    ]
  },
  "plan": {
    "audit": "e33f32a4ae617450e6d92a8bc3d49295a1aa4bb5b5aad4ced1c9de95068109dd",
    "tasks": [
      {
        "key": "approval-single-trigger",
        "title": "Gate all implementation work on approved specs",
        "covers": [
          "1bd201625dc5fbe0164ced05"
        ],
        "required_behavior": "Per agent-runtime.md §1 The Signal Chain: 'There is no other trigger. The SpecApproved event is the universal starting signal.' (1) Task-creation gate: the HTTP task-create handler (crates/gyre-server/src/api/tasks.rs) and the MCP gyre_create_task tool (crates/gyre-server/src/mcp.rs) must reject creating a task with task_type 'implementation' unless its spec_ref resolves to a spec that is currently Approved in the spec approval ledger at exactly the pinned SHA (path@sha); unapproved, Pending, invalidated, or unresolvable spec_ref yields a 4xx error (HTTP) / tool error (MCP) naming the requirement, and no task is created. Delegation and Coordination tasks and untyped (pre-approval push-hook) tasks are unaffected — the orchestrator chain creates them from SpecApproved and pins the approved SHA, so the legitimate chain passes the gate. (2) Scheduler gate (defense in depth): the Phase-4 scan in crates/gyre-server/src/signal_chain.rs must, before claiming a Backlog Implementation task and spawning a worker, verify the task's spec_ref still resolves to an Approved ledger entry at the pinned SHA (an approval invalidated by a later push must stop the spawn); on failure the task stays Backlog with an observable skip label and a logged reason — no agent record, no InProgress transition. (3) Direct-spawn gate: POST /api/v1/agents/spawn must no longer create task-bound implementation worker agents for external callers; the endpoint remains for Phase-8 system-initiated kinds (interrogation via agent_type, and equivalent system-initiated spawns), and a request that would create a worker bound to an Implementation task is rejected 403 for non-scheduler callers. The already-satisfied approval-side behavior (ledger row, SpecApproved emission, modified-spec invalidation to Pending) must be preserved unchanged.",
        "acceptance": [
          "POST /api/v1/tasks (and MCP gyre_create_task) with task_type=implementation and a spec_ref whose spec has no Approved ledger entry at that SHA returns 4xx and creates no task; the identical request after POST /api/v1/specs/:path/approve succeeds and the created task enters the normal chain.",
          "With an Implementation/Backlog task referencing an unapproved spec inserted directly into storage, one scheduler cycle creates zero agent records and leaves the task Backlog, while a twin task referencing an approved spec spawns exactly one worker.",
          "POST /api/v1/agents/spawn with the task_id of an Implementation task from a human or agent principal returns 403; spawning an interrogation agent (agent_type=interrogation) still succeeds."
        ],
        "negative_probe": "Insert directly into task storage a Backlog task with task_type=implementation and spec_ref 'specs/x.md@<sha>' where the spec ledger shows approval_status Pending; run one scheduler cycle; assert no agent record exists and the task is not InProgress. At the base this fails: the Phase-4 scan (signal_chain.rs) spawns a worker for ANY Backlog Implementation task and never consults the approval ledger, and gyre_create_task / POST /api/v1/tasks accept task_type=implementation with no approval precondition (gyre_create_task's only pre-check is target validation) — implementation work starts with no human spec approval anywhere in the chain.",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "orchestrator-boundary-amendments",
        "title": "Remove orchestrator agent-spawning and carry out the spec amendment table",
        "covers": [
          "15aec83b58b3494602d31ce8",
          "a40446f46dbb0536f3c30ed0"
        ],
        "required_behavior": "Per agent-runtime.md §1 Phase 3 ('The repo orchestrator only creates tasks. It never spawns agents.') and the Relationship to Existing Specs amendments table. (1) Remove the gyre_spawn_worker MCP tool entirely — catalog entry, tools/call dispatch, scope validation, prompt references (api/prompts.rs, api/spawn.rs) and tests — so no orchestrator principal can create agent records through any MCP surface; the mechanical scheduler is the only task→agent path. The existing decomposition machinery (Delegation detection, per-repo mutex one-live semantics, sub-tasks carrying spec_ref/parent_task_id/order/depends_on, LLM judgment with deterministic fallback) is preserved unchanged. The needs_write RBAC gate membership derives from handler effects, so removing the handler removes the membership (scripts/check-mcp-write-tools.sh stays green). (2) Remove the /api/v1/personas routes (CRUD, resolve, approve) — 'replaced entirely by /api/v1/meta-specs?kind=meta:persona'. The meta-spec registry persona kind must carry the Persona operational fields (model, temperature, max_tokens, budget) so they remain editable after the personas routes are removed, and spawn-time persona resolution/binding keeps working against registry-backed persona data; all consumers (web client, CLI, tests, ABAC/RBAC registrations for the removed routes) are updated, and the frozen exemption-file count only shrinks. (3) Add the 'archive' action to the ABAC action vocabulary and register the repo archive/unarchive routes with the middleware resolver (removing their entries from scripts/abac-route-registry-exemptions.txt) so archive/unarchive get real policy evaluation, keeping scripts/check-abac-route-registry.sh green. (4) Supersession cleanup per the spec's own directive: update every remaining live cross-reference to ralph-loop.md in specs/ and docs/ to reference agent-runtime.md §1 (e.g. specs/system/authorization-provenance.md) and retire specs/system/ralph-loop.md as fully superseded — carrying out the canonical spec's supersession statement, not altering any contract. (5) hierarchy-enforcement.md §4 amendment carry-out: add the compute-target and meta-spec CRUD routes to its route table with their auth-context notes (compute-target routes use tenant context from auth, no path param; meta-spec routes use scope_id from query param/body), add meta_spec_versions/meta_spec_bindings/compute_targets to its tenant-filter configuration, and correct the stale KvJsonStore note that still lists compute_targets among tenant-less KV namespaces (it is a tenant_id-bearing table since migration 000033). (6) The meta-spec-reconciliation.md row's registry-replacement clauses — retiring the workspace/tenant meta-spec-set endpoints, migrating their consumers to the DB-backed registry model, meta_specs_used-based drift detection, and {meta_spec_id, kind, content_hash, version} snapshot tuples — are delivered by bindings-registry-cascade, on which this task depends; this task's close-out verifies those clauses are carried out (its acceptance includes them), so the amendments-table gap closes only when every row is done.",
        "acceptance": [
          "The MCP tools catalog no longer lists gyre_spawn_worker and tools/call with that name returns an error; a repo-orchestrator JWT has no MCP tool whose handler creates agent records.",
          "GET/POST/PUT/DELETE /api/v1/personas and /api/v1/personas/resolve return 404; persona creation, editing (including operational fields and approval), and spawn-time persona resolution work via the meta-spec registry endpoints with kind=meta:persona.",
          "A Deny policy with action 'archive' on a repo resource blocks repo archive for the affected principal through the middleware (real policy evaluation), and the repo archive/unarchive routes are covered by RouteResourceMapping entries with no exemption-file entries.",
          "grep for 'ralph-loop.md' across specs/ and docs/ returns no live normative cross-references; specs/system/ralph-loop.md no longer exists as a live spec.",
          "hierarchy-enforcement.md's route table and tenant-filter configuration name the compute-target and meta-spec CRUD routes/tables with their auth-context notes, and its KvJsonStore note no longer misdescribes compute_targets storage.",
          "Carried from the dependency: PUT/GET /api/v1/workspaces/:id/meta-spec-set and PUT/GET /api/v1/tenants/:id/meta-specs/set return 404/405 with no production writer to the meta_spec_sets/tenant_meta_spec_sets stores remaining, and merge-time drift enforcement compares merge-attestation meta_specs_used against current required meta-spec versions."
        ],
        "negative_probe": "Mint a repo-orchestrator JWT and call MCP tools/call gyre_spawn_worker with a valid implementation task id — at the base this spawns a real worker agent through spawn_agent_core (worktree, scoped JWT, compute-target resolution), exactly the 'orchestrator spawns agents' capability the spec removes ('Remove agent.spawn MCP tool — orchestrators do not spawn agents'); after the change the tool must not exist and no agent record may be creatable by an orchestrator principal through any MCP surface. At the same base the amendments table is otherwise un-carried-out, all provable in one sweep: GET /api/v1/personas returns 200 (routes still registered at api/mod.rs:1002-1013), a Deny policy with action 'archive' never blocks repo archive because the archive/unarchive routes sit in the frozen exemption file (scripts/abac-route-registry-exemptions.txt:30,34) and the ABAC action vocabulary has no 'archive' action, hierarchy-enforcement.md contains zero compute-target/meta-spec route-table or tenant-filter entries while its KvJsonStore note still lists compute_targets as tenant-less, and specs/system/ralph-loop.md still exists with live cross-references (e.g. specs/system/authorization-provenance.md).",
        "dependencies": [
          "bindings-registry-cascade"
        ],
        "existing_task": null
      },
      {
        "key": "mcp-resource-knowledge-boundary",
        "title": "Scope MCP agents and queue resources to callers",
        "covers": [
          "de73e06638559fcca67b3c5d"
        ],
        "required_behavior": "Per agent-runtime.md §1 Phase 5: 'The agent has no knowledge of other agents, the merge queue, or the broader system state.' (1) Worker agents must not be able to read agents:// or queue:// at all: the MCP resources/list advertisement must omit these resources for worker-tier (task-scoped worker JWT) principals and resources/read must return an error result for them — the boundary is mechanical, not prompt-level. (2) For principals who may read them, per-handler caller-scope enforcement replaces the current unbounded reads in crates/gyre-server/src/mcp.rs: an unscoped agents:// read resolves against the caller's accessible workspaces (a member of workspace A never receives workspace B agents — no cross-workspace enumeration); agents://{workspace_id} requires the caller to have access to that workspace; queue://{repo_id} requires the caller to have access to that repo (load the repo, compare its workspace against the caller's scope, Forbidden on mismatch — per the exempt-handler authorization rule). Orchestrator principals are confined to their JWT's workspace/repo scope. The advertised resource descriptions are updated so they no longer promise 'agents:// for all'.",
        "acceptance": [
          "A worker agent JWT issuing MCP resources/read for agents:// and for queue://{any repo} receives an error result, and its resources/list contains neither resource.",
          "A user who is a member of only workspace A performing an unscoped agents:// read sees only workspace A agents even while workspace B has active agents; reading queue://{repo of workspace B} returns Forbidden for that user.",
          "A workspace-scoped orchestrator reading agents:// sees only its own workspace's agents."
        ],
        "negative_probe": "Create active agents in two workspaces, authenticate as a plain member of workspace A, and issue MCP resources/read with uri 'agents://' — at the base the handler calls state.agents.list() with no caller-scope filter and returns every Active/Idle agent across ALL workspaces (a cross-workspace enumeration leak, with the resource advertised as 'agents:// for all'), so workspace B agent ids appear in the response and the probe fails; the same unscoped dispatch also lets any worker agent read the merge queue, violating the Phase-5 knowledge boundary mechanically.",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "spec-rejection-grace-and-iteration-tier",
        "title": "Real spec-rejection grace period and workspace iteration tier",
        "covers": [
          "e60f62cd06826e5e4de9decf"
        ],
        "required_behavior": "Per agent-runtime.md §1 Phase 7 'Spec rejection mid-flight'. (1) The reject handler (crates/gyre-server/src/api/specs.rs) must implement the specced sequence instead of the current synchronous stop: cancel the in-flight tasks and delegation task, send the shutdown message, then start a real 60-second grace timer using the same mechanism as the budget path (start_budget_grace_timer); agents that commit and call agent.complete within the grace period are marked Idle; at expiry the server kills remaining agents via ComputeTarget::kill_process(), marks them Stopped, and revokes their JWTs; the terminal worktree cleanup (cleanup_worktrees_for_task) runs only after the kill, preserving the agent-workflow terminal-cleanup contract (coordinate with the active agent-workflow plan's terminal-worktree-cleanup task, which owns ephemeral-worktree teardown on terminal states). The existing priority-2 'Spec rejected' notification fanout and task cancellation are preserved. (2) Iteration-ceiling middle tier: add max_agent_iterations: Option<u32> to the Workspace entity (next free migration number, port contract enforced in EVERY adapter including the mem adapter per check-mem-port-contracts), settable via the workspace update API; resolution becomes repo.max_agent_iterations → workspace.max_agent_iterations → server default 10, applied consistently in the Ralph-loop ceiling check and every surface that renders the ceiling (constraints prompt, spawn).",
        "acceptance": [
          "On POST reject with an active implementing agent that does not complete: the agent is still Active within the grace window and has the shutdown message in its inbox; after 60 seconds its process is killed (kill_process invoked on the compute target), its status is Stopped, its JWT is rejected on next use, and its worktree teardown happens after the kill. An agent that completes within the grace window is marked Idle.",
          "A repo with max_agent_iterations unset in a workspace with max_agent_iterations=3 reaches the ceiling (task Blocked, agent Failed, TaskBlocked message to the repo orchestrator) after 3 iterations, not 10; with both unset the ceiling remains 10.",
          "Workspace max_agent_iterations is settable and readable via the workspace API and persisted durably (survives restart)."
        ],
        "negative_probe": "Spawn an agent on a task whose spec is then rejected, keep the agent from completing, and inspect state 5 seconds after reject — at the base the handler has already synchronously transitioned the agent to Stopped and torn down its worktree (api/specs.rs transition_status(Stopped) runs before any grace handling; the 'grace_period_secs': 60 exists only as payload text sent to an already-Stopped agent; no timer, no kill_process, no JWT revocation runs on this path, in contrast to the real budget grace timer), so the 'still Active within the grace window, killed only at expiry, token revoked' assertions fail. Separately, set workspace max_agent_iterations=3 with the repo field unset and drive gate failures — at the base the ceiling resolves repo → server default 10 because the Workspace entity has no max_agent_iterations field (only Repository does, crates/gyre-domain/src/repository.rs:57), so the task is not Blocked at 3 iterations.",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "meta-spec-api-contract",
        "title": "Meta-spec API at specced routes with scope-level admin auth",
        "covers": [
          "917eb99ea432ee472c349487",
          "54a514da8df9bf4d236e09ff"
        ],
        "required_behavior": "Per agent-runtime.md §2 API table and §2 Required vs Optional. (1) Route contract: serve the registry CRUD at the specced paths — GET/POST /api/v1/meta-specs, GET/PUT/DELETE /api/v1/meta-specs/:id, GET /api/v1/meta-specs/:id/versions, GET /api/v1/meta-specs/:id/versions/:version — by moving the existing handlers from /api/v1/meta-specs-registry with their semantics unchanged (list filters ?scope=&scope_id=&kind=&required=; create from body scope/scope_id; update bumps version with history retained; delete 409 when spec bindings reference it; version endpoints). The /api/v1/meta-specs/:path/blast-radius route stays (no structural conflict). ABAC RouteResourceMapping entries, RBAC tests, internal callers (reconciliation.rs), and the web API client are updated to the new paths; the old /meta-specs-registry paths are removed with no silent alias, and scripts/check-abac-route-registry.sh stays green with the exemption file only shrinking. (2) Scope-level authorization: POST and PUT on registry entries (including flipping required in either direction) are authorized by the meta-spec's own scope — Global/tenant-scope meta-specs writable only by tenant Admin; workspace-scope meta-specs writable by that workspace's Owner/Admin (tenant Admin retains bypass). A workspace Owner/Admin writing a tenant-scope meta-spec (including required) is rejected 403 — the privilege-escalation direction the spec warns about stays impossible; the currently missing workspace-admin authority on workspace meta-specs now exists and the misleading 'outer write gate' comment (api/meta_specs.rs:8993) is replaced by real per-scope enforcement. DELETE keeps its existing scope-aware gate.",
        "acceptance": [
          "GET /api/v1/meta-specs?scope=workspace&scope_id={id}&kind=meta:persona returns the filtered list (at the base this path 404s — only /meta-specs-registry serves it); PUT /api/v1/meta-specs/:id bumps the version and retains history; DELETE /api/v1/meta-specs/:id returns 409 while a spec binding references it; GET /api/v1/meta-specs/:id/versions/:version returns that version's content.",
          "A workspace Owner (not tenant Admin) successfully PUTs required=true on a workspace-scope meta-spec; the flag persists and the meta-spec is injected into agents spawned in that workspace — at the base this is rejected because the write gate is tenant-Admin for every scope.",
          "The same workspace Owner PUT (any field, including required) on a Global-scope meta-spec returns 403."
        ],
        "negative_probe": "Authenticate as a workspace Owner and PUT /api/v1/meta-specs/{id of a workspace-scope meta-spec} with {\"required\": true} — at the base the registry write gate requires the platform tenant Admin role for every scope (the 'outer write gate already enforces this for every PUT' comment at api/meta_specs.rs:8993 describes a gate that is tenant-Admin for all scopes), so the specced workspace-admin authority returns 403 and the probe fails; after the change it must succeed and newly spawned agents in that workspace must receive the meta-spec in their required band. At the same base, GET /api/v1/meta-specs?scope=workspace&... returns 404 because the CRUD surface lives only at /api/v1/meta-specs-registry (api/mod.rs:977-991).",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "bindings-registry-cascade",
        "title": "Spec bindings as registry metadata with re-approval cascade, UI panel, and meta-spec-set retirement",
        "covers": [
          "e97ea9a092758dc93c245e6e",
          "9f95c49cf209205750a0177a"
        ],
        "required_behavior": "Per agent-runtime.md §2 Spec-Level Binding ('Stored in the spec registry alongside other spec metadata') and Stale Pin Detection ('Human updates the pin → spec metadata changes → spec SHA changes → spec needs re-approval'), plus the amendments-table meta-spec-reconciliation.md row's registry-replacement clauses. (1) Storage: the binding set becomes part of the spec registry metadata covered by the spec SHA recorded in the approval ledger — updating a pin via PUT /api/v1/specs/:path/meta-spec-bindings produces a new registry SHA for the spec, resets approval_status to Pending (invalidating the prior approval exactly as a content edit does, reusing the existing invalidation machinery), and clears stale-pin dedup keys. The endpoint's existing validation semantics (pin-version-exists, cross-workscope leak guard, role gating) are preserved. (2) Cascade: after a human re-approves, the existing chain fires unchanged — SpecApproved → workspace orchestrator → delegation task → re-implementation with the newly pinned prompt versions; no new trigger mechanism is invented. The hourly stale-pin check keeps producing priority-6 drift alerts, and acting on an alert now genuinely forces re-approval before any agent runs with the new pins. (3) UI: the Specs view Edit flow gains the meta-spec bindings panel — it lists available meta-specs from the tenant and workspace registries, shows required ones pre-selected and locked (cannot be removed), allows selecting optional ones and their pinned versions, and saves via the bindings endpoint; the web API client gains the corresponding calls against the canonical /api/v1/meta-specs contract. (4) Meta-spec-set retirement (amendments table, meta-spec-reconciliation.md row: 'The PUT /api/v1/workspaces/{id}/meta-spec-set endpoint is replaced by updating required flags and spec-level bindings via the meta-spec API'): retire PUT/GET /api/v1/workspaces/:id/meta-spec-set and PUT/GET /api/v1/tenants/:id/meta-specs/set (404/405, no silent alias) and remove every production writer to the meta_spec_sets/tenant_meta_spec_sets KV stores. Migrate the remaining consumers to the DB-backed registry model (required flags + spec-level bindings): reconciliation effective-set resolution (resolve_effective_meta_spec_set), the §10 conformance sweep (run_conformance_sweep), and the user-query prompt context llm_helpers::workspace_meta_spec_context ({{meta_spec_context}} in specs/assist, briefing/ask, explorer/generate). Reconciliation triggering by required-flag changes and meta-spec approval (the existing meta-spec approval handler trigger) is preserved so the replacement surface drives the same behavior the old endpoint drove. Merge-time drift enforcement compares the merge-attestation meta_specs_used array against current required meta-spec versions (replacing the old-model compute_meta_spec_set_sha comparison in merge_processor.rs and reconciliation.rs), and snapshot-shaped data is produced as {meta_spec_id, kind, content_hash, version} tuples rather than path@sha MetaSpecPinnedEntry strings. Coordination: the active ui-layout plan's llm-endpoint-contract-completion task builds its LLM-endpoint prompt-context injection on the workspace meta-spec-set mechanism — this retirement must migrate that context source to the registry model and must not silently invalidate that plan's baseline; the spec-lifecycle plan's spec-lifecycle-sha-pin task owns the push-hook blob-SHA pinning — this task changes only the registry-metadata/SHA semantics for binding edits, not the push hook.",
        "acceptance": [
          "Updating a pin on an approved spec changes the spec's registry SHA and sets approval_status to Pending; no worker agent is spawned for tasks referencing that spec until it is re-approved; after re-approval a SpecApproved event and delegation task are produced (existing chain) and newly spawned agents receive the meta-spec content at the new pinned version in prompt-assembly band 3.",
          "The UI bindings panel renders required meta-specs pre-selected and locked, allows saving an optional selection with a pinned version, and the saved set is returned by GET /api/v1/specs/:path/meta-spec-bindings and consumed by prompt assembly.",
          "The drift alert → pin update → re-approval → re-implementation sequence is demonstrable end to end: a spec pinning v1 of a meta-spec now at v2 gets a priority-6 alert, the pin update forces re-approval, and only post-approval agents use v2.",
          "PUT/GET /api/v1/workspaces/:id/meta-spec-set and PUT/GET /api/v1/tenants/:id/meta-specs/set return 404/405 and no production code path writes meta_spec_sets/tenant_meta_spec_sets; no production reference to diff_meta_spec_set/compute_meta_spec_set_sha remains.",
          "Changing a required flag (or approving a new version of a required meta-spec) drives the same reconciliation sweep the old set-PUT drove, and llm_helpers {{meta_spec_context}} for user-initiated LLM queries resolves from the registry's effective set.",
          "Merge-time drift enforcement compares meta_specs_used against current required meta-spec versions: an attestation recorded against an older required version is flagged, and snapshot data exchanged on that path is shaped {meta_spec_id, kind, content_hash, version}."
        ],
        "negative_probe": "Approve a spec with a binding pinned to v1 of a meta-spec now at v2, then update the pin to v2 via the API and inspect the spec ledger — at the base put_spec_meta_spec_bindings (api/meta_specs.rs:9432) writes only the meta_spec_bindings side table: the spec's SHA and approval_status are unchanged, no re-approval is required, and no SpecApproved/delegation/re-implementation follows (the specced cascade never fires because the ledger SHA is the git blob SHA of the markdown file and does not cover bindings). For the retirement: PUT /api/v1/workspaces/:id/meta-spec-set at the base returns 200 and drives the old-model machinery (diff_meta_spec_set over path@sha pins → emit_meta_spec_set_updated → run_reconciliation), llm_helpers::workspace_meta_spec_context resolves the path@sha effective set for user queries, and reconciliation.rs contains no reference to meta_specs_used — after the change all three assertions must fail at the base paths (endpoint 404, context registry-sourced, drift model meta_specs_used-based).",
        "dependencies": [
          "meta-spec-api-contract"
        ],
        "existing_task": null
      },
      {
        "key": "persona-constraints-delivery",
        "title": "Deliver persona operational fields in constraints prompt",
        "covers": [
          "47d309e09aac1e2e04d334d0"
        ],
        "required_behavior": "Per agent-runtime.md §5: 'system://constraints delivers budget limits and operational parameters from the persona's operational fields.' prompt_constraints (crates/gyre-server/src/mcp.rs:4523) and the spawn-time constraints injection must include a persona operational-parameters section sourced from the persona bound to the agent — model, temperature, max_tokens, and budget — read from real storage via the canonical persona surface established by the registry replacement (the meta-spec registry persona kind carrying the Persona operational fields, once /api/v1/personas is removed by orchestrator-boundary-amendments). The existing sections (workspace budget, trust level, scope boundaries, agent lifetime, iteration ceiling) remain; agents with no bound persona report that explicitly rather than fabricating defaults. The values must be the stored persona's values — a change to the persona's operational fields is reflected on the next constraints read or spawn.",
        "acceptance": [
          "For an agent spawned with a persona whose operational fields are set (e.g. model X, temperature 0.2, max_tokens 8192, budget 500000), the system://constraints MCP prompt contains those exact values read from storage.",
          "Editing the bound persona's operational fields changes the served constraints on the next read/spawn (no cached or hardcoded stand-ins).",
          "An agent with no bound persona receives an explicit 'no persona bound' statement in the operational-parameters section, not fabricated defaults."
        ],
        "negative_probe": "Set a bound persona's max_tokens to a distinctive value (e.g. 12345) in storage, then read system://constraints for that agent — at the base the response contains only scope boundaries, trust level, workspace budget, lifetime, and iteration-ceiling values drawn from other sources (prompt_constraints at mcp.rs:4523 assembles exactly those sections); the Persona operational fields (model, temperature, max_tokens, budget) are consumed by no execution path anywhere (only CRUD DTOs reference them), so the distinctive value is absent and the probe fails at the base.",
        "dependencies": [
          "orchestrator-boundary-amendments"
        ],
        "existing_task": null
      },
      {
        "key": "agent-image-single-definition",
        "title": "Retire Dockerfile and complete backend security defaults",
        "covers": [
          "3ee23b90e9236c59b010822f",
          "ebd16b0ebf8506b9c84eef25"
        ],
        "required_behavior": "Per agent-runtime.md §3 Nix-Based Image Build ('One definition, all targets. The Dockerfile is replaced by the Nix flake.') and Supported Backends ('Container security defaults (all backends)'). (1) Delete docker/gyre-agent/Dockerfile and every reference that builds or instructs building it: the spawn-failure guidance in crates/gyre-server/src/api/spawn.rs must direct operators to the Nix targets (nix build .#agent-image-docker then docker load, or #agent-image-tar for air-gapped transfer) instead of 'docker build -t gyre-agent:latest docker/gyre-agent/'; CI/scripts/docs referencing the Dockerfile are updated. Nothing in the production tree may build or direct operators to a non-reproducible agent image that can diverge from the attested one. (2) Container security defaults on all backends: the Kubernetes backend must enforce the defaults it currently lacks — a PID limit equivalent to pids-limit=512 expressed in the created Pod objects via the supported PID-limiting mechanism, and default network isolation equivalent to --network=none (e.g. a default-deny NetworkPolicy the platform creates alongside the Pod, with opt-in to bridged networking via compute target config) — while Container and SSH backends retain the full set (--network=none, --memory=2g, --pids-limit=512, --user=65534:65534, worktree volume). For the worktree volume mount on remote backends, the task must either provision the worktree content as a volume on the remote target or, if a default is demonstrably unsatisfiable on a remote backend, record the concrete technical evidence and process the amendment through the spec lifecycle — a code-comment deviation with no authorization is not an acceptable closure. The image-digest attestation flow (wl_image_hash pre-spawn, pod imageID post-spawn) is preserved.",
        "acceptance": [
          "docker/gyre-agent/Dockerfile does not exist; grep across the repo for agent-image build guidance returns only Nix-based instructions; nix build of the three flake targets still succeeds (flake.nix/flake.lock unchanged in substance).",
          "A Pod created by the Kubernetes backend carries a 512-equivalent PID limit and default-deny network isolation in the created API objects, with network access beyond that only when the compute target config opts in.",
          "Container and SSH backends still spawn with --network=none, --memory=2g, --pids-limit=512, --user=65534:65534 and the worktree volume (Container) — regression-tested, not just asserted."
        ],
        "negative_probe": "Trigger a container spawn with the agent image missing and read the server log, and list docker/gyre-agent/ — at the base the log instructs 'docker build -t gyre-agent:latest docker/gyre-agent/' (api/spawn.rs:1771) and the Dockerfile exists, building a materially different unpinned node:22-alpine image from the bit-reproducible flake image, so a default deployment following the server's own guidance runs agents on a non-reproducible image that diverges from the attested wl_image_hash; after the change both the file and the guidance must be gone. Additionally, inspect a Pod created by the Kubernetes backend — at the base it carries no PID limit and allows DNS egress with no platform-created network policy (kubernetes.rs:137-189 documents the deferrals in comments only), so the '(all backends)' security-defaults assertions fail.",
        "dependencies": [],
        "existing_task": null
      }
    ]
  },
  "assigned_names": {
    "approval-single-trigger": "task-297",
    "orchestrator-boundary-amendments": "task-298",
    "mcp-resource-knowledge-boundary": "task-299",
    "spec-rejection-grace-and-iteration-tier": "task-300",
    "meta-spec-api-contract": "task-301",
    "bindings-registry-cascade": "task-302",
    "persona-constraints-delivery": "task-303",
    "agent-image-single-definition": "task-304",
    "_publication": "task-305",
    "_base": "bacba2b29a43014b1f150787d37f159beefe919a"
  }
}

## Acceptance Criteria

- [ ] Every planned task faithfully implements its assigned canonical requirements.
- [ ] No duplicate active assignment or missing dependency is introduced.
- [ ] No product behavior, spec requirement, coverage status or enforcement is changed.
- [ ] New tasks remain not-started until this backlog publication merges.

## Shipped

Publication of the reviewed agent-runtime backlog, rebased onto the updated main base (077d942) that carried the dependency-graph (task-286..294) and explorer-canvas (task-306/307) backlogs.

### What was done

- Resolved the `specs/task-spec-inputs.json` merge conflict by keeping both sides: this plan's 12 `backlog/<section-id>` entries plus its 8 `Spec gap spec-41a92c20354c6a108e6074e3 / <key> / 4a3f940495a8` bindings, and main's explorer-canvas entry/binding. The resolved registry is the exact union of main's and the publication commit's sections/bindings (verified programmatically; nothing dropped), and `python3 json.load` validates it.
- Created `specs/tasks/task-297.md` through `specs/tasks/task-304.md` exactly per the reviewed plan (publication commit 93a834af, unchanged by the merge): titles, `spec_ref` gap keys, `depends_on` ([task-298]→[task-302], [task-302]→[task-301], [task-303]→[task-298]), `progress: not-started`, empty `commits`, and the required-behavior / acceptance-criteria / verification-intent / discovery-evidence bodies were compared field-by-field against the embedded plan JSON — all match verbatim (discovery-evidence arrays list the covered audit sections in spec-document order; content identical).
- No product file, canonical spec, existing task, coverage status, or gate was touched by the publication; no coverage rows were marked; new tasks remain `not-started`.

### Audit challenge (per required behavior)

Re-read `specs/system/agent-runtime.md` and spot-checked the audit's gap evidence against the current tree: `gyre_spawn_worker` is dispatched in `crates/gyre-server/src/mcp.rs`; the meta-spec CRUD lives at `/api/v1/meta-specs-registry` while `/api/v1/meta-specs/:path/blast-radius` occupies the specced prefix (`api/mod.rs:977-991`); the `/api/v1/personas` routes remain registered (`api/mod.rs:1002-1013`); the archive/unarchive routes sit in `scripts/abac-route-registry-exemptions.txt:30,34` with no `archive` action in the ABAC vocabulary; the registry write gate is tenant-Admin for every scope; `specs/system/ralph-loop.md` still exists with a live cross-reference in `specs/system/authorization-provenance.md`; the Workspace entity has no `max_agent_iterations`; the reject handler stops agents synchronously while the budget path runs a real grace timer; the unscoped `agents://` read falls through to `state.agents.list()` with no caller-scope filter (`mcp.rs:4664`); `docker/gyre-agent/Dockerfile` exists and `api/spawn.rs:1772` directs operators to build it; the workspace/tenant meta-spec-set routes are still live (`api/mod.rs:821,854`). All twelve gap decisions hold at this base. The decomposition (8 tasks, dependency chain 298→302→301 and 303→298 matching the plan's `dependencies`) is sound, and no other active task binds any of these gap section ids — only task-297..305 reference them, so there is no duplicate active assignment. Task numbering 297-304 collides with nothing on main (which carries up to task-296 plus 306/307).

Task definitions and explicit bindings only; no product implementation claimed.
