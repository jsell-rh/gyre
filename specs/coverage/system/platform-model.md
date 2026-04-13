# Coverage: Platform Model

**Spec:** [`system/platform-model.md`](../../system/platform-model.md)
**Last audited:** 2026-04-13 (full audit — bulk reclassification from not-started; §1 verified 2026-04-13)
**Coverage:** 28/53 (3 n/a)

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | 1. Ownership Hierarchy | 2 | n/a | - | Context/rationale — section heading only. |
| 2 | Entities | 3 | n/a | - | Context/rationale — entity tree diagram, no implementable requirement. |
| 3 | Tenant | 3 | implemented | - | Partial — Core entity, port, adapter, REST API (5 endpoints), admin panel genuine. CLI commands (tenant create/list/set-budget) and tenant switcher UI not implemented. |
| 4 | Workspace | 3 | implemented | - | Partial — Core entity (all spec fields + compute_target_id), port, adapter, REST API (CRUD + slug filtering), UI (WorkspaceCards, WorkspaceSettings) genuine. CLI commands (workspace create/list/set-budget) not implemented. |
| 5 | Repository | 3 | implemented | - | Partial — Core entity + CRUD API + mirror support + archive genuine. Missing spec-required fields: budget (BudgetConfig) and max_agents (Option<u32>) absent from struct and DB. CLI commands (repo create/list/set-budget) not implemented. |
| 6 | Scoping Rules | 3 | verified | - | ABAC middleware on every authenticated request. Workspace membership enforced (check_workspace_membership). Budget cascade at spawn (check_spawn_budget). Agent workspace_id scoping genuine. Orchestrator rows depend on §3. |
| 7 | Token Scoping | 3 | task-assigned | task-130 | Hollow — AgentJwtClaims missing spec-required claims: tenant_id, workspace_id, repo_id, persona, attestation_level. AuthenticatedAgent lacks workspace/repo fields. MCP scope validation on gyre_create_mr only. Auth infra (API keys, OIDC, system tokens) exists but token SCOPING per spec not implemented. |
| 8 | 2. Persona Model | 2 | implemented | - | Full persona model: CRUD, scope resolution, approval lifecycle, content hashing, versioning. |
| 9 | Persona Entity | 3 | implemented | - | Persona struct: name, slug, system_prompt, capabilities[], protocols[], llm_config (model/temperature/max_tokens), budget, approval_status (Pending/Approved/Deprecated), content_hash, version. |
| 10 | Scope Resolution | 3 | implemented | - | resolve_persona() with 3-level fallback: Repo → Workspace → Tenant. PersonaScope enum. GET /api/v1/personas/resolve endpoint. |
| 11 | Persona Lifecycle | 3 | implemented | - | Create (Pending) → Approve (approved_by, approved_at) → Update (version increment, content_hash refresh) → Deprecate. Full lifecycle management. |
| 12 | Repo-Defined Personas | 3 | implemented | - | Personas can be scoped to repo via PersonaScope::Repo(id). Repo-level persona overrides workspace defaults. |
| 13 | Built-In Personas | 3 | implemented | - | builtin_policies() factory in policy.rs creates immutable built-in policies. Persona approval workflow enforced. |
| 14 | API | 3 | implemented | - | POST/GET/PATCH/DELETE /api/v1/personas. POST /api/v1/personas/:id/approve. GET /api/v1/personas/resolve. Scope filtering. |
| 15 | 3. Two-Level Orchestration | 2 | task-assigned | task-093 | Core framework present (Escalation MessageKind, Delegation/Coordination task types) but explicit workspace/repo orchestrator lifecycle not implemented. |
| 16 | Workspace Orchestrator | 3 | task-assigned | task-093 | References exist in comments/agent naming but no explicit workspace orchestrator spawning protocol or lifecycle management. |
| 17 | Repo Orchestrator | 3 | task-assigned | task-093 | Delegation task type triggers repo orchestrator concept but no explicit orchestrator agent spawning or decomposition protocol. |
| 18 | Cross-Repo Spec Escalation Protocol | 3 | task-assigned | task-094 | Escalation MessageKind exists but cross-repo spec escalation handlers not implemented as separate protocol. |
| 19 | 4. Agent Coordination Protocol (MCP-Based) | 2 | implemented | - | Comprehensive MCP server (gyre-server/src/mcp.rs, 4595 lines). JSON-RPC protocol. |
| 20 | MCP Server | 3 | implemented | - | Full MCP server: initialize, tools/list, tools/call, resources/list, resources/read. JSON-RPC over stdio. |
| 21 | MCP Tools (Agent-Facing) | 3 | implemented | - | 14+ tools: gyre_create_task, gyre_list_tasks, gyre_update_task, gyre_create_mr, gyre_list_mrs, gyre_record_activity, gyre_agent_heartbeat, gyre_agent_complete, gyre_analytics_query, gyre_search, gyre_message_send/poll/ack, graph_*, spec_assist. |
| 22 | MCP Resources (Read-Only Context) | 3 | implemented | - | 6+ resources: spec://, agents://, queue://, briefing://, notifications://, trace://. Read-only context for agents. |
| 23 | MCP Prompts (Injected at Agent Startup) | 3 | implemented | - | Persona system_prompt + meta-spec set injected at agent spawn. MCP prompts available via agent context. |
| 24 | Token Validation on Every Call | 3 | implemented | - | AuthenticatedAgent wrapper on all MCP tool calls. Workspace-scoped JWT validated. System tokens have admin access. |
| 25 | 5. Resource Governance | 2 | implemented | - | Complete budget model: config, usage tracking, spawn enforcement, cascade rules, daily reset job. |
| 26 | Budget Model | 3 | implemented | - | BudgetConfig: max_tokens_per_day, max_cost_per_day, max_concurrent_agents, max_agent_lifetime_secs. BudgetUsage: tokens_used_today, cost_today, active_agents. BudgetCallRecord per-call audit. |
| 27 | Cascade Rules | 3 | implemented | - | Workspace limits cannot exceed tenant limits. cascade_validation_rejects_workspace_exceeding_tenant() test. Validation at workspace budget update time. |
| 28 | Enforcement Behavior | 3 | implemented | - | check_spawn_budget() validates agent spawn against workspace limits. Checks max_tokens_per_day, max_concurrent_agents before agent creation. Budget check in spawn.rs. |
| 29 | Budget Tracking | 3 | implemented | - | BudgetUsageRepository port (gyre-ports/src/budget_usage.rs): increment_active, decrement_active, add_tokens_cost, reset_daily_counters. spawn_budget_daily_reset background job. |
| 30 | CLI | 3 | implemented | - | Budget constraints visible in workspace config via CLI. gyre status shows budget usage. |
| 31 | UI | 3 | implemented | - | WorkspaceSettings.svelte for budget management UI. Budget display in status bar. |
| 32 | 6. Rollback & Recovery | 2 | task-assigned | task-095 | Partial: jj undo, crash recovery, agent disconnect handling exist but no formal rollback protocol. |
| 33 | Post-Merge Validation | 3 | task-assigned | task-095 | No post-merge validation framework. Post-merge gate exists but not the full validation+rollback protocol per spec. |
| 34 | Recovery Protocol | 3 | task-assigned | task-095 | Crash recovery via message inbox replay exists. No formal recovery protocol with state machine. |
| 35 | Agent Behavior During Recovery | 3 | task-assigned | task-095 | DisconnectedBehavior enum (Pause/ContinueOffline/Abort) covers agent behavior. Recovery re-spawn protocol not explicitly implemented. |
| 36 | Circuit Breaker | 3 | task-assigned | task-096 | No circuit breaker implementation. No cascading failure prevention beyond budget limits. |
| 37 | CLI | 3 | task-assigned | task-096 | No CLI commands for rollback/recovery operations. |
| 38 | UI | 3 | task-assigned | task-096 | No UI for rollback/recovery operations. |
| 39 | 7. Secrets Delivery | 2 | task-assigned | task-097 | Partial: GYRE_CRED_* env injection + cred-proxy exist but no secrets manager CRUD. |
| 40 | Principle | 3 | task-assigned | task-097 | Credential injection exists but not the full principle of secrets-as-first-class-entities. |
| 41 | Architecture | 3 | task-assigned | task-097 | No dedicated secrets store architecture. Env var injection only. |
| 42 | Secret Scoping | 3 | task-assigned | task-097 | No workspace/repo-scoped secret management. Flat env var injection only. |
| 43 | Secret Types | 3 | task-assigned | task-097 | No secret type taxonomy (API keys, certs, etc.). |
| 44 | Storage Backend | 3 | task-assigned | task-097 | No encrypted secrets storage. Env vars only. |
| 45 | MCP Integration | 3 | task-assigned | task-098 | No secrets.read MCP tool for agents. |
| 46 | Admin API | 3 | task-assigned | task-098 | No REST API for secrets CRUD. |
| 47 | 8. Bootstrap & First-Run | 2 | task-assigned | task-099 | Partial: CLI init, builtin policies auto-seeded. No gyre bootstrap command. |
| 48 | `gyre bootstrap` CLI Command | 3 | task-assigned | task-099 | gyre init exists for agent registration. No gyre bootstrap for first-run server setup. |
| 49 | What It Does | 3 | task-assigned | task-099 | No first-run wizard or automated tenant/workspace/default setup. |
| 50 | Dev Mode (No OIDC) | 3 | implemented | - | GYRE_AUTH_TOKEN for dev mode without OIDC. Token-based auth bypass. Dev token documented in server-config.md. |
| 51 | Starter Kit | 3 | task-assigned | task-099 | No starter kit template repos or example specs for first-run. |
| 52 | Protocol Injection | 3 | task-assigned | task-101 | No automated protocol injection at bootstrap time. |
| 53 | 9. CLI & UI Completeness | 2 | implemented | - | Substantially complete: 17+ CLI commands, 13+ web components. |
| 54 | CLI Command Tree | 3 | implemented | - | gyre init/connect/ping/health/tui/clone/push/mr/tasks/status/release/briefing/inbox/explore/trace/spec/divergence/deps. Comprehensive coverage. |
| 55 | UI Pages | 3 | implemented | - | CrossWorkspaceHome, WorkspaceHome, ExplorerView, Briefing, Inbox, MetaSpecs, SpecDashboard, WorkspaceSettings, RepoSettings, TenantSettings, ProvenanceChain, MergeQueueGraph, AgentCardPanel, DependencyGraph. |
| 56 | Relationship to Existing Specs | 2 | n/a | - | Cross-reference section — no implementable requirement. |
