# Coverage: Agent Runtime

**Spec:** [`system/agent-runtime.md`](../../system/agent-runtime.md)
**Last audited:** 2026-04-13 (§1 Agent Lifecycle group audited 2026-09-30: all 11 rows remain task-assigned — task-115/118 still `progress: not-started`; notes refreshed with code evidence. Groups remaining: §2 (task-116), §3 (task-117), §4 (task-119), §5. Date advances when all groups are done.)
**Coverage:** 0/33 (1 n/a)

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | The Model | 2 | task-assigned | task-115 | Audited 2026-09-30: overview + lifecycle chain diagram; requirements enumerated in child sections (rows 3–11). No standalone implementable requirement. |
| 2 | 1. Agent Lifecycle | 2 | task-assigned | task-115 | Audited 2026-09-30: parent heading for Signal Chain + Phases 1–8 (rows 3–11); no standalone requirement. |
| 3 | The Signal Chain | 3 | task-assigned | task-115 | Audited 2026-09-30: `SpecApproved` kind genuine (message.rs:109, Event tier, server_only, serde `spec_approved`) and emitted by approval handler (specs.rs:680-700, Destination::Workspace, payload {repo_id, spec_path, spec_sha, approved_by, approval_id}). Approval invalidation on spec modify/delete/rename genuine (git_http.rs:1420-1459 revoke_all_for_path; spec_registry.rs:391 SHA-change → Pending). Chain terminates at the bus: no consumer spawns orchestrators (rows 5–6). |
| 4 | Phase 1: Spec Approval Triggers Orchestration | 3 | task-assigned | task-115 | Audited 2026-09-30: emit side genuine (specs.rs:680-700 — ledger record + SpecApproved event, full payload). Receive side (workspace-orchestrator inbox delivery / spawn-on-demand) absent — see row 5. |
| 5 | Phase 2: Workspace Orchestrator — Cross-Repo Coordination | 3 | task-assigned | task-115 | Audited 2026-09-30: only the `workspace-orchestrator` persona seed exists (lib.rs:1348-1353). No orchestrator registry, no server interception of workspace-destined SpecApproved, no spawn-on-demand/one-active semantics (grep orchestrator_registry/ensure_orchestrator/spawn_orchestrator: no matches). |
| 6 | Phase 3: Repo Orchestrator — Task Decomposition | 3 | task-assigned | task-115 | Audited 2026-09-30: only `repo-orchestrator` persona seed (lib.rs:1354-1359). TaskType enum exists (domain/task.rs:39) and spawn path rejects Delegation/Coordination from worker spawning (spawn.rs:289-308) — but nothing behind that error: no task scheduler detecting Delegation tasks, no per-repo mutex. |
| 7 | Phase 4: System Spawns Agents (Mechanical) | 3 | task-assigned | task-118 | Audited 2026-09-30: primitives exist, scheduler absent. Genuine: TaskType discriminator (spawn.rs:289-308), AgentStatus enum (domain/agent.rs:28), spawn-time budget check (spawn.rs:277-280 — but 429s instead of spec's queueing), git worktree + jj init + worktree DB record (spawn.rs:436-509), env injection GYRE_SERVER_URL/AUTH_TOKEN/CLONE_URL/BRANCH/AGENT_ID/TASK_ID/REPO_ID (spawn.rs:594-601), compute-target container provisioning (spawn.rs:562-655), agent token mint/revocation (kv agent_tokens, spawn.rs:1195). Missing: automatic detection of Backlog Implementation tasks (spawn is API-driven only), depends_on/order checks, Archived-repo rejection, max_agents queueing, BudgetExhausted flag (none in spawn.rs). |
| 8 | Phase 5: Implementation | 3 | task-assigned | task-118 | Audited 2026-09-30: MCP surface genuine — gyre_list_tasks/gyre_update_task/gyre_agent_heartbeat/gyre_agent_complete (mcp.rs:151-292), conversation upload (zstd, 10MB cap), agent-JWT scoping (agent cannot act for another agent, mcp.rs:2662-2684). Divergence: MCP gyre_agent_complete (mcp.rs:1012-1091) only marks Idle + emits events + persists completion summary — does NOT create an MR; MR creation lives only in REST POST /agents/:id/complete (spawn.rs:1080-1312). |
| 9 | Phase 6: MR and Gates | 3 | task-assigned | task-118 | Audited 2026-09-30: REST complete_agent creates MR with spec_ref path@sha from task's spec_path (spawn.rs:1113-1163), diff stats + conflict detection, task→Review, token revoked. Gates do NOT run automatically on completion — trigger_gates_for_mr is called only from merge-queue enqueue (merge_queue.rs:77). Gate executor itself genuine incl. agent_review reviewer process with GYRE_REVIEW_TOKEN (gate_executor.rs:23-58, 389-405; see agent-gates.md coverage). |
| 10 | Phase 7: Ralph Loop | 3 | task-assigned | task-118 | Audited 2026-09-30: not implemented. No re-spawn on gate failure (no code path), no max_iterations loop (agent_tracking.rs:115 has unwired max_iterations:50 default; Repository has no max_agent_iterations field — platform-model row 5), no TaskBlocked message kind (absent from message.rs), no spawn-failure retry ×3 backoff (container spawn failure is best-effort warn), no auto-merge on all-pass. Spec rejection mid-flight IS genuine (specs.rs:842-982 — cancels in-flight tasks, stops agents, 60s grace, priority-2 SpecRejected; see HSI row 43). |
| 11 | Phase 8: System-Initiated Agents | 3 | task-assigned | task-118 | Audited 2026-09-30: interrogation genuine (spawn.rs:421-434 — read-only, no worktree, 30-min JWT; HSI rows 29/37). Gate reviewer: subprocess spawned by gate_executor with GYRE_REVIEW_TOKEN, single-shot, token revoked after (gate_executor.rs:389-405) — functionally genuine though process-based, not agent-record lifecycle. Reconciliation: persona seeded (lib.rs:1378-1383) and ReconciliationCompleted flow exists (lib.rs:1134-1207) but no system-spawned Ralph-loop reconciliation agent. |
| 12 | 2. Meta-Spec Prompt Assembly | 2 | task-assigned | task-116 | |
| 13 | Meta-Specs Are Prompts | 3 | task-assigned | task-116 | |
| 14 | Registry Levels | 3 | task-assigned | task-116 | |
| 15 | Required vs Optional | 3 | task-assigned | task-116 | |
| 16 | Spec-Level Binding | 3 | task-assigned | task-116 | |
| 17 | Injection Order | 3 | task-assigned | task-116 | |
| 18 | Versioning and Attestation | 3 | task-assigned | task-116 | |
| 19 | Stale Pin Detection | 3 | task-assigned | task-116 | |
| 20 | Bootstrap | 3 | task-assigned | task-116 | |
| 21 | API | 3 | task-assigned | task-116 | |
| 22 | 3. Compute Target Model | 2 | task-assigned | task-117 | |
| 23 | Abstraction | 3 | task-assigned | task-117 | |
| 24 | Supported Backends | 3 | task-assigned | task-117 | |
| 25 | Nix-Based Image Build | 3 | task-assigned | task-117 | |
| 26 | Tenant and Workspace Configuration | 3 | task-assigned | task-117 | |
| 27 | 4. Budget Enforcement | 2 | task-assigned | task-119 | |
| 28 | Cascade | 3 | task-assigned | task-119 | |
| 29 | Enforcement Levels | 3 | task-assigned | task-119 | |
| 30 | What's Tracked | 3 | task-assigned | task-119 | |
| 31 | Budget Reset | 3 | task-assigned | task-119 | |
| 32 | 5. Agent Prompt Structure | 2 | task-assigned | task-119 | |
| 33 | Relationship to Existing Specs | 2 | n/a | - | Cross-reference/amendment section — no implementable requirement. |
